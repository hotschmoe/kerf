//! Drives `kerf mcp` over raw stdio JSON-RPC: handshake, tools, render (PNG), export (DXF audit).
//! Set KERF_MCP_PNG_DIR to keep the PNG renders of the three reference details for eyeballing.

#![cfg(feature = "mcp")]

use serde_json::{Value, json};
use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStdin, ChildStdout, Command, Stdio};

struct Client {
    child: Child,
    stdin: ChildStdin,
    out: BufReader<ChildStdout>,
    next: i64,
}

impl Client {
    fn start(dir: &Path) -> Client {
        let mut child = Command::new(env!("CARGO_BIN_EXE_kerf"))
            .args(["mcp", "--dir"])
            .arg(dir)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .expect("spawn kerf mcp");
        let stdin = child.stdin.take().unwrap();
        let out = BufReader::new(child.stdout.take().unwrap());
        Client { child, stdin, out, next: 1 }
    }

    fn send(&mut self, v: &Value) {
        writeln!(self.stdin, "{}", v).unwrap();
        self.stdin.flush().unwrap();
    }

    fn rpc(&mut self, method: &str, params: Value) -> Value {
        let id = self.next;
        self.next += 1;
        self.send(&json!({"jsonrpc": "2.0", "id": id, "method": method, "params": params}));
        let mut line = String::new();
        self.out.read_line(&mut line).unwrap();
        let v: Value = serde_json::from_str(&line).unwrap_or_else(|e| panic!("bad JSON-RPC line {:?}: {}", line, e));
        assert_eq!(v["id"], json!(id), "{}", line);
        v
    }

    /// tools/call; returns (isError, content).
    fn tool(&mut self, name: &str, args: Value) -> (bool, Vec<Value>) {
        let r = self.rpc("tools/call", json!({"name": name, "arguments": args}));
        let res = &r["result"];
        assert!(res.is_object(), "protocol error for {}: {}", name, r);
        (res["isError"].as_bool().unwrap(), res["content"].as_array().unwrap().clone())
    }

    fn tool_text(&mut self, name: &str, args: Value) -> String {
        let (is_err, c) = self.tool(name, args);
        let t = c[0]["text"].as_str().unwrap().to_string();
        assert!(!is_err, "{} failed: {}", name, t);
        t
    }
}

impl Drop for Client {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

fn b64_decode(s: &str) -> Vec<u8> {
    let val = |c: u8| match c {
        b'A'..=b'Z' => c - b'A',
        b'a'..=b'z' => c - b'a' + 26,
        b'0'..=b'9' => c - b'0' + 52,
        b'+' => 62,
        b'/' => 63,
        _ => 0,
    };
    let b = s.as_bytes();
    let mut out = vec![];
    for ch in b.chunks(4) {
        let n = (val(ch[0]) as u32) << 18 | (val(ch[1]) as u32) << 12 | (val(ch[2]) as u32) << 6 | val(ch[3]) as u32;
        out.push((n >> 16) as u8);
        if ch[2] != b'=' {
            out.push((n >> 8) as u8);
        }
        if ch[3] != b'=' {
            out.push(n as u8);
        }
    }
    out
}

fn png_size(png: &[u8]) -> (u32, u32) {
    assert_eq!(&png[..8], b"\x89PNG\r\n\x1a\n", "not a PNG");
    assert_eq!(&png[12..16], b"IHDR");
    (u32::from_be_bytes(png[16..20].try_into().unwrap()), u32::from_be_bytes(png[20..24].try_into().unwrap()))
}

fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../..").canonicalize().unwrap()
}

fn workdir(name: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("kerf-mcp-test-{}-{}", name, std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    d
}

fn init(c: &mut Client) {
    let r = c.rpc("initialize", json!({"protocolVersion": "2025-06-18", "capabilities": {}, "clientInfo": {"name": "test", "version": "0"}}));
    assert_eq!(r["result"]["protocolVersion"], "2025-06-18");
    assert_eq!(r["result"]["serverInfo"]["name"], "kerf");
    assert!(r["result"]["capabilities"]["tools"].is_object());
    c.send(&json!({"jsonrpc": "2.0", "method": "notifications/initialized"}));
}

fn small_doc() -> Value {
    json!({
        "kerf": "0.1", "id": "small", "title": "STUD AT PLATE",
        "components": [
            {"id": "plate", "type": "lumber", "size": "2x4", "run": "z", "orient": "flat"},
            {"id": "stud", "type": "lumber", "size": "2x4", "run": "z", "at": {"anchor": "bottom_left", "to": "plate@top_left"}}
        ],
        "views": [{
            "id": "A", "kind": "section", "scale": "3\"=1'-0\"", "crop": {"x": [-3, 6], "y": [-3, 7]},
            "annotations": [
                {"id": "n1", "type": "note", "text": "2X4 STUD @ 16\" O.C.", "target": "stud",
                 "cite": [{"code": "IRC", "edition": 2021, "section": "R602.3", "title": "Design and construction", "status": "verified"}]},
                {"id": "n2", "type": "note", "text": "2X4 PLATE", "target": "plate"}
            ]
        }]
    })
}

#[test]
fn handshake_tools_prompts_resources() {
    let dir = workdir("proto");
    let mut c = Client::start(&dir);
    init(&mut c);
    let tools = c.rpc("tools/list", json!({}));
    let names: Vec<&str> = tools["result"]["tools"].as_array().unwrap().iter().map(|t| t["name"].as_str().unwrap()).collect();
    assert_eq!(names, ["kerf_new", "kerf_open", "kerf_list", "kerf_apply", "kerf_inspect", "kerf_render", "kerf_export", "kerf_catalog"]);
    for t in tools["result"]["tools"].as_array().unwrap() {
        assert_eq!(t["inputSchema"]["type"], "object");
        assert!(t["description"].as_str().unwrap().len() > 40);
    }
    // prompts
    assert_eq!(c.rpc("prompts/list", json!({}))["result"]["prompts"][0]["name"], "kerf_detail");
    let p = c.rpc("prompts/get", json!({"name": "kerf_detail", "arguments": {"request": "flush beam"}}));
    let msgs = p["result"]["messages"].as_array().unwrap();
    assert_eq!(msgs.len(), 2);
    let body = msgs[0]["content"]["text"].as_str().unwrap();
    assert!(body.contains("drafting engine operator") && body.contains("# Component catalog") && body.contains("cmu_wall"));
    // resources
    let rl = c.rpc("resources/list", json!({}));
    let uris: Vec<&str> = rl["result"]["resources"].as_array().unwrap().iter().map(|r| r["uri"].as_str().unwrap()).collect();
    assert!(uris.contains(&"kerf://catalog") && uris.contains(&"kerf://style/kerf-standard"));
    let st = c.rpc("resources/read", json!({"uri": "kerf://style/kerf-standard"}));
    assert!(serde_json::from_str::<Value>(st["result"]["contents"][0]["text"].as_str().unwrap()).is_ok());
    assert_eq!(c.rpc("resources/read", json!({"uri": "kerf://doc/nope"}))["error"]["code"], -32002);
    // errors
    assert_eq!(c.rpc("nope/nothing", json!({}))["error"]["code"], -32601);
    assert_eq!(c.rpc("tools/call", json!({"name": "kerf_nope", "arguments": {}}))["error"]["code"], -32602);
    assert_eq!(c.rpc("ping", json!({}))["result"], json!({}));
    // modern era
    let d = c.rpc("server/discover", json!({"_meta": {"io.modelcontextprotocol/protocolVersion": "2026-07-28"}}));
    assert_eq!(d["result"]["supportedVersions"][0], "2026-07-28");
    assert_eq!(c.rpc("tools/list", json!({"_meta": {"io.modelcontextprotocol/protocolVersion": "1900-01-01"}}))["error"]["code"], -32022);
    // garbage line is a parse error, not a crash
    writeln!(c.stdin, "{{not json").unwrap();
    let mut line = String::new();
    c.out.read_line(&mut line).unwrap();
    assert_eq!(serde_json::from_str::<Value>(&line).unwrap()["error"]["code"], -32700);
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn build_render_export_small_detail() {
    let dir = workdir("small");
    let mut c = Client::start(&dir);
    init(&mut c);

    // no doc yet: actionable error
    let (e, t) = c.tool("kerf_apply", json!({"ops": [{"op": "update", "path": "meta", "value": {}}], "why": "x"}));
    assert!(e && t[0]["text"].as_str().unwrap().contains("kerf_new"));

    let t = c.tool_text("kerf_new", json!({"id": "small", "title": "STUD AT PLATE"}));
    assert!(t.contains("small.kerf.json"));
    let (e, _) = c.tool("kerf_new", json!({"id": "small", "title": "again"}));
    assert!(e, "duplicate id must fail");
    let (e, _) = c.tool("kerf_new", json!({"id": "../evil", "title": "x"}));
    assert!(e, "path traversal id must fail");

    // a failing batch leaves the file untouched and explains itself
    let before = std::fs::read_to_string(dir.join("small.kerf.json")).unwrap();
    let (e, t) = c.tool("kerf_apply", json!({"ops": [{"op": "add", "path": "components", "value": {"id": "x", "type": "lumbr", "size": "2x4"}}], "why": "typo"}));
    assert!(e);
    let msg = t[0]["text"].as_str().unwrap();
    assert!(msg.starts_with("FAILED") && msg.contains("Did you mean \"lumber\""), "{}", msg);
    assert_eq!(before, std::fs::read_to_string(dir.join("small.kerf.json")).unwrap());

    // build the detail; the LLM path must never produce a verified citation
    let t = c.tool_text("kerf_apply", json!({"ops": [{"op": "set", "path": "doc", "value": small_doc()}], "why": "Build stud at plate"}));
    assert!(t.starts_with("ok:") && t.contains("plate") && t.contains("stud"), "{}", t);
    let saved: Value = serde_json::from_str(&std::fs::read_to_string(dir.join("small.kerf.json")).unwrap()).unwrap();
    let cite = &saved["views"][0]["annotations"][0]["cite"][0];
    assert_eq!(cite["status"], "suggested", "LLM must not verify citations: {}", cite);
    // small follow-up op + op log
    c.tool_text("kerf_apply", json!({"ops": [{"op": "update", "path": "components/plate", "value": {"treated": true}}], "why": "PT plate"}));
    let log = std::fs::read_to_string(dir.join("small.kerf.log.jsonl")).unwrap();
    let entries: Vec<Value> = log.lines().map(|l| serde_json::from_str(l).unwrap()).collect();
    assert_eq!(entries.len(), 3); // new, set, update
    assert_eq!(entries[2]["why"], "PT plate");
    assert_eq!(entries[2]["who"], "CLAUDE");

    // inspect
    let t = c.tool_text("kerf_inspect", json!({"q": "anchors", "id": "stud"}));
    assert!(t.contains("top_left"), "{}", t);
    assert!(c.tool_text("kerf_inspect", json!({"q": "summary"})).contains("Views: A"));
    assert!(c.tool_text("kerf_catalog", json!({"type": "truss"})).contains("heel"));
    assert!(c.tool_text("kerf_catalog", json!({})).contains("cmu_wall"));
    let (e, _) = c.tool("kerf_catalog", json!({"type": "trus"}));
    assert!(e);

    // render: image content (PNG, 1400 px wide) + one-line text
    let (e, content) = c.tool("kerf_render", json!({"view": "A"}));
    assert!(!e);
    assert_eq!(content[0]["type"], "image");
    assert_eq!(content[0]["mimeType"], "image/png");
    let png = b64_decode(content[0]["data"].as_str().unwrap());
    let (w, h) = png_size(&png);
    assert_eq!(w, 1400);
    assert!(h > 300, "{}x{}", w, h);
    let line = content[1]["text"].as_str().unwrap();
    assert!(line.contains("3\" = 1'-0\"") && line.contains("2 note(s)"), "{}", line);
    let (e, content) = c.tool("kerf_render", json!({"view": "A", "mode": "sheet"}));
    assert!(!e && content[1]["text"].as_str().unwrap().starts_with("sheet A"));
    let (e, _) = c.tool("kerf_render", json!({"view": "Z"}));
    assert!(e);

    // export
    let t = c.tool_text("kerf_export", json!({"view": "A", "format": "dxf"}));
    let dxf = dir.join("exports/small-A.dxf");
    assert!(t.contains("small-A.dxf") && dxf.is_file(), "{}", t);
    c.tool_text("kerf_export", json!({"format": "pdf", "path": "sheets/small.pdf"}));
    assert!(std::fs::read(dir.join("sheets/small.pdf")).unwrap().starts_with(b"%PDF"));
    let (e, _) = c.tool("kerf_export", json!({"format": "svg", "path": "/etc/kerf-evil.svg"}));
    assert!(e, "export outside the workspace must fail");
    let (e, _) = c.tool("kerf_export", json!({"format": "svg", "path": "../escape.svg"}));
    assert!(e);

    // DXF audit with the repo's checker (skipped when the python venv is not set up)
    let checker = repo_root().join("tools/dxf_check.py");
    if checker.is_file() && repo_root().join("tools/.venv").is_dir() {
        let o = Command::new(&checker).arg(&dxf).output().expect("run dxf_check.py");
        assert!(o.status.success(), "dxf_check failed:\n{}\n{}", String::from_utf8_lossy(&o.stdout), String::from_utf8_lossy(&o.stderr));
    } else {
        eprintln!("SKIP dxf_check.py (tools/.venv missing; run tools/setup.sh)");
    }

    // list / open
    assert!(c.tool_text("kerf_list", json!({})).contains("small"));
    assert!(c.tool_text("kerf_open", json!({"path": "small"})).contains("Views: A"));
    assert!(c.tool("kerf_open", json!({"path": "ghost"})).0);
    drop(c);
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn reference_details_render_through_mcp() {
    let dir = workdir("ref");
    let mut c = Client::start(&dir);
    init(&mut c);
    let keep = std::env::var("KERF_MCP_PNG_DIR").ok().map(PathBuf::from);
    if let Some(k) = &keep {
        std::fs::create_dir_all(k).unwrap();
    }
    for name in ["truss-bearing-cmu", "flush-beam-strap", "monopour-slab-door-recess"] {
        let src = repo_root().join(format!("spec/details/{}.kerf.json", name));
        let doc: Value = serde_json::from_str(&std::fs::read_to_string(src).unwrap()).unwrap();
        c.tool_text("kerf_new", json!({"id": name, "title": "tmp"}));
        let t = c.tool_text("kerf_apply", json!({"ops": [{"op": "set", "path": "doc", "value": doc}], "why": "load reference"}));
        assert!(t.starts_with("ok:"), "{}: {}", name, t);
        let views: Vec<String> = doc["views"].as_array().unwrap().iter().map(|v| v["id"].as_str().unwrap().to_string()).collect();
        for v in views {
            let (e, content) = c.tool("kerf_render", json!({"view": v}));
            assert!(!e, "{} {}: {:?}", name, v, content);
            let png = b64_decode(content[0]["data"].as_str().unwrap());
            let (w, _) = png_size(&png);
            assert_eq!(w, 1400);
            if let Some(k) = &keep {
                std::fs::write(k.join(format!("{}-{}.png", name, v)), &png).unwrap();
                eprintln!("{} {}: {}", name, v, content[1]["text"].as_str().unwrap().lines().next().unwrap());
            }
        }
    }
    drop(c);
    let _ = std::fs::remove_dir_all(dir);
}
