//! Scripted conversation for the mock transport / DEMO MODE. Each inner Vec is one designer turn.
//! Turn 1 builds the truss detail, inspects and renders it (two tool_use blocks in one message),
//! adds a note. Turn 2 shows a rejected op and the recovery.

use crate::chat::MockScript;
use serde_json::{Value, json};

const TRUSS: &str = include_str!("../../../spec/details/truss-bearing-cmu.kerf.json");

fn resp(content: Vec<Value>, stop: &str) -> Value {
    json!({"id": "msg_demo", "type": "message", "role": "assistant", "model": "claude-opus-5-5",
           "content": content, "stop_reason": stop, "usage": {"input_tokens": 1200, "output_tokens": 340}})
}
fn text(t: &str) -> Value {
    json!({"type": "text", "text": t})
}
fn tool(id: &str, name: &str, input: Value) -> Value {
    json!({"type": "tool_use", "id": id, "name": name, "input": input})
}

pub fn script() -> MockScript {
    let doc: Value = serde_json::from_str(TRUSS).expect("truss sample");
    let turn1 = vec![
        resp(
            vec![
                json!({"type": "thinking", "thinking": "Prefab truss on CMU: bond beam, sill plate, J-bolts, hurricane tie.", "signature": "demo-signature"}),
                text("Building a prefab roof truss bearing on an 8\" CMU wall with a grouted bond beam."),
                tool("toolu_demo_1", "kerf_apply", json!({"ops": [{"op": "set", "path": "doc", "value": doc}], "why": "Build truss bearing detail on CMU"})),
            ],
            "tool_use",
        ),
        resp(
            vec![
                text("Checking the summary, then rendering view A."),
                tool("toolu_demo_2", "kerf_inspect", json!({"q": "summary"})),
                tool("toolu_demo_3", "kerf_render", json!({"view": "A"})),
            ],
            "tool_use",
        ),
        resp(
            vec![
                text("Adding a note for the bottom chord bearing."),
                tool(
                    "toolu_demo_4",
                    "kerf_apply",
                    json!({"ops": [{"op": "add", "path": "views/A/annotations", "value": {
                        "id": "n_demo", "type": "note", "text": "2X4 BOTT. CHORD BEARING ON 2X8 PT SILL", "target": "truss",
                        "cite": [{"code": "IRC", "edition": 2021, "section": "R802.10", "title": "Wood trusses", "status": "verified"}]}}],
                        "why": "Add bearing note"}),
                ),
            ],
            "tool_use",
        ),
        resp(
            vec![text(
                "Built the truss heel bearing on an 8\" CMU wall with a grouted bond beam, PT sill plate with J-bolts, and an H2.5A tie.\n\nAssumptions: 4:12 pitch, 18\" overhang, trusses @ 24\" O.C.\nOpen: confirm uplift connector capacity with the truss manufacturer.\nCitations to verify: IRC R802.10, R802.11.1, R403.1.6, R905.1.1, R606.",
            )],
            "end_turn",
        ),
    ];
    let turn2 = vec![
        resp(
            vec![
                text("Removing the soffit blocking note."),
                tool("toolu_demo_5", "kerf_apply", json!({"ops": [{"op": "remove", "path": "views/A/annotations/n_does_not_exist"}], "why": "Remove stale note"})),
            ],
            "tool_use",
        ),
        resp(
            vec![
                text("That id does not exist. Removing the bird blocking note instead."),
                tool("toolu_demo_6", "kerf_apply", json!({"ops": [{"op": "remove", "path": "views/A/annotations/n_block"}], "why": "Remove bird blocking note"})),
            ],
            "tool_use",
        ),
        resp(vec![text("Done. The bird blocking note is removed; the blocking itself is unchanged.")], "end_turn"),
    ];
    MockScript::new(vec![turn1, turn2])
}
