"""NitroSystem sound archive (SDAT) extractor.

    sdat.py sound_data.sdat OUT_DIR

Writes: waves/<wavearc>/NNN.wav (decoded SWAV, with loop point in a 'smpl' chunk), streams/<name>.wav (decoded STRM, loop in 'smpl'),
banks/<name>.json (instrument definitions), sequences/<name>.sseq (raw sequence data) and index.json (names, ids, bank/wave-archive links,
sequence parameters, stream parameters). Written from the SDAT/SWAR/SWAV/SBNK/STRM layouts in the sound manuals and SDK headers (read-only reference).
"""
import json, os, struct, sys, re

STEP = [7, 8, 9, 10, 11, 12, 13, 14, 16, 17, 19, 21, 23, 25, 28, 31, 34, 37, 41, 45, 50, 55, 60, 66, 73, 80, 88, 97, 107, 118, 130, 143, 157, 173, 190,
        209, 230, 253, 279, 307, 337, 371, 408, 449, 494, 544, 598, 658, 724, 796, 876, 963, 1060, 1166, 1282, 1411, 1552, 1707, 1878, 2066, 2272,
        2499, 2749, 3024, 3327, 3660, 4026, 4428, 4871, 5358, 5894, 6484, 7132, 7845, 8630, 9493, 10442, 11487, 12635, 13899, 15289, 16818, 18500,
        20350, 22385, 24623, 27086, 29794, 32767]
IDX = [-1, -1, -1, -1, 2, 4, 6, 8]


def adpcm_block(data, pred=None, idx=None):
    """IMA-ADPCM (NDS flavour): data starts with s16 predictor, u16 step index, then nibbles (low nibble first). Returns int16 samples (excluding header)."""
    if pred is None: pred, idx = struct.unpack_from('<hH', data, 0); body = data[4:]
    else: body = data
    out = []
    idx = min(88, idx)
    for byte in body:
        for nib in (byte & 15, byte >> 4):
            step = STEP[idx]; diff = step >> 3
            if nib & 1: diff += step >> 2
            if nib & 2: diff += step >> 1
            if nib & 4: diff += step
            pred = max(-32768, min(32767, pred - diff if nib & 8 else pred + diff))
            idx = max(0, min(88, idx + IDX[nib & 7])); out.append(pred)
    return out


def write_wav(path, samples_per_channel, rate, loop=None):
    """samples_per_channel: list of int16 lists (one per channel); loop = (start_sample, end_sample) or None"""
    ch = len(samples_per_channel); n = min(len(c) for c in samples_per_channel) if ch else 0
    inter = bytearray()
    for i in range(n):
        for c in samples_per_channel: inter += struct.pack('<h', c[i])
    fmt = struct.pack('<HHIIHH', 1, ch, rate, rate * ch * 2, ch * 2, 16)
    chunks = b'fmt ' + struct.pack('<I', 16) + fmt + b'data' + struct.pack('<I', len(inter)) + bytes(inter)
    if loop:
        smpl = struct.pack('<9I', 0, 0, int(1e9 / rate), 60, 0, 0, 0, 1, 0) + struct.pack('<6I', 0, 0, loop[0], max(loop[0], loop[1] - 1), 0, 0)
        chunks += b'smpl' + struct.pack('<I', len(smpl)) + smpl
    with open(path, 'wb') as f: f.write(b'RIFF' + struct.pack('<I', 4 + len(chunks)) + b'WAVE' + chunks)


def decode_swav(d, off):
    """SWAV at off (12-byte param + data); returns (samples, rate, loop|None, format)"""
    fmt, loopflag, rate, timer, loopstart, looplen = struct.unpack_from('<BBHHHI', d, off)
    return fmt, loopflag, rate, loopstart, looplen


def swav_to_samples(d, off, size):
    fmt, loopflag, rate, timer, loopstart, looplen = struct.unpack_from('<BBHHHI', d, off)
    data = d[off + 12:off + size] if size else d[off + 12:]
    # data length from loop info when size unknown
    if fmt == 0:
        total_words = loopstart + looplen
        raw = data[:total_words * 4] if total_words else data
        s = [(b - 256 if b > 127 else b) << 8 for b in raw]; ls, ll = loopstart * 4, looplen * 4
    elif fmt == 1:
        total_words = loopstart + looplen
        raw = data[:total_words * 4] if total_words else data
        s = list(struct.unpack('<%dh' % (len(raw) // 2), raw[:len(raw) // 2 * 2])); ls, ll = loopstart * 2, looplen * 2
    else:
        total_words = loopstart + looplen
        raw = data[:total_words * 4] if total_words else data
        s = adpcm_block(raw); ls, ll = max(0, (loopstart * 4 - 4) * 2), looplen * 4 * 2
    loop = (ls, ls + ll) if loopflag else None
    return s, rate, loop, fmt


class SDAT:
    def __init__(self, d):
        self.d = d; U32 = self.u32 = lambda o: struct.unpack_from('<I', d, o)[0]; self.u16 = lambda o: struct.unpack_from('<H', d, o)[0]
        assert d[:4] == b'SDAT'
        self.symb, _, self.info, _, self.fat, _, self.file, _ = struct.unpack_from('<8I', d, 0x10)
        n = U32(self.fat + 8)
        self.files = [struct.unpack_from('<II', d, self.fat + 12 + 16 * i) for i in range(n)]
        self.kinds = ['seq', 'seqarc', 'bank', 'wavearc', 'player', 'group', 'strm_player', 'strm']
        self.names = {}; self.info_recs = {}
        symrecs = struct.unpack_from('<8I', d, self.symb + 8); inforecs = struct.unpack_from('<8I', d, self.info + 8)
        for k, kind in enumerate(self.kinds):
            sr = self.symb + symrecs[k]; cnt = U32(sr)
            self.names[kind] = [None] * cnt if kind == 'seqarc' else [self._cstr(self.symb + U32(sr + 4 + 4 * i)) if U32(sr + 4 + 4 * i) else None for i in range(cnt)]
            ir = self.info + inforecs[k]; icnt = U32(ir)
            self.info_recs[kind] = [self.info + U32(ir + 4 + 4 * i) if U32(ir + 4 + 4 * i) else None for i in range(icnt)]

    def _cstr(self, o):
        e = self.d.index(b'\0', o); return self.d[o:e].decode('latin1')

    def file_bytes(self, fid):
        off, size = self.files[fid]; return self.d[off:off + size]


def parse_swar(f):
    """SWAR file -> list of (offset, size) of SWAV entries (within the file)"""
    cnt = struct.unpack_from('<I', f, 0x38)[0]
    offs = [struct.unpack_from('<I', f, 0x3c + 4 * i)[0] for i in range(cnt)] + [len(f)]
    return [(offs[i], offs[i + 1] - offs[i]) for i in range(cnt)]


def parse_sbnk(f):
    cnt = struct.unpack_from('<I', f, 0x38)[0]; insts = []
    def inst_param(o, typ):
        w0, w1, key, a, dcy, s, r, pan = struct.unpack_from('<HHBBBBBB', f, o)
        return {'type': typ, 'wave': w0, 'wavearc': w1, 'original_key': key, 'attack': a, 'decay': dcy, 'sustain': s, 'release': r, 'pan': pan}
    for i in range(cnt):
        v = struct.unpack_from('<I', f, 0x3c + 4 * i)[0]; typ, off = v & 0xff, v >> 8
        if typ == 0: insts.append(None); continue
        if typ <= 5: insts.append(inst_param(off, typ))                  # offset points straight at the 10-byte param (type is in the table)
        elif typ == 0x10:
            lo, hi = f[off], f[off + 1]
            insts.append({'type': 'drumset', 'min': lo, 'max': hi, 'regions': [inst_param(off + 2 + 12 * k + 2, struct.unpack_from('<H', f, off + 2 + 12 * k)[0]) for k in range(hi - lo + 1)]})
        elif typ == 0x11:
            keys = list(f[off:off + 8]); n = sum(1 for k in keys if k) if keys[0] != 0 or True else 0
            n = next((i + 1 for i, k in enumerate(keys) if k == 127), len([k for k in keys if k]))
            insts.append({'type': 'keysplit', 'keys': keys[:n], 'regions': [inst_param(off + 8 + 12 * k + 2, struct.unpack_from('<H', f, off + 8 + 12 * k)[0]) for k in range(n)]})
        else: insts.append({'type': typ})
    return insts


def parse_strm(f):
    """STRM file -> (channels list of int16, rate, loop|None)"""
    head = 16
    wtype, loop, ch, _ = f[head + 8], f[head + 9], f[head + 10], f[head + 11]
    rate, _time = struct.unpack_from('<HH', f, head + 12); loop_off, nsamp, data_off, nblk, blen, bsamp, lblen, lbsamp = struct.unpack_from('<8I', f, head + 16)
    # DATA block follows HEAD; blocks are interleaved per channel
    dpos = data_off; chans = [[] for _ in range(ch)]
    for b in range(nblk):
        ln = blen if b < nblk - 1 else lblen
        for c in range(ch):
            seg = f[dpos:dpos + ln]; dpos += ln
            if wtype == 0: chans[c] += [(x - 256 if x > 127 else x) << 8 for x in seg]
            elif wtype == 1: chans[c] += list(struct.unpack('<%dh' % (len(seg) // 2), seg[:len(seg) // 2 * 2]))
            else: chans[c] += adpcm_block(seg)
    return chans, rate, ((loop_off, nsamp) if loop else None), wtype


def sanitize(s): return re.sub(r'[^\w.\-]', '_', s or 'unnamed')


def export(path, out):
    d = open(path, 'rb').read(); sd = SDAT(d); u32, u16 = sd.u32, sd.u16
    for sub in ('waves', 'streams', 'banks', 'sequences'): os.makedirs(os.path.join(out, sub), exist_ok=True)
    index = {'sequences': [], 'banks': [], 'wavearcs': [], 'streams': [], 'players': []}; stats = {'waves': 0, 'streams': 0, 'banks': 0, 'sequences': 0, 'errors': 0}
    for i, io in enumerate(sd.info_recs['wavearc']):
        if io is None: continue
        fid = u32(io) & 0xffffff; name = sanitize(sd.names['wavearc'][i] or 'wavearc%03d' % i)      # upper byte = load flags
        entry = {'id': i, 'name': name, 'file': fid, 'waves': 0}
        if fid < len(sd.files) and sd.files[fid][1]:
            f = sd.file_bytes(fid)
            try:
                swavs = parse_swar(f); od = os.path.join(out, 'waves', name); os.makedirs(od, exist_ok=True)
                for k, (o, sz) in enumerate(swavs):
                    if sz < 12: continue
                    s, rate, loop, fmt = swav_to_samples(f, o, sz)
                    write_wav(os.path.join(od, '%03d.wav' % k), [s], rate, loop); stats['waves'] += 1; entry['waves'] += 1
            except Exception as e: stats['errors'] += 1; entry['error'] = repr(e)[:80]
        index['wavearcs'].append(entry)
    for i, io in enumerate(sd.info_recs['bank']):
        if io is None: continue
        fid = u32(io) & 0xffffff; wavearcs = [u16(io + 4 + 2 * k) for k in range(4)]; name = sanitize(sd.names['bank'][i] or 'bank%03d' % i)
        entry = {'id': i, 'name': name, 'file': fid, 'wavearcs': [w for w in wavearcs if w != 0xffff]}
        if fid < len(sd.files) and sd.files[fid][1]:
            try: json.dump({'name': name, 'wavearcs': entry['wavearcs'], 'instruments': parse_sbnk(sd.file_bytes(fid))}, open(os.path.join(out, 'banks', name + '.json'), 'w')); stats['banks'] += 1
            except Exception as e: stats['errors'] += 1; entry['error'] = repr(e)[:80]
        index['banks'].append(entry)
    for i, io in enumerate(sd.info_recs['seq']):
        if io is None: continue
        fid = u32(io) & 0xffffff; bank = u16(io + 4); vol, cpr, ppr, ply = sd.d[io + 6], sd.d[io + 7], sd.d[io + 8], sd.d[io + 9]
        name = sanitize(sd.names['seq'][i] or 'seq%03d' % i)
        if fid < len(sd.files) and sd.files[fid][1]:
            open(os.path.join(out, 'sequences', name + '.sseq'), 'wb').write(sd.file_bytes(fid)); stats['sequences'] += 1
        index['sequences'].append({'id': i, 'name': name, 'file': fid, 'bank': bank, 'volume': vol, 'channel_priority': cpr, 'player_priority': ppr, 'player': ply})
    for i, io in enumerate(sd.info_recs['strm']):
        if io is None: continue
        fid = u32(io) & 0xffffff; name = sanitize(sd.names['strm'][i] or 'strm%03d' % i); entry = {'id': i, 'name': name, 'file': fid, 'volume': sd.d[io + 4]}
        try:
            chans, rate, loop, wt = parse_strm(sd.file_bytes(fid))
            write_wav(os.path.join(out, 'streams', name + '.wav'), chans, rate, loop); stats['streams'] += 1
            entry.update({'channels': len(chans), 'rate': rate, 'samples': len(chans[0]) if chans else 0, 'loop': loop, 'format': wt})
        except Exception as e: stats['errors'] += 1; entry['error'] = repr(e)[:80]
        index['streams'].append(entry)
    json.dump(index, open(os.path.join(out, 'index.json'), 'w'), indent=0)
    return stats


if __name__ == '__main__':
    print(export(sys.argv[1], sys.argv[2]))
