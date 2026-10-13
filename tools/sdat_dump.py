"""Python side of the Rust SDAT differential test (engine/src/bin/sdat_dump.rs prints the same text).  usage: sdat_dump.py SOUND_DATA.sdat"""
import hashlib, os, struct, sys
sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import sdat

def sha(b): return hashlib.sha1(b).hexdigest()[:12]

def region(r): return '%s,%d,%d,%d,%d,%d,%d,%d,%d' % (r['type'], r['wave'], r['wavearc'], r['original_key'], r['attack'], r['decay'], r['sustain'], r['release'], r['pan'])

def inst(i):
    if i is None: return 'N'
    t = i['type']
    if t == 'drumset': return 'D %d %d [%s]' % (i['min'], i['max'], ';'.join(region(r) for r in i['regions']))
    if t == 'keysplit': return 'K %s [%s]' % (','.join(map(str, i['keys'])), ';'.join(region(r) for r in i['regions']))
    if isinstance(t, int) and t <= 5: return 'S ' + region(i)
    return 'O %d' % t

def main(path):
    d = open(path, 'rb').read(); sd = sdat.SDAT(d); out = []; u32 = sd.u32; u16 = sd.u16
    for i, io in enumerate(sd.info_recs['wavearc']):
        if io is None: continue
        fid = u32(io) & 0xffffff
        if fid >= len(sd.files) or not sd.files[fid][1]: continue
        f = sd.file_bytes(fid)
        for k, (o, sz) in enumerate(sdat.parse_swar(f)):
            if sz < 12: continue
            s, rate, loop, fmt = sdat.swav_to_samples(f, o, sz)
            out.append('W %d %d fmt=%d rate=%d loop=%s n=%d %s' % (i, k, fmt, rate, loop, len(s), sha(struct.pack('<%dh' % len(s), *s))))
    for i, io in enumerate(sd.info_recs['bank']):
        if io is None: continue
        fid = u32(io) & 0xffffff; wa = [u16(io + 4 + 2 * k) for k in range(4)]
        if fid >= len(sd.files) or not sd.files[fid][1]: continue
        ins = sdat.parse_sbnk(sd.file_bytes(fid))
        out.append('B %d %s %d %s' % (i, wa, len(ins), sha('\n'.join(inst(x) for x in ins).encode())))
    for i, io in enumerate(sd.info_recs['seq']):
        if io is None: continue
        fid = u32(io) & 0xffffff; f = sd.file_bytes(fid); off = struct.unpack_from('<I', f, 24)[0]
        out.append('Q %d bank=%d vol=%d cpr=%d ppr=%d ply=%d n=%d %s' % (i, u16(io + 4), d[io + 6], d[io + 7], d[io + 8], d[io + 9], len(f) - off, sha(f[off:])))
    for i, io in enumerate(sd.info_recs['seqarc']):
        if io is None: continue
        f = sd.file_bytes(u32(io) & 0xffffff); data_off, cnt = struct.unpack_from('<II', f, 0x18)
        out.append('A %d count=%d' % (i, cnt))
        for k in range(cnt):
            off, bank, vol, cpr, ppr, ply = struct.unpack_from('<IHBBBB', f, 0x20 + 12 * k)
            out.append(' a %d %d off=%d bank=%d vol=%d cpr=%d ppr=%d ply=%d %s' % (i, k, off, bank, vol, cpr, ppr, ply, sha(f[data_off:])))
    for i, io in enumerate(sd.info_recs['player']):
        if io is None: continue
        out.append('P %d %d %d' % (i, u16(io), u16(io + 4)))
    sys.stdout.write('\n'.join(out) + '\n')

main(sys.argv[1])
