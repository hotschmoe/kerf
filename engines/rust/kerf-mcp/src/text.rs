//! Static text the server hands to clients: instructions, the `kerf_detail` prompt, helpers.

use serde_json::Value;

pub const SYSTEM_MD: &str = include_str!("../../../../spec/llm/system.md");
pub const STYLE_JSON: &str = kerf_core::style::DEFAULT_STYLE_JSON;

/// Server-level `instructions` (sent in `initialize`; clients typically add it to the model's context).
pub const INSTRUCTIONS: &str = "\
Kerf builds construction details (wall/roof/foundation sections) from a semantic JSON document; \
a deterministic engine draws them. You describe construction with typed components (lumber, cmu_wall, \
concrete, rebar, truss, connector...) positioned by anchors, never by drawing.
Workflow: kerf_catalog (component types and params; resource kerf://catalog) -> kerf_new {id,title} -> \
kerf_apply with ONE {\"op\":\"set\",\"path\":\"doc\",\"value\":{whole document}} -> read the summary and fix every error/warning \
-> kerf_render {view} and LOOK at the image critically -> refine with small add/update/remove ops -> kerf_export {view,format}.
Documents are files <id>.kerf.json in the workspace directory (every kerf_apply saves it and appends to <id>.kerf.log.jsonl). \
Load the prompt kerf_detail for the full drafting instructions (note grammar, abbreviations, citation rules). \
Citations you write are stored as 'suggested'; only the designer can verify them. Never present a detail as engineered or approved.";

/// Appended to spec/llm/system.md in the `kerf_detail` prompt: how the tools map onto this server.
pub const MCP_ADDENDUM: &str = "\
# Working through the Kerf MCP server
- Tools: kerf_new / kerf_open / kerf_list (choose the active document), kerf_apply, kerf_inspect, kerf_render, kerf_export, kerf_catalog.
- The document is a file in the workspace; kerf_apply saves it after every successful batch, so the designer can open it in a Kerf app or diff it in git. Keep ids stable.
- kerf_render returns a PNG of the view exactly as it will export. Always look at it before reporting done.
- kerf_export writes svg/dxf/pdf into the workspace (default exports/<id>-<view>.<ext>) and returns the path.
- Your citations are stored with status \"suggested\"; verification is the designer's job in the Kerf app.";

pub fn prompt_text() -> String {
    format!("{}\n\n{}\n\n# Component catalog\n{}", SYSTEM_MD.trim_end(), MCP_ADDENDUM, kerf_core::catalog::markdown())
}

/// `YYYY-MM-DDTHH:MM:SSZ` (UTC) for op-log entries. Logs are the only place a timestamp appears.
pub fn now_iso() -> String {
    let secs = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs() as i64).unwrap_or(0);
    let days = secs.div_euclid(86400);
    let rem = secs.rem_euclid(86400);
    // civil from days (Howard Hinnant)
    let z = days + 719468;
    let era = z.div_euclid(146097);
    let doe = z.rem_euclid(146097);
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = if m <= 2 { y + 1 } else { y };
    format!("{:04}-{:02}-{:02}T{:02}:{:02}:{:02}Z", y, m, d, rem / 3600, rem % 3600 / 60, rem % 60)
}

/// One diagnostic (engine JSON form) as a single line, same shape as the engine's summary lines.
pub fn diag_line(d: &Value) -> String {
    let tag = match d["level"].as_str().unwrap_or("") {
        "error" => "ERROR",
        "warning" => "WARN",
        _ => "INFO",
    };
    let mut s = format!("{} {}", tag, d["code"].as_str().unwrap_or("?"));
    let msg = d["message"].as_str().unwrap_or("");
    // the engine often repeats the path at the start of the message: don't print it twice
    if let Some(id) = d["id"].as_str().or_else(|| d["path"].as_str()).filter(|id| !msg.starts_with(id)) {
        s.push(' ');
        s.push_str(id);
    }
    s.push_str(": ");
    s.push_str(msg);
    if let Some(f) = d["fix"].as_str() {
        s.push_str(" Fix: ");
        s.push_str(f);
    }
    s
}

#[cfg(test)]
mod tests {
    #[test]
    fn iso() {
        let s = super::now_iso();
        assert_eq!(s.len(), 20);
        assert!(s.starts_with("20"));
    }
}
