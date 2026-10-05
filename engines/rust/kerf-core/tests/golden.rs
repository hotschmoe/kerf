//! Golden and determinism tests over the three reference details.
//! Regenerate with `KERF_UPDATE_GOLDEN=1 cargo test -p kerf-core --test golden`.

use kerf_core::api::{self, Output};
use serde_json::{Value, json};
use std::path::PathBuf;

fn root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../..")
}

fn details() -> Vec<(String, Value)> {
    let dir = root().join("spec/details");
    let mut v: Vec<(String, Value)> = std::fs::read_dir(&dir)
        .unwrap()
        .filter_map(|e| e.ok())
        .filter(|e| e.file_name().to_string_lossy().ends_with(".kerf.json"))
        .map(|e| {
            let name = e.file_name().to_string_lossy().trim_end_matches(".kerf.json").to_string();
            (name, serde_json::from_str(&std::fs::read_to_string(e.path()).unwrap()).unwrap())
        })
        .collect();
    v.sort_by(|a, b| a.0.cmp(&b.0));
    v
}

fn call(f: &str, input: Value) -> Vec<u8> {
    match api::call(f, &input.to_string()) {
        Ok(Output::Json(s)) => s.into_bytes(),
        Ok(Output::Bytes(b)) => b,
        Err(e) => panic!("{} failed: {}", f, e),
    }
}

fn golden_dir(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../tests/golden").join(name)
}

fn check_golden(name: &str, file: &str, data: &[u8]) {
    let dir = golden_dir(name);
    let path = dir.join(file);
    if std::env::var("KERF_UPDATE_GOLDEN").is_ok() {
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(&path, data).unwrap();
        return;
    }
    let want = std::fs::read(&path).unwrap_or_else(|_| panic!("missing golden {} (run with KERF_UPDATE_GOLDEN=1)", path.display()));
    assert!(want == data, "golden mismatch: {} (run with KERF_UPDATE_GOLDEN=1 to update after reviewing the PNG)", path.display());
}

#[test]
fn reference_details_have_no_errors() {
    for (name, doc) in details() {
        let out = call("check", json!({"doc": doc}));
        let v: Value = serde_json::from_slice(&out).unwrap();
        let diags = v["diagnostics"].as_array().unwrap();
        let errors: Vec<&Value> = diags.iter().filter(|d| d["level"] == "error").collect();
        assert!(errors.is_empty(), "{}: {:?}", name, errors);
        // warnings must be explained: only the documented ones are allowed
        for d in diags.iter().filter(|d| d["level"] == "warning") {
            let code = d["code"].as_str().unwrap();
            assert!(["W_VIEW_FIT", "W_NOTE_TARGET"].contains(&code), "{}: unexpected warning {}", name, d);
        }
    }
}

#[test]
fn golden_outputs_and_determinism() {
    for (name, doc) in details() {
        let summary = call("check", json!({"doc": doc}));
        let v: Value = serde_json::from_slice(&summary).unwrap();
        check_golden(&name, "summary.txt", v["summary"].as_str().unwrap().as_bytes());
        check_golden(&name, "mesh.json", &call("mesh", json!({"doc": doc})));
        for view in doc["views"].as_array().unwrap() {
            let id = view["id"].as_str().unwrap();
            let drawing = call("drawing", json!({"doc": doc, "view": id}));
            check_golden(&name, &format!("drawing-{}.json", id), &drawing);
            for (fmt, ext) in [("svg", "svg"), ("dxf", "dxf"), ("pdf", "pdf")] {
                let a = call("export", json!({"doc": doc, "view": id, "format": fmt}));
                let b = call("export", json!({"doc": doc, "view": id, "format": fmt}));
                assert!(a == b, "{} {} {} not deterministic", name, id, fmt);
                check_golden(&name, &format!("{}.{}", id, ext), &a);
            }
            let sheet = call("export", json!({"doc": doc, "view": id, "format": "svg", "sheet": true}));
            check_golden(&name, &format!("{}-sheet.svg", id), &sheet);
        }
    }
}

#[test]
fn fmt_is_canonical_and_idempotent() {
    for (name, doc) in details() {
        let a: Value = serde_json::from_slice(&call("fmt", json!({"doc": doc}))).unwrap();
        let text = a["text"].as_str().unwrap().to_string();
        let again: Value = serde_json::from_slice(&call("fmt", json!({"doc": serde_json::from_str::<Value>(&text).unwrap()}))).unwrap();
        assert_eq!(text, again["text"].as_str().unwrap(), "{} fmt not idempotent", name);
        assert!(text.ends_with("}\n"));
    }
}
