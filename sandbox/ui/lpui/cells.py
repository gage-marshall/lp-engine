"""Cell banks (NCER) and cell animations (NANR) from the game's files, as the sprite system uses them.

anim_cell(nanr, seq, time): the cell index shown at animation time `time` (12.12 frames), as Sprite_UpdateAnim gets it."""
import os, sys
HERE = os.path.dirname(os.path.abspath(__file__))
ROOT = os.path.dirname(os.path.dirname(os.path.dirname(HERE)))
sys.path.insert(0, os.path.join(ROOT, 'tools'))
import nns2d  # noqa: E402
from lpui.vram import file_bytes  # noqa: E402

_C = {}


def ncer(arc, entry):
    k = ('c', arc, entry)
    if k not in _C:
        _C[k] = nns2d.parse_ncer(file_bytes(arc, entry))
    return _C[k]


def nanr(arc, entry):
    k = ('a', arc, entry)
    if k not in _C:
        _C[k] = nns2d.parse_nanr(file_bytes(arc, entry))
    return _C[k]


def cell_oams(arc, entry, index):
    """[(a0, a1, a2)] of cell `index` of an NCER"""
    return list(ncer(arc, entry)['cells'][index]['oams'])


def anim_cell(an, seq, time):
    """cell index of sequence `seq` at animation time `time` (12.12 frames): the first frame whose cumulative end is >= time
    (frame ends are inclusive); past the end, loop sequences wrap to the loop start, others hold the last frame (checked
    against the sprite objects' current cells in RAM)"""
    s = an['sequences'][min(seq, len(an['sequences']) - 1)]
    frames = s['frames']
    total = sum(f['frames'] for f in frames) << 12
    t = time
    if total and t > total:
        if s['play_mode'] in (2, 4):                     # loop / reverse loop
            start = sum(f['frames'] for f in frames[:s['loop_start']]) << 12
            t = start + (t - start) % max(1, total - start)
        else:
            return frames[-1]['index']
    acc = 0
    for f in frames:
        acc += f['frames'] << 12
        if t <= acc:
            return f['index']
    return frames[-1]['index']
