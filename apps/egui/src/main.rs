//! Kerf rust-egui app: native (`cargo run -p kerf-egui --release`) and web (trunk -> wasm, WebGPU).

mod app;
mod chat;
mod console;
mod demo;
mod engine;
mod fmt;
mod gpu3d;
mod imaging;
mod inspector;
mod ir;
mod platform;
mod raster;
mod session;
mod theme;
mod view2d;
mod view3d;
#[cfg(not(target_arch = "wasm32"))]
mod headless;

pub use app::KerfApp;

/// Web: `window.__ready = true` once the first frame is drawn (tools/shot.mjs --wait-for).
pub fn set_ready_flag() {
    #[cfg(target_arch = "wasm32")]
    {
        if let Some(w) = web_sys::window() {
            let _ = js_sys::Reflect::set(&w, &"__ready".into(), &true.into());
        }
    }
}

#[cfg(not(target_arch = "wasm32"))]
fn main() -> eframe::Result {
    env_logger::init();
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.iter().any(|a| a == "--screenshot") {
        if let Err(e) = headless::run(&args) {
            eprintln!("screenshot failed: {e}");
            std::process::exit(1);
        }
        return Ok(());
    }
    let mut open_doc: Option<String> = None;
    let mut demo = false;
    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "--doc" => {
                open_doc = args.get(i + 1).cloned();
                i += 1;
            }
            "--demo" => demo = true,
            _ => {}
        }
        i += 1;
    }
    let native = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default().with_inner_size([1440.0, 860.0]).with_min_inner_size([480.0, 400.0]).with_title("KERF DETAIL WORKSTATION").with_drag_and_drop(true),
        wgpu_options: native_wgpu_options(),
        ..Default::default()
    };
    eframe::run_native(
        "kerf-egui",
        native,
        Box::new(move |cc| {
            let mut app = KerfApp::new(&cc.egui_ctx);
            app.restore(cc.storage);
            if let Some(rs) = &cc.wgpu_render_state {
                app::install_gpu(rs);
                app.has_gpu = true;
            }
            if demo {
                app.enable_demo(true);
            }
            if let Some(d) = open_doc {
                app.open_by_name(&d);
            }
            Ok(Box::new(app))
        }),
    )
}

#[cfg(not(target_arch = "wasm32"))]
fn native_wgpu_options() -> eframe::egui_wgpu::WgpuConfiguration {
    eframe::egui_wgpu::WgpuConfiguration::default()
}

#[cfg(target_arch = "wasm32")]
fn main() {
    console_error_panic_hook::set_once();
    use wasm_bindgen::JsCast;
    wasm_bindgen_futures::spawn_local(async {
        let document = web_sys::window().and_then(|w| w.document()).expect("document");
        let canvas = document.get_element_by_id("kerf_canvas").expect("#kerf_canvas").dyn_into::<web_sys::HtmlCanvasElement>().expect("canvas");
        let runner = eframe::WebRunner::new();
        let opts = eframe::WebOptions::default();
        let res = runner
            .start(
                canvas,
                opts,
                Box::new(|cc| {
                    let mut app = KerfApp::new(&cc.egui_ctx);
                    app.restore(cc.storage);
                    if let Some(rs) = &cc.wgpu_render_state {
                        app::install_gpu(rs);
                        app.has_gpu = true;
                    }
                    app.web_boot();
                    Ok(Box::new(app))
                }),
            )
            .await;
        if let Err(e) = res {
            if let Some(el) = document.get_element_by_id("unsupported") {
                let _ = el.set_attribute("style", "display:block");
            }
            web_sys::console::error_1(&format!("eframe failed: {e:?}").into());
        }
    });
}
