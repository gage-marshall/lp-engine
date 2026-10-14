"""Check sprites from spec: each sprite object's OAM built from its files (cell = NANR(sequence, time) in its NCER, through
the resource handles and the traced loads) instead of its RAM cell pointer, vs the captured hardware OAM.
    .venv/bin/python sandbox/ui/verify_sprites_spec.py TRACE_DIR"""
import collections, glob, os, sys
sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
from lpui import sprites, cells, hudprobe
from lpui.vram import file_bytes
from verify_sprites import visible

if __name__ == '__main__':
    trace = sys.argv[1]
    bufs = hudprobe.loaded_buffers(trace)
    tot = {'main': [0, 0], 'sub': [0, 0]}
    src = collections.Counter()
    full = collections.Counter()
    for d in sorted(glob.glob(os.path.join(trace, 'cap-f*'))):
        f = int(d.rsplit('-f', 1)[1])
        ram = open(os.path.join(d, 'ram.bin'), 'rb').read()
        oam = open(os.path.join(d, 'oam.bin'), 'rb').read()

        def from_files(o):
            carc, cent, _ = hudprobe.resource_file(ram, o.cell_bank, bufs, f)
            aarc, aent, _ = hudprobe.resource_file(ram, o.anim_bank, bufs, f)
            if carc and aarc and file_bytes(carc, cent) and file_bytes(aarc, aent):
                src['files'] += 1
                return cells.cell_oams(carc, cent, cells.anim_cell(cells.nanr(aarc, aent), o.seq, o.time))
            src['ram (file not resolved)'] += 1
            return sprites.cell_oams(ram, o.cell)
        mine = sprites.oam_from_ram(ram, cells=from_files)
        for eng, base in (('main', 0), ('sub', 0x400)):
            got = visible(oam, base)
            m = [e for e in mine[eng] if (e[0] >> 8) & 3 != 2]
            same = sum(1 for a, b in zip(m, got) if a == b)
            tot[eng][0] += same; tot[eng][1] += len(m)
            full[eng] += same == len(m) == len(got)
    print('cell source per object:', dict(src))
    for eng in tot:
        print('%s: %d / %d OAM entries from spec equal the hardware OAM (%.2f%%); %d captures fully equal' % (
            eng, tot[eng][0], tot[eng][1], 100 * tot[eng][0] / max(1, tot[eng][1]), full[eng]))
