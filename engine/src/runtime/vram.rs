//! VRAM as the ARM9 sees it: nine banks (A..I) placed into the address windows by the VRAMCNT registers
//! (0x4000240..0x4000246 for A..G, 0x4000248 H, 0x4000249 I: bit 7 enable, bits 0..2 MST, bits 3..4 OFS).
//! Windows: LCDC 0x6800000, engine A BG 0x6000000 (512 KB), engine A OBJ 0x6400000 (256 KB), engine B BG 0x6200000
//! (128 KB), engine B OBJ 0x6600000 (128 KB); banks also serve as texture image / palette slots and extended
//! palettes, which are not CPU-addressable once mapped there (the renderer reads them through `slot`).

pub const SIZES: [usize; 9] = [0x20000, 0x20000, 0x20000, 0x20000, 0x10000, 0x4000, 0x4000, 0x8000, 0x4000];
const LCDC: [u32; 9] = [0x6800000, 0x6820000, 0x6840000, 0x6860000, 0x6880000, 0x6890000, 0x6894000, 0x6898000, 0x68a0000];

/// where a bank is mapped
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Place {
    Off,
    /// CPU-addressable at this base
    Cpu(u32),
    /// texture image slot n (128 KB each)
    TexImage(u32),
    /// texture palette slot n (16 KB each)
    TexPalette(u32),
    /// BG extended palette of engine A (slot base n) / engine B
    BgExtA(u32),
    BgExtB,
    ObjExtA,
    ObjExtB,
    Arm7,
}

pub struct Vram {
    pub banks: Vec<Vec<u8>>,
    pub place: [Place; 9],
}

impl Default for Vram {
    fn default() -> Self {
        Vram { banks: SIZES.iter().map(|&n| vec![0; n]).collect(), place: [Place::Off; 9] }
    }
}

impl Vram {
    /// a VRAMCNT byte was written (bank index 0..8)
    pub fn set_cnt(&mut self, bank: usize, cnt: u8) {
        let (mst, ofs) = ((cnt & 7) as u32, ((cnt >> 3) & 3) as u32);
        self.place[bank] = if cnt & 0x80 == 0 {
            Place::Off
        } else {
            match (bank, mst) {
                (_, 0) => Place::Cpu(LCDC[bank]),
                (0 | 1 | 2 | 3, 1) => Place::Cpu(0x6000000 + 0x20000 * ofs),
                (0 | 1, 2) => Place::Cpu(0x6400000 + 0x20000 * (ofs & 1)),
                (0..=3, 3) => Place::TexImage(ofs),
                (2 | 3, 2) => Place::Arm7,
                (2, 4) => Place::Cpu(0x6200000),
                (3, 4) => Place::Cpu(0x6600000),
                (4, 1) => Place::Cpu(0x6000000),
                (4, 2) => Place::Cpu(0x6400000),
                (4, 3) => Place::TexPalette(0),
                (4, 4) => Place::BgExtA(0),
                (5 | 6, 1) => Place::Cpu(0x6000000 + 0x4000 * (ofs & 1) + 0x10000 * (ofs >> 1)),
                (5 | 6, 2) => Place::Cpu(0x6400000 + 0x4000 * (ofs & 1) + 0x10000 * (ofs >> 1)),
                (5 | 6, 3) => Place::TexPalette((ofs & 1) + 4 * (ofs >> 1)),
                (5 | 6, 4) => Place::BgExtA((ofs & 1) * 2),
                (5 | 6, 5) => Place::ObjExtA,
                (7, 1) => Place::Cpu(0x6200000),
                (7, 2) => Place::BgExtB,
                (8, 1) => Place::Cpu(0x6208000),
                (8, 2) => Place::Cpu(0x6600000),
                (8, 3) => Place::ObjExtB,
                _ => Place::Off,
            }
        };
    }

    /// the size of the window a CPU address belongs to (banks mirror within it)
    fn window(a: u32) -> Option<(u32, u32)> {
        Some(match a {
            0x6000000..=0x61fffff => (0x6000000, 0x80000),
            0x6200000..=0x63fffff => (0x6200000, 0x20000),
            0x6400000..=0x65fffff => (0x6400000, 0x40000),
            0x6600000..=0x67fffff => (0x6600000, 0x20000),
            0x6800000..=0x68fffff => (0x6800000, 0xa4000),
            _ => return None,
        })
    }

    /// the banks a CPU address hits (overlapping banks: reads OR together, writes go to all)
    fn hits(&self, a: u32) -> impl Iterator<Item = (usize, usize)> + '_ {
        let w = Self::window(a);
        (0..9).filter_map(move |b| {
            let (base, size) = w?;
            let a = base + (a - base) % size;
            match self.place[b] {
                Place::Cpu(p) if a >= p && ((a - p) as usize) < SIZES[b] => Some((b, (a - p) as usize)),
                _ => None,
            }
        })
    }

    pub fn read(&self, a: u32) -> u8 {
        self.hits(a).fold(0, |v, (b, o)| v | self.banks[b][o])
    }
    pub fn write(&mut self, a: u32, v: u8) {
        let h: Vec<(usize, usize)> = self.hits(a).collect();
        for (b, o) in h {
            self.banks[b][o] = v;
        }
    }

    /// a byte of a non-CPU slot (texture images / palettes, extended palettes), for the renderer
    pub fn slot(&self, kind: Place, offset: usize) -> u8 {
        let mut v = 0;
        for b in 0..9 {
            let hit = match (self.place[b], kind) {
                (Place::TexImage(n), Place::TexImage(_)) => offset.checked_sub(n as usize * 0x20000).filter(|&o| o < SIZES[b]),
                (Place::TexPalette(n), Place::TexPalette(_)) => offset.checked_sub(n as usize * 0x4000).filter(|&o| o < SIZES[b]),
                (Place::BgExtA(n), Place::BgExtA(_)) => offset.checked_sub(n as usize * 0x4000).filter(|&o| o < SIZES[b]),
                (p, k) if p == k => Some(offset).filter(|&o| o < SIZES[b]),
                _ => None,
            };
            if let Some(o) = hit {
                v |= self.banks[b][o];
            }
        }
        v
    }
}
