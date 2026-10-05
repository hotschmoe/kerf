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
        self.in_turn = true;
        if self.demo {
            self.mock.begin_turn();
        }
        self.dispatch(host, 0);
    }

    fn dispatch(&mut self, host: &mut dyn ToolHost, attempt: u32) {
        self.round += 1;
        if self.round > MAX_ROUNDS + 1 {
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
            self.ready_at = Some((Instant::now() + Duration::from_millis(450), Ok(resp)));
            if let Some(c) = &self.ctx {
                c.request_repaint_after(Duration::from_millis(500));
            }
            return;
        }
        let mut req = ehttp::Request::post(API_URL, serde_json::to_vec(&body).unwrap_or_default());
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
    format!("APPLY  {n_ops} OPS   {} {errs} ERR {warns} WARN", if ok { "\u{2713}" } else { "X" })
}
