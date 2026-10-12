"""SoundFont 2 writer: builds one .sf2 per SBNK bank from the extracted sound archive (banks/*.json + waves/*/NNN.wav), so the MIDI files written by
sseq.py play with the game's own instruments in any SoundFont player.

Mapping (SBNK -> SF2): simple instrument = one zone covering all keys; key-split = zones per key range; drum set = a normal bank-0 preset (so every track keeps its own channel/program) with one zone per key
(root key = the region's original key, so pitch = key - original key exactly as in the game); loop points from the wave; pan from the instrument pan;
ADSR converted from the NITRO-Composer envelope values (attack: exponential-in-dB model, decay/release: speed in dB/ms -> time for 100 dB, sustain:
squared law -> attenuation). The SBNK program number is the SF2 preset number (always bank 0; drum sets are ordinary presets with one zone per key).
    sf2.py SOUND_DIR OUT_DIR
"""
import struct, json, os, math, sys

try:
    import sseq
except ImportError:
    sys.path.insert(0, os.path.dirname(os.path.abspath(__file__))); import sseq

G_PAN, G_ATTACK, G_DECAY, G_SUSTAIN, G_RELEASE, G_INSTRUMENT, G_KEYRANGE, G_ATTEN, G_SAMPLE, G_MODES, G_ROOT = 17, 34, 36, 37, 38, 41, 43, 48, 53, 54, 58


def _tc(seconds): return int(max(-12000, min(8000, round(1200 * math.log2(max(seconds, 0.001))))))


def _attack_seconds(a):
    r = sseq._attack_rate(a)
    if r <= 0: return 0.001
    return max(0.001, math.ceil(math.log(0.5 / 72.3) / math.log(r)) * 0.005)


def _env_gens(reg):
    a, d, s, r = reg['attack'], reg['decay'], reg['sustain'], reg['release']
    t_dec = 100.0 / (sseq._decay_speed_db_per_ms(d) * 1000.0); t_rel = 100.0 / (sseq._decay_speed_db_per_ms(r) * 1000.0)
    atten = int(min(1440, max(0, round(-sseq._law_db(s) * 10))))
    return [(G_ATTACK, _tc(_attack_seconds(a))), (G_DECAY, _tc(t_dec)), (G_SUSTAIN, atten), (G_RELEASE, _tc(t_rel))]


def _chunk(cid, data, even=False):
    if even and len(data) & 1: data += b'\0'                           # SF2 INFO strings must have an even size
    return cid + struct.pack('<I', len(data)) + data + (b'\0' if len(data) & 1 else b'')


def _list(kind, body): return _chunk(b'LIST', kind + body)


def _name(s, n=20): return s.encode('ascii', 'replace')[:n - 1].ljust(n, b'\0')


def build(bank, sound_dir, out_path, title=None):
    """bank = parsed bank json; returns number of presets written"""
    synth = sseq.Synth.__new__(sseq.Synth); synth.dir = sound_dir; synth.bank = bank; synth.cache = {}
    idx = json.load(open(os.path.join(sound_dir, 'index.json'))); arcs = {w['id']: w['name'] for w in idx['wavearcs']}
    synth.wavearcs = [arcs[i] for i in bank['wavearcs']]
    smpl = bytearray(); shdr = []; sample_ids = {}

    def sample_id(reg):
        key = (reg['wavearc'], reg['wave'])
        if key in sample_ids: return sample_ids[key]
        wv = synth.wave(*key)
        if wv is None or wv[0] is None: return None
        s, rate, loop = wv; pcm = (s * 32767).astype('<i2').tobytes(); start = len(smpl) // 2; n = len(s)
        smpl.extend(pcm); smpl.extend(b'\0' * 92)                       # 46 zero points after each sample
        ls, le = (loop[0], loop[1] + 1) if loop else (0, 0)
        shdr.append((('w%d_%d' % key), start, start + n, start + ls, start + le, rate, reg['original_key'], 0, 0, 1))
        sample_ids[key] = len(shdr) - 1; return sample_ids[key]

    def zone_for(reg, lo, hi, loop):
        sid = sample_id(reg)
        if sid is None: return None
        gens = [(G_KEYRANGE, (lo, hi))] + _env_gens(reg) + [(G_PAN, int(max(-500, min(500, (reg['pan'] - 64) / 64.0 * 500))))]
        wv = synth.wave(reg['wavearc'], reg['wave'])
        if wv[2]: gens.append((G_MODES, 1))
        gens += [(G_ROOT, reg['original_key']), (G_SAMPLE, sid)]
        return gens

    insts = []; presets = []                                            # insts: (name, [zones]); presets: (name, program, bank, inst_index)
    for prg, ins in enumerate(bank['instruments']):
        if not ins or prg > 127: continue
        zones = []; t = ins['type']; drums = False
        if t == 1: z = zone_for(ins, 0, 127, None); zones = [z] if z else []
        elif t == 'keysplit':
            lo = 0
            for k, r in zip(ins['keys'], ins['regions']):
                z = zone_for(r, lo, min(127, k), None)
                if z: zones.append(z)
                lo = min(127, k) + 1
        elif t == 'drumset':
            drums = True
            for i, r in enumerate(ins['regions']):
                k = ins['min'] + i; z = zone_for(r, k, k, None)
                if z: zones.append(z)
        else: continue                                                   # PSG / noise / empty: no sample data
        if not zones: continue
        insts.append(('prg%d' % prg, zones)); presets.append(('prg%d%s' % (prg, ' drums' if drums else ''), prg, 0, len(insts) - 1))
    if not presets: return 0
    ibag = []; igen = []; inst_recs = []
    for name, zones in insts:
        inst_recs.append((name, len(ibag)))
        for gens in zones:
            ibag.append(len(igen))
            for g, v in gens: igen.append((g, v))
    pbag = []; pgen = []; phdr = []
    for name, prg, bk, ii in sorted(presets, key=lambda p: (p[2], p[1])):
        phdr.append((name, prg, bk, len(pbag))); pbag.append(len(pgen)); pgen.append((G_INSTRUMENT, ii))
    def gen_bytes(g, v):
        return struct.pack('<HBB', g, v[0], v[1]) if isinstance(v, tuple) else struct.pack('<Hh', g, v) if g != G_INSTRUMENT and g != G_SAMPLE else struct.pack('<HH', g, v)
    b = bytearray(); b += b''.join(struct.pack('<20sHHHIII', _name(n), p, bk, bi, 0, 0, 0) for n, p, bk, bi in phdr) + struct.pack('<20sHHHIII', _name('EOP'), 0, 0, len(pbag), 0, 0, 0)
    phdr_b = bytes(b)
    pbag_b = b''.join(struct.pack('<HH', g, 0) for g in pbag) + struct.pack('<HH', len(pgen), 0)
    pmod_b = struct.pack('<HHhHH', 0, 0, 0, 0, 0)
    pgen_b = b''.join(gen_bytes(g, v) for g, v in pgen) + struct.pack('<HH', 0, 0)
    inst_b = b''.join(struct.pack('<20sH', _name(n), bi) for n, bi in inst_recs) + struct.pack('<20sH', _name('EOI'), len(ibag))
    ibag_b = b''.join(struct.pack('<HH', g, 0) for g in ibag) + struct.pack('<HH', len(igen), 0)
    imod_b = struct.pack('<HHhHH', 0, 0, 0, 0, 0)
    igen_b = b''.join(gen_bytes(g, v) for g, v in igen) + struct.pack('<HH', 0, 0)
    shdr_b = b''.join(struct.pack('<20sIIIIIBbHH', _name(n), s, e, ls, le, rate, key, corr, link, typ) for n, s, e, ls, le, rate, key, corr, link, typ in shdr) + struct.pack('<20sIIIIIBbHH', _name('EOS'), 0, 0, 0, 0, 0, 0, 0, 0, 0)
    info = _chunk(b'ifil', struct.pack('<HH', 2, 4)) + _chunk(b'isng', b'EMU8000\0') + _chunk(b'INAM', (title or bank.get('name', 'bank')).encode() + b'\0', True) + _chunk(b'ISFT', b'loveplus sf2.py\0', True)
    sdta = _chunk(b'smpl', bytes(smpl))
    pdta = _chunk(b'phdr', phdr_b) + _chunk(b'pbag', pbag_b) + _chunk(b'pmod', pmod_b) + _chunk(b'pgen', pgen_b) + _chunk(b'inst', inst_b) + _chunk(b'ibag', ibag_b) + _chunk(b'imod', imod_b) + _chunk(b'igen', igen_b) + _chunk(b'shdr', shdr_b)
    body = b'sfbk' + _list(b'INFO', info) + _list(b'sdta', sdta) + _list(b'pdta', pdta)
    open(out_path, 'wb').write(b'RIFF' + struct.pack('<I', len(body)) + body)
    return len(presets)


def export_all(sound_dir, out_dir):
    os.makedirs(out_dir, exist_ok=True); n = skipped = 0
    for f in sorted(os.listdir(os.path.join(sound_dir, 'banks'))):
        bank = json.load(open(os.path.join(sound_dir, 'banks', f)))
        if build(bank, sound_dir, os.path.join(out_dir, f[:-5] + '.sf2')): n += 1
        else: skipped += 1
    return {'soundfonts': n, 'empty_banks': skipped}


if __name__ == '__main__': print(export_all(sys.argv[1], sys.argv[2]))
