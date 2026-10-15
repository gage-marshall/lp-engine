//! The 2D compositor above the DS resolution on the GPU, for `present`: the CPU works out each DS pixel's two
//! front-most candidates (`ppu::records`, at the DS resolution) and the GPU finishes every sub-pixel, as
//! `ppu::graphics_hi` does on the CPU: the 3D layer's sample (from `gpu3d`, never read back) inserted by its
//! priority, captured colours replaced by their finer versions (kept on the GPU in a pool of slots), and the
//! colour effects by `ppu::compose`'s rules. The same pass makes the finer captures (the game's capture with
//! source A only: its graphics or its 3D layer) straight into the pool.
use super::gpu3d::Gpu3d;
use super::ppu::Show;
use wgpu::util::DeviceExt;

const W: u32 = 256;
const H: u32 = 192;
/// pool slots: the four capture banks, the in-between picture, the eight text BG planes (5 + engine * 4 + BG)
pub const SLOTS: u32 = 13;
/// the slots' sizes in DS pixels: captures and the in-between picture at most 256 x 192, planes up to 512 x 512
const CAPTURE_PX: u32 = 256 * 256;
const PLANE_PX: u32 = 512 * 512;
/// the first plane slot
const FIRST_PLANE: u32 = 5;
/// a replaced sprite's region in DS pixels (the largest OBJ, 64 x 64)
const SPRITE_PX: u32 = 64 * 64;

/// where slot `slot` starts in the pool (in pixels) at `s` times the DS resolution
fn slot_offset(slot: u32, s: u32) -> u32 {
    let n = if slot < FIRST_PLANE { slot * CAPTURE_PX } else { FIRST_PLANE * CAPTURE_PX + (slot - FIRST_PLANE) * PLANE_PX };
    n * s * s
}

const SHADER: &str = r#"
const W: u32 = 256u;
const H: u32 = 192u;

struct C {
    s: u32, mode: u32, dst: u32, cw: u32,
    ch: u32, show: u32, backdrop: u32, bldcnt: u32,
    bldalpha: u32, bldy: u32, bg0prio: u32, bright: u32,
    use_l3: u32, engine: u32, _p0: u32, _p1: u32,
    slot_cw: array<vec4<u32>, 4>,
    slot_off: array<vec4<u32>, 4>,
};
@group(0) @binding(0) var<uniform> c: C;
@group(0) @binding(1) var<storage, read> rec: array<vec4<u32>>;
@group(0) @binding(2) var<storage, read_write> pool: array<u32>;
@group(0) @binding(3) var l3: texture_2d<u32>;
@group(0) @binding(4) var<storage, read_write> out: array<u32>;
// replaced sprites: per engine and OAM index a 64 x 64 region at the resolution, and each one's width
@group(0) @binding(5) var<storage, read> spool: array<u32>;
@group(0) @binding(6) var<storage, read> spw: array<u32>;

struct P { c: u32, layer: u32, prio: u32, alpha: u32 };

fn slot_cw(i: u32) -> u32 { return c.slot_cw[i / 4u][i % 4u]; }
fn slot_off(i: u32) -> u32 { return c.slot_off[i / 4u][i % 4u]; }

// a candidate, its colour at sub-pixel (sx, sy) from the finer picture when it has one
fn cand(w: u32, src: u32, sx: u32, sy: u32) -> P {
    var p = P(w & 0x7fffu, (w >> 15u) & 7u, (w >> 18u) & 3u, (w >> 20u) & 31u);
    if (src >> 31u) != 0u {
        let slot = (src >> 27u) & 15u;
        var fx = sx;
        var fy = sy;
        if (src & (1u << 26u)) != 0u { fx = c.s - 1u - sx; }
        if (src & (1u << 25u)) != 0u { fy = c.s - 1u - sy; }
        if slot == 15u {
            // a replaced sprite; transparent there: the candidate drops out (layer 7)
            let idx = c.engine * 128u + ((src >> 18u) & 127u);
            let w = spw[idx];
            let v = spool[idx * 4096u * c.s * c.s + ((src >> 9u) & 0x1ffu) * c.s * w * c.s + fy * w * c.s + (src & 0x1ffu) * c.s + fx];
            if (v & 0x8000u) == 0u { p.layer = 7u; }
            p.c = v & 0x7fffu;
            return p;
        }
        let cw = slot_cw(slot);
        let tx = src & 0x1ffu;
        let ty = (src >> 9u) & 0x1ffu;
        p.c = pool[slot_off(slot) + (ty * c.s + fy) * cw * c.s + tx * c.s + fx] & 0x7fffu;
    }
    return p;
}

fn key(p: P) -> u32 {
    var l = p.layer + 1u;
    if p.layer == 4u { l = 0u; }
    return p.prio * 8u + l;
}

fn blend(a: u32, b: u32, ea_in: u32, eb_in: u32) -> u32 {
    let ea = min(ea_in, 16u);
    let eb = min(eb_in, 16u);
    var r = 0u;
    for (var s = 0u; s < 15u; s += 5u) {
        r |= min((((a >> s) & 31u) * ea + ((b >> s) & 31u) * eb) / 16u, 31u) << s;
    }
    return r;
}

fn fade(a: u32, y: u32, up: bool) -> u32 {
    var r = 0u;
    for (var s = 0u; s < 15u; s += 5u) {
        let v = (a >> s) & 31u;
        if up { r |= (v + (31u - v) * y / 16u) << s; } else { r |= (v - v * y / 16u) << s; }
    }
    return r;
}

fn first_target(p: P) -> bool { return (c.bldcnt & (1u << p.layer)) != 0u; }
fn second_target(p: P) -> bool { return (c.bldcnt & (0x100u << p.layer)) != 0u; }

// ppu::compose on the two front-most candidates
fn compose(top: P, second: P, effects: bool) -> u32 {
    var r = top.c;
    if top.layer == 4u && top.alpha != 0u && second_target(second) {
        var ea = c.bldalpha & 31u;
        var eb = (c.bldalpha >> 8u) & 31u;
        if top.alpha > 1u {
            ea = top.alpha - 1u;
            eb = 16u - (top.alpha - 1u);
        }
        r = blend(top.c, second.c, ea, eb);
    } else if effects && first_target(top) {
        switch (c.bldcnt >> 6u) & 3u {
            case 1u: { if second_target(second) { r = blend(top.c, second.c, c.bldalpha & 31u, (c.bldalpha >> 8u) & 31u); } }
            case 2u: { r = fade(top.c, c.bldy, true); }
            case 3u: { r = fade(top.c, c.bldy, false); }
            default: {}
        }
    }
    return r & 0x7fffu;
}

@compute @workgroup_size(8, 8)
fn main(@builtin(global_invocation_id) id: vec3<u32>) {
    let x = id.x;
    let y = id.y;
    let ow = W * c.s;
    if x >= ow || y >= H * c.s { return; }
    if c.mode != 0u && (x >= c.cw * c.s || y >= c.ch * c.s) { return; }
    var l3v = 0u;
    if c.use_l3 != 0u { l3v = textureLoad(l3, vec2<i32>(i32(x), i32(y)), 0).r; }
    if c.mode == 2u {
        // capture of the 3D layer alone
        pool[slot_off(c.dst) + y * c.cw * c.s + x] = l3v & 0xffffu;
        return;
    }
    var col = 0x7fffu;
    if c.show == 1u {
        let r = rec[(y / c.s) * W + x / c.s];
        let sx = x % c.s;
        let sy = y % c.s;
        let bd = P(c.backdrop, 5u, 4u, 0u);
        var c0 = bd;
        var c1 = bd;
        if (r.x >> 31u) != 0u { c0 = cand(r.x, r.z, sx, sy); }
        if (r.y >> 31u) != 0u { c1 = cand(r.y, r.w, sx, sy); }
        if c0.layer == 7u { c0 = c1; c1 = bd; }
        if c1.layer == 7u { c1 = bd; }
        let effects = (r.x & (1u << 25u)) != 0u;
        let behind = compose(c0, c1, effects);
        col = behind;
        if (r.x & (1u << 26u)) != 0u && (l3v & 0x8000u) != 0u {
            // the 3D layer by its priority (after equal keys, as it is added last)
            let p3 = P(l3v & 0x7fffu, 0u, c.bg0prio, 0u);
            var top = c0;
            var second = c1;
            if key(p3) < key(c0) {
                top = p3;
                second = c0;
            } else if key(p3) < key(c1) {
                second = p3;
            }
            col = compose(top, second, effects);
            let cov = (l3v >> 16u) & 31u;
            if cov > 0u && cov < 16u { col = blend(col, behind, cov, 16u - cov); }
        }
    }
    if c.mode == 1u {
        pool[slot_off(c.dst) + y * c.cw * c.s + x] = col | 0x8000u;
        return;
    }
    // displayed: RGBA8 with the master brightness (ppu::put, ppu::brightness)
    var rgba = 0xffffffffu;
    if c.show == 1u {
        let mode = c.bright >> 14u;
        let f = min(c.bright & 31u, 16u);
        rgba = 0xff000000u;
        for (var k = 0u; k < 3u; k++) {
            var v = ((col >> (5u * k)) & 31u) * 255u / 31u;
            if f != 0u {
                if mode == 1u { v = v + (255u - v) * f / 16u; } else if mode == 2u { v = v - v * f / 16u; }
            }
            rgba |= v << (8u * k);
        }
    }
    out[y * ow + x] = rgba;
}
"#;

/// the compositor's buffers for one resolution
pub struct Compositor {
    s: u32,
    pipeline: wgpu::ComputePipeline,
    layout: wgpu::BindGroupLayout,
    pool: wgpu::Buffer,
    rec: [wgpu::Buffer; 2],
    out: [wgpu::Buffer; 2],
    spool: wgpu::Buffer,
    spw: wgpu::Buffer,
    read: [wgpu::Buffer; 2],
    dummy: wgpu::Texture,
}

/// an engine as `ppu::records` describes it
pub type Records = (Show, Vec<[u32; 4]>, [u32; 6]);

impl Compositor {
    fn new(d: &wgpu::Device, s: u32) -> Compositor {
        let module = d.create_shader_module(wgpu::ShaderModuleDescriptor { label: Some("gpu2d"), source: wgpu::ShaderSource::Wgsl(SHADER.into()) });
        let e = |binding: u32, ty: wgpu::BindingType| wgpu::BindGroupLayoutEntry { binding, visibility: wgpu::ShaderStages::COMPUTE, ty, count: None };
        let st = |read_only: bool| wgpu::BindingType::Buffer { ty: wgpu::BufferBindingType::Storage { read_only }, has_dynamic_offset: false, min_binding_size: None };
        let layout = d.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: None,
            entries: &[
                e(0, wgpu::BindingType::Buffer { ty: wgpu::BufferBindingType::Uniform, has_dynamic_offset: false, min_binding_size: None }),
                e(1, st(true)),
                e(2, st(false)),
                e(3, wgpu::BindingType::Texture { sample_type: wgpu::TextureSampleType::Uint, view_dimension: wgpu::TextureViewDimension::D2, multisampled: false }),
                e(4, st(false)),
                e(5, st(true)),
                e(6, st(true)),
            ],
        });
        let pl = d.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor { label: None, bind_group_layouts: &[&layout], push_constant_ranges: &[] });
        let pipeline = d.create_compute_pipeline(&wgpu::ComputePipelineDescriptor { label: Some("gpu2d"), layout: Some(&pl), module: &module, entry_point: "main", compilation_options: Default::default(), cache: None });
        let px = (W * H * s * s) as u64;
        let buf = |size: u64, usage: wgpu::BufferUsages| d.create_buffer(&wgpu::BufferDescriptor { label: None, size, usage, mapped_at_creation: false });
        let pool = buf(slot_offset(SLOTS, s) as u64 * 4, wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_DST);
        let rec = [0, 1].map(|_| buf((W * H * 16) as u64, wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_DST));
        let out = [0, 1].map(|_| buf(px * 4, wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_SRC));
        let spool = buf((2 * 128 * SPRITE_PX * s * s) as u64 * 4, wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_DST);
        let spw = buf(256 * 4, wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_DST);
        let read = [0, 1].map(|_| buf(px * 4, wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST));
        let dummy = d.create_texture(&wgpu::TextureDescriptor { label: None, size: wgpu::Extent3d { width: 1, height: 1, depth_or_array_layers: 1 }, mip_level_count: 1, sample_count: 1, dimension: wgpu::TextureDimension::D2, format: wgpu::TextureFormat::R32Uint, usage: wgpu::TextureUsages::TEXTURE_BINDING, view_formats: &[] });
        Compositor { s, pipeline, layout, pool, rec, out, spool, spw, read, dummy }
    }
}

/// what one pass of the compositor does
struct Job<'a> {
    rec: &'a Records,
    /// 0 displayed (into `out[engine]`), 1 a capture of the graphics into pool slot `dst`, 2 a capture of the 3D layer
    mode: u32,
    dst: u32,
    cw: u32,
    ch: u32,
    /// the 3D layer: `gpu3d`'s output slot
    l3: Option<usize>,
    engine: usize,
}

impl Gpu3d {
    fn compositor(&mut self, s: u32) {
        if self.c2d.as_ref().map_or(true, |c| c.s != s) {
            self.c2d = Some(Compositor::new(&self.device, s));
        }
    }

    fn encode(&self, enc: &mut wgpu::CommandEncoder, j: &Job, slot_cw: &[u32; SLOTS as usize]) {
        let c = self.c2d.as_ref().unwrap();
        let s = c.s;
        let (show, recs, regs) = j.rec;
        if *show == Show::Layers && j.mode != 2 {
            let bytes: Vec<u8> = recs.iter().flat_map(|r| r.iter().flat_map(|w| w.to_le_bytes())).collect();
            self.queue.write_buffer(&c.rec[j.engine], 0, &bytes);
        }
        // the 3D layer when it is drawn at this resolution
        let l3 = j.l3.and_then(|slot| self.targets.as_ref().filter(|t| t.w == W * s).map(|t| &t.outs[slot]));
        let mut u = [s, j.mode, j.dst, j.cw, j.ch, (*show == Show::Layers) as u32, regs[0], regs[1], regs[2], regs[3], regs[4], regs[5], l3.is_some() as u32, j.engine as u32, 0, 0].to_vec();
        u.extend(slot_cw);
        u.resize(32, 0);
        u.extend((0..16).map(|i| slot_offset(i, s)));
        let ub = self.device.create_buffer_init(&wgpu::util::BufferInitDescriptor { label: None, contents: &u.iter().flat_map(|w| w.to_le_bytes()).collect::<Vec<u8>>(), usage: wgpu::BufferUsages::UNIFORM });
        let view = l3.unwrap_or(&c.dummy).create_view(&Default::default());
        let bind = self.device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: None,
            layout: &c.layout,
            entries: &[
                wgpu::BindGroupEntry { binding: 0, resource: ub.as_entire_binding() },
                wgpu::BindGroupEntry { binding: 1, resource: c.rec[j.engine].as_entire_binding() },
                wgpu::BindGroupEntry { binding: 2, resource: c.pool.as_entire_binding() },
                wgpu::BindGroupEntry { binding: 3, resource: wgpu::BindingResource::TextureView(&view) },
                wgpu::BindGroupEntry { binding: 4, resource: c.out[j.engine].as_entire_binding() },
                wgpu::BindGroupEntry { binding: 5, resource: c.spool.as_entire_binding() },
                wgpu::BindGroupEntry { binding: 6, resource: c.spw.as_entire_binding() },
            ],
        });
        let mut pass = enc.begin_compute_pass(&Default::default());
        pass.set_pipeline(&c.pipeline);
        pass.set_bind_group(0, &bind, &[]);
        pass.dispatch_workgroups(W * s / 8, H * s / 8, 1);
    }

    /// the finer version of a capture with control word `cnt` into pool slot `dst`, from engine A as `rec`
    /// describes it and the 3D layer in `gpu3d`'s output slot `l3`; false when the capture's kind is not done here
    /// (source B or blending)
    pub fn capture_hi(&mut self, s: usize, rec: &Records, cnt: u32, l3: Option<usize>, dst: u32, slot_cw: &[u32; SLOTS as usize]) -> bool {
        if (cnt >> 29) & 3 != 0 {
            return false;
        }
        self.compositor(s as u32);
        let (cw, ch) = [(128, 128), (256, 64), (256, 128), (256, 192)][((cnt >> 20) & 3) as usize];
        let mode = if cnt & (1 << 24) != 0 { 2 } else { 1 };
        let mut enc = self.device.create_command_encoder(&Default::default());
        self.encode(&mut enc, &Job { rec, mode, dst, cw, ch, l3, engine: 0 }, slot_cw);
        self.queue.submit([enc.finish()]);
        true
    }

    /// both engines' pictures (A, B) at `s` times the DS resolution, RGBA8; engine A's 3D layer from `gpu3d`'s
    /// output slot `l3`
    /// a picture into pool slot `slot` (RGB555 | bit 15 per pixel; a plane's slot holds up to 512 x 512 at `s`)
    pub fn put_slot(&mut self, s: usize, slot: u32, px: &[u32]) {
        self.compositor(s as u32);
        let c = self.c2d.as_ref().unwrap();
        let bytes: Vec<u8> = px.iter().flat_map(|v| v.to_le_bytes()).collect();
        self.queue.write_buffer(&c.pool, slot_offset(slot, c.s) as u64 * 4, &bytes);
    }

    /// a replaced sprite for engine `engine`'s OAM index `i`: its picture at the resolution (RGB555 | bit 15 where
    /// drawn, `w * s` wide), `w` its width in DS pixels
    pub fn put_sprite(&mut self, s: usize, engine: usize, i: usize, w: usize, px: &[u32]) {
        self.compositor(s as u32);
        let c = self.c2d.as_ref().unwrap();
        let idx = engine * 128 + i;
        let bytes: Vec<u8> = px.iter().flat_map(|v| v.to_le_bytes()).collect();
        self.queue.write_buffer(&c.spool, (idx * SPRITE_PX as usize * s * s) as u64 * 4, &bytes);
        self.queue.write_buffer(&c.spw, idx as u64 * 4, &(w as u32).to_le_bytes());
    }

    pub fn screens_hi(&mut self, s: usize, a: &Records, b: &Records, l3: Option<usize>, slot_cw: &[u32; SLOTS as usize]) -> (Vec<u8>, Vec<u8>) {
        self.compositor(s as u32);
        let mut enc = self.device.create_command_encoder(&Default::default());
        self.encode(&mut enc, &Job { rec: a, mode: 0, dst: 0, cw: 0, ch: 0, l3, engine: 0 }, slot_cw);
        self.encode(&mut enc, &Job { rec: b, mode: 0, dst: 0, cw: 0, ch: 0, l3: None, engine: 1 }, slot_cw);
        let c = self.c2d.as_ref().unwrap();
        let size = (W * H * c.s * c.s * 4) as u64;
        for e in 0..2 {
            enc.copy_buffer_to_buffer(&c.out[e], 0, &c.read[e], 0, size);
        }
        self.queue.submit([enc.finish()]);
        for e in 0..2 {
            c.read[e].slice(..).map_async(wgpu::MapMode::Read, |_| {});
        }
        self.device.poll(wgpu::Maintain::Wait);
        let take = |e: usize| {
            let v = c.read[e].slice(..).get_mapped_range().to_vec();
            c.read[e].unmap();
            v
        };
        (take(0), take(1))
    }
}
