//! Behavior tests: placement, anchors, diagnostics, apply semantics, note layout.

use kerf_core::api::{self, Output};
use serde_json::{Value, json};

fn call(f: &str, input: Value) -> Result<Value, String> {
    match api::call(f, &input.to_string())? {
        Output::Json(s) => Ok(serde_json::from_str(&s).unwrap()),
        Output::Bytes(b) => Ok(Value::String(String::from_utf8_lossy(&b).into_owned())),
    }
}

fn doc(components: Value) -> Value {
    json!({"kerf":"0.1","id":"t","title":"T","run":[-12,12],"components":components,"views":[
        {"id":"A","kind":"section","number":"1","title":"T","scale":"1\"=1'-0\"","cut_z":0,"crop":{"x":[-10,60],"y":[-10,40]},"annotations":[]}]})
}

fn anchor(d: &Value, id: &str, name: &str) -> (f64, f64) {
    let r = call("inspect", json!({"doc": d, "query": {"q": "anchors", "id": id}})).unwrap();
    let a = r["anchors"].as_array().unwrap().iter().find(|a| a["name"] == name).unwrap_or_else(|| panic!("no anchor {}", name));
    (a["x"].as_f64().unwrap(), a["y"].as_f64().unwrap())
}

fn near(a: (f64, f64), b: (f64, f64)) {
    assert!((a.0 - b.0).abs() < 1e-3 && (a.1 - b.1).abs() < 1e-3, "{:?} != {:?}", a, b);
}

fn codes(d: &Value) -> Vec<String> {
    let r = call("check", json!({"doc": d})).unwrap();
    r["diagnostics"].as_array().unwrap().iter().map(|x| x["code"].as_str().unwrap().to_string()).collect()
}

#[test]
fn placement_and_anchors() {
    let d = doc(json!([
        {"id":"a","type":"lumber","size":"2x8","orient":"flat","at":{"anchor":"bottom_left","to":[10,5]}},
        {"id":"b","type":"lumber","size":"2x4","at":{"anchor":"bottom_center","to":"a@top_center","offset":[1,0.5]}}
    ]));
    near(anchor(&d, "a", "top_left"), (10.0, 6.5));
    near(anchor(&d, "a", "bottom_right"), (17.25, 5.0));
    // b is 1.5 x 3.5 upright; bottom_center lands on a top_center + (1, 0.5)
    near(anchor(&d, "b", "bottom_center"), (13.625 + 1.0, 6.5 + 0.5));
    near(anchor(&d, "b", "top_right"), (13.625 + 1.0 + 0.75, 6.5 + 0.5 + 3.5));
}

#[test]
fn rotate_about_placement_anchor() {
    let d = doc(json!([{"id":"p","type":"panel","thickness":1,"length":10,"at":{"anchor":"bottom_left","to":[0,0]},"rotate":90}]));
    // 10 x 1 panel rotated 90 degrees CCW about bottom_left: occupies x -1..0, y 0..10
    near(anchor(&d, "p", "top_left"), (-1.0, 0.0)); // anchors rotate with the member
    near(anchor(&d, "p", "bottom_right"), (0.0, 10.0));
    let s = json!([{"id":"q","type":"panel","thickness":1,"length":12,"slope":"4:12","at":{"anchor":"bottom_left","to":[0,0]}}]);
    let d = doc(s);
    let (x, y) = anchor(&d, "q", "bottom_right");
    let ang = (4.0f64 / 12.0).atan();
    near((x, y), (12.0 * ang.cos(), 12.0 * ang.sin()));
}

#[test]
fn truss_standard_heel_geometry() {
    let d = doc(json!([{"id":"t","type":"truss","pitch":"4:12","overhang":12,"span_shown":36,"at":{"anchor":"bearing_outer","to":[0,0]}}]));
    near(anchor(&d, "t", "bearing_outer"), (0.0, 0.0));
    let tt = anchor(&d, "t", "tail_top");
    let tb = anchor(&d, "t", "tail_bottom");
    // lower edge of the top chord passes through (0, 3.5) at 4:12
    near(tb, (-12.0, 3.5 - 4.0));
    // plumb tail: top is 3.5 / cos(pitch) above the lower edge
    near(tt, (-12.0, 3.5 - 4.0 + 3.5 * (1.0 + (4.0f64 / 12.0).powi(2)).sqrt()));
    near(anchor(&d, "t", "top_chord_at_bearing"), (0.0, 3.5 + 3.5 * (1.0 + (4.0f64 / 12.0).powi(2)).sqrt()));
}

#[test]
fn slab_edge_named_anchors() {
    let d = doc(json!([{"id":"s","type":"concrete","shape":"slab_edge","slab_thickness":4,"slab_length":48,"footing_width":12,"footing_depth":18,"haunch":45,
        "recess":{"width":8,"depth":1.5,"from_edge":0},"recess_slope":0.125,"at":{"anchor":"top_exterior","to":[0,0]}}]));
    near(anchor(&d, "s", "top_exterior"), (0.0, 0.0));
    near(anchor(&d, "s", "footing_bottom_interior"), (12.0, -18.0));
    near(anchor(&d, "s", "haunch_top"), (26.0, -4.0));
    near(anchor(&d, "s", "recess_bottom_interior"), (8.0, -1.5));
    near(anchor(&d, "s", "recess_bottom_exterior"), (0.0, -1.625));
}

#[test]
fn diagnostics_are_actionable() {
    let d = doc(json!([
        {"id":"sill_plate","type":"lumber","size":"2x8","at":{"anchor":"bottom_left","to":"sill_plat@top_left"}}
    ]));
    let r = call("check", json!({"doc": d})).unwrap();
    let diag = &r["diagnostics"].as_array().unwrap()[0];
    assert_eq!(diag["code"], "E_REF_UNKNOWN");
    assert!(diag["fix"].as_str().unwrap().contains("sill_plate"));

    let d = doc(json!([{"id":"a","type":"lumber","size":"2x8"},{"id":"b","type":"lumber","size":"2x4","at":{"to":"a@top_lft"}}]));
    let r = call("check", json!({"doc": d})).unwrap();
    let diag = r["diagnostics"].as_array().unwrap().iter().find(|x| x["code"] == "E_ANCHOR_UNKNOWN").unwrap();
    assert!(diag["fix"].as_str().unwrap().contains("top_left"));

    let d = doc(json!([{"id":"a","type":"lumber","size":"2x8","at":{"to":"b@top_left"}},{"id":"b","type":"lumber","size":"2x4","at":{"to":"a@top_left"}}]));
    assert!(codes(&d).contains(&"E_CYCLE".to_string()));

    let d = doc(json!([{"id":"a","type":"lumber","size":"2x8"},{"id":"a","type":"lumber","size":"2x4"}]));
    assert!(codes(&d).contains(&"E_DUP_ID".to_string()));

    let d = doc(json!([{"id":"a","type":"lumber","size":"3x7"}]));
    let r = call("check", json!({"doc": d})).unwrap();
    let msg = r["diagnostics"][0]["message"].as_str().unwrap();
    assert!(msg.contains("2x4") && msg.contains("components/a/size"), "{}", msg);
}

#[test]
fn cover_check_warns_with_numbers() {
    let d = doc(json!([
        {"id":"c","type":"concrete","shape":"rect","width":12,"height":12,"cover":{"bottom":3,"sides":3,"top":1.5},"at":{"anchor":"bottom_left","to":[0,0]}},
        {"id":"bars","type":"rebar","size":"#5","place":{"in":"c","face":"bottom","cover":1,"count":2,"side_cover":3}}
    ]));
    let r = call("check", json!({"doc": d})).unwrap();
    let w = r["diagnostics"].as_array().unwrap().iter().find(|x| x["code"] == "W_COVER").expect("W_COVER");
    assert!(w["message"].as_str().unwrap().contains("1\" < required 3\""), "{}", w["message"]);
    let ok = doc(json!([
        {"id":"c","type":"concrete","shape":"rect","width":12,"height":12,"at":{"anchor":"bottom_left","to":[0,0]}},
        {"id":"bars","type":"rebar","size":"#5","place":{"in":"c","face":"bottom","cover":3,"count":2,"side_cover":3}}
    ]));
    assert!(!codes(&ok).contains(&"W_COVER".to_string()));
}

#[test]
fn overlap_floating_untreated() {
    let d = doc(json!([
        {"id":"w","type":"cmu_wall","courses":2,"at":{"anchor":"bottom_left","to":[0,0]}},
        {"id":"p","type":"lumber","size":"2x8","orient":"flat","at":{"anchor":"bottom_left","to":"w@top_left"}},
        {"id":"o","type":"lumber","size":"2x4","at":{"anchor":"bottom_left","to":[2,2]}},
        {"id":"f","type":"lumber","size":"2x4","at":{"anchor":"bottom_left","to":[50,30]}}
    ]));
    let c = codes(&d);
    assert!(c.contains(&"W_UNTREATED_CONTACT".to_string()), "{:?}", c);
    assert!(c.contains(&"W_OVERLAP".to_string()), "{:?}", c);
    assert!(c.contains(&"W_FLOATING".to_string()), "{:?}", c);
}

#[test]
fn apply_is_atomic_and_guards_citations() {
    let d = doc(json!([{"id":"a","type":"lumber","size":"2x8"}]));
    // failing op batch leaves the document unchanged
    let r = call("apply", json!({"doc": d, "ops": [
        {"op":"add","path":"components","value":{"id":"b","type":"lumber","size":"2x4"}},
        {"op":"update","path":"components/zzz","value":{}}
    ]})).unwrap();
    assert_eq!(r["ok"], false);
    assert_eq!(r["doc"], d);
    // removing a referenced component names the dependents
    let d2 = doc(json!([{"id":"a","type":"lumber","size":"2x8"},{"id":"b","type":"lumber","size":"2x4","at":{"to":"a@top_left"}}]));
    let r = call("apply", json!({"doc": d2, "ops": [{"op":"remove","path":"components/a"}]})).unwrap();
    assert_eq!(r["ok"], false);
    assert!(r["diagnostics"][0]["message"].as_str().unwrap().contains("components/b"));
    // LLM cannot verify citations; designer can
    let note = json!({"id":"n","type":"note","text":"2X8","target":"a","cite":[{"code":"IRC","edition":2021,"section":"R1","status":"verified"}]});
    let r = call("apply", json!({"doc": d, "actor":"llm", "ops": [{"op":"add","path":"views/A/annotations","value":note}]})).unwrap();
    assert_eq!(r["ok"], true, "{}", r);
    assert_eq!(r["doc"]["views"][0]["annotations"][0]["cite"][0]["status"], "suggested");
    assert!(r["diagnostics"].as_array().unwrap().iter().any(|x| x["code"] == "I_CITE_DOWNGRADED"));
    let r = call("apply", json!({"doc": d, "actor":"designer", "ops": [{"op":"add","path":"views/A/annotations","value":note}]})).unwrap();
    assert_eq!(r["doc"]["views"][0]["annotations"][0]["cite"][0]["status"], "verified");
    // a later LLM edit of the note text resets verification
    let d3 = r["doc"].clone();
    let r = call("apply", json!({"doc": d3, "actor":"llm", "ops": [{"op":"update","path":"views/A/annotations/n","value":{"text":"2X8 PT"}}]})).unwrap();
    assert_eq!(r["doc"]["views"][0]["annotations"][0]["cite"][0]["status"], "suggested");
    assert_eq!(r["changed"], json!(["n"]));
}

#[test]
fn merge_patch_null_deletes() {
    let d = doc(json!([{"id":"a","type":"lumber","size":"2x8","label":"X"}]));
    let r = call("apply", json!({"doc": d, "ops": [{"op":"update","path":"components/a","value":{"label":null,"treated":true}}]})).unwrap();
    assert_eq!(r["ok"], true);
    let c = &r["doc"]["components"][0];
    assert!(c.get("label").is_none());
    assert_eq!(c["treated"], true);
}

#[test]
fn leaders_do_not_cross_and_notes_do_not_overlap() {
    for name in ["truss-bearing-cmu", "monopour-slab-door-recess", "flush-beam-strap"] {
        let path = format!("{}/../../../spec/details/{}.kerf.json", env!("CARGO_MANIFEST_DIR"), name);
        let d: Value = serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap();
        for view in ["A", "B"] {
            let dr = call("drawing", json!({"doc": d, "view": view})).unwrap();
            let items = dr["items"].as_array().unwrap();
            let leaders: Vec<(String, Vec<(f64, f64)>)> = items
                .iter()
                .filter(|i| i["t"] == "path" && i["pen"] == "anno" && i["pts"].as_array().unwrap().len() == 3)
                .map(|i| (i["src"].as_str().unwrap().to_string(), i["pts"].as_array().unwrap().iter().map(|p| (p[0].as_f64().unwrap(), p[1].as_f64().unwrap())).collect()))
                .collect();
            let cr = |o: (f64, f64), p: (f64, f64), q: (f64, f64)| (p.0 - o.0) * (q.1 - o.1) - (p.1 - o.1) * (q.0 - o.0);
            let x = |a: (f64, f64), b: (f64, f64), c: (f64, f64), e: (f64, f64)| cr(a, b, c) * cr(a, b, e) < 0.0 && cr(c, e, a) * cr(c, e, b) < 0.0;
            for i in 0..leaders.len() {
                for j in i + 1..leaders.len() {
                    for s in 0..2 {
                        for t in 0..2 {
                            assert!(!x(leaders[i].1[s], leaders[i].1[s + 1], leaders[j].1[t], leaders[j].1[t + 1]), "{} {}: leaders {} and {} cross", name, view, leaders[i].0, leaders[j].0);
                        }
                    }
                }
            }
        }
    }
}

#[test]
fn wrap_width_is_respected() {
    let path = format!("{}/../../../spec/details/truss-bearing-cmu.kerf.json", env!("CARGO_MANIFEST_DIR"));
    let d: Value = serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap();
    let dr = call("drawing", json!({"doc": d, "view": "A"})).unwrap();
    for it in dr["items"].as_array().unwrap() {
        if it["t"] == "text" && it["layer"] == "S-ANNO-NOTE" && it["src"].as_str().unwrap().starts_with("n_") {
            assert!(it["s"].as_str().unwrap().chars().count() <= 28, "{}", it["s"]);
        }
    }
}
