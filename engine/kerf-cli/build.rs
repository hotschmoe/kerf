//! Embeds the built web UI into `kerf serve`: every file below the directory named by `KERF_UI` (for example
//! `KERF_UI=../apps/web/dist-serve cargo build --release`, relative to the workspace root) becomes an `include_bytes!`
//! entry; without it the table is empty and the server answers with a page saying so. It also stamps `git describe` of
//! the source tree into `KERF_BUILD` for `kerf version` (`KERF_BUILD` in the environment overrides it; "unknown"
//! outside a git checkout).

use std::env;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::process::Command;

fn walk(root: &Path, dir: &Path, out: &mut Vec<(String, PathBuf)>) -> io::Result<()> {
    for entry in fs::read_dir(dir)? {
        let entry = entry?;
        let path = entry.path();
        let kind = entry.file_type()?;
        if kind.is_dir() {
            walk(root, &path, out)?;
        } else if kind.is_file() {
            let rel = path.strip_prefix(root).map_err(io::Error::other)?;
            let rel: Vec<_> = rel.components().map(|c| c.as_os_str().to_string_lossy().into_owned()).collect();
            out.push((rel.join("/"), path));
        }
    }
    Ok(())
}

fn git(dir: &Path, args: &[&str]) -> Option<String> {
    let out = Command::new("git").arg("-C").arg(dir).args(args).output().ok()?;
    let text = String::from_utf8(out.stdout).ok()?;
    let text = text.trim();
    (out.status.success() && !text.is_empty()).then(|| text.to_string())
}

fn build_stamp() -> String {
    println!("cargo:rerun-if-env-changed=KERF_BUILD");
    if let Some(b) = env::var("KERF_BUILD").ok().filter(|b| !b.is_empty()) {
        return b;
    }
    let dir = Path::new(env!("CARGO_MANIFEST_DIR"));
    for p in ["HEAD", "index", "packed-refs"] {
        if let Some(f) = git(dir, &["rev-parse", "--git-path", p]) {
            println!("cargo:rerun-if-changed={}", dir.join(f).display());
        }
    }
    if let Some(r) = git(dir, &["symbolic-ref", "-q", "HEAD"])
        && let Some(f) = git(dir, &["rev-parse", "--git-path", &r])
    {
        println!("cargo:rerun-if-changed={}", dir.join(f).display());
    }
    git(dir, &["describe", "--tags", "--always", "--dirty"]).unwrap_or_else(|| "unknown".to_string())
}

fn main() -> io::Result<()> {
    let stamp = build_stamp();
    println!("cargo:rustc-env=KERF_BUILD={}", stamp.replace(['\n', '\r'], " "));
    println!("cargo:rerun-if-env-changed=KERF_UI");
    let mut files = Vec::new();
    if let Some(dir) = env::var_os("KERF_UI").filter(|d| !d.is_empty()) {
        let workspace = Path::new(env!("CARGO_MANIFEST_DIR")).join("..");
        let root = fs::canonicalize(workspace.join(dir))?;
        println!("cargo:rerun-if-changed={}", root.display());
        walk(&root, &root, &mut files)?;
    }
    files.sort();
    let mut src = String::from("pub static FILES: &[(&str, &[u8])] = &[\n");
    for (rel, path) in &files {
        src.push_str(&format!("    ({rel:?}, include_bytes!({:?})),\n", path.display().to_string()));
    }
    src.push_str("];\n");
    let out = PathBuf::from(env::var_os("OUT_DIR").ok_or_else(|| io::Error::other("OUT_DIR"))?);
    fs::write(out.join("ui_assets.rs"), src)
}
