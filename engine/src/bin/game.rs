//! LovePlus+ on the engine: the game boots as the DS boots it and runs at the DS frame rate (59.8261 Hz). The ported
//! flow and native code run natively; what is not ported yet runs as the game's own code (`runtime::interp`).
//! The two screens are shown as the game is held, like a book: the top screen on the left, the touch screen on the
//! right, both upright.
//!
//!   game CONTENT [--save BACKUP.sav] [--scale N] [--res N] [--mute] [--smooth]
//!
//! Sound is the game's: its sound commands run on the engine's ARM7 sound driver (`runtime::snd7`), whose output
//! plays on the default device. Time is the game's too: frames run as fast as the DS's clock advances, so a frame
//! the game overruns takes longer, as it does on the DS. Beyond the DS (`runtime::present`): `--res N` draws at N
//! times its resolution (3D and the pictures the game captures of it; 2D art is scaled up), `--smooth` shows
//! in-between pictures of the heroine (30 updates a second instead of 15, 2 VBlanks behind). `--scale` sizes the
//! window. The View menu changes the resolution and smooth motion while playing; the picture keeps the DS's
//! proportions in any window size.
//! Mouse on the right-hand screen: the stylus. Keys: arrows = D-pad, X = A, Z = B, S = X, A = Y, Q = L, W = R,
//! Enter = Start, Backspace = Select, Esc = quit. The battery save is written back to its file when the game saves.
use lp_engine::rom::Rom;
use lp_engine::runtime::{present::Present, raster3d::Look, sdk, Game, NoSubsystems, TICK_HZ};
use std::path::{Path, PathBuf};
use std::collections::VecDeque;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use winit::application::ApplicationHandler;
use winit::event::{ElementState, MouseButton, WindowEvent};
use winit::event_loop::{ActiveEventLoop, ControlFlow, EventLoop};
use winit::keyboard::{Key, NamedKey};
use winit::window::{Window, WindowId};

/// the composite: both screens rotated a quarter turn counter-clockwise, side by side
const CW: u32 = 384;
const CH: u32 = 256;

struct Gpu {
    window: Arc<Window>,
    surface: wgpu::Surface<'static>,
    device: wgpu::Device,
    queue: wgpu::Queue,
    config: wgpu::SurfaceConfiguration,
    texture: wgpu::Texture,
    bind: wgpu::BindGroup,
    layout: wgpu::BindGroupLayout,
    sampler: wgpu::Sampler,
    pipeline: wgpu::RenderPipeline,
}

impl Gpu {
    /// window scaling: linear (smooth) or nearest (the DS's pixels)
    fn set_filter(&mut self, linear: bool, size: (u32, u32)) {
        let f = if linear { wgpu::FilterMode::Linear } else { wgpu::FilterMode::Nearest };
        self.sampler = self.device.create_sampler(&wgpu::SamplerDescriptor { mag_filter: f, min_filter: wgpu::FilterMode::Linear, ..Default::default() });
        self.set_texture(size);
    }

    /// the texture the composite is uploaded to, at its size
    fn set_texture(&mut self, (w, h): (u32, u32)) {
        let (texture, bind) = make_texture(&self.device, &self.layout, &self.sampler, w, h);
        self.texture = texture;
        self.bind = bind;
    }
}

fn make_texture(device: &wgpu::Device, layout: &wgpu::BindGroupLayout, sampler: &wgpu::Sampler, w: u32, h: u32) -> (wgpu::Texture, wgpu::BindGroup) {
    let texture = device.create_texture(&wgpu::TextureDescriptor {
        label: None,
        size: wgpu::Extent3d { width: w, height: h, depth_or_array_layers: 1 },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: wgpu::TextureFormat::Rgba8UnormSrgb,
        usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
        view_formats: &[],
    });
    let bind = device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: None,
        layout,
        entries: &[
            wgpu::BindGroupEntry { binding: 0, resource: wgpu::BindingResource::TextureView(&texture.create_view(&Default::default())) },
            wgpu::BindGroupEntry { binding: 1, resource: wgpu::BindingResource::Sampler(sampler) },
        ],
    });
    (texture, bind)
}

/// the menu bar (macOS, Windows): View > resolution, smooth motion, actual size
struct Menus {
    _bar: muda::Menu,
    res: Vec<muda::CheckMenuItem>,
    smooth: muda::CheckMenuItem,
    bilinear: muda::CheckMenuItem,
    smooth_toon: muda::CheckMenuItem,
    thick: muda::CheckMenuItem,
    aa: muda::CheckMenuItem,
    linear: muda::CheckMenuItem,
    actual: muda::MenuItem,
    quit: muda::MenuItem,
}

impl Menus {
    fn new(res: usize, smooth: bool) -> Menus {
        use muda::{CheckMenuItem, Menu, MenuItem, PredefinedMenuItem, Submenu};
        let acc = |s: &str| s.parse().ok();
        let bar = Menu::new();
        let app = Submenu::new("LovePlus+", true);
        let quit = MenuItem::new("Quit", true, acc("CmdOrCtrl+Q"));
        app.append(&quit).unwrap();
        let view = Submenu::new("View", true);
        let res: Vec<CheckMenuItem> = (1..=4).map(|r| CheckMenuItem::new(format!("{r}x Resolution"), true, r == res, acc(&format!("CmdOrCtrl+{r}")))).collect();
        for r in &res {
            view.append(r).unwrap();
        }
        view.append(&PredefinedMenuItem::separator()).unwrap();
        let smooth = CheckMenuItem::new("Smooth Motion (30 fps heroine)", true, smooth, acc("CmdOrCtrl+M"));
        view.append(&smooth).unwrap();
        view.append(&PredefinedMenuItem::separator()).unwrap();
        let bilinear = CheckMenuItem::new("Texture Filtering", true, false, None);
        let smooth_toon = CheckMenuItem::new("Smooth Toon Shading", true, false, None);
        let thick = CheckMenuItem::new("Outlines at DS Thickness", true, false, None);
        let aa = CheckMenuItem::new("Anti-aliasing (2x2 3D)", true, false, None);
        for i in [&bilinear, &smooth_toon, &thick, &aa] {
            view.append(i).unwrap();
        }
        view.append(&PredefinedMenuItem::separator()).unwrap();
        let linear = CheckMenuItem::new("Smooth Window Scaling", true, false, None);
        view.append(&linear).unwrap();
        let actual = MenuItem::new("Actual Size", true, acc("CmdOrCtrl+0"));
        view.append(&actual).unwrap();
        bar.append(&app).unwrap();
        bar.append(&view).unwrap();
        Menus { _bar: bar, res, smooth, bilinear, smooth_toon, thick, aa, linear, actual, quit }
    }
}

struct App {
    game: Game<NoSubsystems>,
    save_path: Option<PathBuf>,
    gpu: Option<Gpu>,
    start: Instant,
    /// the game's clock when the wall clock started
    tick0: u64,
    frames: u64,
    mouse: (f64, f64),
    pressed: bool,
    keys: u16,
    scale: u32,
    last_save_write: Instant,
    /// what is shown (resolution, in-between pictures)
    present: Present,
    menus: Option<Menus>,
    /// time spent per second: running the game, drawing (after_pass + composite), and passes / draws counted
    timing: (Duration, Duration, u32, u32, Instant),
}

const SHADER: &str = r#"
struct V { @builtin(position) p: vec4<f32>, @location(0) uv: vec2<f32> };
@vertex fn vs(@builtin(vertex_index) i: u32) -> V {
    var xy = array<vec2<f32>, 4>(vec2(-1.0, -1.0), vec2(1.0, -1.0), vec2(-1.0, 1.0), vec2(1.0, 1.0));
    var o: V;
    o.p = vec4(xy[i], 0.0, 1.0);
    o.uv = vec2((xy[i].x + 1.0) * 0.5, (1.0 - xy[i].y) * 0.5);
    return o;
}
@group(0) @binding(0) var t: texture_2d<f32>;
@group(0) @binding(1) var s: sampler;
@fragment fn fs(v: V) -> @location(0) vec4<f32> { return textureSample(t, s, v.uv); }
"#;

impl App {
    /// the composite image's size: the DS's at the presentation's resolution
    fn size(&self) -> (u32, u32) {
        let r = self.present.scale as u32;
        (CW * r, CH * r)
    }

    /// the two screens as the composite image (RGBA8, `size`)
    fn composite(&mut self) -> Vec<u8> {
        let (top, bottom) = self.present.screens(&mut self.game.m);
        let r = self.present.scale as u32;
        let (cw, ch) = self.size();
        let (w, h) = (256 * r, 192 * r);
        let mut out = vec![0u8; (cw * ch * 4) as usize];
        for (si, scr) in [top, bottom].iter().enumerate() {
            for y in 0..h {
                for x in 0..w {
                    // a quarter turn counter-clockwise: (x, y) -> (y, 255 - x), the top screen on the left
                    let (cx, cy) = (y + h * si as u32, w - 1 - x);
                    let s = ((y * w + x) * 4) as usize;
                    let d = ((cy * cw + cx) * 4) as usize;
                    out[d..d + 4].copy_from_slice(&scr[s..s + 4]);
                }
            }
        }
        out
    }

    /// where the composite is drawn in the window (x, y, width, height in physical pixels): as large as fits with
    /// the DS's proportions, centred
    fn viewport(&self) -> Option<(f32, f32, f32, f32)> {
        let g = self.gpu.as_ref()?;
        let (ww, wh) = (g.config.width as f32, g.config.height as f32);
        let k = (ww / CW as f32).min(wh / CH as f32);
        let (w, h) = (CW as f32 * k, CH as f32 * k);
        Some(((ww - w) / 2.0, (wh - h) / 2.0, w, h))
    }

    /// the stylus position on the touch screen for a window position, if it is on it
    fn touch_at(&self, wx: f64, wy: f64) -> Option<(u16, u16)> {
        let (vx, vy, vw, vh) = self.viewport()?;
        let (cx, cy) = ((wx - vx as f64) / vw as f64 * CW as f64, (wy - vy as f64) / vh as f64 * CH as f64);
        if !(192.0..384.0).contains(&cx) || !(0.0..256.0).contains(&cy) {
            return None;
        }
        // inverse of the quarter turn: screen x = 255 - cy, y = cx - 192
        Some(((255.0 - cy).clamp(0.0, 255.0) as u16, (cx - 192.0).clamp(0.0, 191.0) as u16))
    }

    fn step(&mut self) {
        self.game.m.input.keys = self.keys;
        self.game.m.input.touch = if self.pressed { self.touch_at(self.mouse.0, self.mouse.1) } else { None };
        let local = sdk::host_local_seconds();
        self.game.m.rtc = sdk::rtc_from_local_seconds(local);
        let t0 = Instant::now();
        if self.game.frame() != 0 {
            eprintln!("the game asked for a reset");
        }
        let t1 = Instant::now();
        self.present.after_pass(&self.game.m);
        self.timing.0 += t1 - t0;
        self.timing.1 += t1.elapsed();
        self.timing.2 += 1;
        for f in self.game.m.faults.drain(..) {
            eprintln!("game code stopped: {f}");
        }
        if self.game.m.backup_dirty && self.last_save_write.elapsed() > Duration::from_secs(2) {
            self.write_save();
        }
    }

    fn write_save(&mut self) {
        if let Some(p) = &self.save_path {
            if let Err(e) = std::fs::write(p, &self.game.m.backup) {
                eprintln!("could not write the save: {e}");
            } else {
                self.game.m.backup_dirty = false;
            }
        }
        self.last_save_write = Instant::now();
    }

    fn draw(&mut self) {
        let t0 = Instant::now();
        let img = self.composite();
        self.timing.1 += t0.elapsed();
        self.timing.3 += 1;
        self.show_timing();
        let (cw, ch) = self.size();
        let Some(g) = self.gpu.as_ref() else { return };
        g.queue.write_texture(
            wgpu::ImageCopyTexture { texture: &g.texture, mip_level: 0, origin: wgpu::Origin3d::ZERO, aspect: wgpu::TextureAspect::All },
            &img,
            wgpu::ImageDataLayout { offset: 0, bytes_per_row: Some(cw * 4), rows_per_image: Some(ch) },
            wgpu::Extent3d { width: cw, height: ch, depth_or_array_layers: 1 },
        );
        let Ok(out) = g.surface.get_current_texture() else { return };
        let view = out.texture.create_view(&Default::default());
        let mut enc = g.device.create_command_encoder(&Default::default());
        {
            let mut pass = enc.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: None,
                color_attachments: &[Some(wgpu::RenderPassColorAttachment { view: &view, resolve_target: None, ops: wgpu::Operations { load: wgpu::LoadOp::Clear(wgpu::Color::BLACK), store: wgpu::StoreOp::Store } })],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
            });
            if let Some((x, y, w, h)) = self.viewport() {
                pass.set_viewport(x, y, w, h, 0.0, 1.0);
            }
            pass.set_pipeline(&g.pipeline);
            pass.set_bind_group(0, &g.bind, &[]);
            pass.draw(0..4, 0..1);
        }
        g.queue.submit([enc.finish()]);
        out.present();
    }
}

impl App {
    fn quit(&mut self, el: &ActiveEventLoop) {
        if self.game.m.backup_dirty {
            self.write_save();
        }
        el.exit();
    }

    fn menu_event(&mut self, el: &ActiveEventLoop, id: &muda::MenuId) {
        let Some(m) = &self.menus else { return };
        if id == m.quit.id() {
            self.quit(el);
        } else if id == m.smooth.id() {
            self.present.smooth = m.smooth.is_checked();
        } else if id == m.linear.id() {
            let (linear, size) = (m.linear.is_checked(), self.size());
            if let Some(g) = &mut self.gpu {
                g.set_filter(linear, size);
            }
        } else if [m.bilinear.id(), m.smooth_toon.id(), m.thick.id(), m.aa.id()].contains(&id) {
            self.apply_options();
        } else if id == m.actual.id() {
            // one window pixel per composite pixel
            if let Some(g) = &self.gpu {
                let (w, h) = self.size();
                let _ = g.window.request_inner_size(winit::dpi::PhysicalSize::new(w, h));
            }
        } else if let Some(i) = m.res.iter().position(|r| id == r.id()) {
            for (k, r) in m.res.iter().enumerate() {
                r.set_checked(k == i);
            }
            if self.present.scale != i + 1 {
                self.present.scale = i + 1;
                self.apply_options(); // rebuilds the presentation at the new scale
                let size = self.size();
                if let Some(g) = &mut self.gpu {
                    g.set_texture(size);
                }
            }
        }
    }

    /// once a second: the time per game pass and per drawn frame in the window title
    fn show_timing(&mut self) {
        let (game, draw, passes, draws, since) = self.timing;
        if since.elapsed() < Duration::from_secs(1) || passes == 0 || draws == 0 {
            return;
        }
        if let Some(g) = &self.gpu {
            let ms = |d: Duration, n: u32| d.as_secs_f64() * 1000.0 / n as f64;
            g.window.set_title(&format!("LovePlus+  ({}x)  game {:.1} ms/pass, display {:.1} ms/frame, {} frames/s", self.present.scale, ms(game, passes), ms(draw, draws), draws));
        }
        self.timing = (Duration::ZERO, Duration::ZERO, 0, 0, Instant::now());
    }

    /// the menu's 3D options into the presentation (outlines at DS thickness: as many pixels as the resolution)
    fn apply_options(&mut self) {
        let Some(m) = &self.menus else { return };
        let scale = self.present.scale;
        let look = Look { bilinear: m.bilinear.is_checked(), smooth_toon: m.smooth_toon.is_checked(), outline: if m.thick.is_checked() { scale } else { 1 } };
        let aa = m.aa.is_checked();
        self.present.set(scale, look, aa);
    }
}

fn key_bit(k: &Key) -> Option<u16> {
    Some(match k {
        Key::Named(NamedKey::ArrowRight) => 1 << 4,
        Key::Named(NamedKey::ArrowLeft) => 1 << 5,
        Key::Named(NamedKey::ArrowUp) => 1 << 6,
        Key::Named(NamedKey::ArrowDown) => 1 << 7,
        Key::Named(NamedKey::Enter) => 1 << 3,
        Key::Named(NamedKey::Backspace) => 1 << 2,
        Key::Character(c) => match c.to_lowercase().as_str() {
            "x" => 1 << 0,
            "z" => 1 << 1,
            "w" => 1 << 8,
            "q" => 1 << 9,
            "s" => 1 << 10,
            "a" => 1 << 11,
            _ => return None,
        },
        _ => return None,
    })
}

impl ApplicationHandler for App {
    fn resumed(&mut self, el: &ActiveEventLoop) {
        if self.gpu.is_some() {
            return;
        }
        let attrs = Window::default_attributes().with_title("LovePlus+").with_inner_size(winit::dpi::LogicalSize::new(CW * self.scale, CH * self.scale));
        let window = Arc::new(el.create_window(attrs).unwrap());
        let instance = wgpu::Instance::default();
        let surface = instance.create_surface(window.clone()).unwrap();
        let adapter = pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions { power_preference: wgpu::PowerPreference::HighPerformance, compatible_surface: Some(&surface), force_fallback_adapter: false })).expect("no adapter");
        let (device, queue) = pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor { label: None, required_features: wgpu::Features::empty(), required_limits: wgpu::Limits::default(), memory_hints: Default::default() }, None)).unwrap();
        let size = window.inner_size();
        let config = surface.get_default_config(&adapter, size.width.max(1), size.height.max(1)).unwrap();
        surface.configure(&device, &config);
        let (cw, ch) = self.size();
        let sampler = device.create_sampler(&wgpu::SamplerDescriptor { mag_filter: wgpu::FilterMode::Nearest, min_filter: wgpu::FilterMode::Linear, ..Default::default() });
        let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: None,
            entries: &[
                wgpu::BindGroupLayoutEntry { binding: 0, visibility: wgpu::ShaderStages::FRAGMENT, ty: wgpu::BindingType::Texture { sample_type: wgpu::TextureSampleType::Float { filterable: true }, view_dimension: wgpu::TextureViewDimension::D2, multisampled: false }, count: None },
                wgpu::BindGroupLayoutEntry { binding: 1, visibility: wgpu::ShaderStages::FRAGMENT, ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering), count: None },
            ],
        });
        let (texture, bind) = make_texture(&device, &layout, &sampler, cw, ch);
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor { label: None, source: wgpu::ShaderSource::Wgsl(SHADER.into()) });
        let pl = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor { label: None, bind_group_layouts: &[&layout], push_constant_ranges: &[] });
        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: None,
            layout: Some(&pl),
            vertex: wgpu::VertexState { module: &shader, entry_point: "vs", buffers: &[], compilation_options: Default::default() },
            fragment: Some(wgpu::FragmentState { module: &shader, entry_point: "fs", targets: &[Some(config.format.into())], compilation_options: Default::default() }),
            primitive: wgpu::PrimitiveState { topology: wgpu::PrimitiveTopology::TriangleStrip, ..Default::default() },
            depth_stencil: None,
            multisample: Default::default(),
            multiview: None,
            cache: None,
        });
        // the menu bar, once the application exists
        if self.menus.is_none() {
            let m = Menus::new(self.present.scale, self.present.smooth);
            #[cfg(target_os = "macos")]
            m._bar.init_for_nsapp();
            #[cfg(target_os = "windows")]
            {
                use winit::raw_window_handle::{HasWindowHandle, RawWindowHandle};
                if let Ok(RawWindowHandle::Win32(h)) = window.window_handle().map(|h| h.as_raw()) {
                    unsafe { m._bar.init_for_hwnd(h.hwnd.get()).ok() };
                }
            }
            self.menus = Some(m);
        }
        self.gpu = Some(Gpu { window, surface, device, queue, config, texture, bind, layout, sampler, pipeline });
        self.start = Instant::now();
        self.tick0 = self.game.m.tick;
        el.set_control_flow(ControlFlow::Poll);
    }

    fn window_event(&mut self, el: &ActiveEventLoop, _id: WindowId, ev: WindowEvent) {
        match ev {
            WindowEvent::CloseRequested => self.quit(el),
            WindowEvent::Resized(s) => {
                if let Some(g) = self.gpu.as_mut() {
                    g.config.width = s.width.max(1);
                    g.config.height = s.height.max(1);
                    g.surface.configure(&g.device, &g.config);
                }
            }
            WindowEvent::CursorMoved { position, .. } => self.mouse = (position.x, position.y),
            WindowEvent::MouseInput { state, button: MouseButton::Left, .. } => self.pressed = state == ElementState::Pressed,
            WindowEvent::KeyboardInput { event, .. } => {
                if event.logical_key == Key::Named(NamedKey::Escape) {
                    self.quit(el);
                    return;
                }
                if let Some(b) = key_bit(&event.logical_key) {
                    if event.state == ElementState::Pressed {
                        self.keys |= b;
                    } else {
                        self.keys &= !b;
                    }
                }
            }
            WindowEvent::RedrawRequested => self.draw(),
            _ => {}
        }
    }

    fn about_to_wait(&mut self, el: &ActiveEventLoop) {
        while let Ok(e) = muda::MenuEvent::receiver().try_recv() {
            self.menu_event(el, e.id());
        }
        // the game's clock keeps pace with the wall clock
        let wall = (self.start.elapsed().as_secs_f64() * TICK_HZ) as u64;
        let mut ran = 0;
        while self.game.m.tick - self.tick0 < wall && ran < 8 {
            self.step();
            self.frames += 1;
            ran += 1;
        }
        if ran == 8 {
            // fell far behind: carry on from here rather than spiral
            self.start = Instant::now();
            self.tick0 = self.game.m.tick;
        }
        let ran = ran > 0;
        if ran {
            if let Some(g) = &self.gpu {
                g.window.request_redraw();
            }
        } else {
            std::thread::sleep(Duration::from_millis(1));
        }
    }
}

fn main() {
    let a: Vec<String> = std::env::args().collect();
    if a.len() < 2 {
        eprintln!("usage: game CONTENT [--save BACKUP.sav] [--scale N] [--res N] [--mute] [--smooth]");
        std::process::exit(1);
    }
    let rom = Rom::open(Path::new(&a[1])).unwrap_or_else(|e| {
        eprintln!("content: {e}");
        std::process::exit(1)
    });
    let mut game = Game::new(rom, NoSubsystems);
    let value = |flag: &str| {
        a.iter().position(|x| x == flag).map(|i| {
            a.get(i + 1).filter(|v| !v.starts_with("--")).cloned().unwrap_or_else(|| {
                eprintln!("{flag} needs a value");
                std::process::exit(1)
            })
        })
    };
    let save_path = value("--save").map(PathBuf::from);
    if let Some(p) = &save_path {
        match std::fs::read(p) {
            Ok(b) => game.m.backup = b,
            Err(_) => eprintln!("{}: no save yet; starting with an empty one", p.display()),
        }
    }
    let scale = value("--scale").map(|v| v.parse().unwrap_or_else(|_| {
        eprintln!("--scale needs a number");
        std::process::exit(1)
    })).unwrap_or(3);
    let _audio = if a.iter().any(|x| x == "--mute") {
        None
    } else {
        match start_audio(&mut game) {
            Ok(s) => Some(s),
            Err(e) => {
                eprintln!("no sound: {e}");
                None
            }
        }
    };
    game.m.rtc = sdk::rtc_from_local_seconds(sdk::host_local_seconds());
    for f in game.power_on() {
        eprintln!("power on stopped: {f}");
    }
    let el = EventLoop::new().unwrap();
    let tick0 = game.m.tick;
    let res = value("--res").map(|v| v.parse().unwrap_or_else(|_| {
        eprintln!("--res needs a number");
        std::process::exit(1)
    })).unwrap_or(1);
    let present = Present::new(res, a.iter().any(|x| x == "--smooth"));
    let mut app = App { game, save_path, gpu: None, start: Instant::now(), tick0, frames: 0, mouse: (0.0, 0.0), pressed: false, keys: 0, scale, last_save_write: Instant::now(), present, menus: None, timing: (Duration::ZERO, Duration::ZERO, 0, 0, Instant::now()) };
    el.run_app(&mut app).unwrap();
}

/// the default output device plays what the sound driver renders (it renders at the device's rate)
fn start_audio(game: &mut Game<NoSubsystems>) -> Result<cpal::Stream, String> {
    use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
    let dev = cpal::default_host().default_output_device().ok_or("no output device")?;
    let cfg = dev.default_output_config().map_err(|e| e.to_string())?;
    let channels = cfg.channels() as usize;
    let sink = Arc::new(Mutex::new(VecDeque::<f32>::new()));
    game.m.snd.rate = cfg.sample_rate().0;
    game.m.snd.sink = Some(sink.clone());
    let stream = dev
        .build_output_stream(
            &cfg.into(),
            move |data: &mut [f32], _| {
                let mut q = sink.lock().unwrap();
                for f in data.chunks_mut(channels) {
                    let (l, r) = if q.len() >= 2 { (q.pop_front().unwrap(), q.pop_front().unwrap()) } else { (0.0, 0.0) };
                    f[0] = l.clamp(-1.0, 1.0);
                    if channels > 1 {
                        f[1] = r.clamp(-1.0, 1.0);
                    }
                    for x in f.iter_mut().skip(2) {
                        *x = 0.0;
                    }
                }
            },
            |e| eprintln!("audio stream error: {e}"),
            None,
        )
        .map_err(|e| e.to_string())?;
    stream.play().map_err(|e| e.to_string())?;
    Ok(stream)
}
