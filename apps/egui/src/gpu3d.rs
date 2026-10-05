//! The 3D viewport renderer. egui_wgpu paint callback: `prepare` renders the scene (flat-shaded
//! faces + constant-pixel-width feature edges + ground grid) into an offscreen MSAA colour+depth
//! target and resolves it; `paint` blits the resolved texture into the viewport rect.

use crate::ir::{Mesh, parse_hex};
use eframe::egui_wgpu::{CallbackResources, CallbackTrait, ScreenDescriptor};
use glam::{Mat4, Vec3};
use std::sync::Arc;
use wgpu::util::DeviceExt;

const DEPTH: wgpu::TextureFormat = wgpu::TextureFormat::Depth32Float;
const MSAA: u32 = 4;

#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
struct Globals {
    view_proj: [[f32; 4]; 4],
    light: [f32; 4],
    viewport: [f32; 2],
    srgb: f32,
    line_bias: f32,
    clip: [f32; 4],
}

#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
struct Vtx {
    pos: [f32; 3],
    nor: [f32; 3],
    col: [f32; 4],
}

#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
struct LineInst {
    a: [f32; 3],
    w: f32,
    b: [f32; 3],
    pad: f32,
    col: [f32; 4],
}

struct Scene {
    key: SceneKey,
    vbuf: wgpu::Buffer,
    ibuf: wgpu::Buffer,
    n_idx: u32,
    lbuf: wgpu::Buffer,
    n_lines: u32,
}

#[derive(Clone, PartialEq)]
struct SceneKey {
    rev: u64,
    selected: Option<String>,
    hover: Option<String>,
    grid_y: i32,
    cut: Option<i32>,
}

struct Targets {
    size: [u32; 2],
    msaa_view: wgpu::TextureView,
    depth_view: wgpu::TextureView,
    resolve_view: wgpu::TextureView,
    blit_bg: wgpu::BindGroup,
}

pub struct Gpu3d {
    format: wgpu::TextureFormat,
    mesh_pipe: wgpu::RenderPipeline,
    line_pipe: wgpu::RenderPipeline,
    blit_pipe: wgpu::RenderPipeline,
    globals: wgpu::Buffer,
    globals_bg: wgpu::BindGroup,
    blit_bgl: wgpu::BindGroupLayout,
    sampler: wgpu::Sampler,
    scene: Option<Scene>,
    targets: Option<Targets>,
}

impl Gpu3d {
    pub fn new(device: &wgpu::Device, format: wgpu::TextureFormat) -> Gpu3d {
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("kerf3d"),
            source: wgpu::ShaderSource::Wgsl(include_str!("shader3d.wgsl").into()),
        });
        let globals = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("kerf3d globals"),
            size: std::mem::size_of::<Globals>() as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let g_bgl = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("kerf3d g"),
            entries: &[wgpu::BindGroupLayoutEntry {
                binding: 0,
                visibility: wgpu::ShaderStages::VERTEX | wgpu::ShaderStages::FRAGMENT,
                ty: wgpu::BindingType::Buffer { ty: wgpu::BufferBindingType::Uniform, has_dynamic_offset: false, min_binding_size: None },
                count: None,
            }],
        });
        let globals_bg = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("kerf3d g"),
            layout: &g_bgl,
            entries: &[wgpu::BindGroupEntry { binding: 0, resource: globals.as_entire_binding() }],
        });
        let layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("kerf3d"),
            bind_group_layouts: &[Some(&g_bgl)],
            ..Default::default()
        });
        let target = [Some(wgpu::ColorTargetState { format, blend: Some(wgpu::BlendState::REPLACE), write_mask: wgpu::ColorWrites::ALL })];
        let depth = |bias: wgpu::DepthBiasState| {
            Some(wgpu::DepthStencilState {
                format: DEPTH,
                depth_write_enabled: Some(true),
                depth_compare: Some(wgpu::CompareFunction::LessEqual),
                stencil: wgpu::StencilState::default(),
                bias,
            })
        };
        let ms = wgpu::MultisampleState { count: MSAA, mask: !0, alpha_to_coverage_enabled: false };
        let mesh_pipe = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("kerf3d mesh"),
            layout: Some(&layout),
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: Some("vs_mesh"),
                compilation_options: Default::default(),
                buffers: &[wgpu::VertexBufferLayout {
                    array_stride: std::mem::size_of::<Vtx>() as u64,
                    step_mode: wgpu::VertexStepMode::Vertex,
                    attributes: &wgpu::vertex_attr_array![0 => Float32x3, 1 => Float32x3, 2 => Float32x4],
                }],
            },
            fragment: Some(wgpu::FragmentState { module: &shader, entry_point: Some("fs_mesh"), compilation_options: Default::default(), targets: &target }),
            primitive: wgpu::PrimitiveState { cull_mode: None, ..Default::default() },
            depth_stencil: depth(wgpu::DepthBiasState { constant: 2, slope_scale: 1.5, clamp: 0.0 }),
            multisample: ms,
            multiview_mask: None,
            cache: None,
        });
        let line_pipe = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("kerf3d line"),
            layout: Some(&layout),
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: Some("vs_line"),
                compilation_options: Default::default(),
                buffers: &[wgpu::VertexBufferLayout {
                    array_stride: std::mem::size_of::<LineInst>() as u64,
                    step_mode: wgpu::VertexStepMode::Instance,
                    attributes: &wgpu::vertex_attr_array![0 => Float32x3, 1 => Float32, 2 => Float32x3, 3 => Float32, 4 => Float32x4],
                }],
            },
            fragment: Some(wgpu::FragmentState { module: &shader, entry_point: Some("fs_line"), compilation_options: Default::default(), targets: &target }),
            primitive: wgpu::PrimitiveState::default(),
            depth_stencil: depth(wgpu::DepthBiasState::default()),
            multisample: ms,
            multiview_mask: None,
            cache: None,
        });
        let blit_bgl = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("kerf3d blit"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: true },
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 1,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                    count: None,
                },
            ],
        });
        let blit_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor { label: Some("kerf3d blit"), bind_group_layouts: &[Some(&blit_bgl)], ..Default::default() });
        let blit_pipe = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("kerf3d blit"),
            layout: Some(&blit_layout),
            vertex: wgpu::VertexState { module: &shader, entry_point: Some("vs_blit"), compilation_options: Default::default(), buffers: &[] },
            fragment: Some(wgpu::FragmentState {
                module: &shader,
                entry_point: Some("fs_blit"),
                compilation_options: Default::default(),
                targets: &[Some(wgpu::ColorTargetState { format, blend: Some(wgpu::BlendState::REPLACE), write_mask: wgpu::ColorWrites::ALL })],
            }),
            primitive: wgpu::PrimitiveState::default(),
            depth_stencil: None,
            multisample: wgpu::MultisampleState::default(),
            multiview_mask: None,
            cache: None,
        });
        let sampler = device.create_sampler(&wgpu::SamplerDescriptor { mag_filter: wgpu::FilterMode::Nearest, min_filter: wgpu::FilterMode::Nearest, ..Default::default() });
        Gpu3d { format, mesh_pipe, line_pipe, blit_pipe, globals, globals_bg, blit_bgl, sampler, scene: None, targets: None }
    }

    fn ensure_targets(&mut self, device: &wgpu::Device, size: [u32; 2]) {
        if self.targets.as_ref().is_some_and(|t| t.size == size) {
            return;
        }
        let ext = wgpu::Extent3d { width: size[0], height: size[1], depth_or_array_layers: 1 };
        let tex = |label: &str, sample_count: u32, format: wgpu::TextureFormat, usage: wgpu::TextureUsages| {
            device
                .create_texture(&wgpu::TextureDescriptor {
                    label: Some(label),
                    size: ext,
                    mip_level_count: 1,
                    sample_count,
                    dimension: wgpu::TextureDimension::D2,
                    format,
                    usage,
                    view_formats: &[],
                })
                .create_view(&wgpu::TextureViewDescriptor::default())
        };
        let msaa_view = tex("kerf3d msaa", MSAA, self.format, wgpu::TextureUsages::RENDER_ATTACHMENT);
        let depth_view = tex("kerf3d depth", MSAA, DEPTH, wgpu::TextureUsages::RENDER_ATTACHMENT);
        let resolve_view = tex("kerf3d resolve", 1, self.format, wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING);
        let blit_bg = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("kerf3d blit"),
            layout: &self.blit_bgl,
            entries: &[
                wgpu::BindGroupEntry { binding: 0, resource: wgpu::BindingResource::TextureView(&resolve_view) },
                wgpu::BindGroupEntry { binding: 1, resource: wgpu::BindingResource::Sampler(&self.sampler) },
            ],
        });
        self.targets = Some(Targets { size, msaa_view, depth_view, resolve_view, blit_bg });
    }

    fn ensure_scene(&mut self, device: &wgpu::Device, p: &Params) {
        let key = SceneKey { rev: p.rev, selected: p.selected.clone(), hover: p.hover.clone(), grid_y: p.grid_y.round() as i32, cut: p.cut.as_ref().map(|c| (c.z * 1000.0) as i32) };
        if self.scene.as_ref().is_some_and(|s| s.key == key) {
            return;
        }
        let srgb = self.format.is_srgb();
        let lin = |c: [u8; 3]| -> [f32; 3] {
            let f = |v: u8| v as f32 / 255.0;
            // the shader converts for sRGB targets; colours are authored in sRGB-encoded space
            let _ = srgb;
            [f(c[0]), f(c[1]), f(c[2])]
        };
        let mut verts: Vec<Vtx> = Vec::new();
        let mut idx: Vec<u32> = Vec::new();
        let mut lines: Vec<LineInst> = Vec::new();
        let ink = lin([0x1A, 0x1A, 0x1A]);
        let blue = lin([0x1D, 0x4E, 0x9E]);
        for part in &p.mesh.parts {
            let base = lin(parse_hex(&part.color));
            let sel = p.selected.as_deref() == Some(part.src.as_str());
            let hov = p.hover.as_deref() == Some(part.src.as_str());
            let mix = |a: [f32; 3], b: [f32; 3], t: f32| [a[0] + (b[0] - a[0]) * t, a[1] + (b[1] - a[1]) * t, a[2] + (b[2] - a[2]) * t];
            let col = if sel {
                mix(base, lin([0x5B, 0x86, 0xC8]), 0.38)
            } else if hov {
                mix(base, lin([0x5B, 0x86, 0xC8]), 0.2)
            } else {
                base
            };
            let b0 = verts.len() as u32;
            let n = part.positions.len() / 3;
            for i in 0..n {
                let nor = if part.normals.len() >= (i + 1) * 3 { [part.normals[i * 3], part.normals[i * 3 + 1], part.normals[i * 3 + 2]] } else { [0.0, 1.0, 0.0] };
                verts.push(Vtx { pos: [part.positions[i * 3], part.positions[i * 3 + 1], part.positions[i * 3 + 2]], nor, col: [col[0], col[1], col[2], 1.0] });
            }
            idx.extend(part.indices.iter().map(|i| i + b0));
            let (lc, lw) = if sel { (blue, 2.5) } else if hov { (blue, 2.0) } else { (ink, 1.25 * p.line_scale) };
            for e in part.edges.chunks_exact(6) {
                lines.push(LineInst { a: [e[0], e[1], e[2]], w: lw, b: [e[3], e[4], e[5]], pad: 0.0, col: [lc[0], lc[1], lc[2], 1.0] });
            }
        }
        // section caps (manila; steel/rebar dark) with their outline
        if let Some(cut) = &p.cut {
            let manila = lin([0xE9, 0xD9, 0xA6]);
            let dark = lin([0x3A, 0x3A, 0x3A]);
            for cap in &cut.caps {
                let steel = matches!(cap.material.as_str(), "steel" | "rebar" | "aluminum");
                let sel = p.selected.as_deref() == Some(cap.src.as_str());
                let hov = p.hover.as_deref() == Some(cap.src.as_str());
                let base = if steel { dark } else { manila };
                let tint = lin([0x5B, 0x86, 0xC8]);
                let col = if sel { [base[0] * 0.55 + tint[0] * 0.45, base[1] * 0.55 + tint[1] * 0.45, base[2] * 0.55 + tint[2] * 0.45] } else if hov { [base[0] * 0.75 + tint[0] * 0.25, base[1] * 0.75 + tint[1] * 0.25, base[2] * 0.75 + tint[2] * 0.25] } else { base };
                for (xy, tri) in crate::section3d::tris(cap) {
                    let b0 = verts.len() as u32;
                    for v in &xy {
                        verts.push(Vtx { pos: [v[0], v[1], cut.z], nor: [0.0; 3], col: [col[0], col[1], col[2], 1.0] });
                    }
                    idx.extend(tri.iter().map(|i| i + b0));
                }
                let (lc, lw) = if sel || hov { (blue, 2.5) } else { (ink, 1.8 * p.line_scale) };
                for group in &cap.groups {
                    for lp in group {
                        for i in 0..lp.len() {
                            let (a, b) = (lp[i], lp[(i + 1) % lp.len()]);
                            lines.push(LineInst { a: [a[0] as f32, a[1] as f32, cut.z], w: lw, b: [b[0] as f32, b[1] as f32, cut.z], pad: 0.0, col: [lc[0], lc[1], lc[2], 1.0] });
                        }
                    }
                }
            }
        }
        // ground grid on the XZ plane under the model
        if let Some((lo, hi)) = p.mesh.bounds() {
            let y = lo[1] - 0.01;
            let step = grid_step(hi[0] - lo[0], hi[2] - lo[2]);
            let (x0, x1) = ((lo[0] / step).floor() * step - step * 2.0, (hi[0] / step).ceil() * step + step * 2.0);
            let (z0, z1) = ((lo[2] / step).floor() * step - step * 2.0, (hi[2] / step).ceil() * step + step * 2.0);
            let g2 = lin([0xA9, 0xC1, 0xDD]);
            let mut x = x0;
            while x <= x1 + 1e-3 {
                lines.push(LineInst { a: [x, y, z0], w: 1.0, b: [x, y, z1], pad: 0.0, col: [g2[0], g2[1], g2[2], 1.0] });
                x += step;
            }
            let mut z = z0;
            while z <= z1 + 1e-3 {
                lines.push(LineInst { a: [x0, y, z], w: 1.0, b: [x1, y, z], pad: 0.0, col: [g2[0], g2[1], g2[2], 1.0] });
                z += step;
            }
        }
        let vbuf = device.create_buffer_init(&wgpu::util::BufferInitDescriptor { label: Some("kerf3d v"), contents: bytemuck::cast_slice(&verts.iter().copied().collect::<Vec<_>>()), usage: wgpu::BufferUsages::VERTEX });
        let ibuf = device.create_buffer_init(&wgpu::util::BufferInitDescriptor { label: Some("kerf3d i"), contents: bytemuck::cast_slice(&idx), usage: wgpu::BufferUsages::INDEX });
        let lbuf = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("kerf3d l"),
            contents: if lines.is_empty() { &[0u8; 48] } else { bytemuck::cast_slice(&lines) },
            usage: wgpu::BufferUsages::VERTEX,
        });
        let vbuf = if verts.is_empty() {
            device.create_buffer(&wgpu::BufferDescriptor { label: Some("kerf3d v"), size: 40, usage: wgpu::BufferUsages::VERTEX, mapped_at_creation: false })
        } else {
            vbuf
        };
        let ibuf = if idx.is_empty() {
            device.create_buffer(&wgpu::BufferDescriptor { label: Some("kerf3d i"), size: 4, usage: wgpu::BufferUsages::INDEX, mapped_at_creation: false })
        } else {
            ibuf
        };
        self.scene = Some(Scene { key, vbuf, ibuf, n_idx: idx.len() as u32, lbuf, n_lines: lines.len() as u32 });
    }
}

fn grid_step(dx: f32, dz: f32) -> f32 {
    let span = dx.max(dz).max(12.0);
    // 1, 2, 5, 10, 12 ... inches, about 8-14 cells over the model
    let raw = span / 10.0;
    let steps = [1.0, 2.0, 3.0, 6.0, 12.0, 24.0, 48.0, 96.0];
    steps.into_iter().find(|s| *s >= raw).unwrap_or(192.0)
}

/// Per-frame parameters for the callback.
#[derive(Clone)]
pub struct Params {
    pub rev: u64,
    pub mesh: Arc<Mesh>,
    pub selected: Option<String>,
    pub hover: Option<String>,
    pub view_proj: Mat4,
    pub size_px: [u32; 2],
    pub bg: [f64; 3],
    pub line_scale: f32,
    pub grid_y: f32,
    pub cut: Option<Arc<CutInfo>>,
}

/// The active section plane and its caps.
pub struct CutInfo {
    pub z: f32,
    pub caps: Vec<crate::section3d::Cap>,
}

pub struct Callback {
    pub p: Params,
}

impl CallbackTrait for Callback {
    fn prepare(
        &self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        _screen: &ScreenDescriptor,
        encoder: &mut wgpu::CommandEncoder,
        res: &mut CallbackResources,
    ) -> Vec<wgpu::CommandBuffer> {
        let Some(gpu) = res.get_mut::<Gpu3d>() else { return vec![] };
        let size = [self.p.size_px[0].max(1), self.p.size_px[1].max(1)];
        gpu.ensure_targets(device, size);
        gpu.ensure_scene(device, &self.p);
        let g = Globals {
            view_proj: self.p.view_proj.to_cols_array_2d(),
            light: [-0.45, 0.8, 0.55, 0.52],
            viewport: [size[0] as f32, size[1] as f32],
            srgb: if gpu.format.is_srgb() { 1.0 } else { 0.0 },
            line_bias: 1.2e-3,
            clip: match &self.p.cut {
                Some(c) => [1.0, c.z, 0.0, 0.0],
                None => [0.0; 4],
            },
        };
        queue.write_buffer(&gpu.globals, 0, bytemuck::bytes_of(&g));
        let (t, s) = (gpu.targets.as_ref().unwrap(), gpu.scene.as_ref().unwrap());
        let bg = self.p.bg;
        let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("kerf3d scene"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: &t.msaa_view,
                depth_slice: None,
                resolve_target: Some(&t.resolve_view),
                ops: wgpu::Operations { load: wgpu::LoadOp::Clear(wgpu::Color { r: bg[0], g: bg[1], b: bg[2], a: 1.0 }), store: wgpu::StoreOp::Discard },
            })],
            depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                view: &t.depth_view,
                depth_ops: Some(wgpu::Operations { load: wgpu::LoadOp::Clear(1.0), store: wgpu::StoreOp::Discard }),
                stencil_ops: None,
            }),
            timestamp_writes: None,
            occlusion_query_set: None,
            multiview_mask: None,
        });
        pass.set_bind_group(0, &gpu.globals_bg, &[]);
        if s.n_idx > 0 {
            pass.set_pipeline(&gpu.mesh_pipe);
            pass.set_vertex_buffer(0, s.vbuf.slice(..));
            pass.set_index_buffer(s.ibuf.slice(..), wgpu::IndexFormat::Uint32);
            pass.draw_indexed(0..s.n_idx, 0, 0..1);
        }
        if s.n_lines > 0 {
            pass.set_pipeline(&gpu.line_pipe);
            pass.set_vertex_buffer(0, s.lbuf.slice(..));
            pass.draw(0..6, 0..s.n_lines);
        }
        drop(pass);
        vec![]
    }

    fn paint(&self, _info: egui::PaintCallbackInfo, pass: &mut wgpu::RenderPass<'static>, res: &CallbackResources) {
        let Some(gpu) = res.get::<Gpu3d>() else { return };
        let Some(t) = gpu.targets.as_ref() else { return };
        pass.set_pipeline(&gpu.blit_pipe);
        pass.set_bind_group(0, &t.blit_bg, &[]);
        pass.draw(0..3, 0..1);
    }
}

/// Orthographic orbit camera.
#[derive(Clone, Copy, Debug)]
pub struct Cam3d {
    pub target: Vec3,
    pub yaw: f32,
    pub pitch: f32,
    /// half-height of the view volume in model inches
    pub half_h: f32,
}

impl Cam3d {
    pub fn iso() -> Cam3d {
        Cam3d { target: Vec3::ZERO, yaw: 45f32.to_radians(), pitch: 35.264f32.to_radians(), half_h: 40.0 }
    }

    pub fn dir(&self) -> Vec3 {
        Vec3::new(self.pitch.cos() * self.yaw.sin(), self.pitch.sin(), self.pitch.cos() * self.yaw.cos())
    }

    pub fn view(&self, radius: f32) -> Mat4 {
        let d = self.dir();
        let eye = self.target + d * (radius * 3.0 + 10.0);
        let up = if self.pitch.abs() > 1.5 { Vec3::new(0.0, 0.0, -self.pitch.signum()) } else { Vec3::Y };
        Mat4::look_at_rh(eye, self.target, up)
    }

    pub fn view_proj(&self, aspect: f32, radius: f32) -> Mat4 {
        let h = self.half_h;
        let far = radius * 6.0 + 20.0;
        let proj = Mat4::orthographic_rh(-h * aspect, h * aspect, -h, h, 0.1, far);
        proj * self.view(radius)
    }

    /// World-space ray (origin, direction) through a normalized-device point.
    pub fn ray(&self, ndc: [f32; 2], aspect: f32, radius: f32) -> (Vec3, Vec3) {
        let inv = self.view_proj(aspect, radius).inverse();
        let a = inv.project_point3(Vec3::new(ndc[0], ndc[1], 0.0));
        let b = inv.project_point3(Vec3::new(ndc[0], ndc[1], 1.0));
        (a, (b - a).normalize())
    }
}

/// Closest hit of a ray against all mesh triangles: (distance, src).
pub fn pick(mesh: &Mesh, origin: Vec3, dir: Vec3, cut: Option<&CutInfo>) -> Option<(f32, String)> {
    let mut best: Option<(f32, &str)> = None;
    for part in &mesh.parts {
        let p = &part.positions;
        for t in part.indices.chunks_exact(3) {
            let v = |i: u32| Vec3::new(p[i as usize * 3], p[i as usize * 3 + 1], p[i as usize * 3 + 2]);
            if let Some(d) = ray_tri(origin, dir, v(t[0]), v(t[1]), v(t[2])) {
                if cut.is_some_and(|c| (origin + dir * d).z > c.z + 0.002) {
                    continue; // clipped away
                }
                if best.is_none_or(|(bd, _)| d < bd) {
                    best = Some((d, &part.src));
                }
            }
        }
    }
    let mut best = best.map(|(d, s)| (d, s.to_owned()));
    if let Some(c) = cut {
        // the cap is a surface at z = cut.z
        if dir.z.abs() > 1e-6 {
            let t = (c.z - origin.z) / dir.z;
            if t > 0.0 {
                let h = origin + dir * t;
                let pt = [h.x as f64, h.y as f64];
                for cap in &c.caps {
                    let hit = cap.groups.iter().any(|g| crate::ir::point_in_loop(pt, &g[0]) && !g[1..].iter().any(|hole| crate::ir::point_in_loop(pt, hole)));
                    if hit && best.as_ref().is_none_or(|(bd, _)| t <= *bd + 1e-3) {
                        best = Some((t, cap.src.clone()));
                    }
                }
            }
        }
    }
    best
}

fn ray_tri(o: Vec3, d: Vec3, a: Vec3, b: Vec3, c: Vec3) -> Option<f32> {
    let e1 = b - a;
    let e2 = c - a;
    let h = d.cross(e2);
    let det = e1.dot(h);
    if det.abs() < 1e-9 {
        return None;
    }
    let f = 1.0 / det;
    let s = o - a;
    let u = f * s.dot(h);
    if !(0.0..=1.0).contains(&u) {
        return None;
    }
    let q = s.cross(e1);
    let v = f * d.dot(q);
    if v < 0.0 || u + v > 1.0 {
        return None;
    }
    let t = f * e2.dot(q);
    (t > 0.0).then_some(t)
}
