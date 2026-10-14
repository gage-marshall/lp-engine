"""Boot the game on a save image and read back what it loaded (settings record, GameState fields).
    .venv/bin/python sandbox/save/boot_check.py SAVE.sav [frames]"""
import os, struct, sys
ROOT = os.path.dirname(os.path.dirname(os.path.dirname(os.path.abspath(__file__))))
sys.path.insert(0, os.path.join(ROOT, 'tools'))
from emu_driver import Emu


def boot(sav, frames=1500, taps=()):
    emu = Emu(os.path.join(ROOT, 'rom', 'loveplus_plus_j.nds'))
    emu.e.backup.import_file(sav)
    emu.e.reset()
    emu.pin_rtc()
    for k in range(frames):
        if k in taps:
            emu.touch(128, 96)
        emu.step()
    return emu


if __name__ == '__main__':
    emu = boot(sys.argv[1], int(sys.argv[2]) if len(sys.argv) > 2 else 1500)
    m = emu.e.memory.unsigned
    rec = m.read_long(0x21544e8 + 4)
    print('settings record @%#x: levels %s text speed %d' % (rec, [m.read_byte(rec + 5 + i) for i in range(3)], m.read_byte(rec + 0xc)))
