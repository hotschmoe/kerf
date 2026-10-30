use std::ffi::{OsStr, OsString, c_void};
use std::fs::File;
use std::io;
use std::os::windows::io::{AsRawHandle, RawHandle};
use std::os::windows::process::CommandExt;
use std::path::PathBuf;
use std::process::{Child, Command, ExitStatus};
use std::sync::OnceLock;
use std::sync::atomic::{AtomicU8, Ordering};
use std::time::Duration;

type Handle = *mut c_void;

#[repr(C)]
struct BasicLimits {
    per_process_user_time: i64,
    per_job_user_time: i64,
    limit_flags: u32,
    min_working_set: usize,
    max_working_set: usize,
    active_process_limit: u32,
    affinity: usize,
    priority_class: u32,
    scheduling_class: u32,
}

#[repr(C)]
struct ExtendedLimits {
    basic: BasicLimits,
    io: [u64; 6],
    process_memory_limit: usize,
    job_memory_limit: usize,
    peak_process_memory: usize,
    peak_job_memory: usize,
}

#[repr(C)]
struct Overlapped {
    internal: usize,
    internal_high: usize,
    offset: u32,
    offset_high: u32,
    event: Handle,
}

#[link(name = "kernel32")]
unsafe extern "system" {
    fn CreateJobObjectW(attrs: *const c_void, name: *const u16) -> Handle;
    fn SetInformationJobObject(job: Handle, class: i32, info: *const c_void, len: u32) -> i32;
    fn AssignProcessToJobObject(job: Handle, process: Handle) -> i32;
    fn TerminateJobObject(job: Handle, exit_code: u32) -> i32;
    fn TerminateProcess(process: Handle, exit_code: u32) -> i32;
    fn OpenProcess(access: u32, inherit: i32, pid: u32) -> Handle;
    fn CloseHandle(h: Handle) -> i32;
    fn LockFileEx(file: Handle, flags: u32, reserved: u32, len_low: u32, len_high: u32, ov: *mut Overlapped) -> i32;
    fn SetConsoleCtrlHandler(handler: Option<unsafe extern "system" fn(u32) -> i32>, add: i32) -> i32;
}

#[link(name = "ntdll", kind = "raw-dylib")]
unsafe extern "system" {
    fn NtResumeProcess(process: Handle) -> i32;
}

#[link(name = "bcryptprimitives", kind = "raw-dylib")]
unsafe extern "system" {
    fn ProcessPrng(buf: *mut u8, len: usize) -> i32;
}

const CREATE_SUSPENDED: u32 = 0x4;
const CREATE_NEW_PROCESS_GROUP: u32 = 0x200;
const PROCESS_TERMINATE: u32 = 0x1;
const JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE: u32 = 0x2000;
const JOB_OBJECT_EXTENDED_LIMIT_INFORMATION: i32 = 9;
const LOCKFILE_EXCLUSIVE_LOCK: u32 = 0x2;

/// Fills `buf` from the system's CSPRNG (`ProcessPrng`, Windows 10 and later).
pub fn random_bytes(buf: &mut [u8]) -> bool {
    // SAFETY: `buf` is valid for `buf.len()` writable bytes for the duration of the call.
    buf.is_empty() || unsafe { ProcessPrng(buf.as_mut_ptr(), buf.len()) } != 0
}

/// The agent is started suspended (so it can join its job before it runs, and a grandchild can never escape it)
/// in its own process group (so the console's Ctrl-C reaches only the server, which then stops the agent).
pub fn prepare_agent(cmd: &mut Command) {
    cmd.creation_flags(CREATE_SUSPENDED | CREATE_NEW_PROCESS_GROUP);
}

/// A handle that is closed on drop.
struct Owned(Handle);

// SAFETY: a kernel handle is a plain token, usable from any thread.
unsafe impl Send for Owned {}
// SAFETY: the calls made through `&Owned` (terminate) are thread-safe kernel operations.
unsafe impl Sync for Owned {}

impl Drop for Owned {
    fn drop(&mut self) {
        // SAFETY: the handle is owned by this value and closed exactly once.
        unsafe { CloseHandle(self.0) };
    }
}

/// The agent and everything it started: a job object that kills its members when it is closed. When the job
/// cannot be set up, only the agent's own process is terminated (grandchildren may survive).
pub struct Group {
    job: Option<Owned>,
    leader: Option<Owned>,
}

fn new_job() -> Option<Owned> {
    // SAFETY: null attributes and name are allowed (an anonymous job with default security).
    let job = Owned(unsafe { CreateJobObjectW(std::ptr::null(), std::ptr::null()) });
    if job.0.is_null() {
        return None;
    }
    let info = ExtendedLimits {
        basic: BasicLimits {
            per_process_user_time: 0,
            per_job_user_time: 0,
            limit_flags: JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE,
            min_working_set: 0,
            max_working_set: 0,
            active_process_limit: 0,
            affinity: 0,
            priority_class: 0,
            scheduling_class: 0,
        },
        io: [0; 6],
        process_memory_limit: 0,
        job_memory_limit: 0,
        peak_process_memory: 0,
        peak_job_memory: 0,
    };
    // SAFETY: `info` is a live `JOBOBJECT_EXTENDED_LIMIT_INFORMATION` of the size passed.
    let ok = unsafe {
        SetInformationJobObject(
            job.0,
            JOB_OBJECT_EXTENDED_LIMIT_INFORMATION,
            (&raw const info).cast(),
            size_of::<ExtendedLimits>() as u32,
        )
    };
    (ok != 0).then_some(job)
}

impl Group {
    /// Puts a child started with [`prepare_agent`] into a new job and lets it run. A child that cannot be
    /// resumed is terminated.
    pub fn adopt(child: &Child) -> io::Result<Group> {
        let process: RawHandle = child.as_raw_handle();
        let job = new_job().filter(|j| {
            // SAFETY: both handles are valid; `process` is owned by `child`, which outlives the call.
            unsafe { AssignProcessToJobObject(j.0, process.cast()) != 0 }
        });
        // SAFETY: plain open of the child's pid; the handle (null on failure) is owned from here on.
        let leader = Owned(unsafe { OpenProcess(PROCESS_TERMINATE, 0, child.id()) });
        let group = Group { job, leader: (!leader.0.is_null()).then_some(leader) };
        // SAFETY: `process` is a valid handle with PROCESS_SUSPEND_RESUME access (the one `Child` holds).
        if unsafe { NtResumeProcess(process.cast()) } < 0 {
            group.signal(true);
            return Err(io::Error::other("could not resume the agent process"));
        }
        Ok(group)
    }

    /// Windows has no soft signal: both are a hard kill of the whole tree.
    pub fn signal(&self, _hard: bool) {
        if let Some(j) = &self.job {
            // SAFETY: `j` is a live job handle.
            unsafe { TerminateJobObject(j.0, 1) };
        } else if let Some(l) = &self.leader {
            // SAFETY: `l` is a live process handle opened with PROCESS_TERMINATE.
            unsafe { TerminateProcess(l.0, 1) };
        }
    }
}

pub fn exit_code_of_signal(_: &ExitStatus) -> i32 {
    -1
}

static SHUTDOWN: OnceLock<&'static AtomicU8> = OnceLock::new();

unsafe extern "system" fn on_ctrl(kind: u32) -> i32 {
    let Some(flag) = SHUTDOWN.get() else { return 0 };
    // CTRL_C_EVENT and CTRL_BREAK_EVENT are SIGINT; closing the console is SIGHUP; logoff and shutdown SIGTERM.
    flag.store(
        match kind {
            0 | 1 => 2,
            2 => 1,
            _ => 15,
        },
        Ordering::Release,
    );
    if kind >= 2 {
        // The process ends when a close/logoff/shutdown handler returns: give the server time to stop the agents.
        std::thread::sleep(Duration::from_secs(10));
    }
    1
}

/// Ctrl-C, Ctrl-Break and console close/logoff/shutdown store a signal number into `flag`.
pub fn install_shutdown_handler(flag: &'static AtomicU8) {
    let _ = SHUTDOWN.set(flag);
    // SAFETY: `on_ctrl` has the HandlerRoutine signature and only touches an atomic and sleeps.
    unsafe { SetConsoleCtrlHandler(Some(on_ctrl), 1) };
}

/// Exclusive lock on one byte far past any real end of file, held until `f` is closed, blocking until granted.
/// Windows byte-range locks are mandatory, so locking the content (std's `File::lock` covers byte 0 on) would make a
/// concurrent read of the log fail. A file system without locks (a network share, FAT) is tolerated: the server's
/// own mutex still serialises its writes.
pub fn lock_exclusive(f: &File) -> io::Result<()> {
    let mut ov =
        Overlapped { internal: 0, internal_high: 0, offset: 0, offset_high: 1 << 30, event: std::ptr::null_mut() };
    // SAFETY: the handle is valid for the call and `ov` outlives it (the call is synchronous on a non-overlapped handle).
    unsafe { LockFileEx(f.as_raw_handle().cast(), LOCKFILE_EXCLUSIVE_LOCK, 0, 1, 0, &raw mut ov) };
    Ok(())
}

fn is_runnable_ext(ext: &str) -> bool {
    ["exe", "com", "cmd", "bat"].iter().any(|e| ext.eq_ignore_ascii_case(e))
}

/// What `CreateProcess` would run for `bin`, looked up like a shell does: each `PATH` directory, with the `PATHEXT`
/// extensions that can be started directly (`.exe`, `.com`, `.cmd`, `.bat`; npm shims are `.cmd`, a `.ps1` or an
/// extensionless script cannot be run). Falls back to `bin` itself, which then fails as not found.
pub fn resolve_program(bin: &str, path: &OsStr) -> PathBuf {
    let given = std::path::Path::new(bin);
    let has_ext = given.extension().and_then(OsStr::to_str).is_some_and(is_runnable_ext);
    let pathext = std::env::var("PATHEXT").unwrap_or_default();
    let mut exts: Vec<&str> =
        pathext.split(';').filter_map(|e| e.strip_prefix('.')).filter(|e| is_runnable_ext(e)).collect();
    if exts.is_empty() {
        exts = vec!["com", "exe", "bat", "cmd"];
    }
    let candidates = |dir: &std::path::Path| -> Option<PathBuf> {
        let base = dir.join(bin);
        if has_ext && base.is_file() {
            return Some(base);
        }
        if !has_ext {
            return exts.iter().map(|e| PathBuf::from(format!("{}.{e}", base.display()))).find(|p| p.is_file());
        }
        None
    };
    if bin.contains(['/', '\\', ':']) {
        return candidates(std::path::Path::new("")).unwrap_or_else(|| bin.into());
    }
    std::env::split_paths(path).find_map(|d| candidates(&d)).unwrap_or_else(|| bin.into())
}

/// `C:\dir` for the verbatim `\\?\C:\dir` that `canonicalize` returns (and `\\server\share` for `\\?\UNC\...`).
pub fn plain_path(p: PathBuf) -> PathBuf {
    let s: OsString = p.clone().into_os_string();
    let Some(text) = s.to_str() else { return p };
    if let Some(unc) = text.strip_prefix(r"\\?\UNC\") {
        return PathBuf::from(format!(r"\\{unc}"));
    }
    match text.strip_prefix(r"\\?\") {
        Some(rest) if rest.as_bytes().get(1) == Some(&b':') && rest.as_bytes()[0].is_ascii_alphabetic() => rest.into(),
        _ => p,
    }
}
