#!/usr/bin/env python3
"""Headless multi-day sampler: cycles days (sleep -> schedule entry with random activities -> tap through events) from a
save state and logs every script start with the event-selection session record, via tools/play_monitor.Monitor.

The state must be a morning room (予定入力 button visible).  Usage (repo root): .venv/bin/python tools/event_sampler.py STATE DAYS SEED OUT.jsonl [--frames-per-day N]
Touch coordinates are raw bottom-screen coordinates (page_to_touch): sleep/plan button (235,108), plan confirm (16,33),
row arrows at page x=18 (left) / 176 (right), rows at page y = 44, 99, 154, 209.  Dialogue advance tap (30,100).
"""
import os
import random
import sys

os.environ.setdefault('SDL_VIDEODRIVER', 'dummy')
HERE = os.path.dirname(os.path.abspath(__file__))
sys.path.insert(0, HERE)
import play_monitor as pm  # noqa: E402
from emu_driver import Emu, page_to_touch  # noqa: E402

ROM = os.path.join(os.path.dirname(HERE), 'rom', 'loveplus_plus_j.nds')


def choice_visible(emu):
    """The choice menu shows a cyan number badge at reading-page (205, 70) = raw bottom-screen (185, 205)."""
    r, g, b = emu.image().getpixel((185, 192 + 13))
    return r < 30 and abs(g - 184) < 30 and abs(b - 248) < 20


def main():
    state, days, seed, out = sys.argv[1], int(sys.argv[2]), int(sys.argv[3]), sys.argv[4]
    fpd = int(sys.argv[sys.argv.index('--frames-per-day') + 1]) if '--frames-per-day' in sys.argv else 4200
    path = state if os.path.exists(state) else os.path.join(pm.STATES, state)
    rng = random.Random(seed)
    emu = Emu(ROM, path)
    mon = pm.Monitor(emu, out)
    mon.install()
    emu.step(30)
    mon.log('sampler', seed=seed, days=days)
    for day in range(days):
        mon.log('day', n=day)
        emu.touch(235, 108, hold=4, release=60)          # 予定入力 (morning room)
        emu.step(40)
        for ry in (44, 99, 154, 209):                      # random activity per period row
            for _ in range(rng.randint(0, 5)):
                emu.touch(*page_to_touch(176, ry), hold=3, release=12)
        emu.touch(16, 33, hold=4, release=60)             # 決定
        emu.step(40)
        start = emu.frames
        while emu.frames - start < fpd:
            if choice_visible(emu):
                x = rng.choice((180, 115))                    # choice 1 or 2 badge (touch coords), then confirm tap
                mon.log('choice', pick=1 if x == 180 else 2)
                emu.touch(x, 16, hold=3, release=20)
                emu.touch(x - 15, 10, hold=3, release=40)
                continue
            emu.touch(30, 100, hold=3, release=27)
        mon.shot('sampler-d%d-s%d' % (day, seed))
        emu.touch(235, 108, hold=4, release=60)          # 寝る (night room) -> next morning
        emu.step(120)
    mon.log('end')


if __name__ == '__main__':
    main()
