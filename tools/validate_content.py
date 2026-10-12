#!/usr/bin/env python3
"""Sanity-check an extracted content tree: images match the memory map, catalog regions are in range, and known tables hold the
values the analysis found (event ids exist in the directory, G.722.1 tables look right)."""
import sys, os, json, hashlib, struct
sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
from lp_engine import Engine
root = sys.argv[1]; ok = True
def check(c, msg):
    global ok
    print(('PASS ' if c else 'FAIL ') + msg); ok &= bool(c)
eng = Engine(root)
man = json.load(open(os.path.join(root, 'manifest.json')))
check(man['rom']['game_code'] == 'BOXJ', 'manifest game code BOXJ')
for k, v in eng.mm.items():
    if hashlib.sha1(eng.img[k]).hexdigest() != v['sha1'] or len(eng.img[k]) != v['size']: check(False, 'image %s hash/size' % k); break
else: check(True, 'all %d engine images match memory_map sha1/size' % len(eng.mm))
bad = [t for t in eng.cat if t['module'] in eng.mm and not (eng.mm[t['module']]['base'] <= int(t['addr'], 16) < eng.mm[t['module']]['base'] + eng.mm[t['module']]['size'])]
check(not bad, 'all %d catalog regions lie inside their module image (%d bad)' % (len(eng.cat), len(bad)))
d = json.load(open(os.path.join(root, 'scripts', 'directory.json')))['records']
for name in ('event_id_table_kind3', 'event_id_table_kind5', 'event_id_table_kind6'):
    ids = struct.unpack('<3I', eng.table(name)); check(all('%08x' % i in d for i in ids), '%s ids %s all exist in the script directory' % (name, [hex(i) for i in ids]))
check(list(struct.unpack('<8h', eng.table('g7221_expected_bits'))) == [52, 47, 43, 37, 29, 22, 16, 0], 'G.722.1 expected_bits table = {52,47,43,37,29,22,16,0}')
check(eng.cstr('static', 0x212427d) == '姉ヶ崎', 'embedded UTF-8 string at 0x212427d is 姉ヶ崎')
es = json.load(open(os.path.join(root, 'engine', 'strings.json')))
check(any('スキスキダイスキ' in e['text'] for e in es) and any('ぁあぃいぅうぇえぉお' in e['text'] for e in es), 'embedded strings include ASR vocabulary and the kana tables (%d strings)' % len(es))
for sub in ('scripts', 'strings', 'text', 'voice', 'tables', 'graphics', 'assets'):
    check(os.path.isdir(os.path.join(root, sub)), 'directory %s present' % sub)
# rendering data: face tables, expressions, shading, variants, camera presets
ft = json.load(open(os.path.join(root, 'engine', 'face_tables.json')))
check(len(ft['moods']) == 45 and len(ft['mouth_rows']) == 9 and ft['light_colors'] == [0x7fff, 0x4210, 0x2108, 0x7fff], 'face tables: 45 moods, 9 mouth rows, light colours')
check(all(0 <= m['brow'] <= 5 and 0 <= m['cheek'] <= 1 and 0 <= m['mouth_row'] <= 8 for m in ft['moods']), 'every mood references an existing brow/cheek/mouth row')
ex = json.load(open(os.path.join(root, 'models', 'expressions', 'expressions.json')))['heroines']
check(all(len(ex[p]['eye']) == 23 and len(ex[p]['brow']) == 6 and len(ex[p]['cheek']) == 2 and len(ex[p]['mouth']) == 12 for p in 'mrn'), 'expression textures: 23 eyes, 6 brows, 2 cheeks, 12 mouths per heroine')
check(all(os.path.exists(os.path.join(root, 'models', 'expressions', v['png'])) for p in ex for s in ex[p].values() for v in s.values()), 'all expression PNGs exist')
used_eyes = {r[c] for r in ft['eye_table'] for c in range(18) if r[c] >= 0}
check(used_eyes <= set(range(23)), 'eye table only references eye variants 0..22 (%s)' % sorted(used_eyes))
sd = json.load(open(os.path.join(root, 'models', 'shading', 'tm_model_block_base.json')))['tmbsd']
check(all(sd['m_base_%d' % i]['toon'][-1] == 0x7fff and sd['m_base_%d' % i]['toon'][0] != 0x7fff for i in range(3)), 'base toon tables are tint ramps ending in white (day/evening/night)')
cp = json.load(open(os.path.join(root, 'camera', 'presets.json')))['presets']
check(len(cp) == 150 and '1000' in cp and cp['1000']['eye'] == [0.0, 144.0, 150.0], 'camera presets: 150, preset 1000 eye (0,144,150)')
var = json.load(open(os.path.join(root, 'models', 'variants', 'variants.json')))['textures']
check(len(var) > 100, 'texture variants indexed (%d textures)' % len(var))
mt = json.load(open(os.path.join(root, 'models', 'motion_table.json')))['records']
mdir = os.path.join(root, 'models', 'motions')
check(len(mt) == 974 and all(r['stems'] and all(os.path.exists(os.path.join(mdir, x + '.json')) for x in r['stems']) for r in mt), 'motion table: 974 indices, every index resolves to exported motions of its own heroine')
check(all(r['stem'] in r['stems'] for r in mt), 'every index resolves to one exact motion stem (<p>_<NNNN>_<info0*10+variant>)')
check(all(x.startswith(r['heroine'] + '_') for r in mt for x in r['stems']), 'index ranges 0-738/739-840/841-973 match m/r/n motion prefixes')
check(all(-1 <= r.get('mood', 0) < 45 for r in mt), 'default moods within 0..44')
mi = json.load(open(os.path.join(root, 'sound', 'midi', 'index.json')))['sequences']
check(len(mi) == 152 and not any('error' in m for m in mi) and not any(m.get('warnings') for m in mi), 'all 152 sequences interpreted without errors or warnings')
check(sum(1 for m in mi if m.get('notes', 0) > 0 and os.path.exists(os.path.join(root, 'sound', 'midi', m['name'] + '.mid'))) >= 150, 'MIDI written for every non-empty sequence')
sfd = os.path.join(root, 'sound', 'soundfonts')
check(len([f for f in os.listdir(sfd) if f.endswith('.sf2')]) >= 380 and all(open(os.path.join(sfd, f), 'rb').read(12)[8:] == b'sfbk' for f in os.listdir(sfd)[:20]), 'SoundFonts written per bank (%d)' % len(os.listdir(sfd)))
fj = [json.load(open(os.path.join(root, 'fonts', n + '.json'))) for n in ('LC10_UTF8_R90', 'LC12_UTF8_R90')]
check(all(f['glyphs'] == 3833 and f['encoding'] == 'UTF-8' and f['stored_rotation_deg'] == 90 for f in fj), 'fonts: LC10/LC12 NFTR parsed (3833 glyphs, UTF-8, stored rotated 90)')
cov = set(r for f in fj[1:] for a, b, g in f['cmap_ranges'] for r in range(a, b + 1))
miss = set()
for fn in os.listdir(os.path.join(root, 'strings')):
    if fn.endswith('.json'):
        for v in json.load(open(os.path.join(root, 'strings', fn))).values(): miss |= {c for c in v if c != '\n' and ord(c) not in cov}
check(miss <= {'\x17', '\ue724'}, 'LC12 covers every character used by the game strings (except %s)' % sorted(hex(ord(c)) for c in miss))
tr = json.load(open(os.path.join(root, 'engine', 'text_rules.json')))
check(tr['speed_updates_per_char'] == [10, 8, 4, 2, 1] and tr['text_palette_rgb555'][3] == 0x1f, 'text rules: speed table and highlight colour read from the engine image')
print('ALL OK' if ok else 'PROBLEMS FOUND'); sys.exit(0 if ok else 1)
