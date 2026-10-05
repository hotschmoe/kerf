// Kerf 3D viewport: flat-shaded faces, constant-pixel-width feature edges, blit to egui.

struct Globals {
    view_proj: mat4x4<f32>,
    light: vec4<f32>,      // xyz = world light dir (towards light), w = ambient
    viewport: vec2<f32>,   // pixels
    srgb: f32,             // 1.0 when the target is an sRGB format (colours are converted)
    line_bias: f32,
    clip: vec4<f32>,       // x = enabled, y = world z of the section plane (keep z <= y)
};
@group(0) @binding(0) var<uniform> g: Globals;

fn to_target(c: vec3<f32>) -> vec3<f32> {
    if (g.srgb > 0.5) {
        let lo = c / 12.92;
        let hi = pow((c + vec3<f32>(0.055)) / 1.055, vec3<f32>(2.4));
        return select(hi, lo, c <= vec3<f32>(0.04045));
    }
    return c;
}

// ---------------------------------------------------------------- faces
struct MeshIn {
    @location(0) pos: vec3<f32>,
    @location(1) nor: vec3<f32>,
    @location(2) col: vec4<f32>,
};
struct MeshOut {
    @builtin(position) p: vec4<f32>,
    @location(0) col: vec4<f32>,
    @location(1) wz: f32,
};

@vertex
fn vs_mesh(v: MeshIn) -> MeshOut {
    var o: MeshOut;
    o.p = g.view_proj * vec4<f32>(v.pos, 1.0);
    o.wz = v.pos.z;
    if (dot(v.nor, v.nor) < 1e-6) {
        // cut caps: flat, unlit
        o.col = v.col;
        return o;
    }
    let n = normalize(v.nor);
    let ndl = max(dot(n, normalize(g.light.xyz)), 0.0);
    // a second, weaker fill light from the opposite side keeps shadowed faces readable
    let fill = max(dot(n, normalize(vec3<f32>(-g.light.x, 0.2, -g.light.z))), 0.0) * 0.12;
    let k = g.light.w + (1.0 - g.light.w) * ndl + fill;
    o.col = vec4<f32>(v.col.rgb * min(k, 1.15), v.col.a);
    return o;
}

@fragment
fn fs_mesh(i: MeshOut) -> @location(0) vec4<f32> {
    if (g.clip.x > 0.5 && i.wz > g.clip.y + 0.002) { discard; }
    return vec4<f32>(to_target(clamp(i.col.rgb, vec3<f32>(0.0), vec3<f32>(1.0))), 1.0);
}

// ---------------------------------------------------------------- lines (screen-space quads)
struct LineIn {
    @builtin(vertex_index) vi: u32,
    @location(0) a: vec3<f32>,
    @location(1) w: f32,
    @location(2) b: vec3<f32>,
    @location(3) pad: f32,
    @location(4) col: vec4<f32>,
};
struct LineOut {
    @builtin(position) p: vec4<f32>,
    @location(0) col: vec4<f32>,
    @location(1) wz: f32,
};

@vertex
fn vs_line(l: LineIn) -> LineOut {
    var o: LineOut;
    let ca = g.view_proj * vec4<f32>(l.a, 1.0);
    let cb = g.view_proj * vec4<f32>(l.b, 1.0);
    let sa = ca.xy / ca.w * g.viewport * 0.5;
    let sb = cb.xy / cb.w * g.viewport * 0.5;
    var d = sb - sa;
    let len = length(d);
    if (len < 1e-4) { d = vec2<f32>(1.0, 0.0); } else { d = d / len; }
    let nrm = vec2<f32>(-d.y, d.x);
    // 6 vertices: two triangles
    var t = array<f32, 6>(0.0, 0.0, 1.0, 0.0, 1.0, 1.0);
    var s = array<f32, 6>(-1.0, 1.0, -1.0, 1.0, 1.0, -1.0);
    let tt = t[l.vi];
    let ss = s[l.vi];
    let c = mix(ca, cb, tt);
    let ext = (tt * 2.0 - 1.0) * l.w * 0.5;
    let off = (nrm * ss * l.w * 0.5 + d * ext) / (g.viewport * 0.5);
    o.p = vec4<f32>(c.xy + off * c.w, c.z - g.line_bias * c.w, c.w);
    o.col = l.col;
    o.wz = mix(l.a.z, l.b.z, tt);
    return o;
}

@fragment
fn fs_line(i: LineOut) -> @location(0) vec4<f32> {
    if (g.clip.x > 0.5 && i.wz > g.clip.y + 0.002) { discard; }
    return vec4<f32>(to_target(i.col.rgb), 1.0);
}

// ---------------------------------------------------------------- blit
@group(0) @binding(0) var blit_tex: texture_2d<f32>;
@group(0) @binding(1) var blit_smp: sampler;

struct BlitOut {
    @builtin(position) p: vec4<f32>,
    @location(0) uv: vec2<f32>,
};

@vertex
fn vs_blit(@builtin(vertex_index) vi: u32) -> BlitOut {
    var o: BlitOut;
    let x = f32((vi << 1u) & 2u);
    let y = f32(vi & 2u);
    o.p = vec4<f32>(x * 2.0 - 1.0, 1.0 - y * 2.0, 0.0, 1.0);
    o.uv = vec2<f32>(x, y);
    return o;
}

@fragment
fn fs_blit(i: BlitOut) -> @location(0) vec4<f32> {
    return textureSample(blit_tex, blit_smp, i.uv);
}
