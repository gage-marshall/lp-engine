"""Check lpui.cells.anim_cell: the cell each sprite object shows (its cell pointer in RAM, as an index into its NCER's cell
table) equals the one computed from the NANR sequence and the object's animation time, on every capture.
    .venv/bin/python sandbox/ui/verify_cells.py TRACE_DIR"""
import collections, glob, os, struct, sys
sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
from lpui import hudprobe, cells, sprites
from lpui.vram import file_bytes
sys.path.insert(0, os.path.join(os.path.dirname(os.path.abspath(__file__)), '../../tools'))
import nns2d  # noqa: E402
B = 0x2000000


def ram_cell_index(ram, o, coff, carc, cent):
    data = struct.unpack_from('<I', ram, (o.cell_bank & 0x3ff) * 0x18 + 0x213e5f4 - B)[0]
    fb = file_bytes(carc, cent)
    sig, blk = nns2d.blocks(fb); bo, bn = blk['CEBK']
    ncell, attr, cells_off = struct.unpack_from('<2HI', fb, bo)
    return (o.cell - (data - coff) - bo - cells_off) // (16 if attr & 1 else 8)


if __name__ == '__main__':
    trace = sys.argv[1]
    bufs = hudprobe.loaded_buffers(trace)
    res = collections.Counter()
    for d in sorted(glob.glob(os.path.join(trace, 'cap-f*'))):
        f = int(d.rsplit('-f', 1)[1])
        ram = open(os.path.join(d, 'ram.bin'), 'rb').read()
        for li in range(0x3f, -1, -1):
            p = struct.unpack_from('<I', ram, 0x213f6a0 - B + 4 * li)[0]
            while B <= p < B + 0x400000:
                o = sprites.SprObj(ram, p)
                if o.kind in (2, 3) and o.cell:
                    carc, cent, coff = hudprobe.resource_file(ram, o.cell_bank, bufs, f)
                    aarc, aent, _ = hudprobe.resource_file(ram, o.anim_bank, bufs, f)
                    if carc and aarc and file_bytes(aarc, aent) and file_bytes(carc, cent):
                        an = cells.nanr(aarc, aent); mine, got = cells.anim_cell(an, o.seq, o.time), ram_cell_index(ram, o, coff, carc, cent)
                        res[mine == got] += 1
                        if mine != got and '-v' in sys.argv:
                            sq = an['sequences'][min(o.seq, len(an['sequences']) - 1)]
                            print(f, aarc, aent, 'seq', o.seq, 't', o.time / 4096, 'mine', mine, 'ram', got, sq['play_mode'], sq['loop_start'],
                                  [(x['index'], x['frames']) for x in sq['frames']])
                    else:
                        res['unresolved file'] += 1
                p = o.next
    print('sprite objects whose current cell = NANR(sequence, time):', dict(res))
