"""Check lpui.logic.hud: the HUD date sprites' sequence and hidden flag computed from the game state (day field, weekday)
equal what the game set in RAM (sprite slots of the HUD struct 0x214b9ac), on every capture with the HUD active.
    .venv/bin/python sandbox/ui/verify_hud.py CAPTURE_DIR..."""
import os, struct, sys
sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
from lpui.logic import hud

B = 0x2000000


def main(dirs):
    n = ok = 0
    bad = []
    for d in dirs:
        ram = open(os.path.join(d, 'ram.bin'), 'rb').read()
        rd = lambda a: struct.unpack_from('<I', ram, a - B)[0]
        rds = lambda a: struct.unpack_from('<i', ram, a - B)[0]
        ui = 0x214b9ac
        slots = {off: rds(ui + off) for off in (hud.WEEKDAY_SLOT,) + hud.DIGIT_SLOTS}
        if any(not 0 <= s < 0x9c for s in slots.values()):
            continue
        objs = {off: rd(0x2150c80 + 0x38 * s) for off, s in slots.items()}
        if any(not B <= o < B + 0x400000 for o in objs.values()):
            continue
        game = {off: (ram[o - B + 0x32], bool(ram[o - B + 0x24] & 2)) for off, o in objs.items()}
        gs = rd(0x21246ac)
        day = rd(gs + 0x138)
        clk = rd(0x2142c6c)
        weekday = rd(clk + 0x54) if rd(clk) else rd(clk + 0x38)
        hide_all = game[hud.WEEKDAY_SLOT][1] and game[0x58][1]
        mine = hud.date_widgets(day, weekday, hide_all)
        n += 1
        if mine == game:
            ok += 1
        else:
            bad.append((os.path.basename(d), day, weekday, game, mine))
    print('%d captures with the HUD date sprites, %d computed exactly from (day field, weekday)' % (n, ok))
    for b in bad[:5]:
        print('  ', b)


if __name__ == '__main__':
    main(sys.argv[1:])
