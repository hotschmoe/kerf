//! Agent bridge: run a local coding-agent CLI (Claude Code, Grok Build, Codex, ...) headless inside the
//! served folder and stream its stdout to SSE `agent` events. Agents are command templates, so a new CLI is
//! configuration (`<dir>/.kerf/agents.json`), not code.
//!
//! Template: `{ "id", "name", "detect": [...], "argv": [...], "resume": [...] }`. Placeholders: `{message}`,
//! `{session_id}`, `{dir}`, `{file}`; the single element `{resume}` expands to the `resume` list when a
//! session id was given and to nothing otherwise. Session ids are picked up from any top-level `session_id`,
//! `sessionId` or `thread_id` string in the agent's JSON lines, plus pi's `{"type":"session","id":...}`.

use crate::events::Hub;
use kerf_core::json::{self, Value};
use std::io::{BufRead, BufReader, Read};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::{Arc, Condvar, Mutex, MutexGuard};
use std::thread;
use std::time::{Duration, Instant};

#[derive(Clone)]
pub struct Template {
    pub id: String,
    pub name: String,
    pub detect: Vec<String>,
    pub argv: Vec<String>,
    pub resume_args: Vec<String>,
    pub from_workspace: bool,
    pub untrusted: bool,
}

pub const UNTRUSTED_REASON: &str = "defined by this folder's .kerf/agents.json and not trusted: it would run a command from the folder. Start `kerf serve --trust-agents` (or answer yes at the prompt) if you wrote it";

pub const PREFIX: &str =
    "You are working in a Kerf details library. Run 'kerf guide' first if you haven't this session.";
pub const DEFAULT_MAX_RUN_S: u32 = 30 * 60;
const DETECT_TTL: Duration = Duration::from_secs(30);
const DETECT_TIMEOUT: Duration = Duration::from_secs(8);
const KILL_GRACE: Duration = Duration::from_millis(3000);
const MAX_EVENT_BYTES: usize = 1 << 20;

fn builtin(id: &str, name: &str, argv: &[&str], resume: &[&str]) -> Template {
    let own = |s: &[&str]| s.iter().map(|x| x.to_string()).collect::<Vec<_>>();
    Template {
        id: id.into(),
        name: name.into(),
        detect: vec![id.into(), "--version".into()],
        argv: own(argv),
        resume_args: own(resume),
        from_workspace: false,
        untrusted: false,
    }
}

pub fn builtin_templates() -> Vec<Template> {
    vec![
        builtin(
            "claude",
            "Claude Code",
            &[
                "claude",
                "-p",
                "{message}",
                "--output-format",
                "stream-json",
                "--verbose",
                "{resume}",
                "--permission-mode",
                "acceptEdits",
                "--allowedTools",
                "Bash(kerf:*)",
                "Bash(kerf *)",
                "Read",
                "Write",
                "Edit",
            ],
            &["--resume", "{session_id}"],
        ),
        builtin(
            "grok",
            "Grok Build",
            &[
                "grok",
                "-p",
                "{message}",
                "--output-format",
                "streaming-json",
                "--always-approve",
                "--cwd",
                "{dir}",
                "{resume}",
            ],
            &["--resume", "{session_id}"],
        ),
        builtin(
            "codex",
            "Codex CLI",
            &[
                "codex",
                "exec",
                "--sandbox",
                "workspace-write",
                "--skip-git-repo-check",
                "--cd",
                "{dir}",
                "--json",
                "{resume}",
                "{message}",
            ],
            &["resume", "{session_id}"],
        ),
        builtin(
            "pi",
            "Pi",
            &["pi", "-p", "--mode", "json", "{resume}", "{message}"],
            &["--session-id", "{session_id}"],
        ),
    ]
}

pub fn context_message(message: &str, file: Option<&str>) -> String {
    match file {
        Some(f) => format!("{PREFIX} Current document: {f}.\n\n{message}"),
        None => format!("{PREFIX}\n\n{message}"),
    }
}

/// Built-ins plus `<dir>/.kerf/agents.json` entries (new id) or, when `trust` is set, replacing (same id). Without
/// `trust` a workspace entry never replaces a built-in and carries `untrusted`: nothing of it is executed.
pub fn load_templates(dir: &Path, trust: bool) -> Vec<Template> {
    let mut list = builtin_templates();
    let Some(text) = read_workspace_file(&dir.join(".kerf/agents.json")) else { return list };
    let Ok(root) = json::parse(&text) else { return list };
    let items = match &root {
        Value::Array(a) => Some(a.as_slice()),
        v => v.get("agents").and_then(Value::arr),
    };
    for item in items.unwrap_or(&[]) {
        let Some(mut t) = parse_template(item) else { continue };
        t.from_workspace = true;
        t.untrusted = !trust;
        match list.iter_mut().find(|e| e.id == t.id) {
            Some(ex) => {
                if trust && !ex.from_workspace {
                    *ex = t;
                }
            }
            None => list.push(t),
        }
    }
    list
}

/// Reads a small file of the served folder, refusing a symlink at the last component (a prompt-injected agent
/// can plant `.kerf/agents.json -> /somewhere`).
fn read_workspace_file(path: &Path) -> Option<Vec<u8>> {
    let meta = std::fs::symlink_metadata(path).ok()?;
    if !meta.is_file() {
        return None;
    }
    let mut buf = Vec::new();
    std::fs::File::open(path).ok()?.take(1 << 20).read_to_end(&mut buf).ok()?;
    Some(buf)
}

fn str_list(v: Option<&Value>) -> Option<Vec<String>> {
    v?.arr()?.iter().map(|e| e.str().map(str::to_owned)).collect()
}

fn parse_template(v: &Value) -> Option<Template> {
    if !matches!(v, Value::Object(_)) {
        return None;
    }
    let id = v.get("id")?.str()?;
    if id.is_empty() || id.len() > 40 || !id.bytes().all(|c| c.is_ascii_alphanumeric() || c == b'-' || c == b'_') {
        return None;
    }
    let argv = str_list(v.get("argv"))?;
    let first = argv.first()?.clone();
    let detect = str_list(v.get("detect")).unwrap_or_else(|| vec![first, "--version".into()]);
    if detect.is_empty() {
        return None;
    }
    Some(Template {
        id: id.into(),
        name: v.get("name").map_or(id, |n| n.str().unwrap_or(id)).into(),
        detect,
        argv,
        resume_args: str_list(v.get("resume")).unwrap_or_default(),
        from_workspace: false,
        untrusted: false,
    })
}

/// Expands placeholders; a `None` session id drops the `{resume}` element.
pub fn expand_argv(t: &Template, message: &str, session_id: Option<&str>, dir: &str, file: &str) -> Vec<String> {
    let mut out = Vec::new();
    for arg in &t.argv {
        if arg == "{resume}" {
            if let Some(sid) = session_id {
                out.extend(t.resume_args.iter().map(|ra| subst(ra, message, sid, dir, file)));
            }
            continue;
        }
        out.push(subst(arg, message, session_id.unwrap_or(""), dir, file));
    }
    out
}

fn subst(s: &str, message: &str, sid: &str, dir: &str, file: &str) -> String {
    if !s.contains('{') {
        return s.to_owned();
    }
    let mut cur = s.to_owned();
    for (key, val) in [("{message}", message), ("{session_id}", sid), ("{dir}", dir), ("{file}", file)] {
        if cur.contains(key) {
            cur = cur.replace(key, val);
        }
    }
    cur
}

#[derive(Clone, Default)]
pub struct Detected {
    pub available: bool,
    pub version: Option<String>,
    pub reason: Option<String>,
}

fn unavailable(reason: String) -> Detected {
    Detected { available: false, version: None, reason: Some(reason) }
}

fn drain_limited(mut r: impl Read + Send + 'static, limit: u64) -> thread::JoinHandle<Vec<u8>> {
    thread::spawn(move || {
        let mut buf = Vec::new();
        let _ = (&mut r).take(limit).read_to_end(&mut buf);
        let _ = std::io::copy(&mut r, &mut std::io::sink());
        buf
    })
}

/// Runs the template's detect command (never for an untrusted template: it would execute a workspace command).
pub fn detect(t: &Template) -> Detected {
    if t.untrusted {
        return unavailable(UNTRUSTED_REASON.into());
    }
    let Some((bin, args)) = t.detect.split_first() else { return unavailable("no detect command".into()) };
    let mut child =
        match Command::new(bin).args(args).stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::piped()).spawn() {
            Ok(c) => c,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                return unavailable(format!("`{bin}` was not found on PATH"));
            }
            Err(e) => return unavailable(format!("could not run `{bin}`: {e}")),
        };
    let out = child.stdout.take().map(|s| drain_limited(s, 8192));
    let err = child.stderr.take().map(|s| drain_limited(s, 8192));
    let start = Instant::now();
    let status = loop {
        match child.try_wait() {
            Ok(Some(s)) => break s,
            Ok(None) if start.elapsed() > DETECT_TIMEOUT => {
                let _ = child.kill();
                let _ = child.wait();
                return unavailable(format!("`{bin}` did not answer within 8 s"));
            }
            Ok(None) => thread::sleep(Duration::from_millis(10)),
            Err(e) => return unavailable(format!("could not run `{bin}`: {e}")),
        }
    };
    let join = |h: Option<thread::JoinHandle<Vec<u8>>>| h.and_then(|h| h.join().ok()).unwrap_or_default();
    let (stdout, stderr) = (join(out), join(err));
    if !status.success() {
        return unavailable(format!("`{bin}` exited with an error (try running it once in a terminal to sign in)"));
    }
    let text = if stdout.trim_ascii().is_empty() { stderr } else { stdout };
    let line = text.split(|&c| c == b'\n').next().unwrap_or(&[]).trim_ascii();
    let line = String::from_utf8_lossy(&line[..line.len().min(80)]).into_owned();
    Detected { available: true, version: Some(line), reason: None }
}

#[cfg(unix)]
mod group {
    unsafe extern "C" {
        fn kill(pid: i32, sig: i32) -> i32;
    }

    pub fn signal(pgid: u32, hard: bool) {
        if pgid > 1 {
            // SAFETY: plain syscall; a negative pid addresses the process group led by the agent.
            unsafe { kill(-(pgid as i32), if hard { 9 } else { 15 }) };
        }
    }
}

#[cfg(not(unix))]
mod group {
    pub fn signal(pid: u32, _hard: bool) {
        let _ = std::process::Command::new("taskkill").args(["/T", "/F", "/PID", &pid.to_string()]).output();
    }
}

#[derive(Default)]
struct RunState {
    pid: Option<u32>,
    finished: bool,
    stop_requested: bool,
    timed_out: bool,
    leader_done: bool,
    term_sent: Option<Instant>,
    session_id: Option<String>,
}

struct Run {
    id: String,
    agent: String,
    started: Instant,
    state: Mutex<RunState>,
    pumps_done: AtomicU32,
}

impl Run {
    fn lock(&self) -> MutexGuard<'_, RunState> {
        self.state.lock().unwrap_or_else(|e| e.into_inner())
    }
}

struct CacheEntry {
    key: String,
    at: Option<Instant>,
    det: Detected,
    busy: bool,
}

#[derive(Default)]
struct Inner {
    next_id: u32,
    active: Option<Arc<Run>>,
    cache: Vec<CacheEntry>,
}

pub struct Manager {
    hub: Arc<Hub>,
    dir_abs: PathBuf,
    exe_dir: PathBuf,
    pub trust_agents: bool,
    pub max_run_s: u32,
    before_exit: Option<Box<dyn Fn() + Send + Sync>>,
    inner: Mutex<Inner>,
    cond: Condvar,
}

pub enum StartError {
    Busy(String),
    Unavailable(String),
    SpawnFailed(String),
    TooLong(String),
}

pub struct StartParams<'a> {
    pub template: &'a Template,
    pub message: &'a str,
    pub session_id: Option<&'a str>,
    pub file: Option<&'a str>,
}

#[derive(PartialEq, Eq, Debug)]
pub enum StopResult {
    Stopped,
    NoSuchRun,
    AlreadyFinished,
}

impl Manager {
    pub fn new(hub: Arc<Hub>, dir_abs: PathBuf, exe_dir: PathBuf, trust_agents: bool, max_run_s: u32) -> Manager {
        Manager {
            hub,
            dir_abs,
            exe_dir,
            trust_agents,
            max_run_s,
            before_exit: None,
            inner: Mutex::new(Inner { next_id: 1, ..Inner::default() }),
            cond: Condvar::new(),
        }
    }

    /// Called after the agent's pipes are drained and before its `exit` event, so the server can publish the
    /// document changes the run made ahead of it.
    pub fn set_before_exit(&mut self, f: impl Fn() + Send + Sync + 'static) {
        self.before_exit = Some(Box::new(f));
    }

    pub fn templates(&self) -> Vec<Template> {
        load_templates(&self.dir_abs, self.trust_agents)
    }

    fn lock(&self) -> MutexGuard<'_, Inner> {
        self.inner.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// Detection results are cached for 30 s per template; concurrent callers share one probe.
    pub fn detect_cached(&self, t: &Template) -> Detected {
        if t.untrusted {
            return unavailable(UNTRUSTED_REASON.into());
        }
        let key = format!("{}\x01{}", t.id, t.detect.join("\0"));
        let mut g = self.lock();
        loop {
            match g.cache.iter().find(|c| c.key == key) {
                Some(c) if c.busy => g = self.cond.wait(g).unwrap_or_else(|e| e.into_inner()),
                Some(c) if c.at.is_some_and(|at| at.elapsed() < DETECT_TTL) => return c.det.clone(),
                _ => break,
            }
        }
        match g.cache.iter_mut().find(|c| c.key == key) {
            Some(c) => c.busy = true,
            None => g.cache.push(CacheEntry { key: key.clone(), at: None, det: Detected::default(), busy: true }),
        }
        drop(g);
        let det = detect(t);
        let mut g = self.lock();
        if let Some(c) = g.cache.iter_mut().find(|c| c.key == key) {
            c.det = det.clone();
            c.at = Some(Instant::now());
            c.busy = false;
        }
        self.cond.notify_all();
        det
    }

    pub fn active_info(&self) -> Option<(String, String)> {
        let r = self.lock().active.clone()?;
        let finished = r.lock().finished;
        (!finished).then(|| (r.id.clone(), r.agent.clone()))
    }

    /// Asks the active run to stop and waits for it to be gone (SIGTERM, then SIGKILL after the grace period).
    pub fn shutdown(&self) {
        if let Some(r) = self.lock().active.clone() {
            let mut st = r.lock();
            if !st.finished {
                st.stop_requested = true;
                if let (Some(pid), None) = (st.pid, st.term_sent) {
                    group::signal(pid, false);
                    st.term_sent = Some(Instant::now());
                }
            }
        }
        let deadline = Instant::now() + KILL_GRACE + Duration::from_secs(3);
        while Instant::now() < deadline && self.lock().active.is_some() {
            thread::sleep(Duration::from_millis(50));
        }
    }

    pub fn start(self: &Arc<Self>, p: StartParams) -> Result<String, StartError> {
        if p.template.untrusted {
            return Err(StartError::Unavailable(UNTRUSTED_REASON.into()));
        }
        let det = self.detect_cached(p.template);
        if !det.available {
            return Err(StartError::Unavailable(det.reason.unwrap_or_else(|| "not available".into())));
        }
        let dir = self.dir_abs.to_string_lossy().into_owned();
        let full = context_message(p.message, p.file);
        let argv = expand_argv(p.template, &full, p.session_id, &dir, p.file.unwrap_or(""));
        let Some((bin, args)) = argv.split_first() else { return Err(StartError::SpawnFailed("empty argv".into())) };

        let mut cmd = Command::new(bin);
        cmd.args(args).current_dir(&self.dir_abs).stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::piped());
        let old_path = std::env::var_os("PATH").unwrap_or_default();
        let mut paths = vec![self.exe_dir.clone()];
        paths.extend(std::env::split_paths(&old_path));
        if let Ok(joined) = std::env::join_paths(paths) {
            cmd.env("PATH", joined);
        }
        cmd.env("KERF_ACTOR", "agent").env_remove("KERF_TOKEN");
        #[cfg(unix)]
        std::os::unix::process::CommandExt::process_group(&mut cmd, 0);

        let run;
        {
            let mut g = self.lock();
            if let Some(r) = &g.active
                && !r.lock().finished
            {
                return Err(StartError::Busy(r.id.clone()));
            }
            let id = format!("r{}", g.next_id);
            g.next_id += 1;
            run = Arc::new(Run {
                id,
                agent: p.template.id.clone(),
                started: Instant::now(),
                state: Mutex::default(),
                pumps_done: AtomicU32::new(0),
            });
            g.active = Some(run.clone());
        }
        let mut child = match cmd.spawn() {
            Ok(c) => c,
            Err(e) => {
                self.release_slot(&run);
                let too_long = e.raw_os_error() == Some(7);
                return Err(if too_long {
                    StartError::TooLong(format!("the message and arguments exceed the OS limit for starting `{bin}`"))
                } else {
                    StartError::SpawnFailed(format!("could not start `{bin}`: {e}"))
                });
            }
        };
        let stop_already = {
            let mut st = run.lock();
            st.pid = Some(child.id());
            if st.stop_requested {
                st.term_sent = Some(Instant::now());
            }
            st.stop_requested
        };
        if stop_already {
            group::signal(child.id(), false);
        }
        let id = run.id.clone();
        let (stdout, stderr) = (child.stdout.take(), child.stderr.take());
        let mgr = self.clone();
        thread::spawn(move || mgr.supervise(run, child, stdout, stderr));
        Ok(id)
    }

    fn release_slot(&self, run: &Arc<Run>) {
        let mut g = self.lock();
        if g.active.as_ref().is_some_and(|a| Arc::ptr_eq(a, run)) {
            g.active = None;
        }
    }

    pub fn stop(&self, run_id: &str) -> StopResult {
        let Some(r) = self.lock().active.clone() else { return StopResult::NoSuchRun };
        if r.id != run_id {
            return StopResult::NoSuchRun;
        }
        let mut st = r.lock();
        if st.finished {
            return StopResult::AlreadyFinished;
        }
        st.stop_requested = true;
        if let (Some(pid), None) = (st.pid, st.term_sent) {
            group::signal(pid, false);
            st.term_sent = Some(Instant::now());
        }
        StopResult::Stopped
    }

    fn publish(&self, run_id: &str, event_json: &[u8]) {
        let mut out = b"{\"run_id\":".to_vec();
        json::write_string(&mut out, run_id);
        out.extend_from_slice(b",\"event\":");
        out.extend_from_slice(event_json);
        out.push(b'}');
        self.hub.publish("agent", &String::from_utf8_lossy(&out));
    }

    fn watchdog(&self, run: &Run) {
        loop {
            thread::sleep(Duration::from_millis(100));
            let mut announce = false;
            {
                let mut st = run.lock();
                if st.leader_done || st.finished {
                    return;
                }
                let now = Instant::now();
                let pid = st.pid.unwrap_or(0);
                if st.term_sent.is_none()
                    && self.max_run_s > 0
                    && run.started.elapsed() > Duration::from_secs(self.max_run_s.into())
                {
                    st.timed_out = true;
                    st.stop_requested = true;
                    st.term_sent = Some(now);
                    group::signal(pid, false);
                    announce = true;
                } else if let Some(t0) = st.term_sent
                    && t0.elapsed() > KILL_GRACE
                {
                    group::signal(pid, true);
                    st.term_sent = Some(now);
                }
            }
            if announce {
                let msg = format!(
                    "{{\"type\":\"stderr\",\"text\":\"kerf: the run exceeded the {} s limit; stopping the agent\"}}",
                    self.max_run_s
                );
                self.publish(&run.id, msg.as_bytes());
            }
        }
    }

    fn pumps_done(run: &Run, expect: u32, wait: Duration) -> bool {
        let start = Instant::now();
        loop {
            if run.pumps_done.load(Ordering::Acquire) >= expect {
                return true;
            }
            if start.elapsed() >= wait {
                return false;
            }
            thread::sleep(Duration::from_millis(10));
        }
    }

    fn supervise(
        self: Arc<Self>,
        run: Arc<Run>,
        mut child: Child,
        stdout: Option<std::process::ChildStdout>,
        stderr: Option<std::process::ChildStderr>,
    ) {
        let pid = child.id();
        let mut expect = 0;
        let mut pumps = Vec::new();
        if let Some(s) = stdout {
            let (m, r) = (self.clone(), run.clone());
            pumps.push(thread::spawn(move || m.pump(&r, s, false)));
            expect += 1;
        }
        if let Some(s) = stderr {
            let (m, r) = (self.clone(), run.clone());
            pumps.push(thread::spawn(move || m.pump(&r, s, true)));
            expect += 1;
        }
        let dog = {
            let (m, r) = (self.clone(), run.clone());
            thread::spawn(move || m.watchdog(&r))
        };

        let code = match child.wait() {
            Ok(st) => st.code().unwrap_or_else(|| signal_code(&st)),
            Err(_) => -1,
        };
        run.lock().leader_done = true;

        if !Self::pumps_done(&run, expect, Duration::from_millis(100)) {
            group::signal(pid, false);
            if !Self::pumps_done(&run, expect, Duration::from_millis(1500)) {
                group::signal(pid, true);
            }
        }
        group::signal(pid, true); // sweep: a stray process that closed its pipes
        for p in pumps {
            let _ = p.join();
        }
        let _ = dog.join();

        let (stopped, timed_out) = {
            let mut st = run.lock();
            st.finished = true;
            (st.stop_requested, st.timed_out)
        };
        if let Some(f) = &self.before_exit {
            f();
        }
        let mut ev = format!("{{\"type\":\"exit\",\"code\":{code}").into_bytes();
        if timed_out {
            ev.extend_from_slice(b",\"stopped\":true,\"timeout\":true");
        } else if stopped {
            ev.extend_from_slice(b",\"stopped\":true");
        }
        if let Some(sid) = run.lock().session_id.clone() {
            ev.extend_from_slice(b",\"session_id\":");
            json::write_string(&mut ev, &sid);
        }
        ev.push(b'}');
        self.release_slot(&run);
        self.publish(&run.id, &ev);
    }

    fn pump(&self, run: &Run, src: impl Read, is_stderr: bool) {
        let mut rd = BufReader::with_capacity(16 * 1024, src);
        let mut line: Vec<u8> = Vec::new();
        'lines: loop {
            line.clear();
            let mut too_long = false;
            loop {
                let chunk = match rd.fill_buf() {
                    Ok([]) | Err(_) => {
                        if !line.is_empty() && !too_long {
                            self.emit_line(run, &line, is_stderr);
                        }
                        break 'lines;
                    }
                    Ok(c) => c,
                };
                let nl = chunk.iter().position(|&c| c == b'\n');
                let take = nl.unwrap_or(chunk.len());
                if !too_long {
                    if line.len() + take > MAX_EVENT_BYTES {
                        too_long = true;
                    } else {
                        line.extend_from_slice(&chunk[..take]);
                    }
                }
                rd.consume(nl.map_or(take, |n| n + 1));
                if nl.is_some() {
                    break;
                }
            }
            if too_long {
                self.publish(&run.id, b"{\"type\":\"truncated\",\"text\":\"(output line over 1 MiB omitted)\"}");
            } else {
                self.emit_line(run, &line, is_stderr);
            }
        }
        run.pumps_done.fetch_add(1, Ordering::Release);
    }

    fn emit_line(&self, run: &Run, raw: &[u8], is_stderr: bool) {
        let text = raw.strip_suffix(b"\r").unwrap_or(raw);
        if text.trim_ascii().is_empty() {
            return;
        }
        if !is_stderr && let Ok(v @ Value::Object(_)) = json::parse(text) {
            capture_session(run, &v);
            let mut compact = Vec::new();
            json::write_compact(&mut compact, &v);
            self.publish(&run.id, &compact);
            return;
        }
        let mut ev = if is_stderr {
            b"{\"type\":\"stderr\",\"text\":".to_vec()
        } else {
            b"{\"type\":\"text\",\"text\":".to_vec()
        };
        json::write_string(&mut ev, &String::from_utf8_lossy(text));
        ev.push(b'}');
        self.publish(&run.id, &ev);
    }
}

#[cfg(unix)]
fn signal_code(st: &std::process::ExitStatus) -> i32 {
    std::os::unix::process::ExitStatusExt::signal(st).map_or(-1, |s| 128 + s)
}

#[cfg(not(unix))]
fn signal_code(_: &std::process::ExitStatus) -> i32 {
    -1
}

fn capture_session(run: &Run, v: &Value) {
    let is_pi_session = v.get("type").and_then(Value::str) == Some("session");
    for key in ["session_id", "sessionId", "thread_id", "id"] {
        if key == "id" && !is_pi_session {
            continue;
        }
        let Some(s) = v.get(key).and_then(Value::str) else { continue };
        if s.is_empty() || s.len() > 200 {
            continue;
        }
        run.lock().session_id = Some(s.to_owned());
        return;
    }
}
