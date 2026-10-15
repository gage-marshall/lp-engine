//! The 3D layer drawn on the GPU (wgpu) by the same rules as `raster3d`, which stays the reference: the polygons
//! come from `raster3d::prepare` (ordered, clipped, projected and culled on the CPU, so both draw the same
//! triangles) and the fragment shader is `raster3d::Target::triangle` line for line: the seven texture formats
//! decoded from the texture slots as the game left them, the four polygon modes, the toon / highlight table, the
//! alpha test, translucency and depth. What the hardware's fixed blending cannot say directly is done with the
//! stencil (a translucent pixel over a pixel nothing has been drawn on, alpha 0, is written, not blended) and by
//! drawing a translucent polygon in three parts (blended, opaque pixels, pixels over nothing). Edge marking, the
//! reduction of anti-aliased samples and RGB555 packing are one final pass; the layer is read back for the 2D
//! compositor.
use super::gx::Frame;
use super::raster3d::{self, Look};
use super::ram::Ram;
use super::vram::{Place, SIZES};

const TEX_SIZE: usize = 0x80000;
const PAL_SIZE: usize = 0x20000;
const COLOR: wgpu::TextureFormat = wgpu::TextureFormat::Rgba16Float;
const ID: wgpu::TextureFormat = wgpu::TextureFormat::Rgba8Unorm;
const DEPTH: wgpu::TextureFormat = wgpu::TextureFormat::Depth24PlusStencil8;
const OUT: wgpu::TextureFormat = wgpu::TextureFormat::R32Uint;
/// the atlas of enhanced textures (pixels per side) and how many it can index
const ATLAS: u32 = 4096;
const ENTRIES: usize = 4096;
/// a vertex: position, colour, texture coordinate, polygon (attr, tex, pal, enhanced entry)
const VERTEX: usize = 52;

const SHADER: &str = r#"
struct U {
    size: vec2<f32>,
    wscale: f32,
    eps: f32,
    alpha_test: f32,
    flags: u32,
    _p0: u32,
    _p1: u32,
    toon: array<vec4<f32>, 32>,
};
@group(0) @binding(0) var<uniform> u: U;
@group(0) @binding(1) var<storage, read> tex: array<u32>;
@group(0) @binding(2) var<storage, read> pal: array<u32>;
// enhanced textures (texcache): the atlas, and per entry its place and size (entry 0: none)
@group(0) @binding(3) var atlas: texture_2d<f32>;
@group(0) @binding(4) var<storage, read> ent: array<vec4<u32>>;

struct VIn {
    @location(0) pos: vec4<f32>,
    @location(1) color: vec3<f32>,
    @location(2) uv: vec2<f32>,
    @location(3) poly: vec4<u32>,
};
struct VOut {
    @builtin(position) pos: vec4<f32>,
    @location(0) color: vec3<f32>,
    @location(1) uv: vec2<f32>,
    @location(2) @interpolate(flat) poly: vec4<u32>,
    @location(3) @interpolate(flat) part: u32,
};

@vertex fn vs(v: VIn, @builtin(instance_index) part: u32) -> VOut {
    var o: VOut;
    let w = v.pos.w;
    let x = v.pos.x / u.size.x * 2.0 - 1.0;
    let y = 1.0 - v.pos.y / u.size.y * 2.0;
    o.pos = vec4<f32>(x * w, y * w, v.pos.z * u.wscale * w, w);
    o.color = v.color;
    o.uv = v.uv;
    o.poly = v.poly;
    o.part = part;
    return o;
}

fn tb(off: u32) -> u32 {
    let o = off & 0x7ffffu;
    return (tex[o >> 2u] >> ((o & 3u) * 8u)) & 0xffu;
}
fn c5(c: u32) -> vec3<f32> {
    return vec3<f32>(f32(c & 31u), f32((c >> 5u) & 31u), f32((c >> 10u) & 31u)) / 31.0;
}
fn pc(off: u32) -> vec3<f32> {
    let o = off & 0x1fffeu;
    let v = (pal[o >> 2u] >> ((o & 2u) * 8u)) & 0xffffu;
    return c5(v);
}
fn wrap(v: i32, n: i32, rep: bool, flip: bool) -> i32 {
    if !rep { return clamp(v, 0, n - 1); }
    if flip {
        let m = ((v % (2 * n)) + 2 * n) % (2 * n);
        if m >= n { return 2 * n - 1 - m; }
        return m;
    }
    return ((v % n) + n) % n;
}

// a texel: rgb and alpha; alpha < 0 where there is none (transparent)
fn texel(t: u32, p: u32, s_in: i32, t_in: i32) -> vec4<f32> {
    let fmt = (t >> 26u) & 7u;
    let base = (t & 0xffffu) * 8u;
    let w = i32(8u << ((t >> 20u) & 7u));
    let h = i32(8u << ((t >> 23u) & 7u));
    let s = u32(wrap(s_in, w, (t & (1u << 16u)) != 0u, (t & (1u << 18u)) != 0u));
    let tt = u32(wrap(t_in, h, (t & (1u << 17u)) != 0u, (t & (1u << 19u)) != 0u));
    let i = tt * u32(w) + s;
    let tr0 = (t & (1u << 29u)) != 0u;
    let palbase = p * 16u;
    let none = vec4<f32>(0.0, 0.0, 0.0, -1.0);
    switch fmt {
        case 1u: {
            let b = tb(base + i);
            let a = (b >> 5u) * 4u + (b >> 5u) / 2u;
            if a == 0u { return none; }
            return vec4<f32>(pc(palbase + (b & 31u) * 2u), f32(a) / 31.0);
        }
        case 2u: {
            let idx = (tb(base + i / 4u) >> (2u * (i % 4u))) & 3u;
            if idx == 0u && tr0 { return none; }
            return vec4<f32>(pc(p * 8u + idx * 2u), 1.0);
        }
        case 3u: {
            let idx = (tb(base + i / 2u) >> (4u * (i % 2u))) & 15u;
            if idx == 0u && tr0 { return none; }
            return vec4<f32>(pc(palbase + idx * 2u), 1.0);
        }
        case 4u: {
            let idx = tb(base + i);
            if idx == 0u && tr0 { return none; }
            return vec4<f32>(pc(palbase + idx * 2u), 1.0);
        }
        case 5u: {
            let block = (tt / 4u) * (u32(w) / 4u) + s / 4u;
            let word = base + block * 4u;
            let bits = (tb(word + tt % 4u) >> (2u * (s % 4u))) & 3u;
            var slot = (word & 0x1ffffu) / 2u;
            if word >= 0x40000u { slot += 0x1000u; }
            let pw = tb(0x20000u + slot) | (tb(0x20000u + slot + 1u) << 8u);
            let poff = palbase + (pw & 0x3fffu) * 4u;
            let mode = pw >> 14u;
            let c0 = pc(poff);
            let c1 = pc(poff + 2u);
            if bits == 0u { return vec4<f32>(c0, 1.0); }
            if bits == 1u { return vec4<f32>(c1, 1.0); }
            if mode == 0u || mode == 2u {
                if bits == 2u { return vec4<f32>(pc(poff + 4u), 1.0); }
                if mode == 0u { return none; }
                return vec4<f32>(pc(poff + 6u), 1.0);
            }
            if mode == 1u {
                if bits == 2u { return vec4<f32>((c0 + c1) / 2.0, 1.0); }
                return none;
            }
            if bits == 2u { return vec4<f32>((c0 * 5.0 + c1 * 3.0) / 8.0, 1.0); }
            return vec4<f32>((c0 * 3.0 + c1 * 5.0) / 8.0, 1.0);
        }
        case 6u: {
            let b = tb(base + i);
            let a = b >> 3u;
            if a == 0u { return none; }
            return vec4<f32>(pc(palbase + (b & 7u) * 2u), f32(a) / 31.0);
        }
        default: {
            let c = tb(base + i * 2u) | (tb(base + i * 2u + 1u) << 8u);
            if (c & 0x8000u) == 0u { return none; }
            return vec4<f32>(c5(c), 1.0);
        }
    }
}

// a texel of the enhanced version of the texture (entry e) at its own size, with the texture's wrap
fn etexel(e: vec4<u32>, t: u32, s_in: i32, t_in: i32) -> vec4<f32> {
    let s = wrap(s_in, i32(e.z), (t & (1u << 16u)) != 0u, (t & (1u << 18u)) != 0u);
    let tt = wrap(t_in, i32(e.w), (t & (1u << 17u)) != 0u, (t & (1u << 19u)) != 0u);
    let c = textureLoad(atlas, vec2<i32>(i32(e.x) + s, i32(e.y) + tt), 0);
    let fmt = (t >> 26u) & 7u;
    if fmt == 1u || fmt == 6u {
        if c.a <= 0.0 { return vec4<f32>(0.0, 0.0, 0.0, -1.0); }
        return c;
    }
    if c.a < 0.5 { return vec4<f32>(0.0, 0.0, 0.0, -1.0); }
    return vec4<f32>(c.rgb, 1.0);
}

// a texel at integer coordinates: of the enhanced version (entry ei, coordinates at its size) or the DS's
fn fetch(ei: u32, t: u32, p: u32, s: i32, tt: i32) -> vec4<f32> {
    if ei != 0u { return etexel(ent[ei], t, s, tt); }
    return texel(t, p, s, tt);
}

fn texel_bilinear(ei: u32, t: u32, p: u32, uu: f32, vv: f32) -> vec4<f32> {
    let uf = uu - 0.5;
    let vf = vv - 0.5;
    let s0 = i32(floor(uf));
    let t0 = i32(floor(vf));
    let fu = uf - floor(uf);
    let fv = vf - floor(vf);
    var c = vec3<f32>(0.0);
    var a = 0.0;
    var ws = array<f32, 4>((1.0 - fu) * (1.0 - fv), fu * (1.0 - fv), (1.0 - fu) * fv, fu * fv);
    for (var k = 0; k < 4; k++) {
        let x = fetch(ei, t, p, s0 + (k & 1), t0 + (k >> 1));
        if x.a >= 0.0 {
            c += x.rgb * x.a * ws[k];
            a += x.a * ws[k];
        }
    }
    let fmt = (t >> 26u) & 7u;
    let translucent = fmt == 1u || fmt == 6u;
    if a <= 0.0 || (!translucent && a < 0.5) { return vec4<f32>(0.0, 0.0, 0.0, -1.0); }
    if translucent { return vec4<f32>(c / a, a); }
    return vec4<f32>(c / a, 1.0);
}

struct FOut {
    @location(0) color: vec4<f32>,
    @location(1) id: vec4<f32>,
};

fn shade(v: VOut) -> FOut {
    let attr = v.poly.x;
    let t = v.poly.y;
    let p = v.poly.z;
    let fmt = (t >> 26u) & 7u;
    var tc = vec3<f32>(1.0);
    var ta = 1.0;
    if fmt != 0u {
        // texture coordinates at the size of the texture sampled (the enhanced version's when there is one)
        let ei = v.poly.w;
        var uv = v.uv;
        if ei != 0u {
            let e = ent[ei];
            uv = uv * vec2<f32>(f32(e.z) / f32(8u << ((t >> 20u) & 7u)), f32(e.w) / f32(8u << ((t >> 23u) & 7u)));
        }
        var x: vec4<f32>;
        if (u.flags & 4u) != 0u { x = texel_bilinear(ei, t, p, uv.x, uv.y); } else { x = fetch(ei, t, p, i32(floor(uv.x)), i32(floor(uv.y))); }
        if x.a < 0.0 { discard; }
        tc = x.rgb;
        ta = x.a;
    }
    let alpha = f32((attr >> 16u) & 31u) / 31.0;
    let mode = (attr >> 4u) & 3u;
    let vc = v.color;
    var rgb: vec3<f32>;
    var a = ta * alpha;
    if mode == 1u {
        rgb = tc * ta + vc * (1.0 - ta);
        a = alpha;
    } else if mode == 2u {
        var tn: vec3<f32>;
        if (u.flags & 8u) != 0u {
            let l = clamp(vc.r * 31.0, 0.0, 31.0);
            let i = u32(floor(l));
            tn = mix(u.toon[i].rgb, u.toon[min(i + 1u, 31u)].rgb, fract(l));
        } else {
            tn = u.toon[min(u32(vc.r * 31.0), 31u)].rgb;
        }
        if (u.flags & 2u) != 0u { rgb = min(tc * vc.r + tn, vec3<f32>(1.0)); } else { rgb = tc * tn; }
    } else {
        rgb = tc * vc;
    }
    if a <= u.alpha_test { discard; }
    if mode == 3u { discard; }
    // the part of a translucent polygon being drawn: 1 the pixels below full alpha, 2 the others
    if v.part == 1u && a >= 1.0 { discard; }
    if v.part == 2u && a < 1.0 { discard; }
    var o: FOut;
    o.color = vec4<f32>(rgb, a);
    var opaque = 0.0;
    if a >= 1.0 { opaque = 1.0; }
    o.id = vec4<f32>(f32((attr >> 24u) & 0x3fu) / 255.0, 0.0, 0.0, opaque);
    return o;
}

@fragment fn fs(v: VOut) -> FOut {
    return shade(v);
}

struct FOutEq {
    @location(0) color: vec4<f32>,
    @location(1) id: vec4<f32>,
    @builtin(frag_depth) depth: f32,
};
// depth-equal polygons: pass where the depth is within eps of what is there (tested as <= depth - eps)
@fragment fn fs_eq(v: VOut) -> FOutEq {
    let s = shade(v);
    var o: FOutEq;
    o.color = s.color;
    o.id = s.id;
    o.depth = max(v.pos.z - u.eps, 0.0);
    return o;
}
"#;

const RESOLVE: &str = r#"
struct R {
    k: u32,
    radius: u32,
    edges: u32,
    _p: u32,
    edge_color: array<vec4<f32>, 8>,
};
@group(0) @binding(0) var<uniform> r: R;
@group(0) @binding(1) var color: texture_2d<f32>;
@group(0) @binding(2) var ids: texture_2d<f32>;
@group(0) @binding(3) var depth: texture_depth_2d;

@vertex fn vs(@builtin(vertex_index) i: u32) -> @builtin(position) vec4<f32> {
    var xy = array<vec2<f32>, 3>(vec2(-1.0, -1.0), vec2(3.0, -1.0), vec2(-1.0, 3.0));
    return vec4<f32>(xy[i], 0.0, 1.0);
}

fn q(v: f32) -> u32 {
    return u32(round(clamp(v, 0.0, 1.0) * 31.0)) & 31u;
}

@fragment fn fs(@builtin(position) pos: vec4<f32>) -> @location(0) u32 {
    let size = vec2<i32>(textureDimensions(color));
    let o = vec2<i32>(pos.xy) * i32(r.k);
    var sum = vec3<f32>(0.0);
    var n = 0u;
    for (var sy = 0; sy < i32(r.k); sy++) {
        for (var sx = 0; sx < i32(r.k); sx++) {
            let p = o + vec2<i32>(sx, sy);
            var c = textureLoad(color, p, 0);
            let me = u32(round(textureLoad(ids, p, 0).r * 255.0));
            if r.edges != 0u && me != 255u && c.a >= 1.0 {
                let d = textureLoad(depth, p, 0);
                var edge = false;
                for (var k = 1; k <= i32(r.radius); k++) {
                    var dirs = array<vec2<i32>, 4>(vec2(k, 0), vec2(-k, 0), vec2(0, k), vec2(0, -k));
                    for (var j = 0; j < 4; j++) {
                        let nb = p + dirs[j];
                        if nb.x < 0 || nb.y < 0 || nb.x >= size.x || nb.y >= size.y { continue; }
                        let other = u32(round(textureLoad(ids, nb, 0).r * 255.0));
                        if other != me && d < textureLoad(depth, nb, 0) { edge = true; }
                    }
                }
                if edge { c = vec4<f32>(r.edge_color[me >> 3u].rgb, 1.0); }
            }
            if c.a > 0.0 {
                sum += c.rgb;
                n += 1u;
            }
        }
    }
    if n == 0u { return 0u; }
    let a = sum / f32(n);
    return 0x8000u | q(a.r) | (q(a.g) << 5u) | (q(a.b) << 10u) | ((n * 16u / (r.k * r.k)) << 16u);
}
"#;

/// the render targets for one size
pub(crate) struct Targets {
    pub(crate) w: u32,
    h: u32,
    k: u32,
    color: wgpu::Texture,
    id: wgpu::Texture,
    depth: wgpu::Texture,
    /// the finished layers: slot 0 the displayed frame, 1 scratch (captures, read-back)
    pub(crate) outs: [wgpu::Texture; 2],
    read: wgpu::Buffer,
    resolve_bind: wgpu::BindGroup,
}

pub struct Gpu3d {
    pub(crate) device: wgpu::Device,
    pub(crate) queue: wgpu::Queue,
    uniforms: wgpu::Buffer,
    resolve_uniforms: wgpu::Buffer,
    tex: wgpu::Buffer,
    pal: wgpu::Buffer,
    bind: wgpu::BindGroup,
    resolve_layout: wgpu::BindGroupLayout,
    /// [over everything, over pixels nothing was drawn on, blended without / with depth write] x [less, equal]
    over: [wgpu::RenderPipeline; 2],
    over_empty: [wgpu::RenderPipeline; 2],
    blend: [[wgpu::RenderPipeline; 2]; 2],
    resolve: wgpu::RenderPipeline,
    pub(crate) targets: Option<Targets>,
    pub name: String,
    /// the 2D compositor (`gpu2d`), made when first used
    pub(crate) c2d: Option<super::gpu2d::Compositor>,
    /// enhanced textures
    pub tc: TexCache,
}

/// enhanced textures on the GPU: the atlas they are packed in (rows), the entries the shader reads, by key
#[derive(Default)]
pub struct TexCache {
    atlas: Option<wgpu::Texture>,
    entries: Option<wgpu::Buffer>,
    by_key: std::collections::HashMap<u64, u32>,
    places: Vec<[u32; 4]>,
    /// the next free place: x, y, the current row's height
    cursor: (u32, u32, u32),
    /// mods (`<dir>/<key>.png`) and where to dump the textures seen
    pub mods: Option<std::path::PathBuf>,
    pub dump: Option<std::path::PathBuf>,
    pub params: super::texcache::Params,
    /// the textures' names by key (`assetindex`), for dumps and mods
    pub names: std::sync::Arc<std::collections::HashMap<u64, String>>,
    /// VRAM banks the game captures pictures into (bit per bank A..D): textures there are made at run time and
    /// change with every capture, so they are not enhanced or dumped
    pub captured: u8,
}

impl TexCache {
    /// forget the enhanced textures (they are made again as they are seen, e.g. after the parameters change)
    pub fn clear(&mut self) {
        self.by_key.clear();
        self.places.clear();
        self.cursor = (0, 0, 0);
    }
}

fn f32s(v: &[f32]) -> Vec<u8> {
    v.iter().flat_map(|x| x.to_le_bytes()).collect()
}

impl Gpu3d {
    /// a GPU device of its own, or None when there is no adapter
    pub fn new() -> Option<Gpu3d> {
        let instance = wgpu::Instance::default();
        let adapter = pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions { power_preference: wgpu::PowerPreference::HighPerformance, compatible_surface: None, force_fallback_adapter: false }))?;
        let name = adapter.get_info().name;
        // the adapter's own buffer limits (the portable defaults cap a storage binding at 128 MB; the compositor's
        // pool of finer pictures is larger at 4x)
        let al = adapter.limits();
        let limits = wgpu::Limits { max_storage_buffer_binding_size: al.max_storage_buffer_binding_size, max_buffer_size: al.max_buffer_size, ..wgpu::Limits::default() };
        let (device, queue) = pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor { label: Some("gpu3d"), required_features: wgpu::Features::empty(), required_limits: limits, memory_hints: Default::default() }, None)).ok()?;
        let buf = |size: usize, usage: wgpu::BufferUsages| device.create_buffer(&wgpu::BufferDescriptor { label: None, size: size as u64, usage: usage | wgpu::BufferUsages::COPY_DST, mapped_at_creation: false });
        let uniforms = buf(32 + 32 * 16, wgpu::BufferUsages::UNIFORM);
        let resolve_uniforms = buf(16 + 8 * 16, wgpu::BufferUsages::UNIFORM);
        let tex = buf(TEX_SIZE, wgpu::BufferUsages::STORAGE);
        let pal = buf(PAL_SIZE, wgpu::BufferUsages::STORAGE);
        let entry = |binding: u32, ty: wgpu::BindingType, vis: wgpu::ShaderStages| wgpu::BindGroupLayoutEntry { binding, visibility: vis, ty, count: None };
        let vf = wgpu::ShaderStages::VERTEX | wgpu::ShaderStages::FRAGMENT;
        let storage = wgpu::BindingType::Buffer { ty: wgpu::BufferBindingType::Storage { read_only: true }, has_dynamic_offset: false, min_binding_size: None };
        let uniform = wgpu::BindingType::Buffer { ty: wgpu::BufferBindingType::Uniform, has_dynamic_offset: false, min_binding_size: None };
        let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: None,
            entries: &[
                entry(0, uniform, vf),
                entry(1, storage, wgpu::ShaderStages::FRAGMENT),
                entry(2, storage, wgpu::ShaderStages::FRAGMENT),
                entry(3, wgpu::BindingType::Texture { sample_type: wgpu::TextureSampleType::Float { filterable: false }, view_dimension: wgpu::TextureViewDimension::D2, multisampled: false }, wgpu::ShaderStages::FRAGMENT),
                entry(4, storage, wgpu::ShaderStages::FRAGMENT),
            ],
        });
        let atlas = device.create_texture(&wgpu::TextureDescriptor { label: Some("texture atlas"), size: wgpu::Extent3d { width: ATLAS, height: ATLAS, depth_or_array_layers: 1 }, mip_level_count: 1, sample_count: 1, dimension: wgpu::TextureDimension::D2, format: wgpu::TextureFormat::Rgba8Unorm, usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST, view_formats: &[] });
        let entries = buf(ENTRIES * 16, wgpu::BufferUsages::STORAGE);
        let bind = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: None,
            layout: &layout,
            entries: &[
                wgpu::BindGroupEntry { binding: 0, resource: uniforms.as_entire_binding() },
                wgpu::BindGroupEntry { binding: 1, resource: tex.as_entire_binding() },
                wgpu::BindGroupEntry { binding: 2, resource: pal.as_entire_binding() },
                wgpu::BindGroupEntry { binding: 3, resource: wgpu::BindingResource::TextureView(&atlas.create_view(&Default::default())) },
                wgpu::BindGroupEntry { binding: 4, resource: entries.as_entire_binding() },
            ],
        });
        let module = device.create_shader_module(wgpu::ShaderModuleDescriptor { label: Some("gpu3d"), source: wgpu::ShaderSource::Wgsl(SHADER.into()) });
        let pl = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor { label: None, bind_group_layouts: &[&layout], push_constant_ranges: &[] });
        let attrs = wgpu::vertex_attr_array![0 => Float32x4, 1 => Float32x3, 2 => Float32x2, 3 => Uint32x4];
        let pipeline = |eq: bool, stencil: wgpu::CompareFunction, mark: bool, blend: bool, depth_write: bool| {
            let face = wgpu::StencilFaceState { compare: stencil, fail_op: wgpu::StencilOperation::Keep, depth_fail_op: wgpu::StencilOperation::Keep, pass_op: if mark { wgpu::StencilOperation::IncrementClamp } else { wgpu::StencilOperation::Keep } };
            let color_blend = blend.then_some(wgpu::BlendState {
                color: wgpu::BlendComponent { src_factor: wgpu::BlendFactor::SrcAlpha, dst_factor: wgpu::BlendFactor::OneMinusSrcAlpha, operation: wgpu::BlendOperation::Add },
                alpha: wgpu::BlendComponent { src_factor: wgpu::BlendFactor::One, dst_factor: wgpu::BlendFactor::One, operation: wgpu::BlendOperation::Max },
            });
            // the polygon id where the pixel ends up opaque (id alpha 1), else kept
            let id_blend = wgpu::BlendState {
                color: wgpu::BlendComponent { src_factor: wgpu::BlendFactor::SrcAlpha, dst_factor: wgpu::BlendFactor::OneMinusSrcAlpha, operation: wgpu::BlendOperation::Add },
                alpha: wgpu::BlendComponent { src_factor: wgpu::BlendFactor::Zero, dst_factor: wgpu::BlendFactor::One, operation: wgpu::BlendOperation::Add },
            };
            device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                label: None,
                layout: Some(&pl),
                vertex: wgpu::VertexState { module: &module, entry_point: "vs", compilation_options: Default::default(), buffers: &[wgpu::VertexBufferLayout { array_stride: VERTEX as u64, step_mode: wgpu::VertexStepMode::Vertex, attributes: &attrs }] },
                primitive: wgpu::PrimitiveState { topology: wgpu::PrimitiveTopology::TriangleList, ..Default::default() },
                depth_stencil: Some(wgpu::DepthStencilState {
                    format: DEPTH,
                    depth_write_enabled: depth_write,
                    depth_compare: if eq { wgpu::CompareFunction::LessEqual } else { wgpu::CompareFunction::Less },
                    stencil: wgpu::StencilState { front: face, back: face, read_mask: 0xff, write_mask: 0xff },
                    bias: Default::default(),
                }),
                multisample: Default::default(),
                fragment: Some(wgpu::FragmentState {
                    module: &module,
                    entry_point: if eq { "fs_eq" } else { "fs" },
                    compilation_options: Default::default(),
                    targets: &[Some(wgpu::ColorTargetState { format: COLOR, blend: color_blend, write_mask: wgpu::ColorWrites::ALL }), Some(wgpu::ColorTargetState { format: ID, blend: Some(id_blend), write_mask: wgpu::ColorWrites::ALL })],
                }),
                multiview: None,
                cache: None,
            })
        };
        use wgpu::CompareFunction::{Always, Equal, NotEqual};
        let over = [pipeline(false, Always, true, false, true), pipeline(true, Always, true, false, true)];
        let over_empty = [pipeline(false, Equal, true, false, true), pipeline(true, Equal, true, false, true)];
        let blend = [[pipeline(false, NotEqual, false, true, false), pipeline(true, NotEqual, false, true, false)], [pipeline(false, NotEqual, false, true, true), pipeline(true, NotEqual, false, true, true)]];
        let tex_entry = |binding: u32, sample_type: wgpu::TextureSampleType| entry(binding, wgpu::BindingType::Texture { sample_type, view_dimension: wgpu::TextureViewDimension::D2, multisampled: false }, wgpu::ShaderStages::FRAGMENT);
        let resolve_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: None,
            entries: &[entry(0, uniform, wgpu::ShaderStages::FRAGMENT), tex_entry(1, wgpu::TextureSampleType::Float { filterable: false }), tex_entry(2, wgpu::TextureSampleType::Float { filterable: false }), tex_entry(3, wgpu::TextureSampleType::Depth)],
        });
        let rmodule = device.create_shader_module(wgpu::ShaderModuleDescriptor { label: Some("gpu3d resolve"), source: wgpu::ShaderSource::Wgsl(RESOLVE.into()) });
        let rpl = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor { label: None, bind_group_layouts: &[&resolve_layout], push_constant_ranges: &[] });
        let resolve = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: None,
            layout: Some(&rpl),
            vertex: wgpu::VertexState { module: &rmodule, entry_point: "vs", compilation_options: Default::default(), buffers: &[] },
            primitive: Default::default(),
            depth_stencil: None,
            multisample: Default::default(),
            fragment: Some(wgpu::FragmentState { module: &rmodule, entry_point: "fs", compilation_options: Default::default(), targets: &[Some(wgpu::ColorTargetState { format: OUT, blend: None, write_mask: wgpu::ColorWrites::ALL })] }),
            multiview: None,
            cache: None,
        });
        Some(Gpu3d { device, queue, uniforms, resolve_uniforms, tex, pal, bind, resolve_layout, over, over_empty, blend, resolve, targets: None, name, c2d: None, tc: TexCache { atlas: Some(atlas), entries: Some(entries), ..Default::default() } })
    }

    fn targets(&mut self, w: u32, h: u32, k: u32) -> &Targets {
        if self.targets.as_ref().map_or(true, |t| (t.w, t.h, t.k) != (w, h, k)) {
            let d = &self.device;
            let tex = |w: u32, h: u32, format: wgpu::TextureFormat, usage: wgpu::TextureUsages| {
                d.create_texture(&wgpu::TextureDescriptor { label: None, size: wgpu::Extent3d { width: w, height: h, depth_or_array_layers: 1 }, mip_level_count: 1, sample_count: 1, dimension: wgpu::TextureDimension::D2, format, usage, view_formats: &[] })
            };
            let ra = wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING;
            let color = tex(w * k, h * k, COLOR, ra);
            let id = tex(w * k, h * k, ID, ra);
            let depth = tex(w * k, h * k, DEPTH, ra);
            let ou = wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC | wgpu::TextureUsages::TEXTURE_BINDING;
            let outs = [tex(w, h, OUT, ou), tex(w, h, OUT, ou)];
            let read = d.create_buffer(&wgpu::BufferDescriptor { label: None, size: (w * h * 4) as u64, usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ, mapped_at_creation: false });
            let view = |t: &wgpu::Texture, aspect: wgpu::TextureAspect| t.create_view(&wgpu::TextureViewDescriptor { aspect, ..Default::default() });
            let resolve_bind = d.create_bind_group(&wgpu::BindGroupDescriptor {
                label: None,
                layout: &self.resolve_layout,
                entries: &[
                    wgpu::BindGroupEntry { binding: 0, resource: self.resolve_uniforms.as_entire_binding() },
                    wgpu::BindGroupEntry { binding: 1, resource: wgpu::BindingResource::TextureView(&view(&color, wgpu::TextureAspect::All)) },
                    wgpu::BindGroupEntry { binding: 2, resource: wgpu::BindingResource::TextureView(&view(&id, wgpu::TextureAspect::All)) },
                    wgpu::BindGroupEntry { binding: 3, resource: wgpu::BindingResource::TextureView(&view(&depth, wgpu::TextureAspect::DepthOnly)) },
                ],
            });
            self.targets = Some(Targets { w, h, k, color, id, depth, outs, read, resolve_bind });
        }
        self.targets.as_ref().unwrap()
    }

    /// the texture image and palette slots as the 3D engine sees them
    fn upload_textures(&self, ram: &Ram) -> (Vec<u8>, Vec<u8>) {
        let v = ram.vram();
        let mut tex = vec![0u8; TEX_SIZE];
        let mut pal = vec![0u8; PAL_SIZE];
        for b in 0..9 {
            let (dst, off) = match v.place[b] {
                Place::TexImage(n) => (&mut tex, n as usize * 0x20000),
                Place::TexPalette(n) => (&mut pal, n as usize * 0x4000),
                _ => continue,
            };
            let n = SIZES[b].min(dst.len().saturating_sub(off));
            for (d, s) in dst[off..off + n].iter_mut().zip(&v.banks[b][..n]) {
                *d |= *s;
            }
        }
        self.queue.write_buffer(&self.tex, 0, &tex);
        self.queue.write_buffer(&self.pal, 0, &pal);
        (tex, pal)
    }

    /// the enhanced textures' entries for the (TEXIMAGE_PARAM, PLTT_BASE) pairs (0: none); those not seen before
    /// are made (in parallel: a mod's PNG, or FSR 1) and packed into the atlas
    fn enhanced(&mut self, ram: &Ram, img: &[u8], pals: &[u8], pairs: &[(u32, u32)]) -> Vec<u32> {
        use super::texcache;
        use rayon::prelude::*;
        let keys: Vec<Option<u64>> = pairs.iter().map(|&(t, p)| texcache::key(img, pals, t, p)).collect();
        let mut new: Vec<(u64, u32, u32)> = Vec::new();
        for (k, &(t, p)) in keys.iter().zip(pairs) {
            if let Some(k) = *k {
                if !self.tc.by_key.contains_key(&k) && !new.iter().any(|n| n.0 == k) {
                    new.push((k, t, p));
                }
            }
        }
        let (mods, dump, sharpness, denoise) = (self.tc.mods.clone(), self.tc.dump.clone(), self.tc.params.tex_sharpness, self.tc.params.tex_denoise);
        let names = self.tc.names.clone();
        let made: Vec<(u64, usize, usize, Vec<u8>, f64)> = new
            .par_iter()
            .map(|&(key, tex, pal)| {
                let t0 = std::time::Instant::now();
                let native = texcache::decode(ram, tex, pal);
                if let Some(d) = &dump {
                    texcache::dump(d, "textures", key, names.get(&key).map(|s| s.as_str()), &native);
                }
                let (w, h, rgba) = match mods.as_deref().and_then(|d| texcache::load_mod(d, "textures", key, names.get(&key).map(|s| s.as_str()))) {
                    Some(m) => m,
                    None => {
                        let up = texcache::fsr(&native, tex, sharpness, denoise);
                        (up.w, up.h, texcache::rgba8(&up))
                    }
                };
                (key, w, h, rgba, t0.elapsed().as_secs_f64() * 1000.0)
            })
            .collect();
        for (key, w, h, rgba, ms) in made {
            if std::env::var("LP_TEX_LOG").is_ok() {
                let (t, _) = new.iter().find(|n| n.0 == key).map(|n| (n.1, n.2)).unwrap_or((0, 0));
                eprintln!("texture {key:016x} ({}): -> {w}x{h} in {ms:.2} ms; at {:#x} (slot {}), banks {:?}, captured {:#b}", self.tc.names.get(&key).map_or("unnamed", |s| s.as_str()), (t & 0xffff) * 8, (t & 0xffff) * 8 / 0x20000, &ram.vram().place[..4], self.tc.captured);
            }
            let (w32, h32) = (w as u32, h as u32);
            // a place in the atlas: along the current row, else a new row (a full atlas leaves the rest unenhanced
            // until it is emptied before a later frame)
            let (mut x, mut y, mut row) = self.tc.cursor;
            if x + w32 > ATLAS {
                (x, y, row) = (0, y + row, 0);
            }
            if y + h32 > ATLAS || self.tc.places.len() + 1 >= ENTRIES || w32 > ATLAS {
                continue;
            }
            self.tc.cursor = (x + w32, y, row.max(h32));
            if self.tc.places.is_empty() {
                self.tc.places.push([0; 4]);
            }
            self.queue.write_texture(
                wgpu::ImageCopyTexture { texture: self.tc.atlas.as_ref().unwrap(), mip_level: 0, origin: wgpu::Origin3d { x, y, z: 0 }, aspect: wgpu::TextureAspect::All },
                &rgba,
                wgpu::ImageDataLayout { offset: 0, bytes_per_row: Some(w32 * 4), rows_per_image: Some(h32) },
                wgpu::Extent3d { width: w32, height: h32, depth_or_array_layers: 1 },
            );
            let e = self.tc.places.len() as u32;
            self.tc.places.push([x, y, w32, h32]);
            self.queue.write_buffer(self.tc.entries.as_ref().unwrap(), e as u64 * 16, &[x, y, w32, h32].iter().flat_map(|v| v.to_le_bytes()).collect::<Vec<u8>>());
            self.tc.by_key.insert(key, e);
        }
        keys.iter().map(|k| k.and_then(|k| self.tc.by_key.get(&k).copied()).unwrap_or(0)).collect()
    }

    /// the 3D layer of `frame` at `s` times the DS resolution (RGB555 with bit 15 where drawn), drawn with `k` x `k`
    /// samples per pixel (k > 1: also the drawn share per pixel, 0..=16), read back
    pub fn draw(&mut self, ram: &Ram, frame: &Frame, s: usize, k: usize, look: Look) -> (Vec<u16>, Option<Vec<u8>>) {
        self.draw_layer(ram, frame, s, k, look, 1);
        let (w, h) = (256 * s as u32, 192 * s as u32);
        let t = self.targets.as_ref().unwrap();
        let mut enc = self.device.create_command_encoder(&Default::default());
        enc.copy_texture_to_buffer(
            wgpu::ImageCopyTexture { texture: &t.outs[1], mip_level: 0, origin: wgpu::Origin3d::ZERO, aspect: wgpu::TextureAspect::All },
            wgpu::ImageCopyBuffer { buffer: &t.read, layout: wgpu::ImageDataLayout { offset: 0, bytes_per_row: Some(w * 4), rows_per_image: Some(h) } },
            wgpu::Extent3d { width: w, height: h, depth_or_array_layers: 1 },
        );
        self.queue.submit([enc.finish()]);
        let slice = t.read.slice(..);
        slice.map_async(wgpu::MapMode::Read, |_| {});
        self.device.poll(wgpu::Maintain::Wait);
        let (layer, cov) = {
            let data = slice.get_mapped_range();
            let px: Vec<u32> = data.chunks_exact(4).map(|b| u32::from_le_bytes([b[0], b[1], b[2], b[3]])).collect();
            let layer: Vec<u16> = px.iter().map(|&v| v as u16).collect();
            let cov = (k > 1).then(|| px.iter().map(|&v| (v >> 16) as u8).collect());
            (layer, cov)
        };
        t.read.unmap();
        (layer, cov)
    }

    /// the 3D layer of `frame` drawn into output slot `slot` (0 displayed, 1 scratch), left on the GPU: per pixel
    /// RGB555 | bit 15 where drawn | the drawn share (0..=16) << 16
    pub fn draw_layer(&mut self, ram: &Ram, frame: &Frame, s: usize, k: usize, look: Look, slot: usize) {
        let (w, h, k32) = (256 * s as u32, 192 * s as u32, k as u32);
        let polys = raster3d::prepare(frame, (s * k) as f32);
        let w_buffer = frame.swap & 2 != 0;
        // depth: z (0..1) or w (scaled by the frame's largest), kept below the cleared 1.0 (the CPU's clear is
        // f32::MAX: a polygon on the far plane is still drawn)
        let wmax = if w_buffer { polys.iter().flat_map(|(_, sv)| sv.iter().map(|v| v.z)).fold(1e-6f32, f32::max) } else { 1.0 };
        let (wscale, eps) = (0.999 / wmax, 1e-3 * 0.999 / wmax);
        let disp = ram.u32(0x4000060);
        let blend_on = disp & 8 != 0;
        let alpha_test = if disp & 4 != 0 { (ram.u8(0x4000340) & 0x1f) as f32 / 31.0 } else { -1.0 };
        let flags = (blend_on as u32) | ((disp & 2 != 0) as u32) << 1 | (look.bilinear as u32) << 2 | (look.smooth_toon as u32) << 3;
        let c5 = |c: u16| [(c & 31) as f32 / 31.0, ((c >> 5) & 31) as f32 / 31.0, ((c >> 10) & 31) as f32 / 31.0];
        let mut u = f32s(&[(w * k32) as f32, (h * k32) as f32, wscale, eps, alpha_test]);
        u.extend(flags.to_le_bytes());
        u.extend([0u8; 8]);
        for i in 0..32 {
            let c = c5(ram.u16(0x4000380 + 2 * i));
            u.extend(f32s(&[c[0], c[1], c[2], 1.0]));
        }
        self.queue.write_buffer(&self.uniforms, 0, &u);
        let mut r: Vec<u8> = [k32, (look.outline.max(1) * k) as u32, (disp & 0x20 != 0) as u32, 0].iter().flat_map(|x| x.to_le_bytes()).collect();
        for i in 0..8 {
            let c = c5(ram.u16(0x4000330 + 2 * i));
            r.extend(f32s(&[c[0], c[1], c[2], 1.0]));
        }
        self.queue.write_buffer(&self.resolve_uniforms, 0, &r);
        let (img, pals) = self.upload_textures(ram);
        // the enhanced textures the polygons use (a full atlas is emptied before the frame, never during it)
        let mut enh: std::collections::HashMap<(u32, u32), u32> = std::collections::HashMap::new();
        if look.upscale {
            let (_, y, row) = self.tc.cursor;
            if y + row > ATLAS * 3 / 4 || self.tc.places.len() > ENTRIES * 3 / 4 {
                self.tc.by_key.clear();
                self.tc.places.clear();
                self.tc.cursor = (0, 0, 0);
            }
            // (textures in a slot with no bank mapped, which read zeros, or in a bank the game captures into, which
            // change with every capture, are left as they are)
            let captured = |tex: u32| {
                let slot = ((tex & 0xffff) * 8 / 0x20000) as u32;
                let banks: Vec<usize> = (0..4).filter(|&b| ram.vram().place[b] == Place::TexImage(slot)).collect();
                banks.is_empty() || banks.iter().any(|&b| self.tc.captured & 1 << b != 0)
            };
            let mut pairs: Vec<(u32, u32)> = polys.iter().map(|(p, _)| (p.tex, p.pal)).filter(|&(t, _)| !captured(t)).collect();
            pairs.sort_unstable();
            pairs.dedup();
            let es = self.enhanced(ram, &img, &pals, &pairs);
            enh.extend(pairs.into_iter().zip(es));
        }
        // the triangles, and per polygon its range and how it is drawn
        let mut verts: Vec<u8> = Vec::new();
        let mut draws: Vec<(std::ops::Range<u32>, bool, bool, bool)> = Vec::new(); // (range, translucent, depth-equal, depth write)
        for (p, sv) in &polys {
            let start = (verts.len() / VERTEX) as u32;
            let e = enh.get(&(p.tex, p.pal)).copied().unwrap_or(0);
            for i in 1..sv.len() - 1 {
                for v in [&sv[0], &sv[i], &sv[i + 1]] {
                    verts.extend(f32s(&[v.x, v.y, v.z, v.w, v.c[0], v.c[1], v.c[2], v.uv[0], v.uv[1]]));
                    for x in [p.attr, p.tex, p.pal, e] {
                        verts.extend(x.to_le_bytes());
                    }
                }
            }
            let end = (verts.len() / VERTEX) as u32;
            let tr = blend_on && raster3d::translucent(p);
            let eq = p.attr & (1 << 14) != 0;
            // merge runs of polygons drawn the same simple way
            match draws.last_mut() {
                Some((rg, false, e, _)) if !tr && *e == eq && rg.end == start => rg.end = end,
                _ => draws.push((start..end, tr, eq, p.attr & (1 << 11) != 0)),
            }
        }
        let vbuf = (!verts.is_empty()).then(|| {
            let b = self.device.create_buffer(&wgpu::BufferDescriptor { label: None, size: verts.len() as u64, usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST, mapped_at_creation: false });
            self.queue.write_buffer(&b, 0, &verts);
            b
        });
        let clear = ram.u32(0x4000350);
        let cc = c5(clear as u16);
        let ca = ((clear >> 16) & 0x1f) as f64 / 31.0;
        let clear_id = ((clear >> 24) & 0x3f) as f64 / 255.0;
        self.targets(w, h, k32);
        let t = self.targets.as_ref().unwrap();
        let mut enc = self.device.create_command_encoder(&Default::default());
        {
            let cv = t.color.create_view(&Default::default());
            let iv = t.id.create_view(&Default::default());
            let dv = t.depth.create_view(&Default::default());
            let mut pass = enc.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: None,
                color_attachments: &[
                    Some(wgpu::RenderPassColorAttachment { view: &cv, resolve_target: None, ops: wgpu::Operations { load: wgpu::LoadOp::Clear(wgpu::Color { r: cc[0] as f64, g: cc[1] as f64, b: cc[2] as f64, a: ca }), store: wgpu::StoreOp::Store } }),
                    Some(wgpu::RenderPassColorAttachment { view: &iv, resolve_target: None, ops: wgpu::Operations { load: wgpu::LoadOp::Clear(wgpu::Color { r: clear_id, g: 0.0, b: 0.0, a: 0.0 }), store: wgpu::StoreOp::Store } }),
                ],
                depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                    view: &dv,
                    depth_ops: Some(wgpu::Operations { load: wgpu::LoadOp::Clear(1.0), store: wgpu::StoreOp::Store }),
                    // the stencil says whether anything (alpha above 0) is in the pixel: the rear plane's alpha
                    stencil_ops: Some(wgpu::Operations { load: wgpu::LoadOp::Clear((ca > 0.0) as u32), store: wgpu::StoreOp::Store }),
                }),
                timestamp_writes: None,
                occlusion_query_set: None,
            });
            if let Some(vb) = &vbuf {
                pass.set_bind_group(0, &self.bind, &[]);
                pass.set_vertex_buffer(0, vb.slice(..));
                pass.set_stencil_reference(0);
                for (rg, tr, eq, dw) in draws {
                    let e = eq as usize;
                    if !tr {
                        pass.set_pipeline(&self.over[e]);
                        pass.draw(rg, 0..1);
                    } else {
                        // blended over what is there, then its full-alpha pixels, then pixels over nothing
                        pass.set_pipeline(&self.blend[dw as usize][e]);
                        pass.draw(rg.clone(), 1..2);
                        pass.set_pipeline(&self.over[e]);
                        pass.draw(rg.clone(), 2..3);
                        pass.set_pipeline(&self.over_empty[e]);
                        pass.draw(rg, 1..2);
                    }
                }
            }
        }
        {
            let ov = t.outs[slot].create_view(&Default::default());
            let mut pass = enc.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: None,
                color_attachments: &[Some(wgpu::RenderPassColorAttachment { view: &ov, resolve_target: None, ops: wgpu::Operations { load: wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT), store: wgpu::StoreOp::Store } })],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
            });
            pass.set_pipeline(&self.resolve);
            pass.set_bind_group(0, &t.resolve_bind, &[]);
            pass.draw(0..3, 0..1);
        }
        self.queue.submit([enc.finish()]);
    }
}
