//! Native/web differences behind one small API: file open/save/download, clipboard images
//! (web), and an inbox of results delivered to the app between frames.

use std::sync::mpsc::{Receiver, Sender, channel};

pub enum Incoming {
    OpenDoc { name: String, bytes: Vec<u8> },
    Attach { name: String, bytes: Vec<u8> },
    /// a dropped file; routed by extension
    Dropped { name: String, bytes: Vec<u8> },
    /// status line text after a save/download
    Saved(String),
    Error(String),
}

pub struct Platform {
    tx: Sender<Incoming>,
    pub rx: Receiver<Incoming>,
    ctx: egui::Context,
}

#[derive(Clone, Copy)]
pub enum Pick {
    Doc,
    Image,
}

impl Platform {
    pub fn new(ctx: egui::Context) -> Platform {
        let (tx, rx) = channel();
        Platform { tx, rx, ctx }
    }

    pub fn sender(&self) -> Sender<Incoming> {
        self.tx.clone()
    }

    /// Open a file picker. The result arrives later through `rx`.
    pub fn pick(&self, what: Pick) {
        let tx = self.tx.clone();
        let ctx = self.ctx.clone();
        let task = async move {
            let mut dlg = rfd::AsyncFileDialog::new();
            dlg = match what {
                Pick::Doc => dlg.add_filter("Kerf document", &["json"]).set_title("Open .kerf.json"),
                Pick::Image => dlg.add_filter("Image", &["png", "jpg", "jpeg"]).set_title("Attach screenshot"),
            };
            if let Some(f) = dlg.pick_file().await {
                let name = f.file_name();
                let bytes = f.read().await;
                let _ = tx.send(match what {
                    Pick::Doc => Incoming::OpenDoc { name, bytes },
                    Pick::Image => Incoming::Attach { name, bytes },
                });
                ctx.request_repaint();
            }
        };
        spawn(task);
    }

    /// Save bytes: native = save dialog; web = browser download.
    pub fn save(&self, name: &str, bytes: Vec<u8>, mime: &str) {
        #[cfg(not(target_arch = "wasm32"))]
        {
            let _ = mime;
            let tx = self.tx.clone();
            let ctx = self.ctx.clone();
            let name = name.to_owned();
            std::thread::spawn(move || {
                // headless / no portal: fall back to the working directory so exports still land
                let path = rfd::FileDialog::new().set_file_name(&name).save_file().unwrap_or_else(|| std::path::PathBuf::from(&name));
                let size = bytes.len();
                let msg = match std::fs::write(&path, &bytes) {
                    Ok(()) => Incoming::Saved(format!("WROTE {} ({})", path.display().to_string().to_uppercase(), human_size(size))),
                    Err(e) => Incoming::Error(format!("SAVE FAILED: {e}")),
                };
                let _ = tx.send(msg);
                ctx.request_repaint();
            });
        }
        #[cfg(target_arch = "wasm32")]
        {
            let msg = match web_download(name, &bytes, mime) {
                Ok(()) => Incoming::Saved(format!("EXPORTED {} ({})", name.to_uppercase(), human_size(bytes.len()))),
                Err(e) => Incoming::Error(format!("DOWNLOAD FAILED: {e}")),
            };
            let _ = self.tx.send(msg);
            self.ctx.request_repaint();
        }
    }

    /// Images pasted on the web (JS side queues them in `window.__kerf_paste`).
    pub fn poll_paste(&self) {
        #[cfg(target_arch = "wasm32")]
        {
            use wasm_bindgen::JsCast;
            let Some(win) = web_sys::window() else { return };
            let Ok(q) = js_sys::Reflect::get(&win, &"__kerf_paste".into()) else { return };
            let Ok(arr) = q.dyn_into::<js_sys::Array>() else { return };
            while arr.length() > 0 {
                let item = arr.shift();
                let name = js_sys::Reflect::get(&item, &"name".into()).ok().and_then(|v| v.as_string()).unwrap_or_else(|| "pasted.png".into());
                let b64 = js_sys::Reflect::get(&item, &"b64".into()).ok().and_then(|v| v.as_string()).unwrap_or_default();
                use base64::Engine;
                if let Ok(bytes) = base64::engine::general_purpose::STANDARD.decode(b64) {
                    let _ = self.tx.send(Incoming::Attach { name, bytes });
                }
            }
        }
    }
}

pub fn human_size(n: usize) -> String {
    if n >= 1 << 20 {
        format!("{:.1} MB", n as f64 / (1 << 20) as f64)
    } else if n >= 1024 {
        format!("{} KB", (n + 512) / 1024)
    } else {
        format!("{n} B")
    }
}

#[cfg(not(target_arch = "wasm32"))]
fn spawn(f: impl std::future::Future<Output = ()> + Send + 'static) {
    std::thread::spawn(move || pollster::block_on(f));
}

#[cfg(target_arch = "wasm32")]
fn spawn(f: impl std::future::Future<Output = ()> + 'static) {
    wasm_bindgen_futures::spawn_local(f);
}

#[cfg(target_arch = "wasm32")]
fn web_download(name: &str, bytes: &[u8], mime: &str) -> Result<(), String> {
    use wasm_bindgen::JsCast;
    let js = |e: wasm_bindgen::JsValue| format!("{e:?}");
    let arr = js_sys::Uint8Array::from(bytes);
    let parts = js_sys::Array::new();
    parts.push(&arr);
    let opts = web_sys::BlobPropertyBag::new();
    opts.set_type(mime);
    let blob = web_sys::Blob::new_with_u8_array_sequence_and_options(&parts, &opts).map_err(js)?;
    let url = web_sys::Url::create_object_url_with_blob(&blob).map_err(js)?;
    let doc = web_sys::window().and_then(|w| w.document()).ok_or("no document")?;
    let a: web_sys::HtmlAnchorElement = doc.create_element("a").map_err(js)?.dyn_into().map_err(|_| "not an anchor")?;
    a.set_href(&url);
    a.set_download(name);
    a.click();
    let _ = web_sys::Url::revoke_object_url(&url);
    Ok(())
}
