//! Kerf MCP server (stdio JSON-RPC 2.0).
//!
//! `kerf mcp [--dir D] [--style S]` (or the standalone `kerf-mcp` binary) lets any MCP client
//! (Claude Code, Claude Desktop, ...) build Kerf details through the same engine tools the
//! apps give Claude. The protocol layer is hand-rolled (see `server.rs`); everything else is
//! `kerf-core` plus `resvg` for PNG renders.

mod raster;
mod server;
mod store;
mod text;
mod tools;

pub use server::Server;
pub use store::Workspace;

const USAGE: &str = "usage: kerf mcp [--dir <workspace dir>] [--style <style.kerfstyle.json>]\n\
  Serves the Kerf MCP protocol over stdio. Documents live in <dir> (default: current directory)\n\
  as <id>.kerf.json with an op log <id>.kerf.log.jsonl.\n";

/// Entry point shared by `kerf mcp` and `kerf-mcp`. `args` are the arguments after the subcommand.
pub fn run_cli(args: &[String]) -> Result<(), String> {
    let mut dir: Option<String> = None;
    let mut style: Option<String> = None;
    let mut i = 0;
    while i < args.len() {
        let a = args[i].as_str();
        let (key, inline) = match a.split_once('=') {
            Some((k, v)) if k.starts_with("--") => (k, Some(v.to_string())),
            _ => (a, None),
        };
        match key {
            "--dir" | "-d" | "--style" => {
                let v = match inline {
                    Some(v) => v,
                    None => {
                        i += 1;
                        args.get(i).cloned().ok_or_else(|| format!("{} needs a value\n{}", key, USAGE))?
                    }
                };
                if key == "--style" { style = Some(v) } else { dir = Some(v) }
            }
            "-h" | "--help" | "help" => {
                print!("{}", USAGE);
                return Ok(());
            }
            other => return Err(format!("unknown argument {:?}\n{}", other, USAGE)),
        }
        i += 1;
    }
    let style_v = match style {
        Some(p) => {
            let t = std::fs::read_to_string(&p).map_err(|e| format!("cannot read style {}: {}", p, e))?;
            Some(kerf_core::json::parse(&t).map_err(|e| format!("style {}: {}", p, e))?)
        }
        None => None,
    };
    let ws = Workspace::new(dir.as_deref().unwrap_or("."), style_v)?;
    eprintln!("kerf-mcp {} serving {}", env!("CARGO_PKG_VERSION"), ws.dir().display());
    let mut srv = Server::new(ws);
    srv.serve_stdio();
    Ok(())
}
