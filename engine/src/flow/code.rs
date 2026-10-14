//! Reading switch tables out of the game's ARM code, so the ports take them from the game (static image):
//!   jump_table(fn)       the cases of the first `addls pc, pc, rN, lsl #2` in fn: target address or None per case
//!   first_literal(addr)  the word the first `ldr rN, [pc, #imm]` at or after addr loads

use crate::rom::Rom;

const COND_AL: u32 = 0xe;
const COND_LS: u32 = 0x9;

fn rotated_imm(ins: u32) -> u32 {
    (ins & 0xff).rotate_right(2 * ((ins >> 8) & 0xf))
}

/// unconditional `cmp rN, #imm`: the immediate
fn cmp_imm(ins: u32) -> Option<u32> {
    // data processing, immediate operand, opcode CMP (0xa), S set
    (ins >> 28 == COND_AL && ins & 0x0ff0_0000 == 0x0350_0000).then(|| rotated_imm(ins))
}

/// `addls pc, pc, rM, lsl #2`
fn is_addls_pc(ins: u32) -> bool {
    ins >> 28 == COND_LS && ins & 0x0fff_fff0 == 0x008f_f100
}

/// unconditional `b`: its target
fn branch(ins: u32, at: u32) -> Option<u32> {
    (ins >> 28 == COND_AL && ins & 0x0f00_0000 == 0x0a00_0000).then(|| {
        let off = ((ins & 0x00ff_ffff) << 8) as i32 >> 6;
        at.wrapping_add(8).wrapping_add(off as u32)
    })
}

/// unconditional `ldr rN, [pc, #+/-imm]` (word, pre-indexed, no writeback): the address it reads
fn ldr_pc(ins: u32, at: u32) -> Option<u32> {
    (ins >> 28 == COND_AL && ins & 0x0f7f_0000 == 0x051f_0000).then(|| {
        let imm = ins & 0xfff;
        let base = at.wrapping_add(8);
        if ins & (1 << 23) != 0 { base.wrapping_add(imm) } else { base.wrapping_sub(imm) }
    })
}

pub fn jump_table(rom: &Rom, func: u32) -> Vec<Option<u32>> {
    let mut limit = None;
    for k in 0..64 {
        let at = func + 4 * k;
        let ins = rom.u32(at);
        if let Some(v) = cmp_imm(ins) {
            limit = Some(v);
        }
        if is_addls_pc(ins) {
            let base = at + 8;
            let n = limit.unwrap_or_else(|| panic!("jump table at {at:#x} without a bound")) + 1;
            return (0..n).map(|c| branch(rom.u32(base + 4 * c), base + 4 * c)).collect();
        }
    }
    panic!("no jump table in {func:#x}")
}

pub fn first_literal(rom: &Rom, addr: u32) -> Option<u32> {
    (0..8).find_map(|k| ldr_pc(rom.u32(addr + 4 * k), addr + 4 * k)).map(|a| rom.u32(a))
}

/// per case, the literal the case starts by loading (None: the case does nothing)
pub fn case_literals(rom: &Rom, func: u32) -> Vec<Option<u32>> {
    jump_table(rom, func).into_iter().map(|t| t.and_then(|t| first_literal(rom, t))).collect()
}
