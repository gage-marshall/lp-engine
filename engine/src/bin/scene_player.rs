//! Play a script event as a scene: the heroine (poses, faces, camera presets, fades), dialogue box with typewriter text, voice, music and sound.
//!   scene_player CONTENT EVENT_ID [--outfit N] [--hair N] [--look soft-s|clean-s|authentic|...] [--size WxH] [--ss N] [--choose N,N] [--fov F]
//!                [--shot T1,T2,... --out PREFIX]   render stills at scene times (seconds) instead of opening a window
//!                [--no-audio] [--seed S] [--month M] [--hour H] [--player NAME]
//! In the window: Esc quits, Space pauses, Left/Right seek 2 s, R restarts.
use lp_engine::audio::AudioOutput;
use lp_engine::player::*;
use lp_engine::render::*;
use std::path::PathBuf;
use std::sync::Arc;
use winit::application::ApplicationHandler;
use winit::event::{ElementState, WindowEvent};
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
    timeline: bool,
}

fn parse() -> Args {
    let a: Vec<String> = std::env::args().collect();
    if a.len() < 3 {
        eprintln!("usage: scene_player CONTENT EVENT_ID [--outfit N] [--hair N] [--look NAME] [--size WxH] [--ss N] [--choose N,N] [--shot T,T --out PREFIX] [--no-audio]");
        std::process::exit(1);
    }
    let mut args = Args { content: PathBuf::from(&a[1]), event: u32::from_str_radix(a[2].trim_start_matches("0x"), 16).expect("event id"), opts: PlayerOptions::default(), w: 1280, h: 720, ss: 2, shots: Vec::new(), out: "scene".into(), audio: true, timeline: false };
    let mut i = 3;
    while i < a.len() {
        let v = a.get(i + 1).cloned().unwrap_or_default();
        match a[i].as_str() {
            "--outfit" => { args.opts.outfit = v.parse().ok(); i += 1; }
            "--hair" => { args.opts.hair = v.parse().ok(); i += 1; }
            "--look" => { args.opts.look = v; i += 1; }
            "--fov" => { args.opts.fov = v.parse().unwrap_or(30.0); i += 1; }
            "--tod" => { args.opts.tod = v.parse().ok(); i += 1; }
            "--timeline" => { args.timeline = true; }
            "--player" => { args.opts.player = Some(v); i += 1; }
            "--month" => { args.opts.month = v.parse().ok(); i += 1; }
            "--hour" => { args.opts.hour = v.parse().ok(); i += 1; }
            "--seed" => { args.opts.seed = v.parse().unwrap_or(1); i += 1; }
            "--choose" => { args.opts.choices = v.split(',').filter_map(|x| x.parse().ok()).collect(); i += 1; }
            "--size" => { if let Some((x, y)) = v.split_once('x') { args.w = x.parse().unwrap_or(1280); args.h = y.parse().unwrap_or(720); } i += 1; }
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

fn main() {
    let args = parse();
    let mut player = ScenePlayer::new(&args.content, args.event, PlayerOptions { outfit: args.opts.outfit, hair: args.opts.hair, look: args.opts.look.clone(), choices: args.opts.choices.clone(), fov: args.opts.fov, seed: args.opts.seed, month: args.opts.month, hour: args.opts.hour, player: args.opts.player.clone(), tod: args.opts.tod }).expect("scene");
    println!("event {:08x}: {} effects, {:.1} s, heroine slot {}", args.event, player.timeline.len(), player.duration, player.slot);
    if args.timeline {
        // one line per voice / motion / dialogue effect with its scene time, for scripts that align the player with recorded play
        use lp_engine::host::Effect;
        for e in &player.timeline {
            match &e.effect {
                Effect::Voice { clip, .. } => println!("T {:.3} voice {}", e.sec, clip),
                Effect::Motion { motion, .. } => println!("T {:.3} motion {}", e.sec, motion),
                Effect::Fade { out, .. } => println!("T {:.3} fade {}", e.sec, if *out { "out" } else { "in" }),
                Effect::Dialogue { text, .. } => println!("T {:.3} text {}", e.sec, text.replace('\n', "/").chars().take(24).collect::<String>()),
                _ => {}
            }
        }
    }
    if !args.shots.is_empty() {
        let (device, queue) = headless_device().expect("no GPU adapter");
        let (sw, sh, ox, oy) = ds_fit(args.w, args.h);
        let mut r = Renderer::new(device, queue, sw * args.ss, sh * args.ss, wgpu::TextureFormat::Rgba8Unorm);
        r.set_opts(player.render_opts.clone());
        r.set_backdrop(&player.backdrop_image(sw as usize, sh as usize));
        for &t in &args.shots {
            let f = player.frame(t, sw, sh).expect("frame");
            if let Some(ui) = &f.ui { r.set_overlay(ui); }
            if f.clear_ui { r.clear_overlay(); }
            r.set_fade(f.fade);
            let mut img = r.render_to_image(&f.data, &f.view, sw, sh);
            if (sw, sh) != (args.w, args.h) {
                // pillarbox: place the 3:4 picture on a black canvas of the requested size
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
    let event_loop = EventLoop::new().unwrap();
    event_loop.set_control_flow(ControlFlow::Poll);
    let mut app = App { args, player, audio, gfx: None, start: std::time::Instant::now(), paused_at: None, offset: 0.0, last_t: -1.0 };
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
    paused_at: Option<f64>,
    offset: f64,
    last_t: f64,
}

impl App {
    fn now(&self) -> f64 {
        self.paused_at.unwrap_or_else(|| self.start.elapsed().as_secs_f64() + self.offset)
    }
}

impl ApplicationHandler for App {
    fn resumed(&mut self, el: &ActiveEventLoop) {
        if self.gfx.is_some() {
            return;
        }
        let window = Arc::new(el.create_window(Window::default_attributes().with_title("LovePlus+ scene").with_inner_size(winit::dpi::LogicalSize::new(self.args.w, self.args.h))).unwrap());
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
        renderer.set_backdrop(&self.player.backdrop_image(sw as usize, sh as usize));
        self.gfx = Some(Gfx { window, surface, config, renderer });
        self.start = std::time::Instant::now();
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
                    g.renderer.set_backdrop(&self.player.backdrop_image(sw as usize, sh as usize));
                }
            }
            WindowEvent::KeyboardInput { event, .. } if event.state == ElementState::Pressed => match event.logical_key.as_ref() {
                Key::Named(NamedKey::Escape) => el.exit(),
                Key::Named(NamedKey::Space) => {
                    let t = self.now();
                    self.paused_at = if self.paused_at.is_some() { self.offset = t - self.start.elapsed().as_secs_f64(); None } else { Some(t) };
                }
                Key::Named(NamedKey::ArrowRight) => self.offset += 2.0,
                Key::Named(NamedKey::ArrowLeft) => self.offset = (self.offset - 2.0).max(-self.start.elapsed().as_secs_f64()),
                Key::Character("r") => {
                    self.offset = -self.start.elapsed().as_secs_f64();
                    self.last_t = -1.0;
                }
                _ => {}
            },
            WindowEvent::RedrawRequested => {
                let t = self.now().min(self.player.duration);
                let g = self.gfx.as_mut().unwrap();
                let (w, h) = (g.config.width, g.config.height);
                // audio: fire the effects that happened since the last frame
                if let Some(a) = &self.audio {
                    if t < self.last_t {
                        if let Ok(mut d) = a.director.lock() { d.driver().stop_all(0); }
                    } else if self.paused_at.is_none() {
                        let slot = self.player.slot;
                        let effs: Vec<_> = self.player.effects_between(self.last_t, t).into_iter().cloned().collect();
                        if let Ok(mut d) = a.director.lock() {
                            for e in &effs { d.apply(e, slot, false); }
                        }
                    }
                }
                self.last_t = t;
                let (sw, sh, ox, oy) = ds_fit(w, h);
                let frame = self.player.frame(t, sw, sh).expect("frame");
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
                if self.now() > self.player.duration + 1.0 && self.args.shots.is_empty() {
                    el.exit();
                }
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
