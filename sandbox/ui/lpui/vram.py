"""Both 2D engines' memory as the game fills it: BG VRAM (main 512 KB, sub 128 KB), OBJ VRAM (256 / 128 KB), palette RAM,
OAM and the display registers. `apply` performs one load the way the replay trace recorded it (target memory, offset, size,
origin = archive / entry / offset in the file, or a zero fill); `render` composes both screens with ds2d."""
import glob, os, sys
import numpy as np
HERE = os.path.dirname(os.path.abspath(__file__))
sys.path.insert(0, os.path.dirname(HERE))
import ds2d  # noqa: E402

ROOT = os.path.dirname(os.path.dirname(os.path.dirname(HERE)))
SIZES = {'bg_main': 0x80000, 'bg_sub': 0x20000, 'obj_main': 0x40000, 'obj_sub': 0x20000, 'pal': 0x800}
_FILES = {}


def file_bytes(arc, entry):
    """an archive entry as the game's buffer holds it (extracted under content/assets, LZ already undone)"""
    key = (arc, entry)
    if key not in _FILES:
        if arc and entry == -1:
            fp = os.path.join(ROOT, 'work/fs', arc)
            _FILES[key] = open(fp, 'rb').read() if os.path.isfile(fp) else None
        else:
            fs = glob.glob(os.path.join(ROOT, 'content/assets', arc, '%04d.*' % entry)) if arc else []
            _FILES[key] = open(fs[0], 'rb').read() if fs else None
    return _FILES[key]


class Vram:
    def __init__(self, regs=None, oam=None, dtcm=None, scroll_regs=None):
        self.mem = {k: np.zeros(n, np.uint8) for k, n in SIZES.items()}
        self.regs = regs or {'main': {}, 'sub': {}, 'vramcnt': [0] * 10, 'powcnt': 0}
        self.oam = bytearray(oam or bytes(0x800))
        self.dtcm = dtcm
        self.scroll_regs = scroll_regs
        self.scroll_override = {}

    def apply(self, op, fallback=None):
        """one load op; `fallback` (bytes of the same memory) fills data the files cannot give (generated content).
        Returns True when the bytes came from a file or a zero fill."""
        name, a, n = op['mem'], op['at'], op['size']
        n = min(n, SIZES[name] - a)
        if n <= 0 or a < 0:
            return False
        o = op.get('origin')
        data = file_bytes(o.get('arc'), o.get('entry', -1)) if o and not o.get('packed') else None
        if data is not None and o['off'] + n <= len(data):
            self.mem[name][a:a + n] = np.frombuffer(data[o['off']:o['off'] + n], np.uint8)
            return True
        if op.get('zero'):
            self.mem[name][a:a + n] = 0
            return True
        if fallback is not None:
            self.mem[name][a:a + n] = np.frombuffer(fallback[a:a + n], np.uint8)
        return False

    # ---- the ds2d.Capture interface
    @property
    def vram(self):
        return {k: self.mem[k].tobytes() for k in ('bg_main', 'bg_sub', 'obj_main', 'obj_sub')}

    @property
    def pal(self):
        return self.mem['pal'].view('<u2')

    @property
    def lcdc(self):
        return {}

    def reg(self, eng, off, size=2):
        return ds2d.Capture.reg(self, eng, off, size)

    def scroll(self, eng, n):
        if (eng, n) in self.scroll_override:
            return self.scroll_override[(eng, n)]
        return ds2d.Capture.scroll(self, eng, n)

    def render(self):
        """(top, bottom): each (rgb 5-bit HxWx3, unknown mask)"""
        return ds2d.both(self)
