//! Embeds the built web UI into `kerf serve`: every file below the directory named by `KERF_UI` (for example
//! `KERF_UI=../../apps/web/dist-serve cargo build --release`) becomes an `include_bytes!` entry; without it the table
//! is empty and the server answers with a page saying so.

use std::env;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

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

fn main() -> io::Result<()> {
    println!("cargo:rerun-if-env-changed=KERF_UI");
    let mut files = Vec::new();
    if let Some(dir) = env::var_os("KERF_UI").filter(|d| !d.is_empty()) {
        let root = fs::canonicalize(PathBuf::from(dir))?;
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
