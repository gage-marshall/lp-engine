"""Headless DeSmuME driver (py-desmume) for ground-truth checks: boot, step, press keys / touch, screenshots, save states, memory access.
The console is played held sideways: screenshots are saved as the raw 256x384 buffer (top screen then bottom screen) and optionally rotated for reading."""
import os, sys, io, contextlib
from PIL import Image
with contextlib.redirect_stdout(io.StringIO()):
    from desmume.emulator import DeSmuME
    from desmume.controls import Keys, keymask

KEYS = {'A': Keys.KEY_A, 'B': Keys.KEY_B, 'X': Keys.KEY_X, 'Y': Keys.KEY_Y, 'L': Keys.KEY_L, 'R': Keys.KEY_R, 'START': Keys.KEY_START, 'SELECT': Keys.KEY_SELECT,
        'UP': Keys.KEY_UP, 'DOWN': Keys.KEY_DOWN, 'LEFT': Keys.KEY_LEFT, 'RIGHT': Keys.KEY_RIGHT}


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
        return Image.frombuffer('RGBA', (256, 384), b, 'raw', 'RGBA', 0, 1).convert('RGB')

    def shot(self, path, rotate=False, scale=1):
        im = self.image()
        if rotate: im = im.crop((0, 0, 256, 192)).rotate(90, expand=True), im.crop((0, 192, 256, 384)).rotate(90, expand=True)
        if isinstance(im, tuple):
            w = im[0].width + im[1].width; out = Image.new('RGB', (w, im[0].height)); out.paste(im[0], (0, 0)); out.paste(im[1], (im[0].width, 0)); im = out
        if scale != 1: im = im.resize((im.width * scale, im.height * scale), Image.NEAREST)
        im.save(path); return im

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
