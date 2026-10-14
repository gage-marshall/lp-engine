//! Play a script event as a scene: the heroine (motions, faces, eyes, lip-sync, camera), backdrop, fades, dialogue window
//! with the game's typewriter, voice, music and sound. The scene runs on the game's frame clock (59.8261 Hz, `sim`); the window
//! draws at the display rate and interpolates between logic updates.
//!   scene_player CONTENT EVENT_ID [--save BACKUP.sav] [--month M] [--hour H] [--player NAME] [--choose N,N] [--look NAME]
//!                [--size WxH] [--ss N] [--manual] [--advance FRAMES] [--no-interp] [--no-audio] [--rng STATE]
//!                [--shot T1,T2,... --out PREFIX]   render stills at scene times (seconds) instead of opening a window
//! In the window: click / Enter / Space taps (advances the text), P pauses, R restarts, Esc quits.
use lp_engine::audio::{AudioEvent, AudioOutput};
use lp_engine::player::*;
use lp_engine::render::*;
use lp_engine::sim::clock::FRAME_HZ;
use std::path::PathBuf;
use std::sync::Arc;
use winit::application::ApplicationHandler;
use winit::event::{ElementState, MouseButton, WindowEvent};
use winit::event_loop::{ActiveEventLoop, ControlFlow, EventLoop};
use winit::keyboard::{Key, NamedKey};
use winit::window::{Window, WindowId};

struct Args {
    content: PathBuf,
    event: u32,
    opts: PlayerOptions,
    w: u32,
    h: u32,
    ss: u32,
    shots: Vec<f64>,
    out: String,
    audio: bool,
}

fn parse() -> Args {
    let a: Vec<String> = std::env::args().collect();
    if a.len() < 3 {
        eprintln!("usage: scene_player CONTENT EVENT_ID [--save BACKUP.sav] [--month M] [--hour H] [--choose N,N] [--look NAME] [--size WxH] [--ss N] [--manual] [--no-interp] [--shot T,T --out PREFIX] [--no-audio]");
        std::process::exit(1);
    }
    let mut args = Args { content: PathBuf::from(&a[1]), event: u32::from_str_radix(a[2].trim_start_matches("0x"), 16).expect("event id"), opts: PlayerOptions::default(), w: 960, h: 1280, ss: 2, shots: Vec::new(), out: "scene".into(), audio: true };
    let mut i = 3;
    while i < a.len() {
        let v = a.get(i + 1).cloned().unwrap_or_default();
        match a[i].as_str() {
            "--save" => { args.opts.save = Some(PathBuf::from(v)); i += 1; }
            "--look" => { args.opts.look = v; i += 1; }
            "--player" => { args.opts.player = Some(v); i += 1; }
            "--month" => { args.opts.month = v.parse().ok(); i += 1; }
            "--hour" => { args.opts.hour = v.parse().ok(); i += 1; }
            "--rng" => { args.opts.rng = v.parse().ok(); i += 1; }
            "--choose" => { args.opts.choices = v.split(',').filter_map(|x| x.parse().ok()).collect(); i += 1; }
            "--advance" => { args.opts.auto_advance = v.parse().ok(); i += 1; }
            "--manual" => args.opts.auto_advance = None,
            "--no-interp" => args.opts.interpolate = false,
            "--size" => { if let Some((x, y)) = v.split_once('x') { args.w = x.parse().unwrap_or(960); args.h = y.parse().unwrap_or(1280); } i += 1; }
            "--ss" => { args.ss = v.parse().unwrap_or(2); i += 1; }
            "--shot" => { args.shots = v.split(',').filter_map(|x| x.parse().ok()).collect(); i += 1; }
            "--out" => { args.out = v; i += 1; }
            "--no-audio" => args.audio = false,
            x => eprintln!("ignoring {x}"),
        }
        i += 1;
    }
    args
}

fn write_png(path: &str, im: &lp_engine::nns2d::Image) {
    let f = std::fs::File::create(path).unwrap();
    let mut enc = png::Encoder::new(std::io::BufWriter::new(f), im.w as u32, im.h as u32);
    enc.set_color(png::ColorType::Rgba);
    enc.set_depth(png::BitDepth::Eight);
    enc.write_header().unwrap().write_image_data(&im.buf).unwrap();
}

fn new_player(args: &Args) -> ScenePlayer {
    let o = &args.opts;
    ScenePlayer::new(&args.content, args.event, PlayerOptions { look: o.look.clone(), choices: o.choices.clone(), save: o.save.clone(), month: o.month, hour: o.hour, player: o.player.clone(), interpolate: o.interpolate, auto_advance: o.auto_advance, rng: o.rng, replay: None }).expect("scene")
}

fn main() {
    let args = parse();
    let mut player = new_player(&args);
    println!("event {:08x}: heroine slot {}", args.event, player.slot);
    if !args.shots.is_empty() {
        let (device, queue) = headless_device().expect("no GPU adapter");
        let (sw, sh, ox, oy) = ds_fit(args.w, args.h);
        let mut r = Renderer::new(device, queue, sw * args.ss, sh * args.ss, wgpu::TextureFormat::Rgba8Unorm);
        r.set_opts(player.render_opts.clone());
        for &t in &args.shots {
            player.run_to(t);
            if let Some(bg) = player.backdrop_image(sw as usize, sh as usize) { r.set_backdrop(&bg); }
            let f = player.frame(sw, sh, 0.0).expect("frame");
            if let Some(ui) = &f.ui { r.set_overlay(ui); }
            if f.clear_ui { r.clear_overlay(); }
            r.set_fade(f.fade);
            let mut img = r.render_to_image(&f.data, &f.view, sw, sh);
            if (sw, sh) != (args.w, args.h) {
                let mut canvas = lp_engine::nns2d::Image::new(args.w as usize, args.h as usize);
                for px in canvas.buf.chunks_exact_mut(4) { px[3] = 255; }
                for y in 0..sh as usize { for x in 0..sw as usize {
                    let (s, d) = ((y * sw as usize + x) * 4, ((y + oy as usize) * args.w as usize + x + ox as usize) * 4);
                    canvas.buf[d..d + 4].copy_from_slice(&img.buf[s..s + 4]);
                } }
                img = canvas;
            }
            let path = format!("{}_{:06.2}.png", args.out, t);
            write_png(&path, &img);
            println!("t={:.2}s -> {}", t, path);
        }
        return;
    }
    let audio = if args.audio { AudioOutput::start(&args.content).map_err(|e| eprintln!("audio disabled: {e}")).ok() } else { None };
    if let Some(a) = &audio {
        if let Ok(mut d) = a.director.lock() { d.levels = player.levels(); }
    }
    let event_loop = EventLoop::new().unwrap();
    event_loop.set_control_flow(ControlFlow::Poll);
    let mut app = App { args, player, audio, gfx: None, start: std::time::Instant::now(), ticks: 0, paused: None };
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
    player: ScenePlayer,
    audio: Option<AudioOutput>,
    gfx: Option<Gfx>,
    start: std::time::Instant,
    /// game frames run since `start`
    ticks: u64,
    paused: Option<std::time::Instant>,
}

impl App {
    /// game frames that should have run by now (fixed step against wall time) and the fraction into the next one
    fn due(&self) -> (u64, f64) {
        let t = self.paused.unwrap_or_else(std::time::Instant::now).duration_since(self.start).as_secs_f64() * FRAME_HZ;
        (t.floor() as u64, t.fract())
    }

    fn feed_audio(&mut self) {
        let evs = self.player.take_audio();
        if let Some(a) = &self.audio {
            if let Ok(mut d) = a.director.lock() {
                let slot = self.player.slot;
                for (_, e) in evs {
                    match e {
                        AudioEvent::Sound(e) => d.apply(&e, slot, false),
                        AudioEvent::Voice(c) => d.play_voice(c),
                    }
                }
            }
        }
    }
}

impl ApplicationHandler for App {
    fn resumed(&mut self, el: &ActiveEventLoop) {
        if self.gfx.is_some() {
            return;
        }
        let window = Arc::new(el.create_window(Window::default_attributes().with_title("LovePlus+ scene").with_inner_size(winit::dpi::LogicalSize::new(self.args.w / 2, self.args.h / 2))).unwrap());
        let instance = wgpu::Instance::default();
        let surface = instance.create_surface(window.clone()).unwrap();
        let adapter = pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions { power_preference: wgpu::PowerPreference::HighPerformance, compatible_surface: Some(&surface), force_fallback_adapter: false })).expect("no adapter");
        let (device, queue) = pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor { label: None, required_features: wgpu::Features::empty(), required_limits: wgpu::Limits::default(), memory_hints: Default::default() }, None)).unwrap();
        let caps = surface.get_capabilities(&adapter);
        let format = caps.formats.iter().copied().find(|f| !f.is_srgb()).unwrap_or(caps.formats[0]);
        let size = window.inner_size();
        let config = wgpu::SurfaceConfiguration { usage: wgpu::TextureUsages::RENDER_ATTACHMENT, format, width: size.width.max(1), height: size.height.max(1), present_mode: wgpu::PresentMode::Fifo, alpha_mode: caps.alpha_modes[0], view_formats: vec![], desired_maximum_frame_latency: 2 };
        surface.configure(&device, &config);
        let (sw, sh, _, _) = ds_fit(config.width, config.height);
        let mut renderer = Renderer::new(device, queue, sw * self.args.ss, sh * self.args.ss, format);
        renderer.set_opts(self.player.render_opts.clone());
        self.player.invalidate_backdrop();
        self.gfx = Some(Gfx { window, surface, config, renderer });
        self.start = std::time::Instant::now();
        self.ticks = 0;
    }

    fn window_event(&mut self, el: &ActiveEventLoop, _id: WindowId, ev: WindowEvent) {
        match ev {
            WindowEvent::CloseRequested => el.exit(),
            WindowEvent::Resized(s) => {
                if let Some(g) = &mut self.gfx {
                    g.config.width = s.width.max(1);
                    g.config.height = s.height.max(1);
                    g.surface.configure(&g.renderer.device, &g.config);
                    let (sw, sh, _, _) = ds_fit(g.config.width, g.config.height);
                    g.renderer.set_size(sw * self.args.ss, sh * self.args.ss);
                    self.player.invalidate_backdrop();
                }
            }
            WindowEvent::MouseInput { state: ElementState::Released, button: MouseButton::Left, .. } => self.player.tap(),
            WindowEvent::KeyboardInput { event, .. } if event.state == ElementState::Pressed => match event.logical_key.as_ref() {
                Key::Named(NamedKey::Escape) => el.exit(),
                Key::Named(NamedKey::Enter) | Key::Named(NamedKey::Space) => self.player.tap(),
                Key::Character("p") => {
                    self.paused = match self.paused.take() {
                        Some(p) => {
                            self.start += p.elapsed();
                            None
                        }
                        None => Some(std::time::Instant::now()),
                    };
                }
                Key::Character("r") => {
                    self.player = new_player(&self.args);
                    if let Some(a) = &self.audio {
                        if let Ok(mut d) = a.director.lock() { d.driver().stop_all(0); }
                    }
                    self.start = std::time::Instant::now();
                    self.ticks = 0;
                }
                _ => {}
            },
            WindowEvent::RedrawRequested => {
                let (due, sub) = self.due();
                while self.ticks < due {
                    self.player.tick();
                    self.ticks += 1;
                }
                self.feed_audio();
                let g = self.gfx.as_mut().unwrap();
                let (w, h) = (g.config.width, g.config.height);
                let (sw, sh, ox, oy) = ds_fit(w, h);
                if let Some(bg) = self.player.backdrop_image(sw as usize, sh as usize) {
                    g.renderer.set_backdrop(&bg);
                }
                let frame = match self.player.frame(sw, sh, if self.paused.is_some() { 0.0 } else { sub }) {
                    Ok(f) => f,
                    Err(_) => return, // nothing simulated yet
                };
                if let Some(ui) = &frame.ui { g.renderer.set_overlay(ui); }
                if frame.clear_ui { g.renderer.clear_overlay(); }
                g.renderer.set_fade(frame.fade);
                let out = match g.surface.get_current_texture() {
                    Ok(o) => o,
                    Err(_) => {
                        g.surface.configure(&g.renderer.device, &g.config);
                        return;
                    }
                };
                g.renderer.render_scene(&frame.data, &frame.view);
                g.renderer.blit(&out.texture.create_view(&Default::default()), Some((ox as f32, oy as f32, sw as f32, sh as f32)));
                out.present();
            }
            _ => {}
        }
    }

    fn about_to_wait(&mut self, _el: &ActiveEventLoop) {
        if let Some(g) = &self.gfx {
            g.window.request_redraw();
        }
    }
}
