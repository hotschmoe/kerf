//! Mutation robustness test: no panics (a wasm panic aborts the host app) on corrupted documents.

use serde_json::{Value, json};

struct Rng(u64);
impl Rng {
    fn next(&mut self) -> u64 {
        self.0 = self.0.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
        self.0 >> 33
    }
    fn below(&mut self, n: usize) -> usize {
        (self.next() as usize) % n.max(1)
    }
}

fn paths(v: &Value, cur: &mut Vec<String>, out: &mut Vec<Vec<String>>) {
    out.push(cur.clone());
    match v {
        Value::Object(m) => {
            for (k, x) in m {
                cur.push(k.clone());
                paths(x, cur, out);
                cur.pop();
            }
        }
        Value::Array(a) => {
            for (i, x) in a.iter().enumerate() {
                cur.push(i.to_string());
                paths(x, cur, out);
                cur.pop();
            }
        }
        _ => {}
    }
}

fn get_mut<'a>(v: &'a mut Value, p: &[String]) -> Option<&'a mut Value> {
    let mut cur = v;
    for k in p {
        cur = match cur {
            Value::Object(m) => m.get_mut(k)?,
            Value::Array(a) => a.get_mut(k.parse::<usize>().ok()?)?,
            _ => return None,
        };
    }
    Some(cur)
}

fn mutate(doc: &mut Value, rng: &mut Rng) {
    let mut ps = vec![];
    paths(doc, &mut vec![], &mut ps);
    let p = ps[rng.below(ps.len())].clone();
    if p.is_empty() {
        return;
    }
    let (parent, key) = (&p[..p.len() - 1], p.last().unwrap().clone());
    let Some(par) = get_mut(doc, parent) else { return };
    let choice = rng.below(9);
    let newv = match choice {
        0 => json!(0),
        1 => json!(-1.5),
        2 => json!(1e9),
        3 => json!(1e-9),
        4 => json!("garbage"),
        5 => json!(null),
        6 => json!([]),
        7 => json!({}),
        _ => json!(true),
    };
    match par {
        Value::Object(m) => {
            if choice == 8 {
                m.shift_remove(&key);
            } else {
                m.insert(key, newv);
            }
        }
        Value::Array(a) => {
            if let Ok(i) = key.parse::<usize>() {
                if i < a.len() {
                    if choice == 8 { a.remove(i); } else { a[i] = newv; }
                }
            }
        }
        _ => {}
    }
}

#[test]
fn mutated_documents_never_panic() {
    let dir = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../../spec/details");
    let mut docs = vec![];
    for e in std::fs::read_dir(&dir).unwrap().filter_map(|e| e.ok()) {
        if e.file_name().to_string_lossy().ends_with(".kerf.json") {
            docs.push(serde_json::from_str::<Value>(&std::fs::read_to_string(e.path()).unwrap()).unwrap());
        }
    }
    let seed: u64 = std::env::var("KERF_FUZZ_SEED").ok().and_then(|s| s.parse().ok()).unwrap_or(0x5eed);
    let mut rng = Rng(seed);
    let n: usize = std::env::var("KERF_FUZZ_N").ok().and_then(|s| s.parse().ok()).unwrap_or(60);
    for round in 0..n {
        for base in &docs {
            let mut d = base.clone();
            for _ in 0..(1 + rng.below(3)) {
                mutate(&mut d, &mut rng);
            }
            let text = d.to_string();
            let r = std::panic::catch_unwind(|| {
                let input = json!({"doc": serde_json::from_str::<Value>(&text).unwrap()}).to_string();
                let _ = kerf_core::api::call("check", &input);
                let _ = kerf_core::api::call("mesh", &input);
                let _ = kerf_core::api::call("fmt", &input);
                for view in ["A", "B"] {
                    let inp = json!({"doc": serde_json::from_str::<Value>(&text).unwrap(), "view": view, "format": "svg"}).to_string();
                    let _ = kerf_core::api::call("drawing", &inp);
                    for f in ["svg", "dxf", "pdf"] {
                        let inp = json!({"doc": serde_json::from_str::<Value>(&text).unwrap(), "view": view, "format": f}).to_string();
                        let _ = kerf_core::api::call("export", &inp);
                    }
                }
            });
            assert!(r.is_ok(), "panic on mutated doc (round {}): {}", round, text);
        }
    }
}
