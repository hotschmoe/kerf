//! JSON-RPC 2.0 / MCP over stdio, hand-rolled.
//!
//! Why not `rmcp`: the official Rust SDK pulls in tokio, schemars and proc macros for what is a
//! ~250 line request/response loop here, and our tool schemas are plain JSON (mirrored from
//! spec/llm/tools.json) rather than derived from Rust types. A synchronous loop keeps the binary
//! small, startup instant and the behaviour easy to audit.
//!
//! Protocol (checked against modelcontextprotocol.io):
//! * Legacy handshake (`initialize` + `notifications/initialized`), versions 2025-11-25,
//!   2025-06-18, 2025-03-26, 2024-11-05: we echo the client's version if we know it, else the latest.
//! * Modern per-request era (2026-07-28): `server/discover`, `_meta` protocol version on every
//!   request, `resultType: "complete"` on results, -32022 for an unsupported version.
//! * stdio framing: one JSON message per line; logs only on stderr.

use crate::store::{Workspace, doc_id_of};
use crate::{text, tools};
use serde_json::{Map, Value, json};
use std::io::{BufRead, Write};

const LEGACY_VERSIONS: [&str; 4] = ["2025-11-25", "2025-06-18", "2025-03-26", "2024-11-05"];
const MODERN_VERSION: &str = "2026-07-28";
const META_VERSION: &str = "io.modelcontextprotocol/protocolVersion";

const E_PARSE: i64 = -32700;
const E_INVALID_REQUEST: i64 = -32600;
const E_METHOD: i64 = -32601;
const E_PARAMS: i64 = -32602;
const E_NOT_FOUND: i64 = -32002;
const E_UNSUPPORTED_VERSION: i64 = -32022;

struct RpcError {
    code: i64,
    message: String,
    data: Option<Value>,
}

impl RpcError {
    fn new(code: i64, message: impl Into<String>) -> RpcError {
        RpcError { code, message: message.into(), data: None }
    }
}

pub struct Server {
    ws: Workspace,
}

fn capabilities() -> Value {
    json!({"tools": {"listChanged": false}, "prompts": {"listChanged": false}, "resources": {"subscribe": false, "listChanged": false}})
}

fn server_info() -> Value {
    json!({
        "name": "kerf",
        "title": "Kerf",
        "version": env!("CARGO_PKG_VERSION"),
        "description": "LLM-driven construction-detail CAD: edit a semantic document, render and export SVG/DXF/PDF."
    })
}

impl Server {
    pub fn new(ws: Workspace) -> Server {
        Server { ws }
    }

    pub fn serve_stdio(&mut self) {
        let stdin = std::io::stdin();
        let stdout = std::io::stdout();
        let mut out = stdout.lock();
        for line in stdin.lock().lines() {
            let Ok(line) = line else { break };
            if line.trim().is_empty() {
                continue;
            }
            if let Some(resp) = self.handle_line(&line) {
                if writeln!(out, "{}", resp).is_err() || out.flush().is_err() {
                    break;
                }
            }
        }
    }

    /// One incoming line -> the line to send back (None for notifications / responses).
    pub fn handle_line(&mut self, line: &str) -> Option<String> {
        let msg: Value = match serde_json::from_str(line) {
            Ok(v) => v,
            Err(e) => return Some(error_response(&Value::Null, RpcError::new(E_PARSE, format!("parse error: {}", e)))),
        };
        match msg {
            Value::Array(batch) => {
                let resps: Vec<Value> = batch.iter().filter_map(|m| self.handle_message(m)).collect();
                if resps.is_empty() { None } else { Some(Value::Array(resps).to_string()) }
            }
            m => self.handle_message(&m).map(|v| v.to_string()),
        }
    }

    fn handle_message(&mut self, msg: &Value) -> Option<Value> {
        let Some(obj) = msg.as_object() else {
            return Some(serde_json::from_str(&error_response(&Value::Null, RpcError::new(E_INVALID_REQUEST, "request must be a JSON object"))).unwrap());
        };
        let Some(method) = obj.get("method").and_then(|m| m.as_str()) else {
            return None; // a response from the client (we never send requests) or garbage: ignore
        };
        let id = obj.get("id").cloned();
        let params = obj.get("params").cloned().unwrap_or(Value::Null);
        let result = self.dispatch(method, &params);
        let id = id?; // notifications get no response
        Some(match result {
            Ok(mut r) => {
                if params.pointer("/_meta").is_some_and(|m| m.get(META_VERSION).is_some()) {
                    if let Some(o) = r.as_object_mut() {
                        o.entry("resultType").or_insert(json!("complete"));
                    }
                }
                json!({"jsonrpc": "2.0", "id": id, "result": r})
            }
            Err(e) => serde_json::from_str(&error_response(&id, e)).unwrap(),
        })
    }

    fn dispatch(&mut self, method: &str, params: &Value) -> Result<Value, RpcError> {
        if let Some(v) = params.pointer(&format!("/_meta/{}", META_VERSION.replace('/', "~1"))).and_then(|v| v.as_str()) {
            if v != MODERN_VERSION && !LEGACY_VERSIONS.contains(&v) {
                return Err(RpcError {
                    code: E_UNSUPPORTED_VERSION,
                    message: "Unsupported protocol version".into(),
                    data: Some(json!({"supported": [MODERN_VERSION, LEGACY_VERSIONS[0]], "requested": v})),
                });
            }
        }
        match method {
            "initialize" => {
                let want = params.get("protocolVersion").and_then(|v| v.as_str()).unwrap_or("");
                let ver = if LEGACY_VERSIONS.contains(&want) { want } else { LEGACY_VERSIONS[0] };
                Ok(json!({"protocolVersion": ver, "capabilities": capabilities(), "serverInfo": server_info(), "instructions": text::INSTRUCTIONS}))
            }
            "server/discover" => Ok(json!({
                "supportedVersions": [MODERN_VERSION],
                "capabilities": capabilities(),
                "_meta": {"io.modelcontextprotocol/serverInfo": server_info()},
                "instructions": text::INSTRUCTIONS
            })),
            "ping" => Ok(json!({})),
            "tools/list" => Ok(json!({"tools": tools::definitions()})),
            "tools/call" => self.tools_call(params),
            "prompts/list" => Ok(json!({"prompts": [{
                "name": "kerf_detail",
                "title": "Draft a construction detail",
                "description": "Kerf drafting instructions (how to build and edit details, note grammar, citation rules) plus the full component catalog. Load this before building a detail.",
                "arguments": [{"name": "request", "description": "What detail to build, in the designer's words (optional).", "required": false}]
            }]})),
            "prompts/get" => self.prompts_get(params),
            "resources/list" => Ok(self.resources_list()),
            "resources/templates/list" => Ok(json!({"resourceTemplates": [{
                "uriTemplate": "kerf://doc/{id}",
                "name": "Kerf document",
                "description": "A document in the workspace as canonical JSON (<id>.kerf.json).",
                "mimeType": "application/json"
            }]})),
            "resources/read" => self.resources_read(params),
            m if m.starts_with("notifications/") => Ok(Value::Null),
            other => Err(RpcError::new(E_METHOD, format!("method not found: {}", other))),
        }
    }

    fn tools_call(&mut self, params: &Value) -> Result<Value, RpcError> {
        let name = params.get("name").and_then(|n| n.as_str()).ok_or_else(|| RpcError::new(E_PARAMS, "tools/call needs params.name"))?;
        let empty = Value::Object(Map::new());
        let args = match params.get("arguments") {
            None | Some(Value::Null) => &empty,
            Some(a @ Value::Object(_)) => a,
            Some(_) => return Err(RpcError::new(E_PARAMS, "params.arguments must be an object")),
        };
        let out = tools::call(&mut self.ws, name, args).ok_or_else(|| RpcError::new(E_PARAMS, format!("Unknown tool: {}", name)))?;
        Ok(json!({"content": out.content, "isError": out.is_error}))
    }

    fn prompts_get(&mut self, params: &Value) -> Result<Value, RpcError> {
        let name = params.get("name").and_then(|n| n.as_str()).unwrap_or("");
        if name != "kerf_detail" {
            return Err(RpcError::new(E_PARAMS, format!("Unknown prompt: {:?} (available: kerf_detail)", name)));
        }
        let mut messages = vec![json!({"role": "user", "content": {"type": "text", "text": text::prompt_text()}})];
        if let Some(r) = params.pointer("/arguments/request").and_then(|r| r.as_str()).filter(|r| !r.trim().is_empty()) {
            messages.push(json!({"role": "user", "content": {"type": "text", "text": r}}));
        }
        Ok(json!({"description": "Kerf drafting instructions and component catalog", "messages": messages}))
    }

    fn resources_list(&self) -> Value {
        let mut r = vec![
            json!({"uri": "kerf://catalog", "name": "catalog", "title": "Kerf component catalog", "description": "Every component type with params, defaults and anchors (markdown).", "mimeType": "text/markdown"}),
            json!({"uri": "kerf://style/kerf-standard", "name": "kerf-standard", "title": "kerf-standard office style", "description": "Pens, hatches, fonts, layers and note grammar settings the engine uses by default.", "mimeType": "application/json"}),
        ];
        for p in self.ws.list() {
            let id = doc_id_of(&p);
            r.push(json!({"uri": format!("kerf://doc/{}", id), "name": id, "title": format!("{}.kerf.json", id), "description": "Kerf document (canonical JSON)", "mimeType": "application/json"}));
        }
        json!({"resources": r})
    }

    fn resources_read(&mut self, params: &Value) -> Result<Value, RpcError> {
        let uri = params.get("uri").and_then(|u| u.as_str()).ok_or_else(|| RpcError::new(E_PARAMS, "resources/read needs params.uri"))?;
        let (mime, body) = match uri {
            "kerf://catalog" => ("text/markdown", kerf_core::catalog::markdown()),
            "kerf://style/kerf-standard" => ("application/json", text::STYLE_JSON.to_string()),
            _ => {
                let id = uri.strip_prefix("kerf://doc/").ok_or_else(|| RpcError { code: E_NOT_FOUND, message: "Resource not found".into(), data: Some(json!({"uri": uri})) })?;
                let nf = || RpcError { code: E_NOT_FOUND, message: "Resource not found".into(), data: Some(json!({"uri": uri})) };
                let path = self.ws.path_for_id(id).map_err(|_| nf())?;
                ("application/json", std::fs::read_to_string(path).map_err(|_| nf())?)
            }
        };
        Ok(json!({"contents": [{"uri": uri, "mimeType": mime, "text": body}]}))
    }
}

fn error_response(id: &Value, e: RpcError) -> String {
    let mut err = json!({"code": e.code, "message": e.message});
    if let Some(d) = e.data {
        err["data"] = d;
    }
    json!({"jsonrpc": "2.0", "id": id, "error": err}).to_string()
}
