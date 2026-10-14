"""Replay a play_monitor session from its anchor state and record main RAM every frame.

    .venv/bin/python sandbox/rec/record.py SESSION.jsonl OUTDIR [--frames N] [--shot-every N]

The session must contain an 'anchor' record (play_monitor --save-at ID) and the 'in' records after it.
Output:
    OUTDIR/ram0.bin            main RAM (0x2000000..0x2400000) at the anchor (before the first replayed frame)
    OUTDIR/diff.npz            per frame: changed 32-bit word indices and new values (frame k = after k+1 frames)
    OUTDIR/hash.txt            per frame sha1 of main RAM (determinism check: two runs must agree)
    OUTDIR/shots/fNNNNNN.png   screenshots
"""
import argparse, hashlib, json, os, sys
import numpy as np
ROOT = os.path.dirname(os.path.dirname(os.path.dirname(os.path.abspath(__file__))))
sys.path.insert(0, os.path.join(ROOT, 'tools'))
from emu_driver import Emu  # noqa: E402

RAM0, RAMSZ = 0x2000000, 0x400000


def load_session(path):
    recs = [json.loads(l) for l in open(path)]
    anc = [r for r in recs if r['ev'] == 'anchor']
    if not anc:
        sys.exit('no anchor record in ' + path)
    f0 = anc[0]['f']
    ins = [r for r in recs if r['ev'] == 'in']
    before = [r for r in ins if r['f'] < f0]
    after = [r for r in ins if r['f'] >= f0]
    init = before[-1] if before else {'mask': 0, 'touch': None}
    return anc[0], f0, init, after, recs


def apply_in(emu, r):
    emu.e.input.keypad_update(r['mask'])
    if r['touch']:
        emu.e.input.touch_set_pos(*r['touch'])
    else:
        emu.e.input.touch_release()


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument('session')
    ap.add_argument('out')
    ap.add_argument('--frames', type=int, default=0, help='0 = until the last logged frame of the session')
    ap.add_argument('--shot-every', type=int, default=4)
    a = ap.parse_args()
    anc, f0, init, ins, recs = load_session(a.session)
    state = anc['path']
    end = a.frames or (recs[-1]['f'] - f0)
    os.makedirs(os.path.join(a.out, 'shots'), exist_ok=True)
    emu = Emu(os.path.join(ROOT, 'rom', 'loveplus_plus_j.nds'), state)
    emu.pin_rtc(*anc['rtc']) if 'rtc' in anc else emu.pin_rtc()
    ram = emu.ram()
    prev = np.frombuffer(ram, dtype='<u4').copy()
    prev.tofile(os.path.join(a.out, 'ram0.bin'))
    apply_in(emu, init)
    idx_all, val_all, cnt = [], [], []
    hashes = []
    qi = 0
    for k in range(end):
        f = f0 + k
        while qi < len(ins) and ins[qi]['f'] <= f:
            apply_in(emu, ins[qi]); qi += 1
        emu.step(1)
        cur = np.frombuffer(ram, dtype='<u4').copy()
        ch = np.nonzero(cur != prev)[0].astype('<u4')
        idx_all.append(ch); val_all.append(cur[ch]); cnt.append(len(ch))
        hashes.append(hashlib.sha1(cur.tobytes()).hexdigest())
        prev = cur
        if a.shot_every and k % a.shot_every == 0:
            emu.shot(os.path.join(a.out, 'shots', 'f%06d.png' % k), rotate=True)
        if k % 500 == 0:
            print('frame', k, '/', end, 'changed words', len(ch), flush=True)
    np.savez_compressed(os.path.join(a.out, 'diff.npz'), count=np.array(cnt, dtype='<u4'),
                        idx=np.concatenate(idx_all) if idx_all else np.zeros(0, '<u4'),
                        val=np.concatenate(val_all) if val_all else np.zeros(0, '<u4'))
    open(os.path.join(a.out, 'hash.txt'), 'w').write('\n'.join(hashes) + '\n')
    json.dump({'session': a.session, 'state': state, 'anchor_frame': f0, 'frames': end}, open(os.path.join(a.out, 'meta.json'), 'w'), indent=1)
    print('done', end, 'frames')


if __name__ == '__main__':
    main()
