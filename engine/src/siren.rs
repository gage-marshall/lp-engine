//! Voice decoder: the game's G.722.1 / G.722.1C (Siren14) fixed-point decoder, ported from tools/siren14dec.c (which is bit-exact against the
//! game's own decoder run in an emulator).  The lookup tables are read from `content/engine/g7221.pack` (dumped from the user's ARM9 image by
//! the extractor), so no table data lives in this repository.

const BASE: u32 = 0x200_0000;

/// table memory: the pack's address ranges laid out at their game addresses
pub struct Tables {
    img: Vec<u8>,
}

impl Tables {
    /// LPTB pack: "LPTB", u32 count, count x (addr, len), then the data of every range in order
    pub fn from_pack(raw: &[u8]) -> Result<Tables, String> {
        if raw.get(0..4) != Some(b"LPTB") {
            return Err("not an LPTB table pack".into());
        }
        let n = u32::from_le_bytes([raw[4], raw[5], raw[6], raw[7]]) as usize;
        let mut ranges = Vec::new();
        for i in 0..n {
            let o = 8 + i * 8;
            let a = u32::from_le_bytes([raw[o], raw[o + 1], raw[o + 2], raw[o + 3]]);
            let l = u32::from_le_bytes([raw[o + 4], raw[o + 5], raw[o + 6], raw[o + 7]]);
            ranges.push((a, l));
        }
        let maxend = ranges.iter().map(|&(a, l)| a + l).max().ok_or("empty pack")?;
        let mut img = vec![0u8; (maxend - BASE) as usize + 16];
        let mut dp = 8 + n * 8;
        for (a, l) in ranges {
            let (a, l) = ((a - BASE) as usize, l as usize);
            img[a..a + l].copy_from_slice(raw.get(dp..dp + l).ok_or("truncated pack")?);
            dp += l;
        }
        Ok(Tables { img })
    }
    fn s16(&self, addr: u32, i: i32) -> i32 {
        let o = (addr - BASE) as i64 + 2 * i as i64;
        match self.img.get(o as usize..o as usize + 2) {
            Some(b) if o >= 0 => i16::from_le_bytes([b[0], b[1]]) as i32,
            _ => 0,
        }
    }
    fn u32(&self, addr: u32) -> u32 {
        let o = (addr - BASE) as usize;
        u32::from_le_bytes([self.img[o], self.img[o + 1], self.img[o + 2], self.img[o + 3]])
    }
}

fn sat(v: i32) -> i16 {
    v.clamp(-32768, 32767) as i16
}

struct Br<'a> {
    left: u16,
    word: u16,
    next: usize,
    bits_left: i16,
    frame: &'a [u16],
}

impl Br<'_> {
    fn bit(&mut self) -> i32 {
        let mut w = self.word;
        let mut l = self.left as i32 - 1;
        if l < 0 {
            w = self.frame.get(self.next).copied().unwrap_or(0);
            self.next += 1;
            self.word = w;
            l = 15;
        }
        self.left = l as u16;
        ((w >> (l & 0xff)) & 1) as i32
    }
}

pub struct Decoder<'t> {
    t: &'t Tables,
    rnd: [u32; 4],
    old_coefs: [i16; 640],
    hist: [i16; 320],
    old_mag: i32,
    nreg: usize,
    pub n: usize,
}

impl<'t> Decoder<'t> {
    /// `bandwidth` 7000 (14 regions, 320 samples/frame, 16 kHz) or 14000 (28 regions, 640 samples/frame, 32 kHz)
    pub fn new(t: &'t Tables, bandwidth: u32) -> Decoder<'t> {
        let (nreg, n) = if bandwidth == 7000 { (14, 320) } else { (28, 640) };
        Decoder { t, rnd: [1; 4], old_coefs: [0; 640], hist: [0; 320], old_mag: 0, nreg, n }
    }

    pub fn sample_rate(&self) -> u32 {
        if self.n == 320 { 16000 } else { 32000 }
    }

    fn bits_tab(&self, i: usize) -> i32 {
        self.t.s16(0x211_3c10, i as i32)
    }

    fn calc_category_offset(&self, pidx: &[i16], avail: i32) -> i32 {
        let nreg = self.nreg;
        let (mut delta, mut offset) = (0x20u32, -0x20i32);
        loop {
            let mut total = 0u32;
            for p in pidx.iter().take(nreg) {
                let c = ((offset.wrapping_add(delta as i32)).wrapping_sub(*p as i32) >> 1).clamp(0, 7);
                total = total.wrapping_add(self.bits_tab(c as usize) as u32);
            }
            if ((avail - 0x20) as u32) <= total {
                offset += delta as i32;
            }
            delta >>= 1;
            if delta == 0 {
                break;
            }
        }
        offset
    }

    fn categorize(&self, avail: i32, ctrl: usize, pidx: &[i16], cat: &mut [i16], bal: &mut [i32]) {
        let nreg = self.nreg;
        let lim = if nreg == 14 { 0x140 } else { 0x280 };
        let mut avail = avail;
        if lim < avail {
            avail = lim + (((avail - lim) * 5) >> 3);
        }
        let offset = self.calc_category_offset(pidx, avail);
        for i in 0..nreg {
            cat[i] = ((offset - pidx[i] as i32) >> 1).clamp(0, 7) as i16;
        }
        // rebalance
        let mut maxc = [0i16; 28];
        let mut minc = [0i16; 28];
        let mut rc = [0i32; 64];
        let total: i32 = (0..nreg).map(|i| self.bits_tab(cat[i] as usize)).sum();
        minc[..nreg].copy_from_slice(&cat[..nreg]);
        maxc[..nreg].copy_from_slice(&cat[..nreg]);
        let (mut imax, mut imin, mut tmax, mut tmin) = (ctrl, ctrl, total, total);
        let (mut lastmax, mut lastmin) = (0usize, 0usize);
        for _ in 0..ctrl - 1 {
            if avail * 2 < tmax + tmin {
                let mut best = -99;
                for j in (0..nreg).rev() {
                    if maxc[j] < 7 {
                        let v = (offset - pidx[j] as i32) + maxc[j] as i32 * -2;
                        if best < v {
                            best = v;
                            lastmax = j;
                        }
                    }
                }
                rc[imax] = lastmax as i32;
                imax += 1;
                let mut c = maxc[lastmax];
                let old = self.bits_tab(c as usize);
                if c < 7 {
                    c += 1;
                }
                maxc[lastmax] = c;
                tmax = (tmax - old) + self.bits_tab(c as usize);
            } else {
                let mut best = 99;
                for j in 0..nreg {
                    if minc[j] > 0 {
                        let v = (offset - pidx[j] as i32) + minc[j] as i32 * -2;
                        if v < best {
                            best = v;
                            lastmin = j;
                        }
                    }
                }
                imin -= 1;
                rc[imin] = lastmin as i32;
                let mut c = minc[lastmin];
                let old = self.bits_tab(c as usize);
                if c > 0 {
                    c -= 1;
                }
                minc[lastmin] = c;
                tmin = (tmin - old) + self.bits_tab(c as usize);
            }
        }
        cat[..nreg].copy_from_slice(&minc[..nreg]);
        bal[..ctrl - 1].copy_from_slice(&rc[imin..imin + ctrl - 1]);
    }

    fn region_power_envelope(&self, b: &mut Br, stddev: &mut [i32], pidx: &mut [i16]) -> i32 {
        let nreg = self.nreg;
        let mut diff = [0i32; 28];
        let mut v = 0;
        for _ in 0..5 {
            v = b.bit() + v * 2;
        }
        diff[0] = v - 7;
        b.bits_left -= 5;
        for r in 1..nreg {
            let tree = 0x211_5110 + r as u32 * 0x5c;
            let mut node = 0i32;
            loop {
                let bit = b.bit();
                node = self.t.s16(tree, bit + node * 2);
                b.bits_left -= 1;
                if node <= 0 {
                    break;
                }
            }
            diff[r] = -node;
        }
        pidx[0] = diff[0] as i16;
        for r in 1..nreg {
            pidx[r] = (pidx[r - 1] as i32 + (diff[r] as i16) as i32 - 12) as i16;
        }
        let st = |i: i32| self.t.s16(0x211_5b60, i);
        let (mut suma, mut maxp) = (0i32, 0i32);
        for r in 0..nreg {
            let idx = pidx[r] as i32 + 0x18;
            if maxp < idx {
                maxp = idx;
            }
            suma += st(idx);
        }
        let mut sh = 9i32;
        loop {
            if sh < 0 || (suma - 8 < 0 && maxp - 0x1c < 1) {
                break;
            }
            suma /= 2;
            maxp -= 2;
            sh -= 1;
        }
        for r in 0..nreg {
            stddev[r] = st(sh * 2 + 0x18 + pidx[r] as i32);
        }
        sh
    }

    fn next_random(&mut self) -> u32 {
        let r = &mut self.rnd;
        let mut v = r[0].wrapping_add(r[3]);
        if v & 0x8000 != 0 {
            v = v.wrapping_add(1);
        }
        r[3] = r[2];
        r[2] = r[1];
        r[1] = r[0];
        r[0] = v & 0xffff;
        r[0]
    }

    fn unpack_index(&self, mut idx: i32, out: &mut [i16; 6], cat: usize) -> i32 {
        let a = self.t.s16(0x211_5b50, cat as i32);
        let bb = self.t.s16(0x211_5b40, cat as i32);
        let dim = self.t.s16(0x211_5b30, cat as i32) - 1;
        let mut cnt = 0;
        let mut k = dim;
        while k >= 0 {
            let q = idx.wrapping_mul(bb) >> 15;
            idx = idx.wrapping_sub(q.wrapping_mul(a + 1));
            out[k as usize] = idx as i16;
            if idx != 0 {
                cnt += 1;
            }
            idx = q;
            k -= 1;
        }
        cnt
    }

    fn decode_vq(&mut self, b: &mut Br, stddev: &[i32], cat: &mut [i16], coefs: &mut [i16]) {
        let nreg = self.nreg;
        let mut err = false;
        let fill = [0i32, 0, 0, 0, 0, self.t.u32(0x20f_bc3c) as i16 as i32, self.t.u32(0x20f_bc40) as i16 as i32, self.t.u32(0x20f_bc44) as i16 as i32];
        let (mut sign_bits, mut sign_mask) = (0u32, 0u32);
        for r in 0..nreg {
            let mut c = cat[r] as usize;
            let sd = stddev[r];
            let mut p = r * 20;
            if c < 7 {
                let tree = self.t.u32(0x211_3c20 + c as u32 * 4);
                let nvec = self.t.s16(0x211_5b20, c as i32);
                let vdim = self.t.s16(0x211_5b30, c as i32);
                let cent = 0x211_3d74 + c as u32 * 0x20;
                for _ in 0..nvec {
                    let mut idx = 0i32;
                    loop {
                        if b.bits_left < 1 {
                            err = true;
                            break;
                        }
                        let bit = b.bit();
                        idx = self.t.s16(tree, bit + idx * 2);
                        b.bits_left -= 1;
                        if idx <= 0 {
                            break;
                        }
                    }
                    if err {
                        break;
                    }
                    let mut dig = [0i16; 6];
                    let nz = self.unpack_index(-idx, &mut dig, c);
                    if (b.bits_left as i32) < nz {
                        err = true;
                        break;
                    }
                    if nz != 0 {
                        sign_bits = 0;
                        for _ in 0..nz {
                            sign_bits = (b.bit() as u32).wrapping_add(sign_bits.wrapping_mul(2));
                            b.bits_left -= 1;
                        }
                        sign_mask = 1u32 << (nz - 1);
                    }
                    for k in 0..vdim as usize {
                        let x0 = sd.wrapping_mul(self.t.s16(cent, dig[k] as i32));
                        let mut x = ((x0 as u32).wrapping_add(((x0 >> 11) as u32) >> 20) as i32) >> 12;
                        if x != 0 {
                            if sign_bits & sign_mask == 0 {
                                x = -x;
                            }
                            sign_mask = ((sign_mask as i32) / 2) as u32;
                        }
                        coefs[p] = x as i16;
                        p += 1;
                    }
                }
                if err {
                    for j in r + 1..nreg {
                        cat[j] = 7;
                    }
                    c = 7;
                }
            }
            if c == 5 || c == 6 {
                p = r * 20;
                let x = sd.wrapping_mul(fill[c]);
                let nz = ((x as u32).wrapping_add(((x >> 14) as u32) >> 17) as i32) >> 15;
                let neg = -nz;
                for _ in 0..2 {
                    let mut rv = self.next_random();
                    for _ in 0..10 {
                        if coefs[p] == 0 {
                            coefs[p] = (if rv & 1 == 0 { neg } else { nz }) as i16;
                            rv = ((rv as i32) / 2) as u32;
                        }
                        p += 1;
                    }
                }
            }
            if c == 7 {
                p = r * 20;
                let x = sd.wrapping_mul(fill[7]);
                let nz = ((((x as u32).wrapping_add(((x >> 14) as u32) >> 17)) as i32) >> 15) as i16;
                for _ in 0..2 {
                    let mut rv = self.next_random();
                    for _ in 0..10 {
                        coefs[p] = if rv & 1 != 0 { nz } else { nz.wrapping_neg() };
                        p += 1;
                        rv = ((rv as i32) / 2) as u32;
                    }
                }
            }
        }
        if err {
            b.bits_left -= 1;
        }
    }

    fn test_errors(&self, b: &mut Br, ctrl: usize, cc: usize, pidx: &[i16]) -> u32 {
        let mut err = 0u32;
        if b.bits_left < 1 {
            if b.bits_left < 0 && cc < ctrl - 1 {
                err |= 2;
            }
        } else {
            for _ in 0..b.bits_left {
                if b.bit() == 0 {
                    err = 1;
                }
            }
        }
        for p in pidx.iter().take(self.nreg) {
            let p = *p as i32;
            if p - 0x18 > 0 || p + 0xf < 0 {
                err |= 4;
            }
        }
        err
    }

    fn inverse_dct4(&self, coefs: &[i16], out: &mut [i16], n: usize) {
        let t = self.t;
        let mut a = [0i16; 640];
        let mut bb = [0i16; 640];
        let mut f = [0i16; 640];
        let lg = if n == 0x140 { 6 } else { 7 };
        // sign butterfly
        {
            let sbase = if n == 0x140 { 0x211_2ea0u32 } else { 0x211_2f18 };
            let mut si = 0u32;
            let (mut p6, mut p8) = (0usize, n - 1);
            let mut cnt = 2;
            let mut bits = 0u32;
            let mut ci = 0usize;
            loop {
                cnt -= 2;
                if cnt == 0 {
                    bits = t.u32(sbase + si * 4);
                    si += 1;
                    cnt = 0x20;
                }
                let (x, y) = (coefs[ci] as i32, coefs[ci + 1] as i32);
                ci += 2;
                let v1 = ((x + y) - ((bits as i32) >> 31)) >> 1;
                a[p6] = sat(v1);
                p6 += 1;
                let tt = bits & 0x7fff_ffff;
                bits <<= 2;
                let v2 = ((x + (tt >> 30) as i32) - y) >> 1;
                a[p8] = sat(v2);
                if p8 == 0 {
                    break;
                }
                p8 -= 1;
                if p6 >= p8 {
                    break;
                }
            }
        }
        let (mut cur, mut oth): (&mut [i16; 640], &mut [i16; 640]) = (&mut a, &mut bb);
        for st in 1..=lg - 2 {
            let blocks = 1usize << st;
            let (mut ii, mut oo) = (0usize, 0usize);
            for _ in 0..blocks {
                let (mut p5, mut p7) = (oo, oo + (n >> st) - 1);
                loop {
                    let (x, y) = (cur[ii] as i32, cur[ii + 1] as i32);
                    ii += 2;
                    oth[p5] = sat(x + y);
                    p5 += 1;
                    oth[p7] = sat(x - y);
                    if p7 == 0 {
                        break;
                    }
                    p7 -= 1;
                    if p5 >= p7 {
                        break;
                    }
                }
                oo += n >> st;
            }
            std::mem::swap(&mut cur, &mut oth);
        }
        // 10x10 matrix groups
        {
            let groups = 1usize << (lg - 1);
            for g in 0..groups {
                for j in 0..10 {
                    let mut s = 0i32;
                    for k in 0..10 {
                        s = s.wrapping_add(t.s16(0x211_3008, (10 * k + j) as i32) * cur[g * 10 + k] as i32);
                    }
                    f[g * 10 + j] = sat(((s >> 14) + 1) >> 1);
                }
            }
            cur[..groups * 10].copy_from_slice(&f[..groups * 10]);
        }
        // twiddle stages
        let mut tp = 0x211_2e60u32;
        for u in (0..=lg - 2).rev() {
            let blocks = 1usize << u;
            let len = n >> u;
            let tab = t.u32(tp);
            let (mut src, mut dst) = (0usize, 0usize);
            for _ in 0..blocks {
                let (mut i1, mut i2) = (src, src + (len >> 1));
                let (mut p3, mut p4) = (dst, dst + len);
                let mut ti = 0i32;
                loop {
                    let (a1, a2) = (cur[i1] as i32, cur[i1 + 1] as i32);
                    i1 += 2;
                    let (b1, b2) = (cur[i2] as i32, cur[i2 + 1] as i32);
                    i2 += 2;
                    let (c5, c6, c7, c8) = (t.s16(tab, ti), t.s16(tab, ti + 1), t.s16(tab, ti + 2), t.s16(tab, ti + 3));
                    ti += 4;
                    let w = |v: i32| -> i16 { sat(((v >> 13) + 1) >> 1) };
                    let o = if u == 0 { &mut *out } else { &mut oth[..] };
                    o[p4 - 1] = w(b1 * c5 + c6 * a1);
                    o[p3] = w(b1 * -c6 + c5 * a1);
                    o[p3 + 1] = w(b2 * c8 + c7 * a2);
                    o[p4 - 2] = w(b2 * -c7 + c8 * a2);
                    p3 += 2;
                    p4 -= 2;
                    if p3 >= p4 {
                        break;
                    }
                }
                dst += len;
                src += len;
            }
            tp += 4;
            std::mem::swap(&mut cur, &mut oth);
        }
        if n == 0x140 {
            for i in 0..0x140 {
                out[i] = sat(out[i] as i32 + t.s16(0x211_3210, i as i32));
            }
        }
    }

    fn window_ola(&mut self, out: &mut [i16], arr: &[i16]) {
        let n = self.n;
        let half = n >> 1;
        let win = if n == 0x140 { 0x211_5be0u32 } else { 0x211_5e60 };
        let w = |i: usize| self.t.s16(win, i as i32);
        let mid = |j: isize| arr[(half as isize + j) as usize] as i32; // mid[j]
        for k in 0..half {
            out[k] = sat(((((self.hist[k] as i32 * w(n - 1 - k)) + (w(k) * mid(-1 - k as isize))) >> 12) + 1) >> 1);
        }
        for k in 0..half {
            out[half + k] = sat(((((self.hist[half - 1 - k] as i32 * -w(n - 1 - half - k)) + (w(half + k) * arr[k] as i32)) >> 12) + 1) >> 1);
        }
        for k in 0..half {
            self.hist[k] = arr[half + k];
        }
    }

    /// decode one frame (`bits` = bits per frame; `frame` = little-endian 16-bit words); returns the PCM samples and whether the frame had errors
    pub fn decode_frame(&mut self, frame: &[u16], bits: i16) -> (Vec<i16>, bool) {
        let (nreg, n) = (self.nreg, self.n);
        let mut mlt = [0i16; 640];
        let mut b = Br { left: 0, word: frame.first().copied().unwrap_or(0), next: 0, bits_left: bits, frame };
        let (ncoefs, ctrl_bits, ctrl) = if nreg == 14 { (0x118, 4, 0x10usize) } else { (0x230, 5, 0x20usize) };
        let (mut pidx, mut cat, mut bal, mut stddev) = ([0i16; 28], [0i16; 28], [0i32; 64], [0i32; 28]);
        let mut mag = self.region_power_envelope(&mut b, &mut stddev, &mut pidx);
        let mut cc = 0usize;
        for _ in 0..ctrl_bits {
            cc = b.bit() as usize + cc * 2;
        }
        b.bits_left -= ctrl_bits as i16;
        self.categorize(b.bits_left as i32, ctrl, &pidx, &mut cat, &mut bal);
        for k in 0..cc {
            cat[bal[k] as usize] += 1;
        }
        self.decode_vq(&mut b, &stddev, &mut cat, &mut mlt);
        let err = self.test_errors(&mut b, ctrl, cc, &pidx);
        if err == 0 {
            self.old_coefs[..ncoefs].copy_from_slice(&mlt[..ncoefs]);
            self.old_mag = mag;
        } else {
            mlt[..ncoefs].copy_from_slice(&self.old_coefs[..ncoefs]);
            self.old_coefs[..ncoefs].fill(0);
            mag = self.old_mag;
            self.old_mag = 0;
        }
        mlt[ncoefs..n].fill(0);
        let mut tmp = [0i16; 640];
        self.inverse_dct4(&mlt, &mut tmp, n);
        if mag > 0 {
            for v in tmp.iter_mut().take(n) {
                *v = (*v as i32 >> mag) as i16;
            }
        } else if mag < 0 {
            for v in tmp.iter_mut().take(n) {
                *v = ((*v as i32) << -mag) as i16;
            }
        }
        let mut out = vec![0i16; n];
        self.window_ola(&mut out, &tmp);
        (out, err != 0)
    }
}

/// decode a whole clip (frames of `bitrate/50/8` bytes); returns PCM and the number of frames with errors
pub fn decode_clip(t: &Tables, data: &[u8], bitrate: u32, bandwidth: u32) -> (Vec<i16>, usize) {
    let bits = (bitrate / 50) as i16;
    let bpf = (bits >> 3) as usize;
    let mut dec = Decoder::new(t, bandwidth);
    let mut pcm = Vec::with_capacity(data.len() / bpf * dec.n);
    let mut errs = 0;
    for chunk in data.chunks_exact(bpf) {
        let mut fr = [0u16; 64];
        for (i, w) in chunk.chunks(2).enumerate() {
            fr[i] = u16::from_le_bytes([w[0], *w.get(1).unwrap_or(&0)]);
        }
        let (o, e) = dec.decode_frame(&fr, bits);
        errs += e as usize;
        pcm.extend(o);
    }
    (pcm, errs)
}
