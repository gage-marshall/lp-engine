//! LovePlus+ on the engine: the game boots as the DS boots it and runs at the DS frame rate (59.8261 Hz). The ported
//! flow and native code run natively; what is not ported yet runs as the game's own code (`runtime::interp`).
//! The two screens are shown as the game is held, like a book: the top screen on the left, the touch screen on the
//! right, both upright.
//!
//!   game CONTENT [--save BACKUP.sav] [--scale N] [--res N] [--mute] [--settings PATH] [--audio-ms N] [--smooth] [--cpu] [--mods DIR] [--dump-textures DIR] [--profile OUT.csv] [--tex-sharpness S] [--tex-denoise D] [--bg-sharpness S] [--bg-denoise D] [--no-record]
//!
//! Sound is the game's: its sound commands run on the engine's ARM7 sound driver (`runtime::snd7`), whose output
//! plays on the default device. Time is the game's too: frames run as fast as the DS's clock advances, so a frame
//! the game overruns takes longer, as it does on the DS. Beyond the DS (`runtime::present`): `--res N` draws at N
//! times its resolution (3D and the pictures the game captures of it; 2D art is scaled up), `--smooth` shows
//! in-between pictures of the heroine (30 updates a second instead of 15, 2 VBlanks behind). `--scale` sizes the
//! window. The View menu's settings and the window's size are kept between runs (`Settings`: settings.json in the
//! user's settings folder, or `--settings PATH`); flags given at start override them. The 3D beyond the DS is drawn on the GPU when there is one (`runtime::gpu3d`; `--cpu`: on the CPU,
//! `runtime::raster3d`, the reference). The View menu changes the resolution and smooth motion while playing; the picture keeps the DS's
//! proportions in any window size. View > Enhanced Textures replaces the 3D textures by finer versions (GPU only):
//! a mod's PNG from `--mods DIR` (default `mods/textures` beside the content) named after the texture's key, else
//! FSR 1 at 4x; `--dump-textures DIR` writes every texture seen as `<key>.png`, to start a mod from.
//! Mouse on the right-hand screen: the stylus. Keys: arrows = D-pad, X = A, Z = B, S = X, A = Y, Q = L, W = R,
//! Enter = Start, Backspace = Select, Esc = quit. The battery save is written back to its file when the game saves.
//! Each session is recorded (`Recorder`: the starting save and the controls and clock by game pass) so a problem met
//! while playing can be run again exactly with `game_probe CONTENT N --replay DIR`; `--no-record` turns it off.
use lp_engine::rom::Rom;
use lp_engine::runtime::{present::Present, ram::Ram, raster3d::Look, sdk, Game, NoSubsystems, TICK_HZ};
use std::path::{Path, PathBuf};
use std::collections::VecDeque;
use std::sync::{mpsc, Arc, Mutex};
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
    upscale: muda::CheckMenuItem,
    /// background and texture upscaling presets: sharp, smooth, soft
    bg_style: Vec<muda::CheckMenuItem>,
    tex_style: Vec<muda::CheckMenuItem>,
    linear: muda::CheckMenuItem,
    gpu: muda::CheckMenuItem,
    actual: muda::MenuItem,
    quit: muda::MenuItem,
}

impl Menus {
    fn new(v: &View) -> Menus {
        use muda::{CheckMenuItem, Menu, MenuItem, PredefinedMenuItem, Submenu};
        let acc = |s: &str| s.parse().ok();
        let bar = Menu::new();
        let app = Submenu::new("LovePlus+", true);
        let quit = MenuItem::new("Quit", true, acc("CmdOrCtrl+Q"));
        app.append(&quit).unwrap();
        let view = Submenu::new("View", true);
        let res: Vec<CheckMenuItem> = (1..=4).map(|r| CheckMenuItem::new(format!("{r}x Resolution"), true, r == v.res, acc(&format!("CmdOrCtrl+{r}")))).collect();
        for r in &res {
            view.append(r).unwrap();
        }
        view.append(&PredefinedMenuItem::separator()).unwrap();
        let smooth = CheckMenuItem::new("Smooth Motion (30 fps heroine)", true, v.smooth, acc("CmdOrCtrl+M"));
        view.append(&smooth).unwrap();
        view.append(&PredefinedMenuItem::separator()).unwrap();
        let bilinear = CheckMenuItem::new("Texture Filtering", true, v.bilinear, None);
        let smooth_toon = CheckMenuItem::new("Smooth Toon Shading", true, v.smooth_toon, None);
        let thick = CheckMenuItem::new("Outlines at DS Thickness", true, v.thick, None);
        let aa = CheckMenuItem::new("Anti-aliasing (2x2 3D)", true, v.aa, None);
        let upscale = CheckMenuItem::new("Enhanced Textures (FSR 1, mods)", true, v.upscale, None);
        for i in [&bilinear, &smooth_toon, &thick, &aa, &upscale] {
            view.append(i).unwrap();
        }
        let style = Submenu::new("Background Upscaling", true);
        let bg_style: Vec<CheckMenuItem> = ["Sharp (FSR)", "Smooth (denoised FSR)", "Soft"].iter().enumerate().map(|(i, n)| CheckMenuItem::new(*n, true, i == v.bg_style, None)).collect();
        for i in &bg_style {
            style.append(i).unwrap();
        }
        view.append(&style).unwrap();
        let tstyle = Submenu::new("Texture Upscaling", true);
        let tex_style: Vec<CheckMenuItem> = ["Sharp (FSR)", "Smooth (denoised FSR)", "Soft"].iter().enumerate().map(|(i, n)| CheckMenuItem::new(*n, true, i == v.tex_style, None)).collect();
        for i in &tex_style {
            tstyle.append(i).unwrap();
        }
        view.append(&tstyle).unwrap();
        view.append(&PredefinedMenuItem::separator()).unwrap();
        let linear = CheckMenuItem::new("Smooth Window Scaling", true, v.linear, None);
        view.append(&linear).unwrap();
        view.append(&PredefinedMenuItem::separator()).unwrap();
        let gpu = CheckMenuItem::new("Draw 3D on the GPU", true, v.gpu, None);
        view.append(&gpu).unwrap();
        let actual = MenuItem::new("Actual Size", true, acc("CmdOrCtrl+0"));
        view.append(&actual).unwrap();
        bar.append(&app).unwrap();
        bar.append(&view).unwrap();
        Menus { _bar: bar, res, smooth, bilinear, smooth_toon, thick, aa, upscale, bg_style, tex_style, linear, gpu, actual, quit }
    }
}

/// settings kept between runs: a JSON object in the user's settings folder (`--settings PATH` for another file);
/// each value is read with a default, so a missing or unknown entry never stops the game
struct Settings {
    path: Option<PathBuf>,
    values: serde_json::Map<String, serde_json::Value>,
}

impl Settings {
    /// the per-user settings file: macOS Library/Application Support, Windows %APPDATA%, else ~/.config
    fn default_path() -> Option<PathBuf> {
        let env = |k: &str| std::env::var_os(k).map(PathBuf::from);
        let dir = if cfg!(target_os = "macos") {
            env("HOME")?.join("Library/Application Support/LovePlus+")
        } else if cfg!(target_os = "windows") {
            env("APPDATA")?.join("LovePlus+")
        } else {
            env("XDG_CONFIG_HOME").or_else(|| env("HOME").map(|h| h.join(".config")))?.join("loveplus")
        };
        Some(dir.join("settings.json"))
    }

    fn load(path: Option<PathBuf>) -> Settings {
        let values = path
            .as_ref()
            .and_then(|p| std::fs::read(p).ok())
            .and_then(|b| serde_json::from_slice::<serde_json::Value>(&b).ok())
            .and_then(|v| v.as_object().cloned())
            .unwrap_or_default();
        Settings { path, values }
    }

    fn bool(&self, k: &str, d: bool) -> bool {
        self.values.get(k).and_then(|v| v.as_bool()).unwrap_or(d)
    }
    fn num(&self, k: &str, d: f64) -> f64 {
        self.values.get(k).and_then(|v| v.as_f64()).unwrap_or(d)
    }
    fn set(&mut self, k: &str, v: impl Into<serde_json::Value>) {
        self.values.insert(k.into(), v.into());
    }

    fn save(&self) {
        let Some(p) = &self.path else { return };
        if let Some(d) = p.parent() {
            let _ = std::fs::create_dir_all(d);
        }
        if let Ok(text) = serde_json::to_string_pretty(&self.values) {
            if let Err(e) = std::fs::write(p, text) {
                eprintln!("could not keep the settings: {e}");
            }
        }
    }
}

/// the View menu's settings and the window's size (logical pixels)
#[derive(Clone)]
struct View {
    res: usize,
    smooth: bool,
    bilinear: bool,
    smooth_toon: bool,
    thick: bool,
    aa: bool,
    upscale: bool,
    /// background and texture upscaling: 0 sharp, 1 smooth, 2 soft
    bg_style: usize,
    tex_style: usize,
    linear: bool,
    gpu: bool,
    window: Option<(f64, f64)>,
}

impl View {
    fn load(s: &Settings) -> View {
        let window = match (s.values.get("window_width").and_then(|v| v.as_f64()), s.values.get("window_height").and_then(|v| v.as_f64())) {
            (Some(w), Some(h)) if w >= 64.0 && h >= 64.0 => Some((w, h)),
            _ => None,
        };
        View {
            res: (s.num("resolution", 1.0) as usize).clamp(1, 4),
            smooth: s.bool("smooth_motion", false),
            bilinear: s.bool("texture_filtering", false),
            smooth_toon: s.bool("smooth_toon_shading", false),
            thick: s.bool("outlines_at_ds_thickness", false),
            aa: s.bool("anti_aliasing", false),
            upscale: s.bool("enhanced_textures", false),
            bg_style: (s.num("background_upscaling", 1.0) as usize).min(2),
            tex_style: (s.num("texture_upscaling", 1.0) as usize).min(2),
            linear: s.bool("smooth_window_scaling", false),
            gpu: s.bool("gpu", true),
            window,
        }
    }

    fn store(&self, s: &mut Settings) {
        s.set("resolution", self.res);
        s.set("smooth_motion", self.smooth);
        s.set("texture_filtering", self.bilinear);
        s.set("smooth_toon_shading", self.smooth_toon);
        s.set("outlines_at_ds_thickness", self.thick);
        s.set("anti_aliasing", self.aa);
        s.set("enhanced_textures", self.upscale);
        s.set("background_upscaling", self.bg_style);
        s.set("texture_upscaling", self.tex_style);
        s.set("smooth_window_scaling", self.linear);
        s.set("gpu", self.gpu);
        if let Some((w, h)) = self.window {
            s.set("window_width", w);
            s.set("window_height", h);
        }
    }

    /// the 3D options as the presentation takes them
    fn look(&self) -> Look {
        Look { bilinear: self.bilinear, smooth_toon: self.smooth_toon, outline: if self.thick { self.res } else { 1 }, upscale: self.upscale }
    }
}

/// what the window tells the game thread
#[derive(Default)]
struct Controls {
    keys: u16,
    touch: Option<(u16, u16)>,
    stop: bool,
}

/// between the window and the game thread: the controls, and the time the game took (total, passes)
#[derive(Default)]
struct Shared {
    controls: Mutex<Controls>,
    game_time: Mutex<(Duration, u32)>,
}

/// a finished game pass as the window draws it: the display's state and the capture the pass made
struct Pass {
    ram: Ram,
    capture: Option<u32>,
}

/// the session's record (`recordings/` beside the settings file, the newest few kept): the battery save as the game
/// started from it and everything the game took from outside, by game pass, so `game_probe --replay DIR` runs the
/// same game again pass for pass (the game's logic depends on nothing else). `events.txt`, one line each: `rate HZ`
/// (the sound driver's), `rtc SECS` (the clock at power on), then `PASS keys HEX`, `PASS touch X Y` / `PASS touch -`,
/// `PASS rtc SECS` (local seconds since 1970) before that pass, `PASS fault TEXT` / `PASS reset` after it
struct Recorder {
    out: std::io::LineWriter<std::fs::File>,
    pass: u64,
    keys: u16,
    touch: Option<(u16, u16)>,
    rtc: i64,
}

/// how many sessions' records are kept
const RECORDINGS_KEPT: usize = 8;

impl Recorder {
    fn start(dir: &Path, backup: &[u8], rate: u32, rtc: i64) -> std::io::Result<Recorder> {
        use std::io::Write;
        let utc = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_secs());
        let root = dir.join("recordings");
        std::fs::create_dir_all(&root)?;
        let mut old: Vec<PathBuf> = std::fs::read_dir(&root)?.filter_map(|e| e.ok().map(|e| e.path())).filter(|p| p.join("events.txt").is_file()).collect();
        old.sort();
        while old.len() >= RECORDINGS_KEPT {
            let _ = std::fs::remove_dir_all(old.remove(0));
        }
        let d = root.join(format!("{utc}"));
        std::fs::create_dir_all(&d)?;
        std::fs::write(d.join("start.sav"), backup)?;
        let mut out = std::io::LineWriter::new(std::fs::File::create(d.join("events.txt"))?);
        writeln!(out, "rate {rate}")?;
        writeln!(out, "rtc {rtc}")?;
        eprintln!("recording this session: {}", d.display());
        Ok(Recorder { out, pass: 0, keys: 0, touch: None, rtc })
    }

    /// what the next pass takes from outside, where it changed
    fn before(&mut self, keys: u16, touch: Option<(u16, u16)>, rtc: i64) {
        use std::io::Write;
        let p = self.pass;
        if keys != self.keys {
            let _ = writeln!(self.out, "{p} keys {keys:x}");
        }
        if touch != self.touch {
            let _ = match touch {
                Some((x, y)) => writeln!(self.out, "{p} touch {x} {y}"),
                None => writeln!(self.out, "{p} touch -"),
            };
        }
        if rtc != self.rtc {
            let _ = writeln!(self.out, "{p} rtc {rtc}");
        }
        (self.keys, self.touch, self.rtc) = (keys, touch, rtc);
    }

    /// what the pass ended with
    fn after(&mut self, faults: &[String], reset: bool) {
        use std::io::Write;
        for f in faults {
            let _ = writeln!(self.out, "{} fault {}", self.pass, f.replace('\n', " "));
        }
        if reset {
            let _ = writeln!(self.out, "{} reset", self.pass);
        }
        self.pass += 1;
    }
}

/// the game thread: runs passes as fast as the DS's clock advances against the wall clock (the sound driver renders
/// as the passes run), hands each pass's display state to the window, and writes the battery save
fn run_game(mut game: Game<NoSubsystems>, save_path: Option<PathBuf>, shared: Arc<Shared>, tx: mpsc::SyncSender<Pass>, profile: Option<String>, mut record: Option<Recorder>) {
    let write_save = |game: &mut Game<NoSubsystems>| {
        if let Some(p) = &save_path {
            match std::fs::write(p, &game.m.backup) {
                Ok(()) => game.m.backup_dirty = false,
                Err(e) => eprintln!("could not write the save: {e}"),
            }
        }
    };
    let (mut start, mut tick0) = (Instant::now(), game.m.tick);
    let mut last_save = Instant::now();
    loop {
        let (keys, touch, stop) = {
            let c = shared.controls.lock().unwrap();
            (c.keys, c.touch, c.stop)
        };
        if stop {
            if game.m.backup_dirty {
                write_save(&mut game);
            }
            if let Some(out) = &profile {
                game.m.profile_flush();
                let labels = game.m.rom.content_dir().join("../fid/labels_all.csv");
                lp_engine::runtime::profile::write(&game.m.profile_epochs, &labels, out);
            }
            return;
        }
        let wall = (start.elapsed().as_secs_f64() * TICK_HZ) as u64;
        let mut ran = 0;
        while game.m.tick - tick0 < wall && ran < 8 {
            game.m.input.keys = keys;
            game.m.input.touch = touch;
            let now = sdk::host_local_seconds();
            game.m.rtc = sdk::rtc_from_local_seconds(now);
            if let Some(r) = &mut record {
                r.before(keys, touch, now);
            }
            let t0 = Instant::now();
            let reset = game.frame() != 0;
            if reset {
                eprintln!("the game asked for a reset");
            }
            if let Some(r) = &mut record {
                r.after(&game.m.faults, reset);
            }
            {
                let mut t = shared.game_time.lock().unwrap();
                t.0 += t0.elapsed();
                t.1 += 1;
            }
            for f in game.m.faults.drain(..) {
                eprintln!("game code stopped: {f}");
            }
            // the window takes what it can; a pass it has no room for is not drawn
            let _ = tx.try_send(Pass { ram: game.m.ram.display_copy(), capture: game.m.last_capture });
            ran += 1;
        }
        if ran == 8 {
            // fell far behind: carry on from here rather than spiral (the sound has a gap here)
            eprintln!("fell 8 passes behind the wall clock: carrying on from here");
            start = Instant::now();
            tick0 = game.m.tick;
        }
        if game.m.backup_dirty && last_save.elapsed() > Duration::from_secs(2) {
            write_save(&mut game);
            last_save = Instant::now();
        }
        if ran == 0 {
            std::thread::sleep(Duration::from_millis(1));
        }
    }
}

struct App {
    shared: Arc<Shared>,
    rx: mpsc::Receiver<Pass>,
    game_thread: Option<std::thread::JoinHandle<()>>,
    /// the newest pass, as drawn
    latest: Option<Pass>,
    /// the threads the presentation's parallel work runs on: all cores but two, so the game thread always has one
    /// (upscaling a background on every core starved it: the sound ran dry)
    pool: Arc<rayon::ThreadPool>,
    gpu: Option<Gpu>,
    mouse: (f64, f64),
    pressed: bool,
    keys: u16,
    scale: u32,
    /// what is shown (resolution, in-between pictures)
    present: Present,
    menus: Option<Menus>,
    /// the GPU drawing the 3D beyond the DS (None: the CPU)
    renderer: Option<String>,
    /// the view settings as they started, and where they are kept between runs
    view: View,
    settings: Settings,
    /// time spent per second: drawing (after_pass + composite), draws counted, since
    timing: (Duration, u32, Instant),
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
    fn composite(&mut self) -> Option<Vec<u8>> {
        let (present, ram) = (&mut self.present, &mut self.latest.as_mut()?.ram);
        let (top, bottom) = self.pool.install(|| present.screens(ram));
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
        Some(out)
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

    /// the controls as they are now, for the game thread
    fn send_controls(&self) {
        let touch = if self.pressed { self.touch_at(self.mouse.0, self.mouse.1) } else { None };
        let mut c = self.shared.controls.lock().unwrap();
        c.keys = self.keys;
        c.touch = touch;
    }

    /// the passes the game finished since the last time: each one's captures, and the newest drawn
    fn take_passes(&mut self) -> bool {
        let mut any = false;
        while let Ok(p) = self.rx.try_recv() {
            let t0 = Instant::now();
            let present = &mut self.present;
            self.pool.install(|| present.after_pass(&p.ram, p.capture));
            self.timing.0 += t0.elapsed();
            self.latest = Some(p);
            any = true;
        }
        any
    }

    fn draw(&mut self) {
        let t0 = Instant::now();
        let Some(img) = self.composite() else { return };
        self.timing.0 += t0.elapsed();
        self.timing.1 += 1;
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
        self.save_view();
        // the game thread writes the save and stops
        self.shared.controls.lock().unwrap().stop = true;
        if let Some(t) = self.game_thread.take() {
            let _ = t.join();
        }
        el.exit();
    }

    fn menu_event(&mut self, el: &ActiveEventLoop, id: &muda::MenuId) {
        self.handle_menu(el, id);
        self.save_view();
    }

    /// the view settings as the menu shows them now, and the window's size, kept for the next run
    fn save_view(&mut self) {
        let Some(m) = &self.menus else { return };
        let window = self.gpu.as_ref().map(|g| {
            let l = g.window.inner_size().to_logical::<f64>(g.window.scale_factor());
            (l.width, l.height)
        });
        let v = View {
            res: m.res.iter().position(|r| r.is_checked()).map_or(1, |i| i + 1),
            smooth: m.smooth.is_checked(),
            bilinear: m.bilinear.is_checked(),
            smooth_toon: m.smooth_toon.is_checked(),
            thick: m.thick.is_checked(),
            aa: m.aa.is_checked(),
            upscale: m.upscale.is_checked(),
            bg_style: m.bg_style.iter().position(|r| r.is_checked()).unwrap_or(1),
            tex_style: m.tex_style.iter().position(|r| r.is_checked()).unwrap_or(1),
            linear: m.linear.is_checked(),
            gpu: m.gpu.is_checked(),
            window: window.or(self.view.window),
        };
        v.store(&mut self.settings);
        self.settings.save();
    }

    fn handle_menu(&mut self, el: &ActiveEventLoop, id: &muda::MenuId) {
        let Some(m) = &self.menus else { return };
        if id == m.quit.id() {
            self.quit(el);
        } else if id == m.smooth.id() {
            self.present.smooth = m.smooth.is_checked();
        } else if let Some(i) = m.bg_style.iter().position(|r| id == r.id()) {
            for (k, r) in m.bg_style.iter().enumerate() {
                r.set_checked(k == i);
            }
            let p = self.present.params().bg_preset(i);
            self.present.set_params(p);
        } else if let Some(i) = m.tex_style.iter().position(|r| id == r.id()) {
            for (k, r) in m.tex_style.iter().enumerate() {
                r.set_checked(k == i);
            }
            let p = self.present.params().tex_preset(i);
            self.present.set_params(p);
        } else if id == m.gpu.id() {
            let want = m.gpu.is_checked();
            self.renderer = self.present.use_gpu(want);
            if want && self.renderer.is_none() {
                m.gpu.set_checked(false);
            }
        } else if id == m.linear.id() {
            let (linear, size) = (m.linear.is_checked(), self.size());
            if let Some(g) = &mut self.gpu {
                g.set_filter(linear, size);
            }
        } else if [m.bilinear.id(), m.smooth_toon.id(), m.thick.id(), m.aa.id(), m.upscale.id()].contains(&id) {
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
        let (draw, draws, since) = self.timing;
        if since.elapsed() < Duration::from_secs(1) || draws == 0 {
            return;
        }
        let (game, passes) = std::mem::take(&mut *self.shared.game_time.lock().unwrap());
        if passes == 0 {
            return;
        }
        if let Some(g) = &self.gpu {
            let ms = |d: Duration, n: u32| d.as_secs_f64() * 1000.0 / n as f64;
            let r = if self.renderer.is_some() { "GPU" } else { "CPU" };
            g.window.set_title(&format!("LovePlus+  ({}x, 3D on {r})  game {:.1} ms/pass, display {:.1} ms/frame, {} frames/s", self.present.scale, ms(game, passes), ms(draw, draws), draws));
        }
        self.timing = (Duration::ZERO, 0, Instant::now());
    }

    /// the menu's 3D options into the presentation (outlines at DS thickness: as many pixels as the resolution)
    fn apply_options(&mut self) {
        let Some(m) = &self.menus else { return };
        let scale = self.present.scale;
        let look = Look { bilinear: m.bilinear.is_checked(), smooth_toon: m.smooth_toon.is_checked(), outline: if m.thick.is_checked() { scale } else { 1 }, upscale: m.upscale.is_checked() };
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
        let size = self.view.window.unwrap_or(((CW * self.scale) as f64, (CH * self.scale) as f64));
        let attrs = Window::default_attributes().with_title("LovePlus+").with_inner_size(winit::dpi::LogicalSize::new(size.0, size.1));
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
            let m = Menus::new(&View { gpu: self.renderer.is_some(), ..self.view.clone() });
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
        if self.view.linear {
            let size = self.size();
            if let Some(g) = &mut self.gpu {
                g.set_filter(true, size);
            }
        }
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
        self.send_controls();
        if self.take_passes() {
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
        eprintln!("usage: game CONTENT [--save BACKUP.sav] [--scale N] [--res N] [--mute] [--settings PATH] [--audio-ms N] [--smooth] [--cpu] [--mods DIR] [--dump-textures DIR] [--profile OUT.csv] [--tex-sharpness S] [--tex-denoise D] [--bg-sharpness S] [--bg-denoise D]");
        std::process::exit(1);
    }
    let rom = Rom::open(Path::new(&a[1])).unwrap_or_else(|e| {
        eprintln!("content: {e}");
        std::process::exit(1)
    });
    let mut game = Game::new(rom, NoSubsystems);
    // --profile OUT.csv: which of the game's code ran (attributed to functions on quit; fid/labels_all.csv)
    let profile = a.iter().position(|x| x == "--profile").and_then(|i| a.get(i + 1)).cloned();
    if profile.is_some() {
        game.m.profile = Some(vec![0; lp_engine::arm::PROFILE_LEN]);
    }
    if !game.m.ram.vrom.patched.is_empty() {
        eprintln!("patch overlay: {} file(s) replaced: {:?}", game.m.ram.vrom.patched.len(), game.m.ram.vrom.patched);
    }
    let value = |flag: &str| {
        a.iter().position(|x| x == flag).map(|i| {
            a.get(i + 1).filter(|v| !v.starts_with("--")).cloned().unwrap_or_else(|| {
                eprintln!("{flag} needs a value");
                std::process::exit(1)
            })
        })
    };
    // without --save the battery save lives in saves/loveplus.sav beside the content (kept between runs)
    let save_path = value("--save").map(PathBuf::from).or_else(|| {
        let d = Path::new(&a[1]).canonicalize().ok()?.parent()?.join("saves");
        std::fs::create_dir_all(&d).ok()?;
        Some(d.join("loveplus.sav"))
    });
    if let Some(p) = &save_path {
        eprintln!("battery save: {}", p.display());
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
        let ms = value("--audio-ms").map(|v| v.parse().unwrap_or_else(|_| {
            eprintln!("--audio-ms needs a number");
            std::process::exit(1)
        })).unwrap_or(AUDIO_CUSHION_MS);
        match start_audio(&mut game, ms) {
            Ok(s) => Some(s),
            Err(e) => {
                eprintln!("no sound: {e}");
                None
            }
        }
    };
    let boot_rtc = sdk::host_local_seconds();
    game.m.rtc = sdk::rtc_from_local_seconds(boot_rtc);
    // the session's record (--no-record: none), from the save as it is before the game touches it
    let record = if a.iter().any(|x| x == "--no-record") {
        None
    } else {
        value("--settings").map(PathBuf::from).or_else(Settings::default_path).and_then(|p| Recorder::start(p.parent()?, &game.m.backup, game.m.snd.rate, boot_rtc).map_err(|e| eprintln!("no recording: {e}")).ok())
    };
    for f in game.power_on() {
        eprintln!("power on stopped: {f}");
    }
    let el = EventLoop::new().unwrap();
    // the view as it was left (settings), flags given now overriding it
    let settings = Settings::load(value("--settings").map(PathBuf::from).or_else(Settings::default_path));
    let mut view = View::load(&settings);
    if let Some(r) = value("--res") {
        view.res = r.parse().unwrap_or_else(|_| {
            eprintln!("--res needs a number");
            std::process::exit(1)
        });
    }
    view.smooth |= a.iter().any(|x| x == "--smooth");
    if a.iter().any(|x| x == "--cpu") {
        view.gpu = false;
    }
    let mut present = Present::new(view.res, view.smooth);
    present.set(view.res, view.look(), view.aa);
    // the patch overlay's finer pictures (default: patch/hires beside the content; textures/, planes/) and dumping
    // the textures and planes seen in the same layout, named by the asset index (`asset_index CONTENT` builds it)
    let mods = value("--mods").map(PathBuf::from).or_else(|| Some(Path::new(&a[1]).join("../patch/hires")).filter(|p| p.is_dir()));
    present.set_textures(mods, value("--dump-textures").map(PathBuf::from));
    let names = lp_engine::assetindex::load_texture_names(Path::new(&a[1]));
    if names.is_empty() {
        eprintln!("no asset index (content/index/textures.json): dumps and mods use keys; `asset_index CONTENT` builds it");
    }
    present.set_names(names);
    present.set_plane_names(lp_engine::assetindex::load_plane_names(Path::new(&a[1])));
    // the enhancement's look: --tex-sharpness / --bg-sharpness (RCAS stops, higher softer), --bg-denoise (0..1)
    let num = |f: &str, d: f32| value(f).map(|v| v.parse().unwrap_or_else(|_| {
        eprintln!("{f} needs a number");
        std::process::exit(1)
    })).unwrap_or(d);
    let d = lp_engine::runtime::texcache::Params::default().bg_preset(view.bg_style).tex_preset(view.tex_style);
    present.set_params(lp_engine::runtime::texcache::Params { tex_sharpness: num("--tex-sharpness", d.tex_sharpness), tex_denoise: num("--tex-denoise", d.tex_denoise), bg_sharpness: num("--bg-sharpness", d.bg_sharpness), bg_denoise: num("--bg-denoise", d.bg_denoise) });
    // the 3D beyond the DS on the GPU when there is one (--cpu: the CPU)
    let renderer = if view.gpu { present.use_gpu(true) } else { None };
    match &renderer {
        Some(n) => eprintln!("3D beyond the DS on the GPU: {n}"),
        None => eprintln!("3D beyond the DS on the CPU"),
    }
    // the game runs on its own thread; the window draws the passes it finishes (a few may queue while the window is
    // busy with a capture's finer pictures)
    let shared = Arc::new(Shared::default());
    let (tx, rx) = mpsc::sync_channel(8);
    let game_thread = {
        let shared = shared.clone();
        std::thread::Builder::new().name("game".into()).spawn(move || run_game(game, save_path, shared, tx, profile, record)).unwrap()
    };
    let cores = std::thread::available_parallelism().map_or(4, |n| n.get());
    let pool = Arc::new(rayon::ThreadPoolBuilder::new().num_threads(cores.saturating_sub(2).max(1)).thread_name(|i| format!("present {i}")).build().unwrap());
    let mut app = App { shared, rx, game_thread: Some(game_thread), latest: None, pool, gpu: None, mouse: (0.0, 0.0), pressed: false, keys: 0, scale, present, menus: None, renderer, view, settings, timing: (Duration::ZERO, 0, Instant::now()) };
    el.run_app(&mut app).unwrap();
}

/// how much sound is queued ahead of the device by default (milliseconds; `--audio-ms`)
const AUDIO_CUSHION_MS: u32 = 150;

/// the default output device plays what the sound driver renders (it renders at the device's rate)
fn start_audio(game: &mut Game<NoSubsystems>, cushion_ms: u32) -> Result<cpal::Stream, String> {
    use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
    let dev = cpal::default_host().default_output_device().ok_or("no output device")?;
    let cfg = dev.default_output_config().map_err(|e| e.to_string())?;
    let channels = cfg.channels() as usize;
    let sink = Arc::new(Mutex::new(VecDeque::<f32>::new()));
    game.m.snd.rate = cfg.sample_rate().0;
    game.m.snd.sink = Some(sink.clone());
    // a cushion of queued sound: playing starts (and, after running dry, restarts) once this much is queued, so a
    // game pass that takes longer than its time (presentation work, a slow frame) does not cut the sound
    let cushion = (cfg.sample_rate().0 as u64 * cushion_ms as u64 / 1000) as usize * 2;
    let mut playing = false;
    // underruns are reported (the time since the last one tells a stall from a game that falls behind)
    let mut last_underrun: Option<Instant> = None;
    let started = Instant::now();
    let stream = dev
        .build_output_stream(
            &cfg.into(),
            move |data: &mut [f32], _| {
                let mut q = sink.lock().unwrap();
                if !playing && q.len() >= cushion {
                    playing = true;
                }
                // the device's clock runs a little slower than the game's: keep the queue near the cushion
                while q.len() > 4 * cushion {
                    q.pop_front();
                    q.pop_front();
                }
                for f in data.chunks_mut(channels) {
                    let (l, r) = if playing && q.len() >= 2 {
                        (q.pop_front().unwrap(), q.pop_front().unwrap())
                    } else {
                        if playing {
                            let now = Instant::now();
                            match last_underrun {
                                Some(t) => eprintln!("audio ran dry at {:.1} s ({:.1} s after the last time)", (now - started).as_secs_f64(), (now - t).as_secs_f64()),
                                None => eprintln!("audio ran dry at {:.1} s", (now - started).as_secs_f64()),
                            }
                            last_underrun = Some(now);
                        }
                        playing = false;
                        (0.0, 0.0)
                    };
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
