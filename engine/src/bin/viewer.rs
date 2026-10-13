//! Heroine viewer: loads the game's own model/motion files from a content tree and renders them with the DS toon pipeline.
//!
//!   viewer CONTENT_DIR [--heroine m|r|n] [--outfit N] [--hair N] [--motion NAME|INDEX] [--frame N] [--mood N] [--state 0-2]
//!          [--yaw DEG] [--pitch DEG] [--zoom X] [--size PX] [--ss N] [--png OUT.png] [--list-motions]
//!
//! With --png it renders one frame headlessly and exits.  Otherwise a window opens:
//!   drag = orbit, wheel = zoom, space = pause, left/right = step, [ ] = previous/next motion, 1/2/3 = heroine,
//!   O/H = next outfit/hair, M = next mood, T = time of day, B = blink, Esc = quit.
use lp_engine::camera;
use lp_engine::heroine::*;
use lp_engine::render::*;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use winit::application::ApplicationHandler;
use winit::event::{ElementState, MouseScrollDelta, WindowEvent};
use winit::event_loop::{ActiveEventLoop, ControlFlow, EventLoop};
use winit::keyboard::{Key, NamedKey};
use winit::window::{Window, WindowId};

#[derive(Clone)]
struct Args {
    content: PathBuf,
    heroine: String,
    opts: ComposeOpts,
    motion_arg: Option<String>,
    yaw: f64,
    pitch: f64,
    zoom: f64,
    size: u32,
    width: u32,
    height: u32,
    ss: u32,
    ropts: RenderOpts,
    fit_height: Option<f64>,
    focus_y: Option<f64>,
    /// outline thickness in OUTPUT pixels (converted to scene pixels with --ss)
    outline_out: Option<f32>,
    /// geometry smoothing: subdivisions per triangle edge (1 = off), PN strength, crease angle
    smooth: u32,
    smooth_strength: f64,
    crease: f64,
    png: Option<PathBuf>,
    list: bool,
    max_frames: Option<u32>,
    bench: Option<u32>,
    bench_window: Option<u32>,
    vsync: bool,
    fullscreen: bool,
}

fn parse_args() -> Args {
    let a: Vec<String> = std::env::args().collect();
    let mut args = Args { content: PathBuf::from("content"), heroine: "m".into(), opts: ComposeOpts::default(), motion_arg: None, yaw: std::f64::consts::PI, pitch: 0.0, zoom: 1.0, size: 768, width: 0, height: 0, ss: 2, ropts: RenderOpts::default(), fit_height: None, focus_y: None, outline_out: None, smooth: 1, smooth_strength: 1.0, crease: 50.0, png: None, list: false, max_frames: None, bench: None, bench_window: None, vsync: false, fullscreen: false };
    let mut i = 1;
    let mut positional = false;
    while i < a.len() {
        let next = |i: &mut usize| -> String {
            *i += 1;
            a.get(*i).cloned().unwrap_or_default()
        };
        match a[i].as_str() {
            "--heroine" => args.heroine = next(&mut i),
            "--outfit" => args.opts.outfit = next(&mut i).parse().unwrap_or(17),
            "--hair" => args.opts.hair = next(&mut i).parse().unwrap_or(0),
            "--motion" => args.motion_arg = Some(next(&mut i)),
            "--frame" => args.opts.frame = next(&mut i).parse().unwrap_or(0),
            "--mood" => args.opts.mood = next(&mut i).parse().unwrap_or(0),
            "--state" => args.opts.state = next(&mut i).parse().unwrap_or(0),
            "--yaw" => args.yaw = next(&mut i).parse::<f64>().unwrap_or(180.0).to_radians(),
            "--pitch" => args.pitch = next(&mut i).parse::<f64>().unwrap_or(0.0).to_radians(),
            "--zoom" => args.zoom = next(&mut i).parse().unwrap_or(1.0),
            "--width" => args.width = next(&mut i).parse().unwrap_or(0),
            "--height" => args.height = next(&mut i).parse().unwrap_or(0),
            "--fit-height" => args.fit_height = next(&mut i).parse().ok(),
            "--focus-y" => args.focus_y = next(&mut i).parse().ok(),
            "--look" => {
                let n = next(&mut i);
                args.ropts = look(&n);
                args.smooth = if n.ends_with("-s") { 4 } else { 1 };
            }
            "--smooth" => args.smooth = next(&mut i).parse().unwrap_or(1),
            "--smooth-strength" => args.smooth_strength = next(&mut i).parse().unwrap_or(1.0),
            "--crease" => args.crease = next(&mut i).parse().unwrap_or(50.0),
            "--shading" => {
                args.ropts.shading = match next(&mut i).as_str() {
                    "smooth" => Shading::Smooth,
                    "vertex" => Shading::PerVertex,
                    s => Shading::Stepped(s.trim_start_matches("steps").parse().unwrap_or(4)),
                }
            }
            "--tex" => {
                args.ropts.tex = match next(&mut i).as_str() {
                    "nearest" => TexMode::Nearest,
                    "filtered" => TexMode::Filtered,
                    s => TexMode::Lanczos(s.trim_start_matches("lanczos").parse().unwrap_or(4)),
                }
            }
            "--outline" => args.outline_out = next(&mut i).parse().ok(),
            "--rim" => args.ropts.rim = next(&mut i).parse().unwrap_or(0.0),
            "--persp" => args.ropts.persp = next(&mut i).parse().ok(),
            "--size" => args.size = next(&mut i).parse().unwrap_or(768),
            "--ss" => args.ss = next(&mut i).parse().unwrap_or(2),
            "--png" => args.png = Some(PathBuf::from(next(&mut i))),
            "--list-motions" => args.list = true,
            "--bench-window" => args.bench_window = next(&mut i).parse().ok(),
            "--vsync" => args.vsync = true,
            "--fullscreen" => args.fullscreen = true,
            "--bench" => args.bench = next(&mut i).parse().ok(),
            "--max-frames" => args.max_frames = next(&mut i).parse().ok(),
            s if !s.starts_with("--") && !positional => {
                args.content = PathBuf::from(s);
                positional = true;
            }
            s => eprintln!("ignoring argument {s}"),
        }
        i += 1;
    }
    args
}

/// named render looks (authentic = the DS pipeline; the others are enhanced-mode experiments)
fn look(name: &str) -> RenderOpts {
    let base = RenderOpts::default();
    match name {
        "authentic" => base,
        // sharper textures and smooth toon, thin consistent outline
        "clean" | "clean-s" => RenderOpts { shading: Shading::Smooth, tex: TexMode::Lanczos(4), outline: 2.2, ..base },
        // flat cel bands, heavier outline, a hint of rim light
        "cel" | "cel-s" => RenderOpts { shading: Shading::Stepped(3), tex: TexMode::Lanczos(4), outline: 3.0, rim: 0.25, ..base },
        // smooth toon, perspective camera, subtle rim light
        "soft" | "soft-s" => RenderOpts { shading: Shading::Smooth, tex: TexMode::Lanczos(4), outline: 1.6, rim: 0.18, persp: Some(3.0), ..base },
        // wide-angle like the game's own camera
        "wide" => RenderOpts { shading: Shading::Smooth, tex: TexMode::Lanczos(4), outline: 1.8, rim: 0.12, persp: Some(1.4), ..base },
        // the DS shading at high resolution with filtered textures only
        "filtered" => RenderOpts { tex: TexMode::Filtered, ..base },
        other => {
            eprintln!("unknown look {other}, using authentic");
            base
        }
    }
}

fn write_png(path: &Path, im: &lp_engine::nns2d::Image) {
    let f = std::fs::File::create(path).unwrap();
    let mut enc = png::Encoder::new(std::io::BufWriter::new(f), im.w as u32, im.h as u32);
    enc.set_color(png::ColorType::Rgba);
    enc.set_depth(png::BitDepth::Eight);
    enc.write_header().unwrap().write_image_data(&im.buf).unwrap();
}

/// everything needed to turn options into frames
struct Scene {
    arc: PathBuf,
    wardrobes: HashMap<String, Wardrobe>,
    tables: FaceTables,
    presets: std::collections::BTreeMap<i32, camera::Preset>,
    cache: ImageCache,
    prefix: String,
    opts: ComposeOpts,
    view: Option<View>,
    fit_height: Option<f64>,
    focus_y: Option<f64>,
    smooth: (u32, f64, f64),
}

impl Scene {
    fn new(args: &Args) -> Scene {
        let arc = args.content.join("assets/arc");
        let tables = FaceTables::from_json(&serde_json::from_str(&std::fs::read_to_string(args.content.join("engine/face_tables.json")).expect("content/engine/face_tables.json")).unwrap()).unwrap();
        let mut s = Scene { presets: camera::load_presets(&arc), arc, wardrobes: HashMap::new(), tables, cache: ImageCache::default(), prefix: args.heroine.clone(), opts: args.opts.clone(), view: None, fit_height: args.fit_height, focus_y: args.focus_y, smooth: (args.smooth, args.smooth_strength, args.crease) };
        s.ensure();
        s
    }
    fn ensure(&mut self) {
        if !self.wardrobes.contains_key(&self.prefix) {
            let w = Wardrobe::load(&self.arc, &self.prefix);
            self.wardrobes.insert(self.prefix.clone(), w);
        }
    }
    fn wardrobe(&self) -> &Wardrobe {
        &self.wardrobes[&self.prefix]
    }
    fn motion_frames(&self) -> usize {
        match &self.opts.motion {
            Some(m) => self.wardrobe().motions.get(m).and_then(|b| b.bca.iter().map(|a| a.frames).max()).unwrap_or(1).max(1),
            None => 1,
        }
    }
    fn lights(&self) -> [(i32, i32); 3] {
        self.presets.get(&1000).map(|p| p.lights).unwrap_or([(-48, -18), (-180, 122), (-180, 180)])
    }
    fn frame(&mut self, yaw: f64, pitch: f64, zoom: f64) -> (FrameData, View) {
        self.ensure();
        let w = &self.wardrobes[&self.prefix];
        let c = w.compose(&self.opts, Some(&self.tables)).expect("compose");
        if self.view.is_none() {
            // fit once from the bind pose so the camera does not jitter during animation
            let bind = w.compose(&ComposeOpts { motion: None, frame: 0, ..self.opts.clone() }, None).expect("compose");
            let mut v = fit_view(&bind.tris, yaw, pitch, self.lights());
            if let Some(m) = self.fit_height {
                v.fit_height(m);
            }
            if let Some(y) = self.focus_y {
                v.center[1] = y;
            }
            self.view = Some(v);
        }
        let mut v = self.view.clone().unwrap();
        v.yaw = yaw;
        v.pitch = pitch;
        v.zoom = zoom;
        let sm = if self.smooth.0 > 1 { Some(self.smooth) } else { None };
        let f = build_frame_smooth(&c.tris, &c.mats, &c.toon, &c.edge, &w.tex, &mut self.cache, sm);
        (f, v)
    }
}

/// animation hot loop: per-stage timings (CPU pose/mesh, tessellation, upload/encode, GPU) over `n` frames
fn bench(args: &Args, scene: &mut Scene, n: u32) {
    use std::time::Instant;
    let (device, queue) = headless_device().expect("no GPU adapter");
    let (w, h) = if args.width > 0 { (args.width, if args.height > 0 { args.height } else { args.width }) } else { (args.size, args.size) };
    let mut r = Renderer::new(device, queue, w * args.ss, h * args.ss, wgpu::TextureFormat::Rgba8Unorm);
    r.set_opts(scene_opts(args));
    let out = r.device.create_texture(&wgpu::TextureDescriptor { label: None, size: wgpu::Extent3d { width: w, height: h, depth_or_array_layers: 1 }, mip_level_count: 1, sample_count: 1, dimension: wgpu::TextureDimension::D2, format: wgpu::TextureFormat::Rgba8Unorm, usage: wgpu::TextureUsages::RENDER_ATTACHMENT, view_formats: &[] });
    let out_view = out.create_view(&Default::default());
    let nf = scene.motion_frames();
    let (mut t_pose, mut t_tess, mut t_build, mut t_encode, mut t_gpu, mut tris) = (0.0, 0.0, 0.0, 0.0, 0.0, 0usize);
    let mut all = Vec::new();
    for i in 0..(n + 10) {
        scene.opts.frame = i as usize % nf;
        let t0 = Instant::now();
        let wd = &scene.wardrobes[&scene.prefix];
        let c = wd.compose(&scene.opts, Some(&scene.tables)).expect("compose");
        let t1 = Instant::now();
        let t2 = Instant::now();
        let sm = if scene.smooth.0 > 1 { Some(scene.smooth) } else { None };
        let f = build_frame_smooth(&c.tris, &c.mats, &c.toon, &c.edge, &wd.tex, &mut scene.cache, sm);
        let t3 = Instant::now();
        let v = scene.view.clone().unwrap_or_else(|| {
            let bind = wd.compose(&ComposeOpts { motion: None, frame: 0, ..scene.opts.clone() }, None).unwrap();
            let mut v = fit_view(&bind.tris, args.yaw, args.pitch, scene.lights());
            v.zoom = args.zoom;
            if let Some(m) = args.fit_height { v.fit_height(m); }
            if let Some(y) = args.focus_y { v.center[1] = y; }
            v
        });
        let mut v = v;
        v.zoom = args.zoom;
        scene.view = Some(v.clone());
        r.render_scene(&f, &v);
        r.blit(&out_view, None);
        let t4 = Instant::now();
        r.device.poll(wgpu::Maintain::Wait);
        let t5 = Instant::now();
        if i >= 10 {
            t_pose += (t1 - t0).as_secs_f64();
            t_tess += (t2 - t1).as_secs_f64();
            t_build += (t3 - t2).as_secs_f64();
            t_encode += (t4 - t3).as_secs_f64();
            t_gpu += (t5 - t4).as_secs_f64();
            all.push((t5 - t0).as_secs_f64());
            tris = f.verts.len();
        }
    }
    all.sort_by(|a, b| a.partial_cmp(b).unwrap());
    let nn = n as f64;
    let ms = |x: f64| x / nn * 1000.0;
    let cpu = ms(t_pose + t_tess + t_build + t_encode);
    let gpu = ms(t_gpu);
    println!(
        "{}x{} ss{} tris={} | pose {:.2} (tess+build fused below) {:.2} build+tess {:.2} encode {:.2} | gpu-wait {:.2} | serial {:.2} ms ({:.0} fps), p99 {:.2} ms | pipelined bound {:.2} ms ({:.0} fps)",
        w, h, args.ss, tris, ms(t_pose), ms(t_tess), ms(t_build), ms(t_encode), gpu, cpu + gpu, 1000.0 / (cpu + gpu), all[(all.len() as f64 * 0.99) as usize - 1] * 1000.0, cpu.max(gpu), 1000.0 / cpu.max(gpu)
    );
}

fn scene_opts(args: &Args) -> RenderOpts {
    let mut o = args.ropts.clone();
    if let Some(px) = args.outline_out {
        o.outline = px * args.ss as f32;
    }
    o
}

fn resolve_motion(scene: &mut Scene, arg: &Option<String>) {
    if let Some(a) = arg {
        let w = scene.wardrobe();
        let names: Vec<String> = w.motions.keys().cloned().collect();
        let m = match a.parse::<usize>() {
            Ok(i) => names.get(i).cloned(),
            Err(_) => names.iter().find(|n| *n == a).cloned(),
        };
        scene.opts.motion = m;
    }
}

fn main() {
    let args = parse_args();
    let mut scene = Scene::new(&args);
    resolve_motion(&mut scene, &args.motion_arg);
    if args.list {
        for (i, n) in scene.wardrobe().motions.keys().enumerate() {
            println!("{i} {n}");
        }
        return;
    }
    if let Some(n) = args.bench {
        bench(&args, &mut scene, n);
        return;
    }
    if let Some(png) = &args.png {
        let (device, queue) = headless_device().expect("no GPU adapter");
        let (w, h) = if args.width > 0 { (args.width, if args.height > 0 { args.height } else { args.width }) } else { (args.size, args.size) };
        let mut r = Renderer::new(device, queue, w * args.ss, h * args.ss, wgpu::TextureFormat::Rgba8Unorm);
        r.set_opts(scene_opts(&args));
        let (f, v) = scene.frame(args.yaw, args.pitch, args.zoom);
        let img = r.render_to_image(&f, &v, w, h);
        write_png(png, &img);
        println!("wrote {} ({} triangles, {} batches)", png.display(), f.verts.len(), f.batches.len());
        return;
    }
    let event_loop = EventLoop::new().unwrap();
    event_loop.set_control_flow(ControlFlow::Poll);
    let mut app = App { args, scene, gfx: None, yaw: std::f64::consts::PI, pitch: 0.0, zoom: 1.0, drag: None, playing: true, frame_f: 0.0, last: std::time::Instant::now(), blink: false, shown: 0, frame_times: Vec::new(), last_present: None };
    app.yaw = app.args.yaw;
    app.pitch = app.args.pitch;
    app.zoom = app.args.zoom;
    event_loop.run_app(&mut app).unwrap();
}

struct Gfx {
    window: Arc<Window>,
    surface: wgpu::Surface<'static>,
    config: wgpu::SurfaceConfiguration,
    renderer: Renderer,
}

struct App {
    args: Args,
    scene: Scene,
    gfx: Option<Gfx>,
    yaw: f64,
    pitch: f64,
    zoom: f64,
    drag: Option<(f64, f64)>,
    playing: bool,
    frame_f: f64,
    last: std::time::Instant,
    blink: bool,
    shown: u32,
    frame_times: Vec<f64>,
    last_present: Option<std::time::Instant>,
}

impl ApplicationHandler for App {
    fn resumed(&mut self, el: &ActiveEventLoop) {
        if self.gfx.is_some() {
            return;
        }
        let (ww, wh) = if self.args.width > 0 { (self.args.width, if self.args.height > 0 { self.args.height } else { self.args.width }) } else { (self.args.size, self.args.size) };
        let mut attrs = Window::default_attributes().with_title("LovePlus+ viewer");
        attrs = if self.args.bench_window.is_some() { attrs.with_inner_size(winit::dpi::PhysicalSize::new(ww, wh)) } else { attrs.with_inner_size(winit::dpi::LogicalSize::new(ww, wh)) };
        if self.args.fullscreen {
            attrs = attrs.with_fullscreen(Some(winit::window::Fullscreen::Borderless(None)));
        }
        let window = Arc::new(el.create_window(attrs).unwrap());
        let instance = wgpu::Instance::default();
        let surface = instance.create_surface(window.clone()).unwrap();
        let adapter = pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions { power_preference: wgpu::PowerPreference::HighPerformance, compatible_surface: Some(&surface), force_fallback_adapter: false })).expect("no adapter");
        let (device, queue) = pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor { label: None, required_features: wgpu::Features::empty(), required_limits: wgpu::Limits::default(), memory_hints: Default::default() }, None)).unwrap();
        let caps = surface.get_capabilities(&adapter);
        let format = caps.formats.iter().copied().find(|f| !f.is_srgb()).unwrap_or(caps.formats[0]);
        let size = window.inner_size();
        let config = wgpu::SurfaceConfiguration { usage: wgpu::TextureUsages::RENDER_ATTACHMENT, format, width: size.width.max(1), height: size.height.max(1), present_mode: if self.args.vsync || self.args.bench_window.is_none() { wgpu::PresentMode::Fifo } else { wgpu::PresentMode::AutoNoVsync }, alpha_mode: caps.alpha_modes[0], view_formats: vec![], desired_maximum_frame_latency: 2 };
        surface.configure(&device, &config);
        let mut renderer = Renderer::new(device, queue, ww * self.args.ss, wh * self.args.ss, format);
        renderer.set_opts(scene_opts(&self.args));
        self.gfx = Some(Gfx { window, surface, config, renderer });
    }

    fn window_event(&mut self, el: &ActiveEventLoop, _id: WindowId, ev: WindowEvent) {
        match ev {
            WindowEvent::CloseRequested => el.exit(),
            WindowEvent::Resized(s) => {
                if let Some(g) = &mut self.gfx {
                    g.config.width = s.width.max(1);
                    g.config.height = s.height.max(1);
                    g.surface.configure(&g.renderer.device, &g.config);
                }
            }
            WindowEvent::MouseInput { state, .. } => {
                self.drag = if state == ElementState::Pressed { Some((f64::NAN, f64::NAN)) } else { None };
            }
            WindowEvent::CursorMoved { position, .. } => {
                if let Some(d) = self.drag {
                    if !d.0.is_nan() {
                        self.yaw += (position.x - d.0) * 0.01;
                        self.pitch = (self.pitch + (position.y - d.1) * 0.01).clamp(-1.4, 1.4);
                    }
                    self.drag = Some((position.x, position.y));
                }
            }
            WindowEvent::MouseWheel { delta, .. } => {
                let y = match delta {
                    MouseScrollDelta::LineDelta(_, y) => y as f64,
                    MouseScrollDelta::PixelDelta(p) => p.y / 50.0,
                };
                self.zoom = (self.zoom * (1.0 + y * 0.1)).clamp(0.2, 8.0);
            }
            WindowEvent::KeyboardInput { event, .. } if event.state == ElementState::Pressed => {
                let nm = self.scene.wardrobe().motions.keys().cloned().collect::<Vec<_>>();
                let cur = self.scene.opts.motion.as_ref().and_then(|m| nm.iter().position(|x| x == m));
                match event.logical_key.as_ref() {
                    Key::Named(NamedKey::Escape) => el.exit(),
                    Key::Named(NamedKey::Space) => self.playing = !self.playing,
                    Key::Named(NamedKey::ArrowRight) => {
                        self.playing = false;
                        self.frame_f += 1.0;
                    }
                    Key::Named(NamedKey::ArrowLeft) => {
                        self.playing = false;
                        self.frame_f = (self.frame_f - 1.0).max(0.0);
                    }
                    Key::Character("]") | Key::Character("[") => {
                        let n = nm.len();
                        let i = match (cur, event.logical_key.as_ref() == Key::Character("]")) {
                            (Some(i), true) => (i + 1) % n,
                            (Some(i), false) => (i + n - 1) % n,
                            (None, _) => 0,
                        };
                        self.scene.opts.motion = Some(nm[i].clone());
                        self.frame_f = 0.0;
                        eprintln!("motion {i}: {}", nm[i]);
                    }
                    Key::Character("1") | Key::Character("2") | Key::Character("3") => {
                        let p = match event.logical_key.as_ref() { Key::Character("1") => "m", Key::Character("2") => "r", _ => "n" };
                        self.scene.prefix = p.into();
                        self.scene.opts.motion = None;
                        self.scene.view = None;
                        self.frame_f = 0.0;
                    }
                    Key::Character("o") => self.scene.opts.outfit = (self.scene.opts.outfit + 1) % 40,
                    Key::Character("h") => self.scene.opts.hair = (self.scene.opts.hair + 1) % 6,
                    Key::Character("m") => self.scene.opts.mood = (self.scene.opts.mood + 1) % 45,
                    Key::Character("t") => self.scene.opts.state = (self.scene.opts.state + 1) % 3,
                    Key::Character("b") => self.blink = !self.blink,
                    _ => {}
                }
            }
            WindowEvent::RedrawRequested => self.redraw(el),
            _ => {}
        }
    }

    fn about_to_wait(&mut self, _el: &ActiveEventLoop) {
        if let Some(g) = &self.gfx {
            g.window.request_redraw();
        }
    }
}

impl App {
    fn redraw(&mut self, el: &ActiveEventLoop) {
        let now = std::time::Instant::now();
        if self.args.bench_window.is_some() {
            self.frame_f += 1.0;
            if let Some(l) = self.last_present {
                self.frame_times.push(now.duration_since(l).as_secs_f64());
            }
            self.last_present = Some(now);
        } else if self.playing {
            self.frame_f += now.duration_since(self.last).as_secs_f64() * 30.0;
        }
        self.last = now;
        let nf = self.scene.motion_frames() as f64;
        self.scene.opts.frame = (self.frame_f % nf) as usize;
        self.scene.opts.blink = self.blink;
        let (f, v) = self.scene.frame(self.yaw, self.pitch, self.zoom);
        let g = self.gfx.as_mut().unwrap();
        let frame = match g.surface.get_current_texture() {
            Ok(f) => f,
            Err(_) => {
                g.surface.configure(&g.renderer.device, &g.config);
                return;
            }
        };
        g.renderer.render_scene(&f, &v);
        let (w, h) = (g.config.width as f32, g.config.height as f32);
        let (sw, sh) = (g.renderer.scene_w as f32, g.renderer.scene_h as f32);
        let k = (w / sw).min(h / sh);
        let (vw, vh) = (sw * k, sh * k);
        g.renderer.blit(&frame.texture.create_view(&Default::default()), Some(((w - vw) / 2.0, (h - vh) / 2.0, vw, vh)));
        frame.present();
        self.shown += 1;
        if let Some(n) = self.args.bench_window {
            if self.shown >= n + 30 {
                let mut t: Vec<f64> = self.frame_times[29..].to_vec();
                let sum: f64 = t.iter().sum();
                t.sort_by(|a, b| a.partial_cmp(b).unwrap());
                let g = self.gfx.as_ref().unwrap();
                println!(
                    "window {}x{} (surface {}x{}), scene {}x{}, {} | {} frames: {:.1} fps avg, median {:.2} ms, p99 {:.2} ms, worst {:.2} ms",
                    g.window.inner_size().width, g.window.inner_size().height, g.config.width, g.config.height, g.renderer.scene_w, g.renderer.scene_h,
                    if self.args.vsync { "vsync" } else { "no vsync" }, t.len(), t.len() as f64 / sum, t[t.len() / 2] * 1000.0, t[(t.len() as f64 * 0.99) as usize - 1] * 1000.0, t[t.len() - 1] * 1000.0
                );
                el.exit();
            }
        }
        if let Some(m) = self.args.max_frames {
            if self.shown >= m {
                el.exit();
            }
        }
    }
}
