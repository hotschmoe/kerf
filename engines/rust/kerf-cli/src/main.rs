//! `kerf` command line (SPEC section 13.2).

use kerf_core::api::{self, Output};
use serde_json::{Value, json};
use std::io::{Read, Write};
use std::process::ExitCode;

fn usage() -> &'static str {
    "kerf: Kerf drafting engine (kerf-rust)\n\
     usage:\n\
     \x20 kerf version\n\
     \x20 kerf catalog [--markdown]\n\
     \x20 kerf fmt <doc> [-w]\n\
     \x20 kerf check <doc> [--style S]\n\
     \x20 kerf apply <doc> <ops.json> [--style S] [-o out.kerf.json]\n\
     \x20 kerf drawing <doc> --view A [--style S] [-o out.json]\n\
     \x20 kerf export <doc> --view A --format svg|dxf|pdf [--style S] [--sheet] -o <file>\n\
     \x20 kerf mesh <doc> [-o mesh.json]\n\
     \x20 kerf call <fn> < input.json\n"
}

struct Args {
    pos: Vec<String>,
    view: Option<String>,
    format: Option<String>,
    style: Option<String>,
    out: Option<String>,
    sheet: bool,
    markdown: bool,
    write: bool,
    actor: Option<String>,
}

fn parse_args(raw: &[String]) -> Result<Args, String> {
    let mut a = Args { pos: vec![], view: None, format: None, style: None, out: None, sheet: false, markdown: false, write: false, actor: None };
    let mut i = 0;
    while i < raw.len() {
        let s = &raw[i];
        let mut next = |name: &str| -> Result<String, String> {
            i += 1;
            raw.get(i).cloned().ok_or_else(|| format!("{} needs a value", name))
        };
        match s.as_str() {
            "--view" => a.view = Some(next("--view")?),
            "--format" => a.format = Some(next("--format")?),
            "--style" => a.style = Some(next("--style")?),
            "-o" | "--out" => a.out = Some(next("-o")?),
            "--actor" => a.actor = Some(next("--actor")?),
            "--sheet" | "--with-sheet" => a.sheet = true,
            "--markdown" => a.markdown = true,
            "-w" => a.write = true,
            _ if s.starts_with("--") => return Err(format!("unknown option {}", s)),
            _ => a.pos.push(s.clone()),
        }
        i += 1;
    }
    Ok(a)
}

fn read_text(path: &str) -> Result<String, String> {
    if path == "-" {
        let mut s = String::new();
        std::io::stdin().read_to_string(&mut s).map_err(|e| e.to_string())?;
        return Ok(s);
    }
    std::fs::read_to_string(path).map_err(|e| format!("cannot read {}: {}", path, e))
}

fn read_json(path: &str) -> Result<Value, String> {
    let t = read_text(path)?;
    serde_json::from_str(&t).map_err(|e| format!("{}: invalid JSON: {} (line {}, column {})", path, e, e.line(), e.column()))
}

fn style_value(a: &Args) -> Result<Value, String> {
    match &a.style {
        Some(p) => read_json(p),
        None => Ok(Value::Null),
    }
}

fn write_out(path: &Option<String>, data: &[u8]) -> Result<(), String> {
    match path {
        Some(p) => std::fs::write(p, data).map_err(|e| format!("cannot write {}: {}", p, e)),
        None => std::io::stdout().write_all(data).map_err(|e| e.to_string()),
    }
}

fn output_text(o: Output) -> Result<String, String> {
    String::from_utf8(o.bytes()).map_err(|e| e.to_string())
}

fn run(args: Vec<String>) -> Result<u8, String> {
    let Some(cmd) = args.first() else {
        print!("{}", usage());
        return Ok(2);
    };
    let a = parse_args(&args[1..])?;
    match cmd.as_str() {
        "version" => {
            println!("{}", output_text(api::call("version", "{}")?)?);
            Ok(0)
        }
        "catalog" => {
            let fmt = if a.markdown { "markdown" } else { "json" };
            let out = output_text(api::call("catalog", &json!({"format": fmt}).to_string())?)?;
            if a.markdown {
                let v: Value = serde_json::from_str(&out).map_err(|e| e.to_string())?;
                print!("{}", v.as_str().unwrap_or(""));
            } else {
                println!("{}", out);
            }
            Ok(0)
        }
        "fmt" => {
            let path = a.pos.first().ok_or("fmt needs a document path")?;
            let doc = read_json(path)?;
            let out = output_text(api::call("fmt", &json!({"doc": doc}).to_string())?)?;
            let v: Value = serde_json::from_str(&out).map_err(|e| e.to_string())?;
            let text = v["text"].as_str().unwrap_or("");
            if a.write {
                std::fs::write(path, text).map_err(|e| e.to_string())?;
            } else {
                print!("{}", text);
            }
            Ok(0)
        }
        "check" => {
            let path = a.pos.first().ok_or("check needs a document path")?;
            let doc = read_json(path)?;
            let out = output_text(api::call("check", &json!({"doc": doc, "style": style_value(&a)?}).to_string())?)?;
            let v: Value = serde_json::from_str(&out).map_err(|e| e.to_string())?;
            print!("{}", v["summary"].as_str().unwrap_or(""));
            let errs = v["diagnostics"].as_array().map(|d| d.iter().filter(|x| x["level"] == "error").count()).unwrap_or(0);
            Ok(if errs > 0 { 1 } else { 0 })
        }
        "apply" => {
            let path = a.pos.first().ok_or("apply needs a document path")?;
            let ops_path = a.pos.get(1).ok_or("apply needs an ops.json path")?;
            let doc = read_json(path)?;
            let ops = read_json(ops_path)?;
            let mut req = json!({"doc": doc, "style": style_value(&a)?, "ops": ops});
            if let Some(actor) = &a.actor {
                req["actor"] = json!(actor);
            }
            let out = output_text(api::call("apply", &req.to_string())?)?;
            let v: Value = serde_json::from_str(&out).map_err(|e| e.to_string())?;
            let ok = v["ok"].as_bool().unwrap_or(false);
            if ok {
                if let Some(p) = &a.out {
                    let text = kerf_core::json::pretty(&v["doc"]);
                    std::fs::write(p, text).map_err(|e| e.to_string())?;
                }
            }
            print!("{}", v["summary"].as_str().unwrap_or(""));
            if !ok {
                eprintln!("apply failed: document unchanged");
                if let Some(d) = v["diagnostics"].as_array() {
                    for x in d.iter().filter(|x| x["level"] == "error") {
                        eprintln!("ERROR {}: {}", x["code"].as_str().unwrap_or(""), x["message"].as_str().unwrap_or(""));
                    }
                }
            }
            Ok(if ok { 0 } else { 1 })
        }
        "drawing" => {
            let path = a.pos.first().ok_or("drawing needs a document path")?;
            let view = a.view.clone().ok_or("drawing needs --view <id>")?;
            let doc = read_json(path)?;
            let out = api::call("drawing", &json!({"doc": doc, "style": style_value(&a)?, "view": view}).to_string())?.bytes();
            write_out(&a.out, &out)?;
            Ok(0)
        }
        "export" => {
            let path = a.pos.first().ok_or("export needs a document path")?;
            let view = a.view.clone().ok_or("export needs --view <id>")?;
            let format = a.format.clone().unwrap_or_else(|| "svg".into());
            let doc = read_json(path)?;
            let out = api::call("export", &json!({"doc": doc, "style": style_value(&a)?, "view": view, "format": format, "sheet": a.sheet}).to_string())?.bytes();
            write_out(&a.out, &out)?;
            Ok(0)
        }
        "mesh" => {
            let path = a.pos.first().ok_or("mesh needs a document path")?;
            let doc = read_json(path)?;
            let out = api::call("mesh", &json!({"doc": doc, "style": style_value(&a)?}).to_string())?.bytes();
            write_out(&a.out, &out)?;
            Ok(0)
        }
        "call" => {
            let f = a.pos.first().ok_or("call needs a function name")?;
            let input = read_text("-")?;
            let out = api::call(f, &input)?.bytes();
            std::io::stdout().write_all(&out).map_err(|e| e.to_string())?;
            Ok(0)
        }
        "help" | "--help" | "-h" => {
            print!("{}", usage());
            Ok(0)
        }
        other => Err(format!("unknown command {:?}\n{}", other, usage())),
    }
}

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match run(args) {
        Ok(code) => ExitCode::from(code),
        Err(e) => {
            eprintln!("kerf: {}", e);
            ExitCode::from(1)
        }
    }
}
