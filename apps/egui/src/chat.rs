//! The Claude chat harness (spec/llm/HARNESS.md): raw HTTP to /v1/messages, append-only
//! history, tool loop capped at 25 rounds, retries with backoff, refusal handling, and a mock
//! transport that replays a scripted conversation (used by the demo mode and the tests).

use serde_json::{Value, json};
use std::collections::VecDeque;
use std::sync::mpsc::{Receiver, Sender, channel};
use web_time::{Duration, Instant};

pub const API_URL: &str = "https://api.anthropic.com/v1/messages";
pub const BETA_FALLBACK: &str = "server-side-fallback-2026-07-01";
pub const MODELS: [&str; 2] = ["claude-opus-5-5", "claude-sonnet-5-5"];
pub const MAX_ROUNDS: u32 = 25;
const SYSTEM_MD: &str = include_str!("../../../spec/llm/system.md");
const TOOLS_JSON: &str = include_str!("../../../spec/llm/tools.json");

// ------------------------------------------------------------------ display model

#[derive(Clone, Debug)]
pub struct Thumb {
    pub name: String,
    pub media_type: String,
    pub png_or_jpg: std::sync::Arc<Vec<u8>>,
}

#[derive(Clone, Debug)]
pub struct ToolActivity {
    pub name: String,
    /// one line, e.g. `APPLY  6 OPS   ✓ 0 ERR 1 WARN`
    pub line: String,
    pub ok: bool,
    pub input: Value,
    pub result: String,
    pub image: Option<Thumb>,
    pub open: bool,
}

#[derive(Clone, Debug)]
pub enum Block {
    Text(String),
    Tool(ToolActivity),
    Fallback(String),
}

#[derive(Clone, Debug)]
pub enum Entry {
    Designer { t: String, text: String, images: Vec<Thumb> },
    Claude { t: String, blocks: Vec<Block> },
    Error(String),
    Info(String),
}

#[derive(Clone, Debug, PartialEq)]
pub enum Phase {
    Idle,
    /// waiting for a response: round number (1-based)
    Busy(u32),
    /// waiting to retry after an error: seconds remaining, attempt
    Backoff(u32),
}

#[derive(Debug)]
pub enum ApiError {
    Auth,
    Retryable(String),
    Fatal(String),
    /// 400 that names the beta/fallbacks params
    BetaRejected(String),
}

/// What a tool run returns to the model.
pub struct ToolOutput {
    pub content: Vec<Value>,
    pub is_error: bool,
    pub line: String,
    pub text: String,
    pub image: Option<Thumb>,
}

/// The app side of the loop: runs engine tools and supplies engine-derived prompt parts.
pub trait ToolHost {
    fn run_tool(&mut self, name: &str, input: &Value) -> ToolOutput;
    fn catalog_markdown(&mut self) -> String;
    /// `[designer edits since your last turn: …]` lines (drained on call)
    fn drain_designer_notes(&mut self) -> Option<String>;
    fn clock(&self) -> String;
}

// ------------------------------------------------------------------ mock transport

/// A scripted conversation: each inner Vec is one designer turn's responses, replayed in order.
#[derive(Clone, Default)]
pub struct MockScript {
    turns: VecDeque<VecDeque<Value>>,
    cur: VecDeque<Value>,
}

impl MockScript {
    pub fn new(turns: Vec<Vec<Value>>) -> Self {
        MockScript { turns: turns.into_iter().map(Into::into).collect(), cur: VecDeque::new() }
    }
    fn begin_turn(&mut self) {
        self.cur = self.turns.pop_front().unwrap_or_else(|| {
            VecDeque::from(vec![msg_text("DEMO SCRIPT EXHAUSTED. NO FURTHER RECORDED TURNS.", "end_turn")])
        });
    }
    fn next(&mut self) -> Value {
        self.cur.pop_front().unwrap_or_else(|| msg_text("DEMO TURN ENDED.", "end_turn"))
    }
}

pub fn msg_text(text: &str, stop: &str) -> Value {
    json!({"id": "msg_demo", "type": "message", "role": "assistant", "model": "claude-opus-5-5",
           "content": [{"type": "text", "text": text}], "stop_reason": stop, "usage": {"input_tokens": 0, "output_tokens": 0}})
}

// ------------------------------------------------------------------ chat

pub struct Chat {
    pub entries: Vec<Entry>,
    /// API messages, append-only
    pub history: Vec<Value>,
    pub phase: Phase,
    pub model: String,
    pub demo: bool,
    pub use_beta: bool,
    pub api_key: String,
    mock: MockScript,
    rx: Option<Receiver<Result<Value, ApiError>>>,
    tx: Sender<Result<Value, ApiError>>,
    ready_at: Option<(Instant, Result<Value, ApiError>)>,
    retry_at: Option<(Instant, u32)>,
    last_body: Option<Value>,
    attempts: u32,
    round: u32,
    /// the tool loop is mid-turn (so a response with tool_use continues it)
    in_turn: bool,
    system_text: Option<String>,
    pub ctx: Option<egui::Context>,
    pub last_usage: Option<Value>,
    pub mock_delay_ms: u64,
    pub api_url: String,
}

impl Chat {
    pub fn new() -> Chat {
        let (tx, rx) = channel();
        Chat {
            entries: Vec::new(),
            history: Vec::new(),
            phase: Phase::Idle,
            model: MODELS[0].to_owned(),
            demo: false,
            use_beta: true,
            api_key: String::new(),
            mock: MockScript::default(),
            rx: Some(rx),
            tx,
            ready_at: None,
            retry_at: None,
            last_body: None,
            attempts: 0,
            round: 0,
            in_turn: false,
            system_text: None,
            ctx: None,
            last_usage: None,
            mock_delay_ms: 450,
            api_url: API_URL.to_owned(),
        }
    }

    pub fn set_mock(&mut self, script: MockScript) {
        self.mock = script;
    }

    pub fn busy(&self) -> bool {
        self.phase != Phase::Idle
    }

    pub fn has_key(&self) -> bool {
        self.demo || !self.api_key.trim().is_empty()
    }

    pub fn tools() -> Value {
        serde_json::from_str(TOOLS_JSON).expect("tools.json")
    }

    fn system(&mut self, host: &mut dyn ToolHost) -> Value {
        if self.system_text.is_none() {
            self.system_text = Some(format!("{}\n\n# Component catalog\n{}", SYSTEM_MD.trim_end(), host.catalog_markdown()));
        }
        json!([{"type": "text", "text": self.system_text.clone().unwrap(), "cache_control": {"type": "ephemeral"}}])
    }

    pub fn build_body(&mut self, host: &mut dyn ToolHost) -> Value {
        let mut body = json!({
            "model": self.model,
            "max_tokens": 32000,
            "thinking": {"type": "adaptive"},
            "output_config": {"effort": "high"},
            "system": self.system(host),
            "tools": Self::tools(),
            "messages": self.history,
        });
        if self.use_beta {
            body["fallbacks"] = json!("default");
        }
        body
    }

    /// Designer submits a message (text + optional images). Starts the loop.
    pub fn send(&mut self, host: &mut dyn ToolHost, text: String, images: Vec<Thumb>) {
        if self.busy() {
            return;
        }
        let mut content: Vec<Value> = Vec::new();
        for im in &images {
            use base64::Engine;
            content.push(json!({"type": "image", "source": {"type": "base64", "media_type": im.media_type,
                "data": base64::engine::general_purpose::STANDARD.encode(&*im.png_or_jpg)}}));
        }
        let mut full = text.clone();
        if let Some(notes) = host.drain_designer_notes() {
            full.push_str("\n\n");
            full.push_str(&notes);
        }
        content.push(json!({"type": "text", "text": full}));
        self.history.push(json!({"role": "user", "content": content}));
        self.entries.push(Entry::Designer { t: host.clock(), text, images });
        self.round = 0;
        self.attempts = 0;
        self.in_turn = true;
        if self.demo {
            self.mock.begin_turn();
        }
        self.dispatch(host, 0);
    }

    fn dispatch(&mut self, host: &mut dyn ToolHost, attempt: u32) {
        self.round += 1;
        if self.round > MAX_ROUNDS {
            self.entries.push(Entry::Error(format!("TOOL LOOP CAPPED AT {MAX_ROUNDS} ROUNDS. SEND A MESSAGE TO CONTINUE.")));
            self.finish();
            return;
        }
        self.phase = Phase::Busy(self.round);
        let body = self.build_body(host);
        self.last_body = Some(body.clone());
        self.transmit(body, attempt);
    }

    fn transmit(&mut self, body: Value, _attempt: u32) {
        if self.demo {
            let resp = self.mock.next();
            self.ready_at = Some((Instant::now() + Duration::from_millis(self.mock_delay_ms), Ok(resp)));
            if let Some(c) = &self.ctx {
                c.request_repaint_after(Duration::from_millis(500));
            }
            return;
        }
        let mut req = ehttp::Request::post(self.api_url.clone(), serde_json::to_vec(&body).unwrap_or_default());
        req.headers.insert("content-type", "application/json");
        req.headers.insert("x-api-key", self.api_key.trim());
        req.headers.insert("anthropic-version", "2023-06-01");
        req.headers.insert("anthropic-dangerous-direct-browser-access", "true");
        if self.use_beta {
            req.headers.insert("anthropic-beta", BETA_FALLBACK);
        }
        let tx = self.tx.clone();
        let ctx = self.ctx.clone();
        ehttp::fetch(req, move |res| {
            let out = match res {
                Err(e) => Err(ApiError::Retryable(format!("NETWORK: {e}"))),
                Ok(r) => classify(r.status, &r.bytes),
            };
            let _ = tx.send(out);
            if let Some(c) = ctx {
                c.request_repaint();
            }
        });
    }

    /// Call every frame. Drives responses, backoff timers and tool execution.
    pub fn poll(&mut self, host: &mut dyn ToolHost) {
        // mock latency
        if let Some((at, _)) = &self.ready_at {
            if Instant::now() >= *at {
                let (_, r) = self.ready_at.take().unwrap();
                self.on_result(host, r);
            } else if let Some(c) = &self.ctx {
                c.request_repaint_after(Duration::from_millis(100));
            }
        }
        // retry timer
        if let Some((at, attempt)) = self.retry_at {
            let now = Instant::now();
            if now >= at {
                self.retry_at = None;
                self.phase = Phase::Busy(self.round);
                if let Some(b) = self.last_body.clone() {
                    self.transmit(b, attempt);
                }
            } else {
                self.phase = Phase::Backoff((at - now).as_secs() as u32 + 1);
                if let Some(c) = &self.ctx {
                    c.request_repaint_after(Duration::from_millis(250));
                }
            }
        }
        let mut got = Vec::new();
        if let Some(rx) = &self.rx {
            while let Ok(r) = rx.try_recv() {
                got.push(r);
            }
        }
        for r in got {
            self.on_result(host, r);
        }
    }

    fn finish(&mut self) {
        self.phase = Phase::Idle;
        self.in_turn = false;
        self.retry_at = None;
    }

    fn on_result(&mut self, host: &mut dyn ToolHost, r: Result<Value, ApiError>) {
        match r {
            Ok(resp) => self.on_response(host, resp),
            Err(ApiError::Auth) => {
                self.entries.push(Entry::Error("INVALID API KEY".into()));
                self.pop_pending_user();
                self.finish();
            }
            Err(ApiError::BetaRejected(m)) if self.use_beta => {
                self.use_beta = false;
                self.entries.push(Entry::Info(format!("FALLBACKS PARAM REJECTED ({m}); RETRYING WITHOUT IT")));
                self.round -= 1;
                self.dispatch(host, 0);
            }
            Err(ApiError::BetaRejected(m)) | Err(ApiError::Fatal(m)) => {
                self.entries.push(Entry::Error(m));
                self.pop_pending_user();
                self.finish();
            }
            Err(ApiError::Retryable(m)) => {
                let attempt = self.attempts_hint();
                if attempt >= 3 {
                    self.entries.push(Entry::Error(format!("{m} (GAVE UP AFTER 3 RETRIES)")));
                    self.pop_pending_user();
                    self.finish();
                } else {
                    let wait = 2u64 << attempt; // 2, 4, 8
                    self.entries.push(Entry::Info(format!("{m}. RETRY IN {wait} S")));
                    self.set_attempts_hint(attempt + 1);
                    self.retry_at = Some((Instant::now() + Duration::from_secs(wait), attempt + 1));
                    self.phase = Phase::Backoff(wait as u32);
                }
            }
        }
    }

    fn attempts_hint(&self) -> u32 {
        self.attempts
    }
    fn set_attempts_hint(&mut self, n: u32) {
        self.attempts = n;
    }

    /// When a request ultimately fails the user message stays in history only if the model has
    /// seen an answer to it; otherwise a dangling user turn would break alternation on resend.
    fn pop_pending_user(&mut self) {
        if matches!(self.history.last(), Some(m) if m["role"] == "user") {
            // keep tool_result user messages out of danger too: both leave an unanswered tail.
            // The designer can resend; drop the tail so roles keep alternating.
            let drop_n = self.unanswered_tail();
            for _ in 0..drop_n {
                self.history.pop();
            }
        }
    }

    fn unanswered_tail(&self) -> usize {
        // number of trailing user messages (should be 1)
        self.history.iter().rev().take_while(|m| m["role"] == "user").count()
    }

    fn on_response(&mut self, host: &mut dyn ToolHost, resp: Value) {
        self.set_attempts_hint(0);
        self.last_usage = resp.get("usage").cloned();
        let content = resp.get("content").cloned().unwrap_or(json!([]));
        // append VERBATIM, incl. thinking / fallback blocks
        self.history.push(json!({"role": "assistant", "content": content.clone()}));
        let stop = resp.get("stop_reason").and_then(Value::as_str).unwrap_or("end_turn").to_owned();

        // render blocks into the open manila card
        let mut blocks: Vec<Block> = Vec::new();
        for b in content.as_array().into_iter().flatten() {
            match b["type"].as_str() {
                Some("text") => {
                    let t = b["text"].as_str().unwrap_or("").trim();
                    if !t.is_empty() {
                        blocks.push(Block::Text(t.to_owned()));
                    }
                }
                Some("fallback") => {
                    let from = b["from"]["model"].as_str().unwrap_or("?");
                    let to = b["to"]["model"].as_str().unwrap_or("?");
                    blocks.push(Block::Fallback(format!("FALLBACK {from} \u{2192} {to}")));
                }
                _ => {}
            }
        }
        self.push_blocks(host, blocks);

        if stop == "refusal" {
            let why = resp["stop_details"]["explanation"].as_str().unwrap_or("NO EXPLANATION GIVEN");
            self.entries.push(Entry::Error(format!("REFUSAL: {why}")));
            self.finish();
            return;
        }
        if stop == "max_tokens" && content.as_array().is_some_and(|a| a.iter().any(|b| b["type"] == "tool_use")) {
            // a truncated tool_use input must not run; answer it with errors so the model can retry
            let mut results = Vec::new();
            for b in content.as_array().into_iter().flatten() {
                if b["type"] == "tool_use" {
                    results.push(json!({"type": "tool_result", "tool_use_id": b["id"], "is_error": true,
                        "content": "Your response hit max_tokens before this tool call finished. Send a smaller batch of ops."}));
                }
            }
            self.entries.push(Entry::Error("RESPONSE TRUNCATED (MAX TOKENS); ASKING CLAUDE TO RETRY IN SMALLER STEPS".into()));
            self.history.push(json!({"role": "user", "content": results}));
            self.dispatch(host, 0);
            return;
        }
        if stop != "tool_use" {
            self.finish();
            return;
        }
        // run every tool_use in order; ALL results in ONE user message
        let mut results = Vec::new();
        let mut acts = Vec::new();
        for b in content.as_array().into_iter().flatten() {
            if b["type"] != "tool_use" {
                continue;
            }
            let name = b["name"].as_str().unwrap_or("").to_owned();
            let input = b.get("input").cloned().unwrap_or(json!({}));
            let out = host.run_tool(&name, &input);
            results.push(json!({"type": "tool_result", "tool_use_id": b["id"], "content": out.content, "is_error": out.is_error}));
            acts.push(Block::Tool(ToolActivity { name, line: out.line, ok: !out.is_error, input, result: out.text, image: out.image, open: false }));
        }
        self.push_blocks(host, acts);
        self.history.push(json!({"role": "user", "content": results}));
        self.dispatch(host, 0);
    }

    fn push_blocks(&mut self, host: &dyn ToolHost, blocks: Vec<Block>) {
        if blocks.is_empty() {
            return;
        }
        if let Some(Entry::Claude { blocks: bs, .. }) = self.entries.last_mut() {
            if self.in_turn {
                bs.extend(blocks);
                return;
            }
        }
        self.entries.push(Entry::Claude { t: host.clock(), blocks });
    }
}

impl Default for Chat {
    fn default() -> Self {
        Self::new()
    }
}

fn classify(status: u16, bytes: &[u8]) -> Result<Value, ApiError> {
    let text = String::from_utf8_lossy(bytes);
    if status == 200 {
        return serde_json::from_slice(bytes).map_err(|e| ApiError::Fatal(format!("BAD RESPONSE JSON: {e}")));
    }
    let msg = serde_json::from_slice::<Value>(bytes)
        .ok()
        .and_then(|v| v["error"]["message"].as_str().map(str::to_owned))
        .unwrap_or_else(|| text.chars().take(300).collect());
    match status {
        401 => Err(ApiError::Auth),
        429 | 529 | 500 | 502 | 503 | 504 => Err(ApiError::Retryable(format!("API {status}: {msg}"))),
        0 => Err(ApiError::Retryable(format!("NETWORK: {msg}"))),
        400 if msg.to_lowercase().contains("fallback") || msg.to_lowercase().contains("beta") => Err(ApiError::BetaRejected(msg)),
        _ => Err(ApiError::Fatal(format!("API {status}: {msg}"))),
    }
}

// ------------------------------------------------------------------ one-line tool summaries

/// `▸ APPLY  6 OPS   ✓ 0 ERR 1 WARN` style line (glyphs are painted by the UI).
pub fn apply_line(n_ops: usize, errs: usize, warns: usize, ok: bool) -> String {
    format!("APPLY  {n_ops} OPS   {} {errs} ERR {warns} WARN", if ok { "\u{2713}" } else { "\u{2717}" })
}

#[cfg(test)]
mod tests {
    use super::*;

    struct FakeHost {
        calls: Vec<String>,
    }
    impl ToolHost for FakeHost {
        fn run_tool(&mut self, name: &str, _input: &Value) -> ToolOutput {
            self.calls.push(name.to_owned());
            ToolOutput { content: vec![json!({"type": "text", "text": "ok"})], is_error: false, line: format!("{name} ok"), text: "ok".into(), image: None }
        }
        fn catalog_markdown(&mut self) -> String {
            "CATALOG".into()
        }
        fn drain_designer_notes(&mut self) -> Option<String> {
            None
        }
        fn clock(&self) -> String {
            "00:00".into()
        }
    }

    fn tool_resp(id: &str) -> Value {
        json!({"stop_reason": "tool_use", "content": [{"type": "thinking", "thinking": "t", "signature": "s"}, {"type": "tool_use", "id": id, "name": "kerf_inspect", "input": {"q": "summary"}}]})
    }

    fn run_until_idle(chat: &mut Chat, host: &mut FakeHost) {
        let start = Instant::now();
        while chat.busy() && start.elapsed() < Duration::from_secs(10) {
            chat.poll(host);
            std::thread::sleep(Duration::from_millis(20));
        }
    }

    #[test]
    fn tool_loop_keeps_history_append_only_and_batches_results() {
        let mut chat = Chat::new();
        chat.demo = true;
        let two = json!({"stop_reason": "tool_use", "content": [
            {"type": "tool_use", "id": "a", "name": "kerf_inspect", "input": {}},
            {"type": "tool_use", "id": "b", "name": "kerf_render", "input": {"view": "A"}}]});
        chat.set_mock(MockScript::new(vec![vec![two, msg_text("done", "end_turn")]]));
        let mut host = FakeHost { calls: vec![] };
        chat.send(&mut host, "hi".into(), vec![]);
        run_until_idle(&mut chat, &mut host);
        assert_eq!(host.calls, ["kerf_inspect", "kerf_render"]);
        let roles: Vec<&str> = chat.history.iter().map(|m| m["role"].as_str().unwrap()).collect();
        assert_eq!(roles, ["user", "assistant", "user", "assistant"]);
        // ALL results in ONE user message, in order
        let results = chat.history[2]["content"].as_array().unwrap();
        assert_eq!(results.len(), 2);
        assert_eq!(results[0]["tool_use_id"], "a");
        assert_eq!(results[1]["tool_use_id"], "b");
        assert_eq!(chat.phase, Phase::Idle);
    }

    #[test]
    fn thinking_blocks_are_echoed_verbatim() {
        let mut chat = Chat::new();
        chat.demo = true;
        chat.set_mock(MockScript::new(vec![vec![tool_resp("x"), msg_text("ok", "end_turn")]]));
        let mut host = FakeHost { calls: vec![] };
        chat.send(&mut host, "go".into(), vec![]);
        run_until_idle(&mut chat, &mut host);
        assert_eq!(chat.history[1]["content"][0]["type"], "thinking");
        assert_eq!(chat.history[1]["content"][0]["signature"], "s");
    }

    #[test]
    fn refusal_stops_the_loop_and_shows_explanation() {
        let mut chat = Chat::new();
        chat.demo = true;
        let r = json!({"stop_reason": "refusal", "stop_details": {"explanation": "policy"}, "content": []});
        chat.set_mock(MockScript::new(vec![vec![r]]));
        let mut host = FakeHost { calls: vec![] };
        chat.send(&mut host, "x".into(), vec![]);
        run_until_idle(&mut chat, &mut host);
        assert!(matches!(chat.entries.last(), Some(Entry::Error(m)) if m.contains("policy")));
        assert_eq!(chat.phase, Phase::Idle);
    }

    #[test]
    fn loop_is_capped_at_25_rounds() {
        let mut chat = Chat::new();
        chat.demo = true;
        chat.mock_delay_ms = 0;
        let mut script = Vec::new();
        for i in 0..40 {
            script.push(tool_resp(&format!("t{i}")));
        }
        chat.set_mock(MockScript::new(vec![script]));
        let mut host = FakeHost { calls: vec![] };
        chat.send(&mut host, "x".into(), vec![]);
        let start = Instant::now();
        while chat.busy() && start.elapsed() < Duration::from_secs(60) {
            chat.poll(&mut host);
            std::thread::sleep(Duration::from_millis(5));
        }
        assert_eq!(host.calls.len() as u32, MAX_ROUNDS);
        assert!(matches!(chat.entries.last(), Some(Entry::Error(m)) if m.contains("CAPPED")));
    }

    #[test]
    fn http_status_classification() {
        assert!(matches!(classify(401, b"{}"), Err(ApiError::Auth)));
        assert!(matches!(classify(429, b"{}"), Err(ApiError::Retryable(_))));
        assert!(matches!(classify(529, b"{}"), Err(ApiError::Retryable(_))));
        let beta = br#"{"type":"error","error":{"type":"invalid_request_error","message":"unknown parameter: fallbacks"}}"#;
        assert!(matches!(classify(400, beta), Err(ApiError::BetaRejected(_))));
        let other = br#"{"type":"error","error":{"type":"invalid_request_error","message":"max_tokens too big"}}"#;
        assert!(matches!(classify(400, other), Err(ApiError::Fatal(m)) if m.contains("max_tokens")));
        assert!(classify(200, br#"{"content":[]}"#).is_ok());
    }

    #[test]
    fn request_body_matches_harness() {
        let mut chat = Chat::new();
        let mut host = FakeHost { calls: vec![] };
        chat.history.push(json!({"role": "user", "content": [{"type": "text", "text": "hi"}]}));
        let b = chat.build_body(&mut host);
        assert_eq!(b["model"], "claude-opus-5-5");
        assert_eq!(b["max_tokens"], 32000);
        assert_eq!(b["thinking"]["type"], "adaptive");
        assert_eq!(b["output_config"]["effort"], "high");
        assert_eq!(b["fallbacks"], "default");
        assert_eq!(b["system"][0]["cache_control"]["type"], "ephemeral");
        assert!(b["system"][0]["text"].as_str().unwrap().contains("CATALOG"));
        assert_eq!(b["tools"].as_array().unwrap().len(), 3);
        assert!(b.get("temperature").is_none() && b.get("tool_choice").is_none());
        chat.use_beta = false;
        assert!(chat.build_body(&mut host).get("fallbacks").is_none());
    }

    /// A throwaway HTTP server that records requests and replies from a queue of (status, body).
    fn serve(replies: Vec<(u16, String)>) -> (String, std::sync::Arc<std::sync::Mutex<Vec<(String, Value)>>>) {
        use std::io::{Read, Write};
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let url = format!("http://{}/v1/messages", listener.local_addr().unwrap());
        let seen = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
        let seen2 = seen.clone();
        std::thread::spawn(move || {
            for (status, body) in replies {
                let (mut s, _) = listener.accept().unwrap();
                let mut buf = Vec::new();
                let mut chunk = [0u8; 4096];
                let (head, len) = loop {
                    let n = s.read(&mut chunk).unwrap();
                    buf.extend_from_slice(&chunk[..n]);
                    if let Some(p) = buf.windows(4).position(|w| w == b"\r\n\r\n") {
                        let head = String::from_utf8_lossy(&buf[..p]).to_string();
                        let len = head.lines().find_map(|l| l.to_lowercase().strip_prefix("content-length:").map(|v| v.trim().parse::<usize>().unwrap())).unwrap_or(0);
                        break (head, len + p + 4);
                    }
                };
                while buf.len() < len {
                    let n = s.read(&mut chunk).unwrap();
                    buf.extend_from_slice(&chunk[..n]);
                }
                let json_body: Value = serde_json::from_slice(&buf[len - (len - head.len() - 4)..]).unwrap_or(Value::Null);
                seen2.lock().unwrap().push((head, json_body));
                let resp = format!("HTTP/1.1 {status} X\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{body}", body.len());
                s.write_all(resp.as_bytes()).unwrap();
            }
        });
        (url, seen)
    }

    #[test]
    fn http_transport_sends_documented_headers_and_body() {
        let reply = json!({"id": "m", "type": "message", "role": "assistant", "content": [{"type": "text", "text": "hello"}], "stop_reason": "end_turn", "usage": {}});
        let (url, seen) = serve(vec![(200, reply.to_string())]);
        let mut chat = Chat::new();
        chat.api_url = url;
        chat.api_key = "sk-ant-test".into();
        let mut host = FakeHost { calls: vec![] };
        chat.send(&mut host, "hi".into(), vec![]);
        run_until_idle(&mut chat, &mut host);
        let seen = seen.lock().unwrap();
        let (head, body) = &seen[0];
        let h = head.to_lowercase();
        assert!(h.contains("x-api-key: sk-ant-test"), "{h}");
        assert!(h.contains("anthropic-version: 2023-06-01"));
        assert!(h.contains("anthropic-dangerous-direct-browser-access: true"));
        assert!(h.contains(&format!("anthropic-beta: {BETA_FALLBACK}")));
        assert!(h.contains("content-type: application/json"));
        assert_eq!(body["fallbacks"], "default");
        assert_eq!(body["messages"][0]["content"][0]["text"], "hi");
        assert!(matches!(chat.entries.last(), Some(Entry::Claude { blocks, .. }) if matches!(&blocks[0], Block::Text(t) if t == "hello")));
    }

    #[test]
    fn rejected_fallbacks_param_is_retried_once_without_it() {
        let err = json!({"type": "error", "error": {"type": "invalid_request_error", "message": "fallbacks: Extra inputs are not permitted"}});
        let ok = json!({"content": [{"type": "text", "text": "ok"}], "stop_reason": "end_turn"});
        let (url, seen) = serve(vec![(400, err.to_string()), (200, ok.to_string())]);
        let mut chat = Chat::new();
        chat.api_url = url;
        chat.api_key = "k".into();
        let mut host = FakeHost { calls: vec![] };
        chat.send(&mut host, "hi".into(), vec![]);
        run_until_idle(&mut chat, &mut host);
        let seen = seen.lock().unwrap();
        assert_eq!(seen.len(), 2);
        assert!(seen[0].1.get("fallbacks").is_some());
        assert!(seen[1].1.get("fallbacks").is_none());
        assert!(!seen[1].0.to_lowercase().contains("anthropic-beta"));
        assert!(!chat.use_beta, "remembered for the session");
        // history was not corrupted by the retry: one user message, one assistant message
        assert_eq!(chat.history.len(), 2);
    }

    #[test]
    fn invalid_key_shows_message_and_leaves_history_clean() {
        let err = json!({"type": "error", "error": {"type": "authentication_error", "message": "invalid x-api-key"}});
        let (url, _seen) = serve(vec![(401, err.to_string())]);
        let mut chat = Chat::new();
        chat.api_url = url;
        chat.api_key = "bad".into();
        let mut host = FakeHost { calls: vec![] };
        chat.send(&mut host, "hi".into(), vec![]);
        run_until_idle(&mut chat, &mut host);
        assert!(matches!(chat.entries.last(), Some(Entry::Error(m)) if m == "INVALID API KEY"));
        assert!(chat.history.is_empty(), "unanswered user turn is dropped so the designer can resend");
    }

    #[test]
    fn overloaded_is_retried_after_backoff() {
        let err = json!({"type": "error", "error": {"type": "overloaded_error", "message": "Overloaded"}});
        let ok = json!({"content": [{"type": "text", "text": "ok"}], "stop_reason": "end_turn"});
        let (url, seen) = serve(vec![(529, err.to_string()), (200, ok.to_string())]);
        let mut chat = Chat::new();
        chat.api_url = url;
        chat.api_key = "k".into();
        let mut host = FakeHost { calls: vec![] };
        let t = Instant::now();
        chat.send(&mut host, "hi".into(), vec![]);
        run_until_idle(&mut chat, &mut host);
        assert_eq!(seen.lock().unwrap().len(), 2);
        assert!(t.elapsed() >= Duration::from_secs(2), "first backoff is 2 s");
        assert!(chat.entries.iter().any(|e| matches!(e, Entry::Info(m) if m.contains("RETRY IN 2 S"))));
        assert!(matches!(chat.entries.last(), Some(Entry::Claude { .. })));
    }
}
