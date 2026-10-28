mod hash;
mod workspace;

use kerf_core::json::{self, Value};
use kerf_core::{api, canon, model, ops, schema, units};
use std::fs;
use std::io::{self, Read, Write};
use workspace::{DocLock, LogMeta};

const USAGE: &str = include_str!("usage.txt");
const CLI_GUIDE: &str = include_str!("../../../../spec/llm/cli-guide.md");
const SYSTEM_MD: &str = include_str!("../../../../spec/llm/system.md");
const FULL_MARKER: &str = "<!-- FULL -->";
const MAX_READ: u64 = 256 << 20;
const TOOL: &str = "kerf-cli";

/// A failure that ends the process as `kerf: <name>` with exit code 2 (the names are the Zig engine's error names).
struct Fail(String);

impl From<io::Error> for Fail {
    fn from(e: io::Error) -> Fail {
        Fail(
            match e.kind() {
                io::ErrorKind::NotFound => "FileNotFound",
                io::ErrorKind::PermissionDenied => "AccessDenied",
                io::ErrorKind::IsADirectory => "IsDir",
                io::ErrorKind::AlreadyExists => "PathAlreadyExists",
                io::ErrorKind::BrokenPipe => "BrokenPipe",
                io::ErrorKind::InvalidData => "StreamTooLong",
                _ => "Unexpected",
            }
            .to_string(),
        )
    }
}

type Res<T> = Result<T, Fail>;

fn bad_engine_output() -> Fail {
    Fail("BadEngineOutput".to_string())
}

fn stderr(text: &str) {
    let _ = io::stderr().write_all(text.as_bytes());
}

fn main() {
    let args: Vec<String> = std::env::args_os().skip(1).map(|a| a.to_string_lossy().into_owned()).collect();
    let actor = std::env::var("KERF_ACTOR").unwrap_or_else(|_| "agent".to_string());
    let code = match run(&args, &actor) {
        Ok(c) => c,
        Err(Fail(name)) => {
            stderr(&format!("kerf: {name}\n"));
            2
        }
    };
    let _ = io::stdout().flush();
    if code != 0 {
        std::process::exit(i32::from(code));
    }
}

fn read_file(path: &str) -> Res<Vec<u8>> {
    let mut buf = Vec::new();
    let n = fs::File::open(path)?.take(MAX_READ + 1).read_to_end(&mut buf)?;
    if n as u64 > MAX_READ {
        return Err(Fail("FileTooBig".to_string()));
    }
    Ok(buf)
}

fn read_stdin() -> Res<Vec<u8>> {
    let mut buf = Vec::new();
    io::stdin().lock().take(MAX_READ + 1).read_to_end(&mut buf)?;
    if buf.len() as u64 > MAX_READ {
        return Err(Fail("StreamTooLong".to_string()));
    }
    Ok(buf)
}

fn write_out(path: Option<&str>, bytes: &[u8]) -> Res<()> {
    match path {
        Some(p) => fs::write(p, bytes)?,
        None => {
            let mut o = io::stdout().lock();
            o.write_all(bytes)?;
            o.flush()?;
        }
    }
    Ok(())
}

fn say(text: &str) -> Res<()> {
    write_out(None, text.as_bytes())
}

use workspace::trim_ws;

/// `--ops` takes the ops themselves (starts with `[` or `{`) or the path of a file that holds them.
fn ops_from_arg(arg: &str) -> Res<Vec<u8>> {
    let t = arg.trim_start_matches([' ', '\t', '\r', '\n']);
    if t.starts_with(['[', '{']) {
        return Ok(arg.as_bytes().to_vec());
    }
    read_file(arg)
}

/// Fold text to pure ASCII for Windows consoles (PowerShell mangles UTF-8): dashes, quotes, x-sign, degree sign,
/// vulgar fractions and NBSP get readable ASCII; anything else becomes `?`.
fn ascii_fold(bytes: &[u8]) -> Res<Vec<u8>> {
    let text = std::str::from_utf8(bytes).map_err(|_| Fail("InvalidUtf8".to_string()))?;
    let mut out = String::with_capacity(text.len());
    for c in text.chars() {
        if c.is_ascii() {
            out.push(c);
            continue;
        }
        out.push_str(match c as u32 {
            0x2010..=0x2015 | 0x2212 | 0x2043 => "-",
            0x2018 | 0x2019 | 0x201A | 0x2032 => "'",
            0x201C | 0x201D | 0x201E | 0x2033 => "\"",
            0xD7 => "x",
            0xB0 => " deg",
            0xBD => " 1/2",
            0xBC => " 1/4",
            0xBE => " 3/4",
            0x215B => " 1/8",
            0x215C => " 3/8",
            0x215D => " 5/8",
            0x215E => " 7/8",
            0xA0 | 0x2009 | 0x202F | 0x2002 | 0x2003 => " ",
            0x2026 => "...",
            0x2022 | 0xB7 => "*",
            0x2192 => "->",
            0x2190 => "<-",
            0x2264 => "<=",
            0x2265 => ">=",
            0x2248 => "~",
            0xB1 => "+/-",
            _ => "?",
        });
    }
    Ok(out.into_bytes())
}

/// `cli-guide.md` has two parts: the short guide, then (after the marker line) notes that only `kerf guide --full` prints.
fn guide_split(text: &str) -> (&str, &str) {
    match text.find(FULL_MARKER) {
        None => (text, ""),
        Some(i) => (text[..i].trim_end_matches('\n'), text[i + FULL_MARKER.len()..].trim_start_matches('\n')),
    }
}

#[derive(Default)]
struct Opts {
    view: Option<String>,
    format: Option<String>,
    style: Option<String>,
    out: Option<String>,
    sheet: bool,
    markdown: bool,
    full: bool,
    write: bool,
    ops: Option<String>,
    id: Option<String>,
    title: Option<String>,
    px: Option<String>,
    why: Option<String>,
    dry_run: bool,
    if_match: Option<String>,
    template: Option<String>,
    pos: Vec<String>,
}

/// `Err(())` means print the usage and exit 2 (any message was already written).
fn parse_opts(args: &[String]) -> Result<Opts, ()> {
    let mut o = Opts::default();
    let mut it = args.iter();
    while let Some(a) = it.next() {
        let mut value = |flag: &str| -> Result<String, ()> {
            it.next().cloned().ok_or_else(|| stderr(&format!("kerf: {flag} needs a value\n")))
        };
        match a.as_str() {
            "--view" => o.view = Some(value(a)?),
            "--format" => o.format = Some(value(a)?),
            "--style" => o.style = Some(value(a)?),
            "-o" => o.out = Some(value(a)?),
            "--sheet" => o.sheet = true,
            "--markdown" => o.markdown = true,
            "--full" => o.full = true,
            "-w" => o.write = true,
            "--ops" => o.ops = Some(value(a)?),
            "--id" => o.id = Some(value(a)?),
            "--title" => o.title = Some(value(a)?),
            "--px" => o.px = Some(value(a)?),
            "--why" => o.why = Some(value(a)?),
            "--dry-run" => o.dry_run = true,
            "--if-match" => o.if_match = Some(value(a)?),
            "--template" => o.template = Some(value(a)?),
            "-h" | "--help" => return Err(()),
            s if s.len() > 1 && s.starts_with('-') => {
                stderr(&format!("kerf: unknown option {s}\n"));
                return Err(());
            }
            _ => {
                if o.pos.len() >= 16 {
                    return Err(());
                }
                o.pos.push(a.clone());
            }
        }
    }
    Ok(o)
}

/// `{"doc": <file>, "style": <file>?, ...extra}` from raw document text.
fn build_input(doc: &[u8], style_path: Option<&str>, extra: &str) -> Res<Vec<u8>> {
    let mut out = b"{\"doc\":".to_vec();
    out.extend_from_slice(trim_ws(doc));
    if let Some(sp) = style_path {
        let st = read_file(sp)?;
        out.extend_from_slice(b",\"style\":");
        out.extend_from_slice(trim_ws(&st));
    }
    if !extra.is_empty() {
        out.push(b',');
        out.extend_from_slice(extra.as_bytes());
    }
    out.push(b'}');
    Ok(out)
}

fn json_string(s: &str) -> String {
    let mut b = Vec::new();
    json::write_string(&mut b, s);
    String::from_utf8_lossy(&b).into_owned()
}

/// Appends the op-log line. The document is already written (atomically), so a failure here is reported on stderr
/// (what failed, that the document is fine) and the caller exits 3. Returns true when the log line was appended.
fn log_write(doc_path: &str, lock: Option<&mut DocLock>, meta: &LogMeta, ops_text: &str, changed: &[String], summary: &str) -> bool {
    let lp = workspace::log_path(doc_path);
    let line = workspace::build_entry(meta, ops_text, changed, summary);
    let res = match lock {
        Some(lk) => lk.append_line(&line),
        None => workspace::append_line(&lp, &line),
    };
    match res {
        Ok(()) => true,
        Err(e) => {
            let name = if e.to_string() == "SymLink" { "SymLink".to_string() } else { Fail::from(e).0 };
            stderr(&format!("kerf: error: could not append to op log {lp}: {name}. The document itself WAS written; only the log entry is missing. Exit code 3.\n"));
            false
        }
    }
}

/// The server's ETag of a document: `"<mtime_ms>-<size>-<wyhash64 hex16 of the bytes>"`.
fn etag_of(mtime_ms: i128, size: u64, bytes: &[u8]) -> String {
    format!("\"{mtime_ms}-{size}-{:016x}\"", hash::wyhash(0, bytes))
}

pub fn mtime_ms(meta: &fs::Metadata) -> i128 {
    use std::time::UNIX_EPOCH;
    match meta.modified().ok().map(|t| t.duration_since(UNIX_EPOCH)) {
        Some(Ok(d)) => d.as_millis() as i128,
        Some(Err(e)) => -(e.duration().as_millis() as i128),
        None => 0,
    }
}

/// Does `want` (an ETag with or without quotes, or just the 16-hex content hash) name the current content of `doc_path`?
fn if_match_ok(doc_path: &str, want: &str) -> Res<bool> {
    let bytes = read_file(doc_path)?;
    let meta = fs::metadata(doc_path)?;
    let have = etag_of(mtime_ms(&meta), meta.len(), &bytes);
    let w = want.trim_matches(['"', ' ']);
    if w == have.trim_matches('"') {
        return Ok(true);
    }
    Ok(w == format!("{:016x}", hash::wyhash(0, &bytes)))
}

fn create_op(file: &str, id: &str, title: &str) -> String {
    format!("[{{\"op\":\"create\",\"file\":{},\"id\":{},\"title\":{}}}]", json_string(file), json_string(id), json_string(title))
}

/// `ERROR <code> <path>: <message>  Fix: <fix>` (SPEC 19). `path` falls back to the diagnostic id, then `-`.
fn diag_error_line(d: &Value) -> String {
    let field = |k: &str| d.get(k).and_then(Value::str).filter(|s| !s.is_empty());
    let code = d.get("code").and_then(Value::str).unwrap_or("E_UNKNOWN");
    let path = field("path").or_else(|| field("id")).unwrap_or("-");
    let msg = d.get("message").and_then(Value::str).unwrap_or("");
    let mut line = format!("ERROR {code} {path}: {msg}");
    if let Some(fix) = field("fix") {
        line.push_str("  Fix: ");
        line.push_str(fix);
    }
    line.push('\n');
    line
}

fn print_error_diags(res: &Value) {
    for d in res.get("diagnostics").and_then(Value::arr).unwrap_or(&[]) {
        if d.get("level").and_then(Value::str) == Some("error") {
            stderr(&diag_error_line(d));
        }
    }
    stderr("nothing written\n");
}

struct PreparedOps {
    /// Compact JSON array of ops (what the engine and the op log get).
    text: String,
    /// The `why` of an `{"ops":[...],"why":"..."}` envelope.
    why: Option<String>,
}

/// Parse the ops text and normalize it to an ops array (SPEC 21: array, single op, or `{"ops":[...],"why":"..."}`).
/// On failure prints the error (`nothing written`) and returns None.
fn prepare_ops(raw: &[u8]) -> Option<PreparedOps> {
    let parsed = match json::parse(trim_ws(raw)) {
        Ok(v) => v,
        Err(e) => {
            stderr(&format!(
                "ERROR E_JSON ops: the ops are not valid JSON: {} (line {}, column {})  Fix: pass a JSON array of ops, e.g. [{{\"op\":\"update\",\"path\":\"components/x\",\"value\":{{...}}}}]; on PowerShell write the ops to a file\nnothing written\n",
                e.msg, e.line, e.col
            ));
            return None;
        }
    };
    let diags = model::Diags::new();
    let Some(norm) = ops::normalize(&parsed, &diags) else {
        for d in diags.list().iter() {
            stderr(&diag_error_line(&model::diag_to_json(d)));
        }
        stderr("nothing written\n");
        return None;
    };
    let mut out = Vec::new();
    json::write_compact(&mut out, &norm.ops);
    Some(PreparedOps { text: String::from_utf8_lossy(&out).into_owned(), why: norm.why })
}

/// Print an engine failure `{"error": {code, message}}` as `ERROR <code>: <message>`; falls back to the raw bytes.
fn print_api_error(bytes: &[u8]) {
    if let Ok(v) = json::parse(bytes) {
        if let Some(e) = v.get("error") {
            let code = e.get("code").and_then(Value::str).unwrap_or("E_UNKNOWN");
            let msg = e.get("message").and_then(Value::str).unwrap_or("");
            stderr(&format!("ERROR {code}: {msg}\n"));
            return;
        }
    }
    let _ = io::stderr().write_all(bytes);
}

fn cmd_version() -> Res<u8> {
    let r = api::call(b"version", b"{}");
    write_out(None, &r.bytes)?;
    Ok(0)
}

fn cmd_catalog(o: &Opts) -> Res<u8> {
    let r = api::call(b"catalog", if o.markdown { b"{\"format\":\"markdown\"}" } else { b"{\"format\":\"json\"}" });
    if o.markdown && r.ok {
        write_out(o.out.as_deref(), &ascii_fold(&r.bytes)?)?;
        return Ok(0);
    }
    write_out(o.out.as_deref(), &r.bytes)?;
    Ok(u8::from(!r.ok))
}

fn cmd_schema(o: &Opts) -> Res<u8> {
    let mut text = String::new();
    let mut failed = false;
    let topics: Vec<Option<&str>> = if o.pos.is_empty() { vec![None] } else { o.pos.iter().map(|s| Some(s.as_str())).collect() };
    for topic in topics {
        let input = match topic {
            Some(t) => format!("{{\"topic\":{}}}", json_string(t)),
            None => "{}".to_string(),
        };
        let r = api::call(b"schema", input.as_bytes());
        if !r.ok {
            print_api_error(&r.bytes);
            failed = true;
            continue;
        }
        let res = json::parse(&r.bytes).map_err(|_| bad_engine_output())?;
        let t = res.get("text").and_then(Value::str).ok_or_else(bad_engine_output)?;
        if !text.is_empty() {
            text.push_str("\n----\n\n");
        }
        text.push_str(t);
    }
    write_out(o.out.as_deref(), &ascii_fold(text.as_bytes())?)?;
    Ok(u8::from(failed))
}

fn cmd_guide(o: &Opts) -> Res<u8> {
    let (short, full) = guide_split(CLI_GUIDE);
    let mut text = String::from(short);
    if o.full {
        let r = api::call(b"catalog", b"{\"format\":\"markdown\"}");
        text.push('\n');
        text.push_str(full);
        text.push('\n');
        text.push_str(&schema::guide_section());
        text.push_str("\n# Drafting instructions\n\n");
        text.push_str(SYSTEM_MD);
        text.push_str("\n# Component catalog\n\n");
        text.push_str(&String::from_utf8_lossy(&r.bytes));
    } else {
        text.push('\n');
        text.push_str(&schema::compact_section());
        text.push_str("\nMore: `kerf guide --full` (drafting rules, long-form notes, full schema and component catalog).\n");
    }
    write_out(o.out.as_deref(), &ascii_fold(text.as_bytes())?)?;
    Ok(0)
}

const AGENTS_MD: &str = "# Kerf details library

This folder holds construction details as `*.kerf.json` files, built with the `kerf` CLI.

Before creating or editing any detail, run `kerf guide` and follow it exactly. It holds the
workflow, drafting rules, note grammar, and component catalog. Edit details only via
`kerf apply <file> ... -w`, never by hand. After changes, export a PNG
(`kerf export <file> --view A --format png -o <file>-A.png`) and look at it before reporting.
Shell rules: ONE kerf command per tool call (no `&&`, `;`, pipes or heredocs); create ops files with your file-writing tool.
The designer reviews and exports in the Kerf web UI.
";

fn cmd_init(o: &Opts) -> Res<u8> {
    let dir = o.pos.first().map_or(".", String::as_str);
    let _ = fs::create_dir_all(dir);
    if !fs::metadata(dir)?.is_dir() {
        return Err(Fail("NotDir".to_string()));
    }
    for name in ["AGENTS.md", "CLAUDE.md"] {
        let p = std::path::Path::new(dir).join(name);
        if p.exists() {
            say(&format!("kept existing {name}\n"))?;
        } else {
            fs::write(&p, if name == "CLAUDE.md" { "@AGENTS.md\n" } else { AGENTS_MD })?;
            say(&format!("wrote {name}\n"))?;
        }
    }
    say("ready: open Claude Code / Grok in this folder and ask for a detail.\n")?;
    Ok(0)
}

fn cmd_new(actor: &str, o: &Opts) -> Res<u8> {
    let Some(path) = o.pos.first() else {
        stderr("kerf new: needs <file>, e.g. kerf new truss-cmu.kerf.json --title \"TRUSS BEARING\"\n");
        return Ok(2);
    };
    if fs::metadata(path).is_ok() {
        stderr(&format!("kerf new: {path} already exists (refusing to overwrite)\n"));
        return Ok(1);
    }
    let stem = workspace::doc_stem(path);
    let id_src = o.id.as_deref().unwrap_or(stem);
    let title = o.title.as_deref().unwrap_or("");
    let text = if let Some(tpl) = &o.template {
        if tpl != "section" {
            stderr(&format!("kerf new: unknown template \"{tpl}\"; available: section (a section view with two members, two notes, a dim, a label)\n"));
            return Ok(2);
        }
        schema::template_text(&workspace::slug(id_src), title).ok_or_else(|| Fail("BadDoc".to_string()))?
    } else {
        workspace::new_doc_text(id_src, title).ok_or_else(|| Fail("BadDoc".to_string()))?
    };
    let mut final_text = text.clone();
    let mut ops_text: Option<String> = None;
    let mut why = String::new();
    let mut apply_changed: Vec<String> = Vec::new();
    let mut apply_summary = String::new();
    if let Some(oa) = &o.ops {
        let Ok(raw) = ops_from_arg(oa) else {
            stderr(&format!("kerf new: cannot read ops '{oa}' (--ops takes a file path, or inline JSON starting with [)\n"));
            return Ok(2);
        };
        let Some(prep) = prepare_ops(&raw) else { return Ok(1) };
        why = o.why.clone().or(prep.why).unwrap_or_default();
        let input = build_input(&text, o.style.as_deref(), &format!("\"ops\":{}", prep.text))?;
        ops_text = Some(prep.text);
        let r = api::call(b"apply", &input);
        if !r.ok {
            print_api_error(&r.bytes);
            stderr("nothing written\n");
            return Ok(1);
        }
        let res = json::parse(&r.bytes).map_err(|_| bad_engine_output())?;
        let summary = res.get("summary").and_then(Value::str).unwrap_or("");
        if !matches!(res.get("ok"), Some(Value::Bool(true))) {
            print_error_diags(&res);
            return Ok(1);
        }
        final_text = canon::write(res.get("doc").ok_or_else(bad_engine_output)?);
        for it in res.get("changed").and_then(Value::arr).unwrap_or(&[]) {
            if let Some(c) = it.str() {
                apply_changed.push(c.to_string());
            }
        }
        apply_summary = summary.to_string();
        say(summary)?;
    }
    workspace::write_file_atomic(path, &final_text)?;
    let create = create_op(
        std::path::Path::new(path).file_name().and_then(|n| n.to_str()).unwrap_or(path),
        id_src,
        title,
    );
    let mut logged = log_write(path, None, &LogMeta { who: actor, tool: TOOL, why: "create" }, &create, &[], &workspace::check_summary(&text));
    if let Some(ot) = &ops_text {
        logged &= log_write(path, None, &LogMeta { who: actor, tool: TOOL, why: &why }, ot, &apply_changed, &apply_summary);
    }
    say(&format!("created {path}"))?;
    say(if ops_text.is_some() {
        "\nnext: LOOK at it: kerf export <file> --view A --format png -o <file>-A.png\n"
    } else if o.template.is_some() {
        "\nnext: edit it with kerf apply <file> ops.json -w   (see `kerf schema`, `kerf guide`)\n"
    } else {
        "\nnext: kerf apply <file> ops.json -w   (see `kerf guide`)\n"
    })?;
    Ok(if logged { 0 } else { 3 })
}

fn cmd_call(o: &Opts) -> Res<u8> {
    let Some(name) = o.pos.first() else {
        stderr(USAGE);
        return Ok(2);
    };
    // `kerf call help` needs no input: do not block on an interactive stdin
    let input = if name == "help" { b"{}".to_vec() } else { read_stdin()? };
    let r = api::call(name.as_bytes(), &input);
    write_out(o.out.as_deref(), &r.bytes)?;
    Ok(u8::from(!r.ok))
}

/// What a document command sends to the engine besides the document: the extra JSON members and, for apply, the ops
/// text and the `why` of an `{"ops":[...],"why":"..."}` envelope (the op log needs both after the call).
#[derive(Default)]
struct DocCall {
    extra: String,
    ops_text: String,
    why: Option<String>,
}

/// Check the command line of a document command and fill `call`. Returns an exit code (after printing why) when it is wrong.
fn prepare_doc_call(cmd: &str, o: &Opts, call: &mut DocCall) -> Res<Option<u8>> {
    call.ops_text = "[]".to_string();
    match cmd {
        "drawing" => {
            let Some(v) = &o.view else {
                stderr("kerf drawing: --view <id> is required\n");
                return Ok(Some(2));
            };
            call.extra = format!("\"view\":{}", json_string(v));
        }
        "export" => {
            let Some(v) = &o.view else {
                stderr("kerf export: --view <id> is required\n");
                return Ok(Some(2));
            };
            let f = o.format.as_deref().unwrap_or("svg");
            call.extra = format!("\"view\":{},\"format\":{},\"sheet\":{}", json_string(v), json_string(f), o.sheet);
            if let Some(px) = &o.px {
                let Some(pxn) = units::parse_finite(px) else {
                    stderr(&format!("kerf export: --px must be a number (got \"{px}\")\n"));
                    return Ok(Some(2));
                };
                let mut pb = [0u8; 40];
                call.extra.push_str(&format!(",\"px\":{}", json::fmt_number(&mut pb, pxn)));
            }
            if o.out.is_none() {
                stderr("kerf export: -o <file> is required\n");
                return Ok(Some(2));
            }
        }
        "apply" => {
            let ops: Vec<u8> = if let Some(arg) = &o.ops {
                let Ok(b) = ops_from_arg(arg) else {
                    stderr(&format!("kerf apply: cannot read ops '{arg}' (--ops takes inline JSON starting with [, or a file path)\n"));
                    return Ok(Some(2));
                };
                b
            } else if o.pos.len() >= 2 && o.pos[1] != "-" {
                read_file(&o.pos[1])?
            } else if o.pos.len() >= 2 {
                read_stdin()?
            } else {
                stderr("kerf apply: needs ops: <doc> <ops.json>, <doc> - (stdin), or --ops '<json>'\n");
                return Ok(Some(2));
            };
            if o.dry_run && o.write {
                stderr("kerf apply: --dry-run and -w contradict each other (dry-run writes nothing)\n");
                return Ok(Some(2));
            }
            let Some(prep) = prepare_ops(&ops) else { return Ok(Some(1)) };
            if o.why.is_none() {
                call.why = prep.why;
            }
            call.extra = format!("\"ops\":{}", prep.text);
            call.ops_text = prep.text;
        }
        "fmt" | "check" | "mesh" => {}
        _ => {
            stderr(&format!("kerf: unknown command '{cmd}'\n"));
            stderr(USAGE);
            return Ok(Some(2));
        }
    }
    Ok(None)
}

/// `apply -w` / `fmt -w` read, edit and write under the advisory lock `kerf serve` uses for the same document, so parallel
/// writers queue instead of silently losing each other's edits; --if-match makes a stale writer fail instead of overwriting.
fn lock_for_write(cmd: &str, o: &Opts, doc_path: &str) -> Res<Result<Option<DocLock>, u8>> {
    let is_apply_w = cmd == "apply" && o.write && !o.dry_run;
    if o.if_match.is_some() && !is_apply_w {
        stderr("kerf: --if-match only applies to `kerf apply <doc> ... -w`\n");
        return Ok(Err(2));
    }
    if !(is_apply_w || (cmd == "fmt" && o.write)) {
        return Ok(Ok(None));
    }
    let lp = workspace::log_path(doc_path);
    let lock = match DocLock::acquire(&lp) {
        Ok(l) => l,
        Err(e) => {
            let name = if e.to_string() == "SymLink" { "SymLink".to_string() } else { Fail::from(e).0 };
            stderr(&format!("kerf {cmd}: cannot lock {lp} for writing: {name}\n"));
            return Ok(Err(1));
        }
    };
    if let Some(want) = &o.if_match {
        match if_match_ok(doc_path, want) {
            Err(Fail(name)) => {
                stderr(&format!("kerf apply: cannot read {doc_path} to check --if-match: {name}\n"));
                return Ok(Err(1));
            }
            Ok(false) => {
                stderr(&format!("ERROR E_STALE {doc_path}: --if-match {want} does not match the document as it is now (it changed since you read it)  Fix: read {doc_path} again (kerf check / cat), rebuild your ops against the current content and retry\nnothing written\n"));
                return Ok(Err(1));
            }
            Ok(true) => {}
        }
    }
    Ok(Ok(Some(lock)))
}

fn finish_fmt(o: &Opts, doc_path: &str, bytes: &[u8]) -> Res<u8> {
    let res = json::parse(bytes).map_err(|_| bad_engine_output())?;
    let text = res.get("text").and_then(Value::str).ok_or_else(bad_engine_output)?;
    if o.write {
        workspace::write_file_atomic(doc_path, text.as_bytes())?;
    } else {
        write_out(o.out.as_deref(), text.as_bytes())?;
    }
    Ok(0)
}

/// Report the result of `apply` or `check` (summary, errors on stderr) and, for `apply -w`, write the document and its log line.
fn finish_apply_check(actor: &str, cmd: &str, o: &Opts, doc_path: &str, lock: Option<&mut DocLock>, call: &DocCall, bytes: &[u8]) -> Res<u8> {
    let res = json::parse(bytes).map_err(|_| bad_engine_output())?;
    let summary = res.get("summary").and_then(Value::str).unwrap_or("");
    if let Some(txt) = res.get("summary").and_then(Value::str) {
        say(txt)?;
    }
    if cmd == "check" {
        let has_error = res
            .get("diagnostics")
            .and_then(Value::arr)
            .unwrap_or(&[])
            .iter()
            .any(|d| d.get("level").and_then(Value::str) == Some("error"));
        return Ok(u8::from(has_error));
    }
    if !matches!(res.get("ok"), Some(Value::Bool(true))) {
        print_error_diags(&res);
        return Ok(1);
    }
    let mut logged = true;
    if let Some(dv) = res.get("doc") {
        let text = canon::write(dv);
        if o.write && !o.dry_run {
            workspace::write_file_atomic(doc_path, &text)?;
            let changed: Vec<String> =
                res.get("changed").and_then(Value::arr).unwrap_or(&[]).iter().filter_map(|c| c.str().map(String::from)).collect();
            let why = o.why.as_deref().or(call.why.as_deref()).unwrap_or("");
            logged = log_write(doc_path, lock, &LogMeta { who: actor, tool: TOOL, why }, &call.ops_text, &changed, summary);
            say(&format!("wrote {doc_path}\n"))?;
        } else if o.dry_run {
            say("(dry run: nothing written)\n")?;
        } else if o.out.is_some() {
            write_out(o.out.as_deref(), &text)?;
        } else {
            say("(dry run: add -w to write the result back to the document)\n")?;
        }
    }
    Ok(if logged { 0 } else { 3 })
}

/// The commands that take a document: drawing, export, apply, check, fmt, mesh (anything else is an unknown command).
fn cmd_doc(actor: &str, cmd: &str, o: &Opts) -> Res<u8> {
    let Some(doc_path) = o.pos.first() else {
        stderr(&format!("kerf {cmd}: missing <doc>\n"));
        stderr(USAGE);
        return Ok(2);
    };
    let mut call = DocCall::default();
    if let Some(code) = prepare_doc_call(cmd, o, &mut call)? {
        return Ok(code);
    }
    let mut lock = match lock_for_write(cmd, o, doc_path)? {
        Ok(l) => l,
        Err(code) => return Ok(code),
    };
    let input = build_input(&read_file(doc_path)?, o.style.as_deref(), &call.extra)?;
    let r = api::call(cmd.as_bytes(), &input);
    if !r.ok {
        print_api_error(&r.bytes);
        if cmd == "apply" {
            stderr("nothing written\n");
        }
        return Ok(1);
    }
    match cmd {
        "fmt" => finish_fmt(o, doc_path, &r.bytes),
        "apply" | "check" => finish_apply_check(actor, cmd, o, doc_path, lock.as_mut(), &call, &r.bytes),
        _ => {
            write_out(o.out.as_deref(), &r.bytes)?;
            Ok(0)
        }
    }
}

fn run(args: &[String], actor: &str) -> Res<u8> {
    let Some(cmd) = args.first() else {
        stderr(USAGE);
        return Ok(2);
    };
    if cmd == "serve" {
        stderr("kerf serve: not built into this engine yet\n");
        return Ok(2);
    }
    let Ok(o) = parse_opts(&args[1..]) else {
        stderr(USAGE);
        return Ok(2);
    };
    match cmd.as_str() {
        "version" | "--version" | "-V" => cmd_version(),
        "catalog" => cmd_catalog(&o),
        "schema" => cmd_schema(&o),
        "help" | "--help" | "-h" => say(USAGE).map(|()| 0),
        "guide" => cmd_guide(&o),
        "init" => cmd_init(&o),
        "new" => cmd_new(actor, &o),
        "call" => cmd_call(&o),
        _ => cmd_doc(actor, cmd, &o),
    }
}
