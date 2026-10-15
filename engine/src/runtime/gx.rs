//! The DS geometry engine as the game drives it: commands arrive packed through the GX FIFO (0x4000400, fed by
//! DMA or CPU writes) or one by one through the command ports (0x4000440 + 4 * command). The engine keeps the matrix
//! stacks, the current vertex attributes, builds polygons in clip space and, at SWAP_BUFFERS, hands the frame's
//! polygon list to the renderer. Tests (box / position / vector) and the current clip / vector matrices are
//! readable as on the hardware (`read`), since the game's code uses them (NitroSystem culls with box tests).
//!
//! Arithmetic is in f64 (the hardware's fixed-point formats are converted on input); what is drawn is the game's.

pub type M4 = [[f64; 4]; 4];

pub const IDENT: M4 = [[1.0, 0.0, 0.0, 0.0], [0.0, 1.0, 0.0, 0.0], [0.0, 0.0, 1.0, 0.0], [0.0, 0.0, 0.0, 1.0]];

/// row vector times matrix, the DS's convention (v' = v * M)
fn mul(a: &M4, b: &M4) -> M4 {
    let mut r = [[0.0; 4]; 4];
    for i in 0..4 {
        for j in 0..4 {
            r[i][j] = (0..4).map(|k| a[i][k] * b[k][j]).sum();
        }
    }
    r
}
fn apply(v: [f64; 4], m: &M4) -> [f64; 4] {
    let mut r = [0.0; 4];
    for j in 0..4 {
        r[j] = (0..4).map(|k| v[k] * m[k][j]).sum();
    }
    r
}

/// a vertex of a finished polygon: clip-space position, colour (0..31 per channel), texture coordinate (texels)
#[derive(Clone, Copy, Debug, Default)]
pub struct Vert {
    pub clip: [f64; 4],
    pub color: [f64; 3],
    pub uv: [f64; 2],
}

/// a polygon as it was submitted: its vertices and the attributes in force
#[derive(Clone, Debug)]
pub struct Poly {
    pub verts: Vec<Vert>,
    /// POLYGON_ATTR (light enables, mode, culling, depth / fog flags, alpha, polygon id)
    pub attr: u32,
    /// TEXIMAGE_PARAM
    pub tex: u32,
    /// PLTT_BASE
    pub pal: u32,
}

/// a swapped frame: the polygons and the SWAP_BUFFERS parameter (bit 0 translucent sort, bit 1 w-buffering)
#[derive(Clone, Debug, Default)]
pub struct Frame {
    pub polys: Vec<Poly>,
    pub swap: u32,
    pub viewport: (u32, u32, u32, u32),
}

#[derive(Clone)]
pub struct Gx {
    mode: u32,
    proj: M4,
    proj_stack: M4,
    pos: M4,
    vec: M4,
    pos_stack: [M4; 32],
    vec_stack: [M4; 32],
    sp: usize,
    tex_mtx: M4,
    tex_stack: M4,
    /// stack overflow / underflow (GXSTAT bit 15)
    pub stack_error: bool,
    // current vertex state
    color: [f64; 3],
    uv: [f64; 2],
    raw_uv: [i16; 2],
    last: [f64; 3],
    poly_attr: u32,
    poly_attr_pending: u32,
    tex: u32,
    pal: u32,
    dif: [f64; 3],
    amb: [f64; 3],
    spe: [f64; 3],
    emi: [f64; 3],
    use_shine: bool,
    lights: [([f64; 3], [f64; 3]); 4],
    shine: [u8; 128],
    // the primitive being built
    prim: u32,
    strip: Vec<Vert>,
    strip_n: usize,
    // the frame under construction / the last swapped one
    polys: Vec<Poly>,
    pub frame: Frame,
    pub frames: u64,
    viewport: (u32, u32, u32, u32),
    // FIFO packing
    pend: Vec<u8>,
    params: Vec<u32>,
    port_mode: bool,
    // test results
    pub box_result: bool,
    /// counters (diagnostics): FIFO words, port writes, vertices, commands by id
    pub n_fifo: u64,
    pub n_port: u64,
    pub n_vtx: u64,
    pub n_cmd: [u64; 128],
    pub pos_result: [i32; 4],
    pub vec_result: [i16; 3],
}

impl Default for Gx {
    fn default() -> Self {
        Gx {
            mode: 0,
            proj: IDENT,
            proj_stack: IDENT,
            pos: IDENT,
            vec: IDENT,
            pos_stack: [IDENT; 32],
            vec_stack: [IDENT; 32],
            sp: 0,
            tex_mtx: IDENT,
            tex_stack: IDENT,
            stack_error: false,
            color: [31.0; 3],
            uv: [0.0; 2],
            raw_uv: [0; 2],
            last: [0.0; 3],
            poly_attr: 0,
            poly_attr_pending: 0,
            tex: 0,
            pal: 0,
            dif: [0.0; 3],
            amb: [0.0; 3],
            spe: [0.0; 3],
            emi: [0.0; 3],
            use_shine: false,
            lights: [([0.0; 3], [0.0; 3]); 4],
            shine: [0; 128],
            prim: 0,
            strip: Vec::new(),
            strip_n: 0,
            polys: Vec::new(),
            frame: Frame::default(),
            frames: 0,
            viewport: (0, 0, 255, 191),
            pend: Vec::new(),
            params: Vec::new(),
            port_mode: false,
            box_result: true,
            n_fifo: 0,
            n_port: 0,
            n_vtx: 0,
            n_cmd: [0; 128],
            pos_result: [0; 4],
            vec_result: [0; 3],
        }
    }
}

/// parameters each command takes
pub fn param_count(cmd: u8) -> usize {
    match cmd {
        0x10 | 0x12 | 0x13 | 0x14 => 1,
        0x11 | 0x15 | 0x41 => 0,
        0x16 | 0x18 => 16,
        0x17 | 0x19 => 12,
        0x1a => 9,
        0x1b | 0x1c => 3,
        0x20..=0x22 | 0x24..=0x2b | 0x30..=0x33 | 0x40 | 0x50 | 0x60 | 0x72 => 1,
        0x23 | 0x71 => 2,
        0x34 => 32,
        0x70 => 3,
        _ => 0,
    }
}

fn fx12(v: u32) -> f64 {
    v as i32 as f64 / 4096.0
}
fn s16_12(v: u32) -> f64 {
    v as u16 as i16 as f64 / 4096.0
}
fn s10_6(v: u32) -> f64 {
    (((v & 0x3ff) << 22) as i32 >> 22) as f64 / 64.0
}
fn s10_9(v: u32) -> f64 {
    (((v & 0x3ff) << 22) as i32 >> 22) as f64 / 512.0
}
fn rgb(v: u32) -> [f64; 3] {
    [(v & 0x1f) as f64, ((v >> 5) & 0x1f) as f64, ((v >> 10) & 0x1f) as f64]
}

impl Gx {
    /// a word written to the GX FIFO (0x4000400): a packed command word, or the next parameter
    pub fn fifo(&mut self, w: u32) {
        self.n_fifo += 1;
        if self.port_mode {
            self.pend.clear();
            self.params.clear();
            self.port_mode = false;
        }
        if self.pend.is_empty() {
            let mut cmds: Vec<u8> = (0..4).map(|i| (w >> (8 * i)) as u8).collect();
            while cmds.last() == Some(&0) {
                cmds.pop(); // trailing NOPs
            }
            if cmds.is_empty() {
                return;
            }
            self.pend = cmds;
            self.params.clear();
            self.run_ready();
        } else {
            self.params.push(w);
            self.run_ready();
        }
    }

    /// run the pending packed commands whose parameters have all arrived
    fn run_ready(&mut self) {
        while let Some(&c) = self.pend.first() {
            let n = param_count(c);
            if self.params.len() < n {
                return;
            }
            let p: Vec<u32> = self.params.drain(..n).collect();
            self.pend.remove(0);
            self.exec(c, &p);
        }
    }

    /// a write to a command port (0x4000440 + 4 * cmd): parameters accumulate until the command has them all
    pub fn port(&mut self, cmd: u8, v: u32) {
        self.n_port += 1;
        let n = param_count(cmd);
        if n == 0 {
            self.exec(cmd, &[]);
            return;
        }
        if self.pend.first() != Some(&cmd) || !self.port_mode {
            self.pend.clear();
            self.pend.push(cmd);
            self.params.clear();
            self.port_mode = true;
        }
        self.params.push(v);
        if self.params.len() >= n {
            let p: Vec<u32> = self.params.drain(..).collect();
            self.pend.clear();
            self.port_mode = false;
            self.exec(cmd, &p);
        }
    }

    /// multiply the current matrix (and the vector matrix in mode 2) by m on the left
    fn mult(&mut self, m: M4, vec_too: bool) {
        match self.mode {
            0 => self.proj = mul(&m, &self.proj),
            3 => self.tex_mtx = mul(&m, &self.tex_mtx),
            1 => self.pos = mul(&m, &self.pos),
            _ => {
                self.pos = mul(&m, &self.pos);
                if vec_too {
                    self.vec = mul(&m, &self.vec);
                }
            }
        }
    }

    fn load(&mut self, m: M4) {
        match self.mode {
            0 => self.proj = m,
            3 => self.tex_mtx = m,
            1 => self.pos = m,
            _ => {
                self.pos = m;
                self.vec = m;
            }
        }
    }

    pub fn clip_matrix(&self) -> M4 {
        mul(&self.pos, &self.proj)
    }
    pub fn vec_matrix(&self) -> M4 {
        self.vec
    }

    fn exec(&mut self, c: u8, p: &[u32]) {
        self.n_cmd[(c & 0x7f) as usize] += 1;
        match c {
            0x10 => self.mode = p[0] & 3,
            0x11 => match self.mode {
                0 => self.proj_stack = self.proj,
                3 => self.tex_stack = self.tex_mtx,
                _ => {
                    if self.sp >= 31 {
                        self.stack_error = true;
                    }
                    let i = self.sp.min(31);
                    self.pos_stack[i] = self.pos;
                    self.vec_stack[i] = self.vec;
                    self.sp = (self.sp + 1).min(31);
                }
            },
            0x12 => match self.mode {
                0 => self.proj = self.proj_stack,
                3 => self.tex_mtx = self.tex_stack,
                _ => {
                    let n = (((p[0] & 0x3f) << 26) as i32 >> 26) as isize;
                    let sp = self.sp as isize - n;
                    if !(0..=31).contains(&sp) {
                        self.stack_error = true;
                    }
                    self.sp = sp.clamp(0, 31) as usize;
                    self.pos = self.pos_stack[self.sp];
                    self.vec = self.vec_stack[self.sp];
                }
            },
            0x13 => match self.mode {
                0 => self.proj_stack = self.proj,
                3 => self.tex_stack = self.tex_mtx,
                _ => {
                    let i = (p[0] & 0x1f) as usize;
                    self.pos_stack[i] = self.pos;
                    self.vec_stack[i] = self.vec;
                }
            },
            0x14 => match self.mode {
                0 => self.proj = self.proj_stack,
                3 => self.tex_mtx = self.tex_stack,
                _ => {
                    let i = (p[0] & 0x1f) as usize;
                    self.pos = self.pos_stack[i];
                    self.vec = self.vec_stack[i];
                }
            },
            0x15 => self.load(IDENT),
            0x16 => {
                let mut m = [[0.0; 4]; 4];
                for i in 0..16 {
                    m[i / 4][i % 4] = fx12(p[i]);
                }
                self.load(m);
            }
            0x17 => {
                let mut m = IDENT;
                for i in 0..12 {
                    m[i / 3][i % 3] = fx12(p[i]);
                }
                self.load(m);
            }
            0x18 => {
                let mut m = [[0.0; 4]; 4];
                for i in 0..16 {
                    m[i / 4][i % 4] = fx12(p[i]);
                }
                self.mult(m, true);
            }
            0x19 => {
                let mut m = IDENT;
                for i in 0..12 {
                    m[i / 3][i % 3] = fx12(p[i]);
                }
                self.mult(m, true);
            }
            0x1a => {
                let mut m = IDENT;
                for i in 0..9 {
                    m[i / 3][i % 3] = fx12(p[i]);
                }
                self.mult(m, true);
            }
            0x1b => {
                let mut m = IDENT;
                for i in 0..3 {
                    m[i][i] = fx12(p[i]);
                }
                self.mult(m, false); // scaling leaves the vector matrix alone
            }
            0x1c => {
                let mut m = IDENT;
                for i in 0..3 {
                    m[3][i] = fx12(p[i]);
                }
                self.mult(m, true);
            }
            0x20 => self.color = rgb(p[0]),
            0x21 => self.normal(p[0]),
            0x22 => {
                self.raw_uv = [p[0] as u16 as i16, (p[0] >> 16) as u16 as i16];
                let (s, t) = (self.raw_uv[0] as f64 / 16.0, self.raw_uv[1] as f64 / 16.0);
                self.uv = if (self.tex >> 30) == 1 {
                    // texcoord transform mode 1: by the texture matrix
                    let r = apply([s, t, 1.0 / 16.0, 1.0 / 16.0], &self.tex_mtx);
                    [r[0], r[1]]
                } else {
                    [s, t]
                };
            }
            0x23 => self.vertex([s16_12(p[0]), s16_12(p[0] >> 16), s16_12(p[1])]),
            0x24 => self.vertex([s10_6(p[0]), s10_6(p[0] >> 10), s10_6(p[0] >> 20)]),
            0x25 => self.vertex([s16_12(p[0]), s16_12(p[0] >> 16), self.last[2]]),
            0x26 => self.vertex([s16_12(p[0]), self.last[1], s16_12(p[0] >> 16)]),
            0x27 => self.vertex([self.last[0], s16_12(p[0]), s16_12(p[0] >> 16)]),
            0x28 => {
                let d = |v: u32| s10_9(v) / 8.0; // 10-bit signed, 1/8 of the VTX_16 unit
                self.vertex([self.last[0] + d(p[0]), self.last[1] + d(p[0] >> 10), self.last[2] + d(p[0] >> 20)]);
            }
            0x29 => self.poly_attr_pending = p[0],
            0x2a => self.tex = p[0],
            0x2b => self.pal = p[0] & 0x1fff,
            0x30 => {
                self.dif = rgb(p[0]);
                self.amb = rgb(p[0] >> 16);
                if p[0] & 0x8000 != 0 {
                    self.color = self.dif;
                }
            }
            0x31 => {
                self.spe = rgb(p[0]);
                self.emi = rgb(p[0] >> 16);
                self.use_shine = p[0] & 0x8000 != 0;
            }
            0x32 => {
                let l = (p[0] >> 30) as usize;
                let v = [s10_9(p[0]), s10_9(p[0] >> 10), s10_9(p[0] >> 20)];
                let r = apply([v[0], v[1], v[2], 0.0], &self.vec);
                self.lights[l].0 = [r[0], r[1], r[2]];
            }
            0x33 => self.lights[(p[0] >> 30) as usize].1 = rgb(p[0]),
            0x34 => {
                for (i, w) in p.iter().enumerate() {
                    for k in 0..4 {
                        self.shine[4 * i + k] = (w >> (8 * k)) as u8;
                    }
                }
            }
            0x40 => {
                self.prim = p[0] & 3;
                self.strip.clear();
                self.strip_n = 0;
                self.poly_attr = self.poly_attr_pending;
            }
            0x41 => {}
            0x50 => {
                self.frame = Frame { polys: std::mem::take(&mut self.polys), swap: p[0], viewport: self.viewport };
                self.frames += 1;
            }
            0x60 => self.viewport = (p[0] & 0xff, (p[0] >> 8) & 0xff, (p[0] >> 16) & 0xff, (p[0] >> 24) & 0xff),
            0x70 => self.box_test(p),
            0x71 => {
                let v = [s16_12(p[0]), s16_12(p[0] >> 16), s16_12(p[1]), 1.0];
                let r = apply(v, &self.clip_matrix());
                self.pos_result = r.map(|x| (x * 4096.0) as i32);
            }
            0x72 => {
                let v = [s10_9(p[0]), s10_9(p[0] >> 10), s10_9(p[0] >> 20), 0.0];
                let r = apply(v, &self.vec);
                self.vec_result = [r[0], r[1], r[2]].map(|x| (x * 4096.0).clamp(-32768.0, 32767.0) as i16);
            }
            _ => {}
        }
    }

    /// lighting at a NORMAL command (the vertex colour becomes the lit colour)
    fn normal(&mut self, v: u32) {
        let n = [s10_9(v), s10_9(v >> 10), s10_9(v >> 20)];
        if (self.tex >> 30) == 2 {
            // texcoord transform mode 2: by the normal
            let r = apply([n[0], n[1], n[2], 1.0 / 16.0], &self.tex_mtx);
            self.uv = [r[0] + self.raw_uv[0] as f64 / 16.0, r[1] + self.raw_uv[1] as f64 / 16.0];
        }
        let t = apply([n[0], n[1], n[2], 0.0], &self.vec);
        let n = [t[0], t[1], t[2]];
        let mut c = self.emi;
        for (i, (dir, col)) in self.lights.iter().enumerate() {
            if self.poly_attr_pending & (1 << i) == 0 && self.poly_attr & (1 << i) == 0 {
                continue;
            }
            let ndl = -(dir[0] * n[0] + dir[1] * n[1] + dir[2] * n[2]);
            let dif = ndl.clamp(0.0, 1.0);
            // half vector with the line of sight (0, 0, -1)
            let h = [(dir[0]) / 2.0, (dir[1]) / 2.0, (dir[2] - 1.0) / 2.0];
            let mut spec = (-(h[0] * n[0] + h[1] * n[1] + h[2] * n[2])).clamp(0.0, 1.0);
            spec = (spec * spec).min(1.0);
            if self.use_shine {
                spec = self.shine[((spec * 127.0) as usize).min(127)] as f64 / 255.0;
            }
            for k in 0..3 {
                c[k] += (self.spe[k] * col[k] * spec + self.dif[k] * col[k] * dif) / 31.0 + self.amb[k] * col[k] / 31.0;
            }
        }
        self.color = c.map(|x| x.min(31.0));
    }

    fn vertex(&mut self, p: [f64; 3]) {
        self.n_vtx += 1;
        self.last = p;
        let clip = apply([p[0], p[1], p[2], 1.0], &self.clip_matrix());
        let uv = if (self.tex >> 30) == 3 {
            // texcoord transform mode 3: by the vertex
            let r = apply([p[0], p[1], p[2], 1.0], &self.tex_mtx);
            [r[0] + self.raw_uv[0] as f64 / 16.0, r[1] + self.raw_uv[1] as f64 / 16.0]
        } else {
            self.uv
        };
        let v = Vert { clip, color: self.color, uv };
        self.strip.push(v);
        self.strip_n += 1;
        let poly = |verts: Vec<Vert>, s: &Self| Poly { verts, attr: s.poly_attr, tex: s.tex, pal: s.pal };
        match self.prim {
            0 if self.strip.len() == 3 => {
                let v = std::mem::take(&mut self.strip);
                self.polys.push(poly(v, self));
            }
            1 if self.strip.len() == 4 => {
                let v = std::mem::take(&mut self.strip);
                self.polys.push(poly(v, self));
            }
            2 if self.strip.len() == 3 => {
                // triangle strip: odd triangles reverse their winding
                let s = &self.strip;
                let v = if self.strip_n % 2 == 0 { vec![s[1], s[0], s[2]] } else { vec![s[0], s[1], s[2]] };
                self.polys.push(poly(v, self));
                self.strip.remove(0);
            }
            3 if self.strip.len() == 4 => {
                let s = &self.strip;
                let v = vec![s[0], s[1], s[3], s[2]];
                self.polys.push(poly(v, self));
                self.strip.drain(..2);
            }
            _ => {}
        }
    }

    /// BOX_TEST: is any part of the box inside the view volume (clip space, -w..w)?
    fn box_test(&mut self, p: &[u32]) {
        let (x, y, z) = (s16_12(p[0]), s16_12(p[0] >> 16), s16_12(p[1]));
        let (w, h, d) = (s16_12(p[1] >> 16), s16_12(p[2]), s16_12(p[2] >> 16));
        let m = self.clip_matrix();
        let corners: Vec<[f64; 4]> = (0..8).map(|i| apply([x + if i & 1 != 0 { w } else { 0.0 }, y + if i & 2 != 0 { h } else { 0.0 }, z + if i & 4 != 0 { d } else { 0.0 }, 1.0], &m)).collect();
        // outside when all corners are beyond the same clip plane
        let mut inside = true;
        for axis in 0..3 {
            if corners.iter().all(|c| c[axis] > c[3]) || corners.iter().all(|c| c[axis] < -c[3]) {
                inside = false;
            }
        }
        self.box_result = inside;
    }

    /// a word of the readable geometry registers: GXSTAT test bit, results, the clip and vector matrices
    pub fn read(&self, a: u32) -> Option<u32> {
        Some(match a {
            0x4000620..=0x400062c => self.pos_result[((a - 0x4000620) / 4) as usize] as u32,
            0x4000630 => (self.vec_result[0] as u16 as u32) | (self.vec_result[1] as u16 as u32) << 16,
            0x4000634 => self.vec_result[2] as u16 as u32,
            0x4000640..=0x400067c => {
                let i = ((a - 0x4000640) / 4) as usize;
                let m = self.clip_matrix();
                (m[i / 4][i % 4] * 4096.0).round() as i64 as u32
            }
            0x4000680..=0x40006a0 => {
                let i = ((a - 0x4000680) / 4) as usize;
                (self.vec[i / 3][i % 3] * 4096.0).round() as i64 as u32
            }
            _ => return None,
        })
    }
}
