//! Persisted settings (API key, model, demo flag) without eframe's `persistence` feature
//! (which drags in `ron`): localStorage on the web, a JSON file in the config dir natively.

use serde::{Deserialize, Serialize};

#[derive(Default, Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct Settings {
    #[serde(default)]
    pub api_key: String,
    #[serde(default)]
    pub model: String,
    #[serde(default)]
    pub demo: bool,
}

const KEY: &str = "kerf-egui.settings";

#[cfg(not(target_arch = "wasm32"))]
fn path() -> Option<std::path::PathBuf> {
    let base = std::env::var_os("XDG_CONFIG_HOME").map(std::path::PathBuf::from).or_else(|| std::env::var_os("HOME").map(|h| std::path::PathBuf::from(h).join(".config")))?;
    Some(base.join("kerf-egui").join("settings.json"))
}

pub fn load() -> Settings {
    #[cfg(not(target_arch = "wasm32"))]
    {
        path().and_then(|p| std::fs::read_to_string(p).ok()).and_then(|t| serde_json::from_str(&t).ok()).unwrap_or_default()
    }
    #[cfg(target_arch = "wasm32")]
    {
        web_sys::window()
            .and_then(|w| w.local_storage().ok().flatten())
            .and_then(|s| s.get_item(KEY).ok().flatten())
            .and_then(|t| serde_json::from_str(&t).ok())
            .unwrap_or_default()
    }
}

pub fn save(s: &Settings) {
    let Ok(text) = serde_json::to_string(s) else { return };
    #[cfg(not(target_arch = "wasm32"))]
    {
        if let Some(p) = path() {
            if let Some(dir) = p.parent() {
                let _ = std::fs::create_dir_all(dir);
            }
            #[cfg(unix)]
            {
                use std::io::Write;
                use std::os::unix::fs::OpenOptionsExt;
                // the key is a secret: owner-only file
                if let Ok(mut f) = std::fs::OpenOptions::new().write(true).create(true).truncate(true).mode(0o600).open(&p) {
                    let _ = f.write_all(text.as_bytes());
                }
            }
            #[cfg(not(unix))]
            {
                let _ = std::fs::write(p, text);
            }
        }
    }
    #[cfg(target_arch = "wasm32")]
    {
        if let Some(st) = web_sys::window().and_then(|w| w.local_storage().ok().flatten()) {
            let _ = st.set_item(KEY, &text);
        }
    }
}
