"""Headless DeSmuME driver (py-desmume) for ground-truth checks: boot, step, press keys / touch, screenshots, save states, memory access.
The console is played held sideways: screenshots are saved as the raw 256x384 buffer (top screen then bottom screen) and optionally rotated for reading."""
import os, sys, io, contextlib, datetime
from PIL import Image
with contextlib.redirect_stdout(io.StringIO()):
    from desmume.emulator import DeSmuME
    from desmume.controls import Keys, keymask

KEYS = {'A': Keys.KEY_A, 'B': Keys.KEY_B, 'X': Keys.KEY_X, 'Y': Keys.KEY_Y, 'L': Keys.KEY_L, 'R': Keys.KEY_R, 'START': Keys.KEY_START, 'SELECT': Keys.KEY_SELECT,
        'UP': Keys.KEY_UP, 'DOWN': Keys.KEY_DOWN, 'LEFT': Keys.KEY_LEFT, 'RIGHT': Keys.KEY_RIGHT}


def dt_ticks(dt):
    """DeSmuME DateTime ticks (100 ns since 0001-01-01, local time)"""
    return int((dt - datetime.datetime(1, 1, 1)).total_seconds()) * 10_000_000


class Emu:
    def __init__(self, rom, state=None):
        with contextlib.redirect_stdout(io.StringIO()):
            self.e = DeSmuME(); self.e.open(rom); self.e.volume_set(0)
        self.frames = 0
        if state: self.load(state)

    def step(self, n=1):
        for _ in range(n): self.e.cycle(False)
        self.frames += n

    def press(self, *keys, hold=6, release=6):
        self.e.input.keypad_update(sum(keymask(KEYS[k]) for k in keys)); self.step(hold); self.e.input.keypad_update(0); self.step(release)

    def touch(self, x, y, hold=6, release=6):
        self.e.input.touch_set_pos(x, y); self.step(hold); self.e.input.touch_release(); self.step(release)

    def image(self):
        b = bytes(self.e.display_buffer_as_rgbx())
        return Image.frombuffer('RGBA', (256, 384), b, 'raw', 'BGRA', 0, 1).convert('RGB')   # py-desmume's buffer is BGRX: reading it as RGBX swapped red and blue (blue skin, maroon navy)

    def shot(self, path, rotate=False, scale=1):
        im = self.image()
        if rotate: im = im.crop((0, 0, 256, 192)).rotate(90, expand=True), im.crop((0, 192, 256, 384)).rotate(90, expand=True)
        if isinstance(im, tuple):
            w = im[0].width + im[1].width; out = Image.new('RGB', (w, im[0].height)); out.paste(im[0], (0, 0)); out.paste(im[1], (im[0].width, 0)); im = out
        if scale != 1: im = im.resize((im.width * scale, im.height * scale), Image.NEAREST)
        im.save(path); return im

    def pin_rtc(self, start_ticks=None, counter=0):
        """Make the RTC follow emulated time instead of the host clock (needed for exact replays: the game reads the RTC constantly).
        libdesmume's rtcGetTime() returns movie rtcStart + currFrameCounter frames when a movie is active, else the host clock;
        the movie-mode checks are patched out and rtcStart / currFrameCounter are set here.  Offsets are for the bundled
        libdesmume.dylib (arm64, 0.9.12): rtcGetTime at 0x1d8600, cbz at 0x1d8614 / 0x1d8e78 / 0x1d8ed4, movieMode 0x63568ec, currFrameCounter
        0x63568f8, rtcStart (DateTime ticks, 100 ns since 0001-01-01) at 0x6356bc0."""
        import ctypes
        lib = self.e.lib
        base = ctypes.addressof(ctypes.c_char.in_dll(lib, 'SF_RTC')) - 0x363420
        assert ctypes.c_uint32.from_address(base + 0x1d8600).value == 0xa9bf7bfd, 'unexpected libdesmume build'
        libc = ctypes.CDLL(None)
        vmp = libc.mach_vm_protect
        vmp.argtypes = (ctypes.c_uint32, ctypes.c_uint64, ctypes.c_uint64, ctypes.c_int, ctypes.c_int)
        libc.sys_icache_invalidate.argtypes = (ctypes.c_void_p, ctypes.c_size_t)
        task = ctypes.c_uint32.in_dll(libc, 'mach_task_self_').value
        for cbz in (base + 0x1d8614, base + 0x1d8e78, base + 0x1d8ed4):   # rtcGetTime and its two inlined copies in rtcRecv
            if ctypes.c_uint32.from_address(cbz).value == 0xd503201f:
                continue
            page = cbz & ~0x3fff
            r = vmp(task, page, 0x4000, 0, 0x1 | 0x2 | 0x10)       # READ|WRITE|COPY
            assert r == 0, 'mach_vm_protect rw failed %d' % r
            ctypes.c_uint32.from_address(cbz).value = 0xd503201f   # nop: always take the movie-time path
            r = vmp(task, page, 0x4000, 0, 0x1 | 0x4)              # READ|EXEC
            assert r == 0, 'mach_vm_protect rx failed %d' % r
            libc.sys_icache_invalidate(cbz, 4)
        if start_ticks is None:
            start_ticks = dt_ticks(datetime.datetime(2026, 10, 12, 17, 0, 0))
        ctypes.c_int64.from_address(base + 0x6356bc0).value = start_ticks
        ctypes.c_int32.from_address(base + 0x63568f8).value = counter
        self._rtc = base

    def rtc_state(self):
        import ctypes
        return ctypes.c_int64.from_address(self._rtc + 0x6356bc0).value, ctypes.c_int32.from_address(self._rtc + 0x63568f8).value

    def ram(self):
        """main RAM (0x2000000..0x2400000) as a live 4 MB ctypes buffer inside libdesmume (MMU.MAIN_MEM at MMU+0xc000); far faster than memory.unsigned slicing"""
        import ctypes
        if not hasattr(self, '_ram'):
            mmu = ctypes.addressof(ctypes.c_char.in_dll(self.e.lib, 'MMU'))
            self._ram = (ctypes.c_char * 0x400000).from_address(mmu + 0xc000)
            assert bytes(self._ram[0x100000:0x100400]) == bytes(self.e.memory.unsigned[0x2100000:0x2100400]), 'MAIN_MEM offset mismatch'
        return self._ram

    def save(self, path): self.e.savestate.save_file(path)
    def load(self, path): self.e.savestate.load_file(path)
    def read32(self, addr): return self.e.memory.unsigned.read_long(addr) if hasattr(self.e.memory.unsigned, 'read_long') else self.e.memory.unsigned[addr]


def _drag(self, pts, hold=2, release=8):
    """touch down at pts[0], move through pts, release (stroke)"""
    for x, y in pts: self.e.input.touch_set_pos(x, y); self.step(hold)
    self.e.input.touch_release(); self.step(release)


Emu.drag = _drag


def page_to_touch(rx, ry):
    """reading-orientation coordinates on the bottom page (192 x 256, as saved by shot(rotate=True)) -> touch coordinates"""
    return 255 - int(ry), int(rx)
