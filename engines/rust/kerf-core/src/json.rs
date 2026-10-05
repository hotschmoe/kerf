//! Canonical JSON writing (SPEC section 16) and small Value helpers.

use crate::num::fmt_num;
use serde_json::{Map, Value};

fn write_num(out: &mut String, n: &serde_json::Number) {
    if let Some(i) = n.as_i64() {
        out.push_str(&i.to_string());
    } else if let Some(u) = n.as_u64() {
        out.push_str(&u.to_string());
    } else {
        out.push_str(&fmt_num(n.as_f64().unwrap_or(0.0)));
    }
}

fn write_str(out: &mut String, s: &str) {
    out.push_str(&serde_json::to_string(s).unwrap_or_else(|_| "\"\"".to_string()));
}

/// Compact JSON with canonical numbers; object key order is preserved.
pub fn compact(v: &Value) -> String {
    let mut out = String::new();
    write_compact(&mut out, v);
    out
}

pub fn write_compact(out: &mut String, v: &Value) {
    match v {
        Value::Null => out.push_str("null"),
        Value::Bool(b) => out.push_str(if *b { "true" } else { "false" }),
        Value::Number(n) => write_num(out, n),
        Value::String(s) => write_str(out, s),
        Value::Array(a) => {
            out.push('[');
            for (i, x) in a.iter().enumerate() {
                if i > 0 {
                    out.push(',');
                }
                write_compact(out, x);
            }
            out.push(']');
        }
        Value::Object(m) => {
            out.push('{');
            for (i, (k, x)) in m.iter().enumerate() {
                if i > 0 {
                    out.push(',');
                }
                write_str(out, k);
                out.push(':');
                write_compact(out, x);
            }
            out.push('}');
        }
    }
}

fn all_numbers(a: &[Value]) -> bool {
    a.iter().all(|x| x.is_number())
}

/// Canonical pretty JSON: 2-space indent, arrays of numbers inline, trailing newline.
pub fn pretty(v: &Value) -> String {
    let mut out = String::new();
    write_pretty(&mut out, v, 0);
    out.push('\n');
    out
}

fn indent(out: &mut String, n: usize) {
    for _ in 0..n {
        out.push_str("  ");
    }
}

pub fn write_pretty(out: &mut String, v: &Value, lvl: usize) {
    match v {
        Value::Array(a) => {
            if a.is_empty() {
                out.push_str("[]");
            } else if all_numbers(a) {
                out.push('[');
                for (i, x) in a.iter().enumerate() {
                    if i > 0 {
                        out.push_str(", ");
                    }
                    write_compact(out, x);
                }
                out.push(']');
            } else {
                out.push_str("[\n");
                for (i, x) in a.iter().enumerate() {
                    indent(out, lvl + 1);
                    write_pretty(out, x, lvl + 1);
                    if i + 1 < a.len() {
                        out.push(',');
                    }
                    out.push('\n');
                }
                indent(out, lvl);
                out.push(']');
            }
        }
        Value::Object(m) => {
            if m.is_empty() {
                out.push_str("{}");
            } else {
                out.push_str("{\n");
                let n = m.len();
                for (i, (k, x)) in m.iter().enumerate() {
                    indent(out, lvl + 1);
                    write_str(out, k);
                    out.push_str(": ");
                    write_pretty(out, x, lvl + 1);
                    if i + 1 < n {
                        out.push(',');
                    }
                    out.push('\n');
                }
                indent(out, lvl);
                out.push('}');
            }
        }
        other => write_compact(out, other),
    }
}

pub fn parse(s: &str) -> Result<Value, String> {
    serde_json::from_str::<Value>(s).map_err(|e| format!("invalid JSON: {} (line {}, column {})", e, e.line(), e.column()))
}

/// Build an object from (key, value) pairs preserving order.
pub fn obj(pairs: Vec<(&str, Value)>) -> Value {
    let mut m = Map::new();
    for (k, v) in pairs {
        m.insert(k.to_string(), v);
    }
    Value::Object(m)
}

pub fn num(x: f64) -> Value {
    // Numbers are rounded at write time; keep the raw value here but make -0 and NaN safe.
    if x.is_finite() {
        serde_json::Number::from_f64(x).map(Value::Number).unwrap_or(Value::Null)
    } else {
        Value::Null
    }
}

pub fn s(x: &str) -> Value {
    Value::String(x.to_string())
}

pub fn arr_nums(xs: &[f64]) -> Value {
    Value::Array(xs.iter().map(|&x| num(x)).collect())
}
