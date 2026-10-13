//! wgpu renderer for assembled heroine meshes: DS-style toon shading and edge marking.
//! Reproduces the rules of tools/toonrender.py (the reference rasteriser): per-vertex lighting index into the toon table,
//! texture modulation, alpha-test, and an edge pass keyed on polygon id (edge colour table [id >> 3]).
//!
//! Passes: scene -> (colour, polygon id, depth) at `scene_size`; edge -> colour with outlines; blit -> output (linear, so
//! `scene_size > output size` gives supersampling).

use crate::nns2d::Image;
use crate::nns3d::*;
use std::collections::HashMap;
use std::sync::Arc;
use wgpu::util::DeviceExt;

/// how a pixel's toon colour is chosen
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Shading {
    /// as the DS: light index per vertex, toon colours interpolated across the triangle
    PerVertex,
    /// light index per pixel, quantised to `n` bands of the toon table (cel look)
    Stepped(u32),
    /// light index per pixel, linearly interpolated between toon table entries (no banding)
    Smooth,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum TexMode {
    /// nearest texel, no mips (DS)
    Nearest,
    /// trilinear + anisotropic filtering of the original texels
    Filtered,
    /// Lanczos-3 upscale by the factor, then trilinear + anisotropic
    Lanczos(u32),
}

/// renderer switches; `RenderOpts::default()` is the authentic DS pipeline (the one verified against the reference rasteriser)
#[derive(Clone, Debug, PartialEq)]
pub struct RenderOpts {
    pub shading: Shading,
    pub tex: TexMode,
    /// edge-marking radius in scene pixels (1 = the DS 1 px outline)
    pub outline: f32,
    /// rim-light strength (0 = off)
    pub rim: f32,
    /// perspective camera distance in units of the model radius (None = orthographic like the reference)
    pub persp: Option<f64>,
}

impl Default for RenderOpts {
    fn default() -> Self {
        RenderOpts { shading: Shading::PerVertex, tex: TexMode::Nearest, outline: 1.0, rim: 0.0, persp: None }
    }
}

/// named render looks: (switches, PN subdivision level).  `authentic` is the verified DS pipeline; the rest are enhanced-mode presets.
pub fn look(name: &str) -> Option<(RenderOpts, u32)> {
    let base = RenderOpts::default();
    Some(match name {
        "authentic" => (base, 1),
        "filtered" => (RenderOpts { tex: TexMode::Filtered, ..base }, 1),
        "clean" => (RenderOpts { shading: Shading::Smooth, tex: TexMode::Lanczos(4), outline: 2.2, ..base }, 1),
        "clean-s" => (RenderOpts { shading: Shading::Smooth, tex: TexMode::Lanczos(4), outline: 2.2, ..base }, 4),
        "soft" => (RenderOpts { shading: Shading::Smooth, tex: TexMode::Lanczos(4), outline: 1.6, rim: 0.18, persp: Some(3.0), ..base }, 1),
        "soft-s" => (RenderOpts { shading: Shading::Smooth, tex: TexMode::Lanczos(4), outline: 1.6, rim: 0.18, persp: Some(3.0), ..base }, 4),
        "wide" => (RenderOpts { shading: Shading::Smooth, tex: TexMode::Lanczos(4), outline: 1.8, rim: 0.12, persp: Some(1.4), ..base }, 4),
        _ => return None,
    })
}

pub const LIGHT_COLORS: [u32; 4] = [0x7fff, 0x4210, 0x2108, 0x7fff];

pub fn rgb555_f(c: u32) -> [f32; 3] {
    [(c & 31) as f32 / 31.0, ((c >> 5) & 31) as f32 / 31.0, ((c >> 10) & 31) as f32 / 31.0]
}

/// light travel direction in view space (DS convention, camera looks down -z); same construction as the reference
pub fn norm3(a: [f64; 3]) -> [f64; 3] {
    let l = (a[0] * a[0] + a[1] * a[1] + a[2] * a[2]).sqrt().max(1e-12);
    [a[0] / l, a[1] / l, a[2] / l]
}

fn cross3(a: [f64; 3], b: [f64; 3]) -> [f64; 3] {
    [a[1] * b[2] - a[2] * b[1], a[2] * b[0] - a[0] * b[2], a[0] * b[1] - a[1] * b[0]]
}

pub fn light_vector(yaw_deg: f64, pitch_deg: f64) -> [f32; 3] {
    let (y, p) = (yaw_deg.to_radians(), pitch_deg.to_radians());
    let (x, z) = (-y.sin(), -y.cos());
    let (yy, zz) = (z * -p.sin(), z * p.cos());
    [x as f32, yy as f32, zz as f32]
}

// ------------------------------------------------------------------ CPU-side frame
#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
pub struct GpuVertex {
    pos: [f32; 3],
    nrm: [f32; 3],
    uv: [f32; 2],
    col: [f32; 3],
}

pub struct MatInfo {
    pub enable: u32,
    pub mode: u32,
    pub pid: u32,
    pub alpha: u32,
    pub diffuse: f32,
    pub ambient: f32,
    pub cull: u32,
    pub tex_param: u32,
    pub tex: Option<Arc<Image>>,
}

pub struct Batch {
    pub mat: usize,
    /// first / count in vertices
    pub first: u32,
    pub count: u32,
}

pub struct FrameData {
    /// three vertices per triangle, grouped by material (see `batches`)
    pub verts: Vec<[GpuVertex; 3]>,
    pub batches: Vec<Batch>,
    pub mats: Vec<MatInfo>,
    pub toon: Vec<u16>,
    pub edge: Vec<u16>,
}

/// decoded-texture cache (textures are decoded once per (texture, palette); variant overrides are keyed by pointer)
#[derive(Default)]
pub struct ImageCache {
    map: HashMap<(String, String), Option<Arc<Image>>>,
}

impl ImageCache {
    pub fn get(&mut self, mat: &Material, idx: &TexIndex) -> Option<Arc<Image>> {
        if mat.image.is_some() {
            return mat.image.clone();
        }
        let key = (mat.tex.clone().unwrap_or_default(), mat.pal.clone().unwrap_or_default());
        self.map.entry(key).or_insert_with(|| material_image(mat, idx)).clone()
    }
}

fn to_gpu(t: &Tri) -> [GpuVertex; 3] {
    let g = |v: &TriVert| GpuVertex {
        pos: [v.pos[0] as f32, v.pos[1] as f32, v.pos[2] as f32],
        nrm: [v.nrm[0] as f32, v.nrm[1] as f32, v.nrm[2] as f32],
        uv: [v.uv[0] as f32, v.uv[1] as f32],
        col: [v.col[0] as f32, v.col[1] as f32, v.col[2] as f32],
    };
    [g(&t.verts[0]), g(&t.verts[1]), g(&t.verts[2])]
}

/// group triangles by material and flatten to GPU vertices (parallel)
pub fn build_frame(tris: &[Tri], mats: &[Material], toon: &[u16], edge: &[u16], idx: &TexIndex, cache: &mut ImageCache) -> FrameData {
    build_frame_smooth(tris, mats, toon, edge, idx, cache, None)
}

/// as `build_frame`, optionally tessellating on the fly: `smooth = Some((subdivisions, strength, crease_deg))`
pub fn build_frame_smooth(tris: &[Tri], mats: &[Material], toon: &[u16], edge: &[u16], idx: &TexIndex, cache: &mut ImageCache, smooth: Option<(u32, f64, f64)>) -> FrameData {
    use rayon::prelude::*;
    // counting sort of triangle indices by material
    let mut counts = vec![0u32; mats.len() + 1];
    for t in tris {
        if t.material < mats.len() {
            counts[t.material + 1] += 1;
        }
    }
    for i in 0..mats.len() {
        counts[i + 1] += counts[i];
    }
    let mut fill = counts.clone();
    let mut order = vec![0u32; counts[mats.len()] as usize];
    for (i, t) in tris.iter().enumerate() {
        if t.material < mats.len() {
            order[fill[t.material] as usize] = i as u32;
            fill[t.material] += 1;
        }
    }
    let (verts, per): (Vec<[GpuVertex; 3]>, u32) = match smooth {
        Some((n, strength, crease)) if n > 1 => (crate::tessellate::smooth_map(tris, &order, n, strength, crease, |t| to_gpu(&t)), n * n),
        _ => (order.par_iter().map(|&ti| to_gpu(&tris[ti as usize])).collect(), 1),
    };
    let mut batches = Vec::new();
    let mut infos = Vec::new();
    for (mi, m) in mats.iter().enumerate() {
        let a = m.polyattr;
        let (alpha, enable, mode, pid) = ((a >> 16) & 31, a & 15, (a >> 4) & 3, (a >> 24) & 63);
        infos.push(MatInfo { enable, mode, pid, alpha, diffuse: (m.diffuse & 31) as f32 / 31.0, ambient: (m.ambient & 31) as f32 / 31.0, cull: m.cull, tex_param: m.tex_param, tex: cache.get(m, idx) });
        let n = counts[mi + 1] - counts[mi];
        if alpha != 0 && n > 0 {
            batches.push(Batch { mat: mi, first: counts[mi] * per * 3, count: n * per * 3 });
        }
    }
    FrameData { verts, batches, mats: infos, toon: toon.to_vec(), edge: edge.to_vec() }
}

/// orthographic orbit view (same projection as the reference rasteriser)
#[derive(Clone, Debug)]
pub struct View {
    pub yaw: f64,
    pub pitch: f64,
    pub zoom: f64,
    pub center: [f64; 3],
    /// largest half-extent of the model in the rotated frame (sets the scale: 0.9 / ext of the viewport)
    pub ext: f64,
    /// largest half-height after centring (for fitting portraits by height)
    pub ext_y: f64,
    pub radius: f64,
    pub lights: [(i32, i32); 3],
    pub bg: [f32; 3],
    /// a real camera (eye/target in the game's world units, vertical field of view in degrees); overrides the orbit view
    pub camera: Option<Camera>,
}

/// perspective camera as used by the game's camera presets.  World frame = the model rotated half a turn about y (the heroine faces the camera),
/// so the camera sits at `eye` with z mirrored relative to the preset numbers (the viewer's frame looks along +z).
#[derive(Clone, Copy, Debug)]
pub struct Camera {
    pub eye: [f64; 3],
    pub target: [f64; 3],
    pub fov_deg: f64,
}

/// centre, extent and bounding radius from a set of triangles for a given yaw/pitch
pub fn fit_view(tris: &[Tri], yaw: f64, pitch: f64, lights: [(i32, i32); 3]) -> View {
    let mut c = [0.0; 3];
    let mut n = 0.0;
    for t in tris {
        for v in &t.verts {
            for k in 0..3 {
                c[k] += v.pos[k];
            }
            n += 1.0;
        }
    }
    if n > 0.0 {
        for k in 0..3 {
            c[k] /= n;
        }
    }
    let rot = rot_matrix(yaw, pitch);
    let (mut ext, mut ext_y, mut radius) = (0.0f64, 0.0f64, 0.0f64);
    for t in tris {
        for v in &t.verts {
            let p = [v.pos[0] - c[0], v.pos[1] - c[1], v.pos[2] - c[2]];
            let r: Vec<f64> = (0..3).map(|i| rot[i][0] * p[0] + rot[i][1] * p[1] + rot[i][2] * p[2]).collect();
            ext = ext.max(r[0].abs()).max(r[1].abs());
            ext_y = ext_y.max(r[1].abs());
            radius = radius.max((p[0] * p[0] + p[1] * p[1] + p[2] * p[2]).sqrt());
        }
    }
    View { yaw, pitch, zoom: 1.0, center: c, ext: ext.max(1e-6), ext_y: ext_y.max(1e-6), radius: radius.max(1e-6), lights, bg: [46.0 / 255.0, 46.0 / 255.0, 58.0 / 255.0], camera: None }
}

impl View {
    /// fit by height: the figure fills `1/margin` of the frame height
    pub fn fit_height(&mut self, margin: f64) {
        self.ext = self.ext_y * margin;
    }
}

pub fn rot_matrix(yaw: f64, pitch: f64) -> [[f64; 3]; 3] {
    let (cy, sy, cp, sp) = (yaw.cos(), yaw.sin(), pitch.cos(), pitch.sin());
    let r1 = [[cy, 0.0, sy], [0.0, 1.0, 0.0], [-sy, 0.0, cy]];
    let r2 = [[1.0, 0.0, 0.0], [0.0, cp, -sp], [0.0, sp, cp]];
    let mut m = [[0.0; 3]; 3];
    for i in 0..3 {
        for j in 0..3 {
            m[i][j] = (0..3).map(|k| r2[i][k] * r1[k][j]).sum();
        }
    }
    m
}

// ------------------------------------------------------------------ GPU
#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
struct Globals {
    rot: [[f32; 4]; 4],
    scale: [f32; 4],   // kx, ky, depth scale, edge depth epsilon
    mode: [f32; 4],    // shading (0 vertex, 1 stepped, 2 smooth), bands, rim, edge radius (scene px)
    persp: [f32; 4],   // distance, near, far, enabled
    lights: [[f32; 4]; 3],
    toon: [[f32; 4]; 32],
    edge: [[f32; 4]; 8],
    bg: [f32; 4],
}

#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
struct MatUniform {
    info: [u32; 4],
    da: [f32; 4],
}

const SHADER: &str = r#"
struct Globals {
  rot: mat4x4<f32>,
  scale: vec4<f32>,
  mode: vec4<f32>,
  persp: vec4<f32>,
  lights: array<vec4<f32>, 3>,
  toon: array<vec4<f32>, 32>,
  edge: array<vec4<f32>, 8>,
  bg: vec4<f32>,
};
struct Mat { info: vec4<u32>, da: vec4<f32> };
@group(0) @binding(0) var<uniform> g: Globals;
@group(1) @binding(0) var<uniform> m: Mat;
@group(1) @binding(1) var tex: texture_2d<f32>;
@group(1) @binding(2) var smp: sampler;

struct VIn { @location(0) pos: vec3<f32>, @location(1) nrm: vec3<f32>, @location(2) uv: vec2<f32>, @location(3) col: vec3<f32> };
struct VOut { @builtin(position) pos: vec4<f32>, @location(0) uv: vec2<f32>, @location(1) col: vec3<f32>, @location(2) nv: vec3<f32> };

@vertex fn vs(v: VIn) -> VOut {
  let p = (g.rot * vec4<f32>(v.pos, 1.0)).xyz;
  let n = (g.rot * vec4<f32>(v.nrm, 0.0)).xyz;
  let nv = vec3<f32>(-n.x, n.y, -n.z);
  var o: VOut;
  if (g.persp.w > 1.5) {
    let zc = g.persp.z / (g.persp.z - g.persp.y) * p.z - g.persp.z * g.persp.y / (g.persp.z - g.persp.y);
    o.pos = vec4<f32>(p.x * g.scale.x, p.y * g.scale.y, zc, p.z);
  } else if (g.persp.w > 0.5) {
    let w = p.z + g.persp.x;
    let zc = g.persp.z / (g.persp.z - g.persp.y) * w - g.persp.z * g.persp.y / (g.persp.z - g.persp.y);
    o.pos = vec4<f32>(p.x * g.scale.x * g.persp.x, p.y * g.scale.y * g.persp.x, zc, w);
  } else {
    o.pos = vec4<f32>(p.x * g.scale.x, p.y * g.scale.y, p.z * g.scale.z + 0.5, 1.0);
  }
  o.uv = v.uv / vec2<f32>(m.da.z, m.da.w);
  o.nv = nv;
  let lit = m.info.x;
  if (lit != 0u && g.mode.x < 0.5) {
    var acc = 0.0;
    for (var i = 0u; i < 3u; i = i + 1u) {
      if (((lit >> i) & 1u) == 1u) {
        let d = max(0.0, -dot(nv, g.lights[i].xyz));
        acc = acc + (m.da.x * d + m.da.y) * g.lights[i].w;
      }
    }
    let idx = min(31, i32(floor(acc * 31.0 + 0.5)));
    if (m.info.y == 2u) { o.col = g.toon[idx].rgb; } else { o.col = vec3<f32>(f32(idx) / 31.0); }
  } else {
    o.col = v.col;
  }
  return o;
}

struct FOut { @location(0) color: vec4<f32>, @location(1) id: u32 };

@fragment fn fs(i: VOut) -> FOut {
  var rgb = i.col;
  let lit = m.info.x;
  if (lit != 0u && g.mode.x > 0.5) {
    let n = normalize(i.nv);
    var acc = 0.0;
    for (var k = 0u; k < 3u; k = k + 1u) {
      if (((lit >> k) & 1u) == 1u) {
        let d = max(0.0, -dot(n, g.lights[k].xyz));
        acc = acc + (m.da.x * d + m.da.y) * g.lights[k].w;
      }
    }
    if (m.info.y == 2u) {
      var x = clamp(acc * 31.0, 0.0, 31.0);
      if (g.mode.x > 1.5) {
        let lo = u32(floor(x));
        let hi = min(lo + 1u, 31u);
        rgb = mix(g.toon[lo].rgb, g.toon[hi].rgb, x - floor(x));
      } else {
        let bands = g.mode.y;
        let t = clamp(acc, 0.0, 0.9999) * bands;
        let f = floor(t);
        let feather = smoothstep(0.88, 1.0, t - f);          // anti-aliased band edges
        let q = clamp((f + feather) / max(bands - 1.0, 1.0), 0.0, 1.0) * 31.0;
        let lo = u32(floor(q));
        rgb = mix(g.toon[lo].rgb, g.toon[min(lo + 1u, 31u)].rgb, q - floor(q));
      }
    } else {
      rgb = vec3<f32>(clamp(acc, 0.0, 1.0));
    }
  }
  if (m.info.w == 1u) {
    let t = textureSample(tex, smp, i.uv);
    if (t.a < 0.5) { discard; }
    rgb = rgb * t.rgb;
  }
  if (g.mode.z > 0.0) {
    let n = normalize(i.nv);
    let rim = pow(1.0 - clamp(n.z, 0.0, 1.0), 3.0) * g.mode.z;
    rgb = rgb + rim * vec3<f32>(0.55, 0.65, 0.9);
  }
  var o: FOut;
  o.color = vec4<f32>(rgb, 1.0);
  o.id = m.info.z + 1u;
  return o;
}

struct FsOut { @builtin(position) pos: vec4<f32>, @location(0) uv: vec2<f32> };
@vertex fn vs_full(@builtin(vertex_index) vi: u32) -> FsOut {
  var p = array<vec2<f32>, 3>(vec2<f32>(-1.0, -1.0), vec2<f32>(3.0, -1.0), vec2<f32>(-1.0, 3.0));
  var o: FsOut;
  o.pos = vec4<f32>(p[vi], 0.0, 1.0);
  o.uv = vec2<f32>((p[vi].x + 1.0) * 0.5, (1.0 - p[vi].y) * 0.5);
  return o;
}

@group(0) @binding(0) var color_tex: texture_2d<f32>;
@group(0) @binding(1) var id_tex: texture_2d<u32>;
@group(0) @binding(2) var depth_tex: texture_depth_2d;
@group(0) @binding(3) var<uniform> ge: Globals;

@fragment fn fs_edge(@builtin(position) fp: vec4<f32>) -> @location(0) vec4<f32> {
  let c = vec2<i32>(fp.xy);
  let dims = vec2<i32>(textureDimensions(id_tex));
  let id = textureLoad(id_tex, c, 0).r;
  var col = textureLoad(color_tex, c, 0);
  if (id > 0u) {
    let z = textureLoad(depth_tex, c, 0);
    let eps = ge.scale.w;
    var mark = false;
    let rad = ge.mode.w;
    if (rad <= 1.01) {
      var offs = array<vec2<i32>, 4>(vec2<i32>(1, 0), vec2<i32>(-1, 0), vec2<i32>(0, 1), vec2<i32>(0, -1));
      for (var k = 0; k < 4; k = k + 1) {
        let q = clamp(c + offs[k], vec2<i32>(0, 0), dims - vec2<i32>(1, 1));
        let nid = textureLoad(id_tex, q, 0).r;
        let nz = textureLoad(depth_tex, q, 0);
        if (nid != id && z <= nz + eps) { mark = true; }
      }
    } else {
      let r = i32(ceil(rad));
      for (var dy = -r; dy <= r; dy = dy + 1) {
        for (var dx = -r; dx <= r; dx = dx + 1) {
          if (f32(dx * dx + dy * dy) > rad * rad) { continue; }
          let q = clamp(c + vec2<i32>(dx, dy), vec2<i32>(0, 0), dims - vec2<i32>(1, 1));
          let nid = textureLoad(id_tex, q, 0).r;
          let nz = textureLoad(depth_tex, q, 0);
          if (nid != id && z <= nz + eps) { mark = true; }
        }
      }
    }
    if (mark) { col = vec4<f32>(ge.edge[min(7u, (id - 1u) >> 3u)].rgb, 1.0); }
  }
  return col;
}

@group(0) @binding(0) var ov_tex: texture_2d<f32>;
@group(0) @binding(1) var ov_smp: sampler;
@fragment fn fs_overlay(i: FsOut) -> @location(0) vec4<f32> {
  return textureSampleLevel(ov_tex, ov_smp, i.uv, 0.0);
}

@group(0) @binding(0) var blit_tex: texture_2d<f32>;
@group(0) @binding(1) var blit_smp: sampler;
@fragment fn fs_blit(i: FsOut) -> @location(0) vec4<f32> {
  return textureSampleLevel(blit_tex, blit_smp, i.uv, 0.0);
}
"#;

struct GpuTex {
    view: wgpu::TextureView,
    w: u32,
    h: u32,
    _keep: Arc<Image>,
}

pub struct Renderer {
    pub device: wgpu::Device,
    pub queue: wgpu::Queue,
    pub scene_w: u32,
    pub scene_h: u32,
    pub opts: RenderOpts,
    globals_buf: wgpu::Buffer,
    globals_bg: wgpu::BindGroup,
    mat_layout: wgpu::BindGroupLayout,
    scene_pipes: [wgpu::RenderPipeline; 3],
    edge_pipe: wgpu::RenderPipeline,
    edge_layout: wgpu::BindGroupLayout,
    blit_pipe: wgpu::RenderPipeline,
    blit_layout: wgpu::BindGroupLayout,
    linear: wgpu::Sampler,
    white: GpuTex,
    tex_cache: HashMap<usize, GpuTex>,
    targets: Targets,
    edge_bg: wgpu::BindGroup,
    vbuf: Option<(wgpu::Buffer, u64)>,
    samplers: HashMap<(bool, u32), wgpu::Sampler>,
    mat_bgs: HashMap<BgKey, wgpu::BindGroup>,
    overlay_pipe: wgpu::RenderPipeline,
    over_pipe: wgpu::RenderPipeline,
    backdrop: Option<wgpu::BindGroup>,
    overlay: Option<wgpu::BindGroup>,
    fade_tex: wgpu::Texture,
    fade_bg: wgpu::BindGroup,
    fade: f32,
}

#[derive(Clone, PartialEq, Eq, Hash)]
struct BgKey {
    tex: usize,
    tex_param: u32,
    info: [u32; 4],
    da: [u32; 4],
}

struct Targets {
    color: wgpu::TextureView,
    id: wgpu::TextureView,
    depth: wgpu::TextureView,
    edged: wgpu::TextureView,
}

fn make_targets(device: &wgpu::Device, w: u32, h: u32) -> Targets {
    let mk = |fmt: wgpu::TextureFormat, usage: wgpu::TextureUsages| {
        device
            .create_texture(&wgpu::TextureDescriptor {
                label: None,
                size: wgpu::Extent3d { width: w, height: h, depth_or_array_layers: 1 },
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format: fmt,
                usage,
                view_formats: &[],
            })
            .create_view(&Default::default())
    };
    let ra_tb = wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING;
    Targets {
        color: mk(wgpu::TextureFormat::Rgba8Unorm, ra_tb),
        id: mk(wgpu::TextureFormat::R32Uint, ra_tb),
        depth: mk(wgpu::TextureFormat::Depth32Float, ra_tb),
        edged: mk(wgpu::TextureFormat::Rgba8Unorm, ra_tb),
    }
}

fn make_edge_bg(device: &wgpu::Device, layout: &wgpu::BindGroupLayout, t: &Targets, globals: &wgpu::Buffer) -> wgpu::BindGroup {
    device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: None,
        layout,
        entries: &[
            wgpu::BindGroupEntry { binding: 0, resource: wgpu::BindingResource::TextureView(&t.color) },
            wgpu::BindGroupEntry { binding: 1, resource: wgpu::BindingResource::TextureView(&t.id) },
            wgpu::BindGroupEntry { binding: 2, resource: wgpu::BindingResource::TextureView(&t.depth) },
            wgpu::BindGroupEntry { binding: 3, resource: globals.as_entire_binding() },
        ],
    })
}

fn upload(device: &wgpu::Device, queue: &wgpu::Queue, img: &Arc<Image>, mode: TexMode) -> GpuTex {
    let (ow, oh) = (img.w.max(1) as u32, img.h.max(1) as u32);
    let levels: Vec<Image> = match mode {
        TexMode::Nearest => vec![(**img).clone()],
        TexMode::Filtered => crate::upscale::mip_chain(img),
        TexMode::Lanczos(k) => crate::upscale::mip_chain(&crate::upscale::lanczos_upscale(img, k.max(1) as usize, false)),
    };
    let (w, h) = (levels[0].w as u32, levels[0].h as u32);
    let tex = device.create_texture(&wgpu::TextureDescriptor {
        label: None,
        size: wgpu::Extent3d { width: w, height: h, depth_or_array_layers: 1 },
        mip_level_count: levels.len() as u32,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: wgpu::TextureFormat::Rgba8Unorm,
        usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
        view_formats: &[],
    });
    for (i, l) in levels.iter().enumerate() {
        let (lw, lh) = (l.w as u32, l.h as u32);
        queue.write_texture(
            wgpu::ImageCopyTexture { texture: &tex, mip_level: i as u32, origin: wgpu::Origin3d::ZERO, aspect: wgpu::TextureAspect::All },
            &l.buf,
            wgpu::ImageDataLayout { offset: 0, bytes_per_row: Some(4 * lw), rows_per_image: Some(lh) },
            wgpu::Extent3d { width: lw, height: lh, depth_or_array_layers: 1 },
        );
    }
    // `w`/`h` stay the ORIGINAL size: uvs are in original texels
    GpuTex { view: tex.create_view(&Default::default()), w: ow, h: oh, _keep: img.clone() }
}

impl Renderer {
    pub fn new(device: wgpu::Device, queue: wgpu::Queue, scene_w: u32, scene_h: u32, out_format: wgpu::TextureFormat) -> Renderer {
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor { label: Some("toon"), source: wgpu::ShaderSource::Wgsl(SHADER.into()) });
        let bgl = |entries: &[wgpu::BindGroupLayoutEntry]| device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor { label: None, entries });
        let ent = |binding: u32, vis: wgpu::ShaderStages, ty: wgpu::BindingType| wgpu::BindGroupLayoutEntry { binding, visibility: vis, ty, count: None };
        let uniform = wgpu::BindingType::Buffer { ty: wgpu::BufferBindingType::Uniform, has_dynamic_offset: false, min_binding_size: None };
        let vf = wgpu::ShaderStages::VERTEX | wgpu::ShaderStages::FRAGMENT;
        let global_layout = bgl(&[ent(0, vf, uniform)]);
        let mat_layout = bgl(&[
            ent(0, vf, uniform),
            ent(1, wgpu::ShaderStages::FRAGMENT, wgpu::BindingType::Texture { sample_type: wgpu::TextureSampleType::Float { filterable: true }, view_dimension: wgpu::TextureViewDimension::D2, multisampled: false }),
            ent(2, wgpu::ShaderStages::FRAGMENT, wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering)),
        ]);
        let f = wgpu::ShaderStages::FRAGMENT;
        let edge_layout = bgl(&[
            ent(0, f, wgpu::BindingType::Texture { sample_type: wgpu::TextureSampleType::Float { filterable: false }, view_dimension: wgpu::TextureViewDimension::D2, multisampled: false }),
            ent(1, f, wgpu::BindingType::Texture { sample_type: wgpu::TextureSampleType::Uint, view_dimension: wgpu::TextureViewDimension::D2, multisampled: false }),
            ent(2, f, wgpu::BindingType::Texture { sample_type: wgpu::TextureSampleType::Depth, view_dimension: wgpu::TextureViewDimension::D2, multisampled: false }),
            ent(3, f, uniform),
        ]);
        let blit_layout = bgl(&[
            ent(0, f, wgpu::BindingType::Texture { sample_type: wgpu::TextureSampleType::Float { filterable: true }, view_dimension: wgpu::TextureViewDimension::D2, multisampled: false }),
            ent(1, f, wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering)),
        ]);
        let pl = |layouts: &[&wgpu::BindGroupLayout]| device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor { label: None, bind_group_layouts: layouts, push_constant_ranges: &[] });
        let scene_layout = pl(&[&global_layout, &mat_layout]);
        let attrs = wgpu::vertex_attr_array![0 => Float32x3, 1 => Float32x3, 2 => Float32x2, 3 => Float32x3];
        let vbuf = wgpu::VertexBufferLayout { array_stride: std::mem::size_of::<GpuVertex>() as u64, step_mode: wgpu::VertexStepMode::Vertex, attributes: &attrs };
        let mk_scene = |cull: Option<wgpu::Face>| {
            device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                label: None,
                layout: Some(&scene_layout),
                vertex: wgpu::VertexState { module: &shader, entry_point: "vs", compilation_options: Default::default(), buffers: &[vbuf.clone()] },
                primitive: wgpu::PrimitiveState { topology: wgpu::PrimitiveTopology::TriangleList, front_face: wgpu::FrontFace::Cw, cull_mode: cull, ..Default::default() },
                depth_stencil: Some(wgpu::DepthStencilState { format: wgpu::TextureFormat::Depth32Float, depth_write_enabled: true, depth_compare: wgpu::CompareFunction::Less, stencil: Default::default(), bias: Default::default() }),
                multisample: Default::default(),
                fragment: Some(wgpu::FragmentState {
                    module: &shader,
                    entry_point: "fs",
                    compilation_options: Default::default(),
                    targets: &[
                        Some(wgpu::ColorTargetState { format: wgpu::TextureFormat::Rgba8Unorm, blend: None, write_mask: wgpu::ColorWrites::ALL }),
                        Some(wgpu::ColorTargetState { format: wgpu::TextureFormat::R32Uint, blend: None, write_mask: wgpu::ColorWrites::ALL }),
                    ],
                }),
                multiview: None,
                cache: None,
            })
        };
        let scene_pipes = [mk_scene(None), mk_scene(Some(wgpu::Face::Front)), mk_scene(Some(wgpu::Face::Back))];
        let full = |layout: &wgpu::PipelineLayout, fs: &str, fmt: wgpu::TextureFormat| {
            device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                label: None,
                layout: Some(layout),
                vertex: wgpu::VertexState { module: &shader, entry_point: "vs_full", compilation_options: Default::default(), buffers: &[] },
                primitive: Default::default(),
                depth_stencil: None,
                multisample: Default::default(),
                fragment: Some(wgpu::FragmentState { module: &shader, entry_point: fs, compilation_options: Default::default(), targets: &[Some(wgpu::ColorTargetState { format: fmt, blend: None, write_mask: wgpu::ColorWrites::ALL })] }),
                multiview: None,
                cache: None,
            })
        };
        let edge_pipe = full(&pl(&[&edge_layout]), "fs_edge", wgpu::TextureFormat::Rgba8Unorm);
        let blit_pipe = full(&pl(&[&blit_layout]), "fs_blit", out_format);
        let overlay_pipe = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: None,
            layout: Some(&pl(&[&blit_layout])),
            vertex: wgpu::VertexState { module: &shader, entry_point: "vs_full", compilation_options: Default::default(), buffers: &[] },
            primitive: Default::default(),
            depth_stencil: None,
            multisample: Default::default(),
            fragment: Some(wgpu::FragmentState { module: &shader, entry_point: "fs_overlay", compilation_options: Default::default(), targets: &[Some(wgpu::ColorTargetState { format: out_format, blend: Some(wgpu::BlendState::ALPHA_BLENDING), write_mask: wgpu::ColorWrites::ALL })] }),
            multiview: None,
            cache: None,
        });
        let over_pipe = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: None,
            layout: Some(&pl(&[&blit_layout])),
            vertex: wgpu::VertexState { module: &shader, entry_point: "vs_full", compilation_options: Default::default(), buffers: &[] },
            primitive: Default::default(),
            depth_stencil: None,
            multisample: Default::default(),
            fragment: Some(wgpu::FragmentState { module: &shader, entry_point: "fs_blit", compilation_options: Default::default(), targets: &[Some(wgpu::ColorTargetState { format: out_format, blend: Some(wgpu::BlendState::PREMULTIPLIED_ALPHA_BLENDING), write_mask: wgpu::ColorWrites::ALL })] }),
            multiview: None,
            cache: None,
        });
        let fade_tex = device.create_texture(&wgpu::TextureDescriptor { label: None, size: wgpu::Extent3d { width: 1, height: 1, depth_or_array_layers: 1 }, mip_level_count: 1, sample_count: 1, dimension: wgpu::TextureDimension::D2, format: wgpu::TextureFormat::Rgba8Unorm, usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST, view_formats: &[] });
        let fade_view = fade_tex.create_view(&Default::default());
        let globals_buf = device.create_buffer(&wgpu::BufferDescriptor { label: None, size: std::mem::size_of::<Globals>() as u64, usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST, mapped_at_creation: false });
        let globals_bg = device.create_bind_group(&wgpu::BindGroupDescriptor { label: None, layout: &global_layout, entries: &[wgpu::BindGroupEntry { binding: 0, resource: globals_buf.as_entire_binding() }] });
        let linear = device.create_sampler(&wgpu::SamplerDescriptor { mag_filter: wgpu::FilterMode::Linear, min_filter: wgpu::FilterMode::Linear, ..Default::default() });
        let fade_bg = device.create_bind_group(&wgpu::BindGroupDescriptor { label: None, layout: &blit_layout, entries: &[wgpu::BindGroupEntry { binding: 0, resource: wgpu::BindingResource::TextureView(&fade_view) }, wgpu::BindGroupEntry { binding: 1, resource: wgpu::BindingResource::Sampler(&linear) }] });
        let white = upload(&device, &queue, &Arc::new(Image { w: 1, h: 1, buf: vec![255; 4] }), TexMode::Nearest);
        let targets = make_targets(&device, scene_w, scene_h);
        let edge_bg = make_edge_bg(&device, &edge_layout, &targets, &globals_buf);
        Renderer { device, queue, scene_w, scene_h, opts: RenderOpts::default(), globals_buf, globals_bg, mat_layout, scene_pipes, edge_pipe, edge_layout, blit_pipe, blit_layout, linear, white, tex_cache: HashMap::new(), targets, edge_bg, vbuf: None, samplers: HashMap::new(), mat_bgs: HashMap::new(), overlay_pipe, over_pipe, backdrop: None, overlay: None, fade_tex, fade_bg, fade: 0.0 }
    }

    pub fn set_size(&mut self, w: u32, h: u32) {
        if (w, h) != (self.scene_w, self.scene_h) {
            self.scene_w = w;
            self.scene_h = h;
            self.targets = make_targets(&self.device, w, h);
            self.edge_bg = make_edge_bg(&self.device, &self.edge_layout, &self.targets, &self.globals_buf);
        }
    }

    pub fn set_opts(&mut self, o: RenderOpts) {
        if o.tex != self.opts.tex {
            self.tex_cache.clear();
            self.mat_bgs.clear();
            self.samplers.clear();
        }
        self.opts = o;
    }

    fn globals(&self, v: &View, f: &FrameData) -> Globals {
        let r = rot_matrix(v.yaw, v.pitch);
        // model -> rotated: p' = R (p - center)
        let mut rot = [[0.0f32; 4]; 4];
        for i in 0..3 {
            for j in 0..3 {
                rot[j][i] = r[i][j] as f32; // column-major
            }
            let t: f64 = (0..3).map(|k| r[i][k] * v.center[k]).sum();
            rot[3][i] = -t as f32;
        }
        rot[3][3] = 1.0;
        let m = self.scene_w.min(self.scene_h) as f64;
        let sc = 0.9 / v.ext * v.zoom;
        let (mut kx, mut ky) = ((sc * m / self.scene_w as f64) as f32, (sc * m / self.scene_h as f64) as f32);
        let mut toon = [[0.0f32; 4]; 32];
        for (i, c) in f.toon.iter().take(32).enumerate() {
            let [r, g, b] = rgb555_f(*c as u32);
            toon[i] = [r, g, b, 1.0];
        }
        let mut edge = [[0.0f32; 4]; 8];
        for (i, c) in f.edge.iter().take(8).enumerate() {
            let [r, g, b] = rgb555_f(*c as u32);
            edge[i] = [r, g, b, 1.0];
        }
        let mut lights = [[0.0f32; 4]; 3];
        for i in 0..3 {
            let lv = light_vector(v.lights[i].0 as f64, v.lights[i].1 as f64);
            lights[i] = [lv[0], lv[1], lv[2], (LIGHT_COLORS[i] & 31) as f32 / 31.0];
        }
        let (mode, bands) = match self.opts.shading {
            Shading::PerVertex => (0.0, 0.0),
            Shading::Stepped(n) => (1.0, n.max(2) as f32),
            Shading::Smooth => (2.0, 0.0),
        };
        let zs = (0.45 / v.radius) as f32;
        let mut rot = rot;
        let cam_mode = if let Some(c) = v.camera {
            // model -> world: x' = -x, z' = -z (half turn about y); the preset eye keeps x and y, z is mirrored into this frame
            // (NOTE: only verified for presets with x = 0; the sign of x for other presets is not established, see docs/CAMERA_NOTES.md)
            let eye = [c.eye[0], c.eye[1], -c.eye[2]];
            let tgt = [c.target[0], c.target[1], -c.target[2]];
            let f = norm3([tgt[0] - eye[0], tgt[1] - eye[1], tgt[2] - eye[2]]);
            let r = norm3(cross3([0.0, 1.0, 0.0], f));
            let u = cross3(f, r);
            // rows r, u, f applied to (R_y(pi) * p - eye): p' = R_y(pi) p  =>  x' = -x, z' = -z
            let rows = [r, u, f];
            for i in 0..3 {
                // combined linear part: row_i * diag(-1, 1, -1)
                rot[0][i] = (rows[i][0] * -1.0) as f32;
                rot[1][i] = rows[i][1] as f32;
                rot[2][i] = (rows[i][2] * -1.0) as f32;
                rot[3][i] = -(rows[i][0] * eye[0] + rows[i][1] * eye[1] + rows[i][2] * eye[2]) as f32;
            }
            rot[0][3] = 0.0;
            rot[1][3] = 0.0;
            rot[2][3] = 0.0;
            rot[3][3] = 1.0;
            let t = (c.fov_deg.to_radians() / 2.0).tan();
            kx = (1.0 / (t * self.scene_w as f64 / self.scene_h as f64)) as f32;
            ky = (1.0 / t) as f32;
            true
        } else {
            false
        };
        let (persp, eps) = if cam_mode {
            ([0.0, 5.0, 4000.0, 2.0], 1e-6f32)
        } else {
            match self.opts.persp {
            Some(ratio) => {
                let d = ratio.max(1.15) * v.radius;
                ([d as f32, (d - v.radius * 1.05) as f32, (d + v.radius * 1.1) as f32, 1.0], 1e-5f32)
            }
            None => ([0.0, 0.0, 0.0, 0.0], 0.001 * zs),
            }
        };
        Globals { rot, scale: [kx, ky, zs, eps], mode: [mode, bands, self.opts.rim, self.opts.outline], persp, lights, toon, edge, bg: [v.bg[0], v.bg[1], v.bg[2], 1.0] }
    }

    fn make_sampler(&self, tex_param: u32) -> wgpu::Sampler {
        let mode = |repeat: bool, flip: bool| match (repeat, flip) {
            (true, true) => wgpu::AddressMode::MirrorRepeat,
            (true, false) => wgpu::AddressMode::Repeat,
            _ => wgpu::AddressMode::ClampToEdge,
        };
        let filtered = self.opts.tex != TexMode::Nearest;
        self.device.create_sampler(&wgpu::SamplerDescriptor {
            address_mode_u: mode(tex_param & (1 << 16) != 0, tex_param & (1 << 18) != 0),
            address_mode_v: mode(tex_param & (1 << 17) != 0, tex_param & (1 << 19) != 0),
            mag_filter: if filtered { wgpu::FilterMode::Linear } else { wgpu::FilterMode::Nearest },
            min_filter: if filtered { wgpu::FilterMode::Linear } else { wgpu::FilterMode::Nearest },
            mipmap_filter: if filtered { wgpu::FilterMode::Linear } else { wgpu::FilterMode::Nearest },
            anisotropy_clamp: if filtered { 16 } else { 1 },
            ..Default::default()
        })
    }

    /// Render the scene (scene -> edge passes); the result is in `self.targets.edged`.
    pub fn render_scene(&mut self, f: &FrameData, v: &View) {
        let g = self.globals(v, f);
        self.queue.write_buffer(&self.globals_buf, 0, bytemuck::bytes_of(&g));
        // persistent vertex buffer (grown on demand)
        let bytes: &[u8] = bytemuck::cast_slice(&f.verts);
        let need = bytes.len().max(4) as u64;
        if self.vbuf.as_ref().map_or(true, |(_, cap)| *cap < need) {
            let cap = (need * 3 / 2).max(1 << 16);
            self.vbuf = Some((self.device.create_buffer(&wgpu::BufferDescriptor { label: None, size: cap, usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST, mapped_at_creation: false }), cap));
        }
        self.queue.write_buffer(&self.vbuf.as_ref().unwrap().0, 0, bytes);
        // per-material bind groups, cached by (texture, sampler state, material constants)
        for m in &f.mats {
            if let Some(t) = &m.tex {
                let key = Arc::as_ptr(t) as usize;
                if !self.tex_cache.contains_key(&key) {
                    let g = upload(&self.device, &self.queue, t, self.opts.tex);
                    self.tex_cache.insert(key, g);
                }
            }
        }
        let mut keys = Vec::with_capacity(f.batches.len());
        for b in &f.batches {
            let m = &f.mats[b.mat];
            let (tex_id, w, h, has) = match &m.tex {
                Some(t) => {
                    let g = &self.tex_cache[&(Arc::as_ptr(t) as usize)];
                    (Arc::as_ptr(t) as usize, g.w, g.h, 1u32)
                }
                None => (0, 1, 1, 0u32),
            };
            let key = BgKey { tex: tex_id, tex_param: m.tex_param & 0xf_0000, info: [m.enable, m.mode, m.pid, has], da: [m.diffuse.to_bits(), m.ambient.to_bits(), (w as f32).to_bits(), (h as f32).to_bits()] };
            if !self.mat_bgs.contains_key(&key) {
                let mu = MatUniform { info: key.info, da: [m.diffuse, m.ambient, w as f32, h as f32] };
                let ub = self.device.create_buffer_init(&wgpu::util::BufferInitDescriptor { label: None, contents: bytemuck::bytes_of(&mu), usage: wgpu::BufferUsages::UNIFORM });
                let sk = (self.opts.tex != TexMode::Nearest, key.tex_param);
                if !self.samplers.contains_key(&sk) {
                    let smp = self.make_sampler(m.tex_param);
                    self.samplers.insert(sk, smp);
                }
                let view = if has == 1 { &self.tex_cache[&tex_id].view } else { &self.white.view };
                let bg = self.device.create_bind_group(&wgpu::BindGroupDescriptor {
                    label: None,
                    layout: &self.mat_layout,
                    entries: &[
                        wgpu::BindGroupEntry { binding: 0, resource: ub.as_entire_binding() },
                        wgpu::BindGroupEntry { binding: 1, resource: wgpu::BindingResource::TextureView(view) },
                        wgpu::BindGroupEntry { binding: 2, resource: wgpu::BindingResource::Sampler(&self.samplers[&sk]) },
                    ],
                });
                self.mat_bgs.insert(key.clone(), bg);
            }
            keys.push(key);
        }
        let mut enc = self.device.create_command_encoder(&Default::default());
        {
            let mut pass = enc.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: None,
                color_attachments: &[
                    Some(wgpu::RenderPassColorAttachment { view: &self.targets.color, resolve_target: None, ops: wgpu::Operations { load: wgpu::LoadOp::Clear(if self.backdrop.is_some() { wgpu::Color::TRANSPARENT } else { wgpu::Color { r: v.bg[0] as f64, g: v.bg[1] as f64, b: v.bg[2] as f64, a: 1.0 } }), store: wgpu::StoreOp::Store } }),
                    Some(wgpu::RenderPassColorAttachment { view: &self.targets.id, resolve_target: None, ops: wgpu::Operations { load: wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT), store: wgpu::StoreOp::Store } }),
                ],
                depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment { view: &self.targets.depth, depth_ops: Some(wgpu::Operations { load: wgpu::LoadOp::Clear(1.0), store: wgpu::StoreOp::Store }), stencil_ops: None }),
                timestamp_writes: None,
                occlusion_query_set: None,
            });
            pass.set_bind_group(0, &self.globals_bg, &[]);
            pass.set_vertex_buffer(0, self.vbuf.as_ref().unwrap().0.slice(..));
            for (b, key) in f.batches.iter().zip(&keys) {
                let bg = &self.mat_bgs[key];
                let cull = match f.mats[b.mat].cull {
                    1 => 1,
                    2 => 2,
                    _ => 0,
                };
                pass.set_pipeline(&self.scene_pipes[cull]);
                pass.set_bind_group(1, bg, &[]);
                pass.draw(b.first..b.first + b.count, 0..1);
            }
        }
        {
            let mut pass = enc.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: None,
                color_attachments: &[Some(wgpu::RenderPassColorAttachment { view: &self.targets.edged, resolve_target: None, ops: wgpu::Operations { load: wgpu::LoadOp::Clear(wgpu::Color::BLACK), store: wgpu::StoreOp::Store } })],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
            });
            pass.set_pipeline(&self.edge_pipe);
            pass.set_bind_group(0, &self.edge_bg, &[]);
            pass.draw(0..3, 0..1);
        }
        self.queue.submit([enc.finish()]);
    }

    /// Set the 2D overlay (UI): straight-alpha RGBA, stretched over the whole output.  Call only when it changes.
    pub fn set_overlay(&mut self, img: &Image) {
        let (w, h) = (img.w.max(1) as u32, img.h.max(1) as u32);
        let tex = self.device.create_texture(&wgpu::TextureDescriptor {
            label: None,
            size: wgpu::Extent3d { width: w, height: h, depth_or_array_layers: 1 },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rgba8Unorm,
            usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
            view_formats: &[],
        });
        self.queue.write_texture(
            wgpu::ImageCopyTexture { texture: &tex, mip_level: 0, origin: wgpu::Origin3d::ZERO, aspect: wgpu::TextureAspect::All },
            &img.buf,
            wgpu::ImageDataLayout { offset: 0, bytes_per_row: Some(4 * w), rows_per_image: Some(h) },
            wgpu::Extent3d { width: w, height: h, depth_or_array_layers: 1 },
        );
        let view = tex.create_view(&Default::default());
        self.overlay = Some(self.device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: None,
            layout: &self.blit_layout,
            entries: &[wgpu::BindGroupEntry { binding: 0, resource: wgpu::BindingResource::TextureView(&view) }, wgpu::BindGroupEntry { binding: 1, resource: wgpu::BindingResource::Sampler(&self.linear) }],
        }));
    }

    /// Set a backdrop image drawn behind the 3D scene (the scene then renders with a transparent background).
    pub fn set_backdrop(&mut self, img: &Image) {
        let (w, h) = (img.w.max(1) as u32, img.h.max(1) as u32);
        let tex = self.device.create_texture(&wgpu::TextureDescriptor {
            label: None,
            size: wgpu::Extent3d { width: w, height: h, depth_or_array_layers: 1 },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rgba8Unorm,
            usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
            view_formats: &[],
        });
        self.queue.write_texture(
            wgpu::ImageCopyTexture { texture: &tex, mip_level: 0, origin: wgpu::Origin3d::ZERO, aspect: wgpu::TextureAspect::All },
            &img.buf,
            wgpu::ImageDataLayout { offset: 0, bytes_per_row: Some(4 * w), rows_per_image: Some(h) },
            wgpu::Extent3d { width: w, height: h, depth_or_array_layers: 1 },
        );
        let view = tex.create_view(&Default::default());
        self.backdrop = Some(self.device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: None,
            layout: &self.blit_layout,
            entries: &[wgpu::BindGroupEntry { binding: 0, resource: wgpu::BindingResource::TextureView(&view) }, wgpu::BindGroupEntry { binding: 1, resource: wgpu::BindingResource::Sampler(&self.linear) }],
        }));
    }

    pub fn clear_backdrop(&mut self) {
        self.backdrop = None;
    }

    pub fn clear_overlay(&mut self) {
        self.overlay = None;
    }

    /// fade to black: 0 = clear, 1 = black
    pub fn set_fade(&mut self, a: f32) {
        self.fade = a.clamp(0.0, 1.0);
    }

    /// Blit the finished scene to `out` (any size; linear filtering = supersampling when out is smaller).
    pub fn blit(&self, out: &wgpu::TextureView, viewport: Option<(f32, f32, f32, f32)>) {
        self.queue.write_texture(
            wgpu::ImageCopyTexture { texture: &self.fade_tex, mip_level: 0, origin: wgpu::Origin3d::ZERO, aspect: wgpu::TextureAspect::All },
            &[0, 0, 0, (self.fade * 255.0) as u8],
            wgpu::ImageDataLayout { offset: 0, bytes_per_row: Some(4), rows_per_image: Some(1) },
            wgpu::Extent3d { width: 1, height: 1, depth_or_array_layers: 1 },
        );
        let bg = self.device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: None,
            layout: &self.blit_layout,
            entries: &[wgpu::BindGroupEntry { binding: 0, resource: wgpu::BindingResource::TextureView(&self.targets.edged) }, wgpu::BindGroupEntry { binding: 1, resource: wgpu::BindingResource::Sampler(&self.linear) }],
        });
        let mut enc = self.device.create_command_encoder(&Default::default());
        {
            let mut pass = enc.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: None,
                color_attachments: &[Some(wgpu::RenderPassColorAttachment { view: out, resolve_target: None, ops: wgpu::Operations { load: wgpu::LoadOp::Clear(wgpu::Color::BLACK), store: wgpu::StoreOp::Store } })],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
            });
            if let Some((x, y, w, h)) = viewport {
                pass.set_viewport(x, y, w, h, 0.0, 1.0);
            }
            if let Some(b) = &self.backdrop {
                pass.set_pipeline(&self.blit_pipe);
                pass.set_bind_group(0, b, &[]);
                pass.draw(0..3, 0..1);
                pass.set_pipeline(&self.over_pipe);
            } else {
                pass.set_pipeline(&self.blit_pipe);
            }
            pass.set_bind_group(0, &bg, &[]);
            pass.draw(0..3, 0..1);
            if let Some(ov) = &self.overlay {
                pass.set_pipeline(&self.overlay_pipe);
                pass.set_bind_group(0, ov, &[]);
                pass.draw(0..3, 0..1);
            }
            if self.fade > 0.0 {
                pass.set_pipeline(&self.overlay_pipe);
                pass.set_bind_group(0, &self.fade_bg, &[]);
                pass.draw(0..3, 0..1);
            }
        }
        self.queue.submit([enc.finish()]);
    }

    /// Render and read back an `out_w` x `out_h` RGBA image (headless screenshots, tests).
    pub fn render_to_image(&mut self, f: &FrameData, v: &View, out_w: u32, out_h: u32) -> Image {
        self.render_scene(f, v);
        let tex = self.device.create_texture(&wgpu::TextureDescriptor {
            label: None,
            size: wgpu::Extent3d { width: out_w, height: out_h, depth_or_array_layers: 1 },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rgba8Unorm,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
            view_formats: &[],
        });
        self.blit(&tex.create_view(&Default::default()), None);
        let bpr = (out_w * 4 + 255) / 256 * 256;
        let buf = self.device.create_buffer(&wgpu::BufferDescriptor { label: None, size: (bpr * out_h) as u64, usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ, mapped_at_creation: false });
        let mut enc = self.device.create_command_encoder(&Default::default());
        enc.copy_texture_to_buffer(
            wgpu::ImageCopyTexture { texture: &tex, mip_level: 0, origin: wgpu::Origin3d::ZERO, aspect: wgpu::TextureAspect::All },
            wgpu::ImageCopyBuffer { buffer: &buf, layout: wgpu::ImageDataLayout { offset: 0, bytes_per_row: Some(bpr), rows_per_image: Some(out_h) } },
            wgpu::Extent3d { width: out_w, height: out_h, depth_or_array_layers: 1 },
        );
        self.queue.submit([enc.finish()]);
        let slice = buf.slice(..);
        slice.map_async(wgpu::MapMode::Read, |_| {});
        self.device.poll(wgpu::Maintain::Wait);
        let data = slice.get_mapped_range();
        let (w, h) = (out_w as usize, out_h as usize);
        let mut img = Image::new(w, h);
        for y in 0..h {
            img.buf[y * w * 4..(y + 1) * w * 4].copy_from_slice(&data[y * bpr as usize..y * bpr as usize + w * 4]);
        }
        img
    }
}

/// create a headless device (None when no adapter is available, e.g. in CI)
pub fn headless_device() -> Option<(wgpu::Device, wgpu::Queue)> {
    let instance = wgpu::Instance::default();
    let adapter = pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions { power_preference: wgpu::PowerPreference::HighPerformance, compatible_surface: None, force_fallback_adapter: false }))?;
    pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor { label: None, required_features: wgpu::Features::empty(), required_limits: wgpu::Limits::default(), memory_hints: Default::default() }, None)).ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tri(a: [f64; 3], b: [f64; 3], c: [f64; 3]) -> Tri {
        let v = |p: [f64; 3]| TriVert { pos: p, nrm: [0.0, 0.0, 1.0], uv: [0.0, 0.0], col: [1.0, 0.0, 0.0], joints: None };
        Tri { material: 0, verts: [v(a), v(b), v(c)] }
    }

    fn mat(polyattr: u32) -> Material {
        Material { name: "m".into(), diffuse: 0x7fff, ambient: 0, polyattr, alpha: (polyattr >> 16) & 31, cull: (polyattr >> 6) & 3, tex_param: 0, pltt_base: 0, flag: 0, orig_w: 0, orig_h: 0, tex: None, pal: None, image: None }
    }

    #[test]
    fn light_and_rotation_math() {
        let l = light_vector(0.0, 0.0);
        assert_eq!((l[0].abs(), l[1].abs(), l[2]), (0.0, 0.0, -1.0));
        let l = light_vector(90.0, 0.0);
        assert!((l[0] + 1.0).abs() < 1e-6 && l[2].abs() < 1e-6);
        let r = rot_matrix(0.7, -0.3);
        for i in 0..3 {
            for j in 0..3 {
                let dot: f64 = (0..3).map(|k| r[i][k] * r[j][k]).sum();
                assert!((dot - if i == j { 1.0 } else { 0.0 }).abs() < 1e-12);
            }
        }
        assert_eq!(rgb555_f(0x7fff), [1.0, 1.0, 1.0]);
    }

    #[test]
    fn view_fits_the_mesh() {
        let t = [tri([-1.0, 0.0, 0.0], [1.0, 0.0, 0.0], [0.0, 3.0, 0.0])];
        let v = fit_view(&t, 0.0, 0.0, [(0, 0); 3]);
        assert!((v.center[0]).abs() < 1e-12 && (v.center[1] - 1.0).abs() < 1e-12);
        assert!((v.ext - 2.0).abs() < 1e-9); // largest half extent after centring (y: 3 - 1 = 2)
        assert!(v.radius >= v.ext);
    }

    #[test]
    fn frame_grouping_skips_invisible_materials() {
        let mats = vec![mat(31 << 16), mat(0)]; // second has alpha 0 -> not drawn
        let mut t1 = tri([0.0; 3], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0]);
        let mut t2 = t1.clone();
        t1.material = 0;
        t2.material = 1;
        let f = build_frame(&[t1, t2], &mats, &[0; 32], &[0; 8], &TexIndex::new(), &mut ImageCache::default());
        assert_eq!(f.batches.len(), 1);
        assert_eq!(f.batches[0].count, 3);
        assert_eq!(f.batches[0].first, 0);
        assert_eq!(f.mats.len(), 2);
    }

    /// renders a triangle on the GPU (skipped when no adapter is available)
    #[test]
    fn gpu_draws_a_triangle_with_outline() {
        let (device, queue) = match headless_device() {
            Some(d) => d,
            None => {
                eprintln!("no GPU adapter; skipping");
                return;
            }
        };
        let mut r = Renderer::new(device, queue, 64, 64, wgpu::TextureFormat::Rgba8Unorm); // scene size == output size: no supersampling, 1px outline stays white
        // large triangle facing the camera at the default yaw=pi: use yaw 0 and a clockwise winding for the cull test material (cull = none)
        let t = tri([-1.0, -1.0, 0.0], [0.0, 1.0, 0.0], [1.0, -1.0, 0.0]);
        let tris = vec![t];
        let mats = vec![mat(31 << 16)];
        let toon = [0u16; 32];
        let edge = [0x7fffu16; 8]; // white outline
        let f = build_frame(&tris, &mats, &toon, &edge, &TexIndex::new(), &mut ImageCache::default());
        let v = fit_view(&tris, 0.0, 0.0, [(0, 0); 3]);
        let img = r.render_to_image(&f, &v, 64, 64);
        let px = |x: usize, y: usize| &img.buf[(y * 64 + x) * 4..(y * 64 + x) * 4 + 3];
        let bg = [46u8, 46, 58];
        assert!(px(1, 1).iter().zip(bg).all(|(a, b)| (*a as i32 - b as i32).abs() <= 2), "corner is background");
        let c = px(32, 42);
        assert!(c[0] > 200 && c[1] < 60 && c[2] < 60, "triangle interior is red (unlit vertex colour), got {:?}", c);
        let white = (0..64 * 64).filter(|i| img.buf[i * 4] > 230 && img.buf[i * 4 + 1] > 230 && img.buf[i * 4 + 2] > 230).count();
        assert!(white > 20, "outline pixels present ({white})");
    }

    /// every enhanced switch renders a valid, non-empty frame (skipped without a GPU)
    #[test]
    fn gpu_enhanced_modes_render() {
        let (device, queue) = match headless_device() {
            Some(d) => d,
            None => return,
        };
        let mut r = Renderer::new(device, queue, 160, 90, wgpu::TextureFormat::Rgba8Unorm); // non-square scene
        let tris = vec![tri([-1.0, -1.0, 0.0], [0.0, 1.0, 0.0], [1.0, -1.0, 0.0])];
        let mats = vec![mat((31 << 16) | 0b1111 | (2 << 4))]; // lit, toon mode
        let toon: Vec<u16> = (0..32).map(|i| (i as u16) | ((i as u16) << 5) | ((i as u16) << 10)).collect();
        let f = build_frame(&tris, &mats, &toon, &[0x7fff; 8], &TexIndex::new(), &mut ImageCache::default());
        let v = fit_view(&tris, 0.0, 0.0, [(-48, -18), (-180, 122), (-180, 180)]);
        let cover = |img: &Image| (0..img.w * img.h).filter(|i| (0..3).any(|k| (img.buf[i * 4 + k] as i32 - [46, 46, 58][k]).abs() > 12)).count();
        let base = r.render_to_image(&f, &v, 160, 90);
        assert!(cover(&base) > 500);
        for opts in [
            RenderOpts { shading: Shading::Smooth, ..Default::default() },
            RenderOpts { shading: Shading::Stepped(3), tex: TexMode::Filtered, outline: 3.0, rim: 0.3, ..Default::default() },
            RenderOpts { shading: Shading::Smooth, tex: TexMode::Lanczos(2), persp: Some(3.0), ..Default::default() },
        ] {
            r.set_opts(opts.clone());
            let img = r.render_to_image(&f, &v, 160, 90);
            assert!(cover(&img) > 500, "{opts:?} drew nothing");
        }
    }
}
