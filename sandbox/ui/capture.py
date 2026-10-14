"""Capture the DS display state (both engines) at a frame: screenshot + IO registers + palettes + OAM + VRAM banks.
    .venv/bin/python sandbox/ui/capture.py STATE.dst OUT_DIR [frames_to_run] [tap_frames...]
Writes OUT_DIR/{shot.png, regs.json, pal.bin, oam.bin, vram_<bank>.bin}. The DS here is held sideways: the shot is the
raw upright layout (top screen over bottom screen)."""
import json, os, sys
ROOT = os.path.dirname(os.path.dirname(os.path.dirname(os.path.abspath(__file__))))
sys.path.insert(0, os.path.join(ROOT, 'tools'))
from emu_driver import Emu  # noqa: E402

# VRAM banks in the LCDC (plain) mapping: bank -> (address, size)
BANKS = {'A': (0x6800000, 0x20000), 'B': (0x6820000, 0x20000), 'C': (0x6840000, 0x20000), 'D': (0x6860000, 0x20000),
         'E': (0x6880000, 0x10000), 'F': (0x6890000, 0x4000), 'G': (0x6894000, 0x4000), 'H': (0x6898000, 0x8000),
         'I': (0x68a0000, 0x4000)}


def capture(e, out):
    os.makedirs(out, exist_ok=True)
    m = e.e.memory.unsigned
    rd = lambda a, n: bytes(m[a + i] for i in range(n))
    e.shot(os.path.join(out, 'shot.png'))
    regs = {}
    for base, eng in ((0x4000000, 'main'), (0x4001000, 'sub')):
        regs[eng] = {'%03x' % o: m.read_long(base + o) & 0xffffffff for o in range(0, 0x70, 4)}
    regs['vramcnt'] = list(rd(0x4000240, 10))
    regs['powcnt'] = m.read_long(0x4000304) & 0xffffffff
    json.dump(regs, open(os.path.join(out, 'regs.json'), 'w'), indent=1)
    open(os.path.join(out, 'pal.bin'), 'wb').write(rd(0x5000000, 0x800))
    open(os.path.join(out, 'oam.bin'), 'wb').write(rd(0x7000000, 0x800))
    # the banks as currently mapped are not readable in the LCDC window; read the engine views instead
    # the BG scroll registers are write-only: the game's shadow table (Display_ApplyBgScrollRegs: flag byte per BG, x at +8+8i,
    # y at +0xc+8i, main then sub) and all of main RAM (game state behind the screen)
    open(os.path.join(out, 'dtcm.bin'), 'wb').write(bytes(m[0x27e0000:0x27e4000]))   # BG scroll shadow at 0x27e25a0, window x at 0x27e25f4/f8
    for b, (a, n) in BANKS.items():                                                  # LCDC view of banks mapped to LCDC (display mode 2)
        open(os.path.join(out, 'lcdc_%s.bin' % b), 'wb').write(bytes(m[a:a + n]))
    open(os.path.join(out, 'ram.bin'), 'wb').write(bytes(m[0x2000000:0x2400000]))
    for name, (a, n) in (('bg_main', (0x6000000, 0x80000)), ('bg_sub', (0x6200000, 0x20000)),
                         ('obj_main', (0x6400000, 0x40000)), ('obj_sub', (0x6600000, 0x20000))):
        open(os.path.join(out, 'vram_%s.bin' % name), 'wb').write(rd(a, n))


if __name__ == '__main__':
    state, out = sys.argv[1], sys.argv[2]
    n = int(sys.argv[3]) if len(sys.argv) > 3 else 0
    taps = {int(x) for x in sys.argv[4:]}
    e = Emu(os.path.join(ROOT, 'rom', 'loveplus_plus_j.nds'), state)
    for i in range(n):
        if i in taps:
            e.touch(128, 96)
        e.step()
    capture(e, out)
    e.save(os.path.join(out, 'state.dst'))
    print('captured', out, 'frame', n)
