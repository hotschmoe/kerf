//! Headless screenshot mode (native): renders the full UI for one frame offscreen with wgpu and
//! writes a PNG. Doubles as the CI smoke test.
//!
//!   kerf-egui --screenshot out.png [--doc truss-bearing-cmu|path] [--tab A|B|3d|sheet]
//!             [--select ID] [--insp parts|notes|diff] [--size 1440x860] [--ppp 1] [--demo-turns N]
//!             [--settings] [--frames 4]

use crate::KerfApp;
use eframe::egui_wgpu::{Renderer, RendererOptions, ScreenDescriptor};
use std::time::Instant;

fn arg<'a>(args: &'a [String], name: &str) -> Option<&'a str> {
    args.iter().position(|a| a == name).and_then(|i| args.get(i + 1)).map(String::as_str)
}

pub fn run(args: &[String]) -> Result<(), String> {
    let out_path = arg(args, "--screenshot").ok_or("--screenshot needs a path")?.to_owned();
    let (w, h) = arg(args, "--size")
        .and_then(|s| s.split_once('x'))
        .and_then(|(a, b)| Some((a.parse::<u32>().ok()?, b.parse::<u32>().ok()?)))
        .unwrap_or((1440, 860));
    let ppp: f32 = arg(args, "--ppp").and_then(|s| s.parse().ok()).unwrap_or(1.0);
    let frames: usize = arg(args, "--frames").and_then(|s| s.parse().ok()).unwrap_or(4);
    let (pw, ph) = ((w as f32 * ppp) as u32, (h as f32 * ppp) as u32);

    // ---- wgpu without a surface
    let instance = wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle());
    let adapter = pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions { power_preference: wgpu::PowerPreference::HighPerformance, ..Default::default() }))
        .map_err(|e| format!("no wgpu adapter: {e}"))?;
    let info = adapter.get_info();
    eprintln!("adapter: {} ({:?}, {:?})", info.name, info.device_type, info.backend);
    let (device, queue) = pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor {
        required_limits: wgpu::Limits::default().using_resolution(adapter.limits()),
        ..Default::default()
    }))
    .map_err(|e| format!("request_device: {e}"))?;

    let format = wgpu::TextureFormat::Rgba8Unorm;
    let mut renderer = Renderer::new(&device, format, RendererOptions::default());
    renderer.callback_resources.insert(crate::gpu3d::Gpu3d::new(&device, format));

    // ---- app
    let ctx = egui::Context::default();
    let mut app = KerfApp::new(&ctx);
    app.has_gpu = true;
    if args.iter().any(|a| a == "--demo") || arg(args, "--demo-turns").is_some() {
        app.enable_demo(true);
    }
    if let Some(d) = arg(args, "--doc") {
        app.open_by_name(d);
    }
    app.apply_view_args(arg(args, "--tab"), arg(args, "--select"), arg(args, "--insp"));
    if args.iter().any(|a| a == "--settings") {
        app.settings_open = true;
    }
    let mut run_frame = |app: &mut KerfApp, t: f64, renderer: &mut Renderer, ctx: &egui::Context, events: Vec<egui::Event>| -> egui::FullOutput {
        let mut raw = egui::RawInput::default();
        raw.screen_rect = Some(egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(w as f32, h as f32)));
        raw.time = Some(t);
        raw.events = events;
        raw.viewports.entry(egui::ViewportId::ROOT).or_default().native_pixels_per_point = Some(ppp);
        let out = ctx.run_ui(raw, |ui| app.draw(ui));
        for (id, deltas) in &out.textures_delta.set {
            for delta in deltas {
                renderer.update_texture(&device, &queue, *id, delta);
            }
        }
        out
    };

    // optional scripted chat turns (demo mode): send, then pump until idle
    let mut t = 0.0;
    if let Some(n) = arg(args, "--demo-turns").and_then(|s| s.parse::<usize>().ok()) {
        for k in 0..n {
            app.input = if k == 0 { "need a detail of a prefab truss bearing on an 8 inch CMU wall w/ bond beam".into() } else { "remove the bird blocking note".into() };
            app.send_chat();
            let start = Instant::now();
            while app.chat.busy() && start.elapsed().as_secs() < 20 {
                t += 0.05;
                app.headless_tick(&ctx);
                let _ = run_frame(&mut app, t, &mut renderer, &ctx, vec![]);
                std::thread::sleep(std::time::Duration::from_millis(30));
            }
        }
        if arg(args, "--tab").is_none() {
            app.apply_view_args(Some("A"), None, None);
        }
    }

    let t_start = Instant::now();
    let mut last = None;
    let mut frame_times = Vec::new();
    for _ in 0..frames.max(1) {
        t += 0.1;
        let f0 = Instant::now();
        app.headless_tick(&ctx);
        last = Some(run_frame(&mut app, t, &mut renderer, &ctx, vec![]));
        frame_times.push(f0.elapsed().as_secs_f32() * 1000.0);
    }
    let out = last.unwrap();
    eprintln!("ui frame ms: {:?}", frame_times.iter().map(|x| (x * 10.0).round() / 10.0).collect::<Vec<_>>());

    // ---- render the last frame
    let jobs = ctx.tessellate(out.shapes, out.pixels_per_point);
    let screen = ScreenDescriptor { size_in_pixels: [pw, ph], pixels_per_point: out.pixels_per_point };
    let target = device.create_texture(&wgpu::TextureDescriptor {
        label: Some("shot"),
        size: wgpu::Extent3d { width: pw, height: ph, depth_or_array_layers: 1 },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format,
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
        view_formats: &[],
    });
    let view = target.create_view(&Default::default());
    let mut encoder = device.create_command_encoder(&Default::default());
    let extra = renderer.update_buffers(&device, &queue, &mut encoder, &jobs, &screen);
    {
        let pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("shot"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: &view,
                depth_slice: None,
                resolve_target: None,
                ops: wgpu::Operations { load: wgpu::LoadOp::Clear(wgpu::Color { r: 0.949, g: 0.937, b: 0.902, a: 1.0 }), store: wgpu::StoreOp::Store },
            })],
            depth_stencil_attachment: None,
            timestamp_writes: None,
            occlusion_query_set: None,
            multiview_mask: None,
        });
        renderer.render(&mut pass.forget_lifetime(), &jobs, &screen);
    }
    let bpr = (pw * 4).div_ceil(256) * 256;
    let buf = device.create_buffer(&wgpu::BufferDescriptor { label: Some("readback"), size: (bpr * ph) as u64, usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ, mapped_at_creation: false });
    encoder.copy_texture_to_buffer(
        wgpu::TexelCopyTextureInfo { texture: &target, mip_level: 0, origin: wgpu::Origin3d::ZERO, aspect: wgpu::TextureAspect::All },
        wgpu::TexelCopyBufferInfo { buffer: &buf, layout: wgpu::TexelCopyBufferLayout { offset: 0, bytes_per_row: Some(bpr), rows_per_image: Some(ph) } },
        wgpu::Extent3d { width: pw, height: ph, depth_or_array_layers: 1 },
    );
    queue.submit(extra.into_iter().chain(Some(encoder.finish())));
    let slice = buf.slice(..);
    let (tx, rx) = std::sync::mpsc::channel();
    slice.map_async(wgpu::MapMode::Read, move |r| {
        let _ = tx.send(r);
    });
    device.poll(wgpu::PollType::wait_indefinitely()).map_err(|e| format!("poll: {e:?}"))?;
    rx.recv().map_err(|e| e.to_string())?.map_err(|e| format!("map: {e:?}"))?;
    let data = slice.get_mapped_range().map_err(|e| format!("map range: {e:?}"))?;
    let mut img = image::RgbaImage::new(pw, ph);
    for y in 0..ph as usize {
        let row = &data[y * bpr as usize..y * bpr as usize + (pw * 4) as usize];
        for x in 0..pw as usize {
            img.put_pixel(x as u32, y as u32, image::Rgba([row[x * 4], row[x * 4 + 1], row[x * 4 + 2], 255]));
        }
    }
    drop(data);
    img.save(&out_path).map_err(|e| format!("write {out_path}: {e}"))?;
    eprintln!("wrote {out_path} ({pw}x{ph}) in {:.0} ms total", t_start.elapsed().as_secs_f32() * 1000.0);
    Ok(())
}
