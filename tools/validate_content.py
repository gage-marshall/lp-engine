#!/usr/bin/env python3
"""Sanity-check an extracted content tree (usage: validate_content.py CONTENT [EXPORT]): images match the memory map, catalog regions are in range, and known tables hold the
values the analysis found (event ids exist in the directory, G.722.1 tables look right)."""
import sys, os, json, hashlib, struct
sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
from lp_engine import Engine
from event_trace import SUPPORTED_OPCODES, TraceMachine, trace as trace_event
from lpvm import read_typed_cell, write_typed_cell
root = sys.argv[1]; ok = True
X = sys.argv[2] if len(sys.argv) > 2 else os.path.join(os.path.dirname(os.path.abspath(root)), 'export')   # layer-2 export tree
import lp_paths
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
d = lp_paths.load_directory(root)['records']
resource_map = lp_paths.load_resource_map(root)
pool_entries = resource_map['slots']
entry_counts = {}
unresolved = []
for event_id, ref in d.items():
    pool = pool_entries.get(str(ref['slot']))
    if pool is None or ref['script'] >= len(pool['entries']):
        unresolved.append(event_id); continue
    resource = pool['entries'][ref['script']]
    if resource not in entry_counts:
        try:
            with open(os.path.join(lp_paths.scripts_dir(root), '%04d.sbin' % resource), 'rb') as f:
                entry_counts[resource] = struct.unpack('>I', f.read(12)[8:12])[0]
        except (OSError, struct.error):
            entry_counts[resource] = -1
    if ref['entry'] >= entry_counts[resource]: unresolved.append(event_id)
check(not unresolved and set(pool_entries) == {'0', '1', '2', '3'}, 'all %d script-directory records resolve through the four static resource pools (%d SBIN files)' % (len(d), len(entry_counts)))
sample = d['a063f380']; sample_resource = pool_entries[str(sample['slot'])]['entries'][sample['script']]
check((sample_resource, sample['entry'], entry_counts.get(sample_resource)) == (381, 22, 23), 'event a063f380 resolves to 0381.sbin entry 22 (23 entries)')
used_opcodes = set()
for resource in entry_counts:
    path = os.path.join(lp_paths.scripts_dir(root), '%04d.sbin' % resource)
    try:
        data = open(path, 'rb').read()
        header = struct.unpack('>11I', data[8:0x34])
        count = header[0]
        entries = struct.unpack('>%dI' % count, data[0x40:0x40 + 4 * count]) if count else ()
        if not entries:
            continue
        end = min(value for value in header[1:] if value > entries[0])
        pc = entries[0]
        while pc < end:
            if pc + 2 > end:
                break
            opcode, argc = data[pc], data[pc + 1]
            if opcode not in SUPPORTED_OPCODES or pc + 2 + 2 * argc > end:
                # Sections can contain non-instruction payload after their
                # reachable opcode stream; do not treat that payload as an
                # unsupported VM opcode.
                break
            used_opcodes.add(opcode)
            pc += 2 + 2 * argc
    except (OSError, ValueError, struct.error):
        pass
check(0x85 in used_opcodes and used_opcodes <= SUPPORTED_OPCODES, 'parsed SBIN opcode streams use tracer-supported opcodes, including native no-op 85 (%d opcode values)' % len(used_opcodes))
local_descriptor_count = 0
local_descriptor_scripts = 0
for resource in entry_counts:
    with open(os.path.join(lp_paths.scripts_dir(root), '%04d.sbin' % resource), 'rb') as source:
        count = struct.unpack('>11I', source.read(0x34)[8:0x34])[8]
        local_descriptor_count += count
        local_descriptor_scripts += int(count != 0)
check((local_descriptor_scripts, local_descriptor_count) == (12, 33), 'local typed-cell declarations: 33 across 12 extracted scripts')
typed_memory = bytearray()
write_typed_cell(typed_memory, 0x44000002, 0x89abcdef)
write_typed_cell(typed_memory, 0x83000001, 1)
check(read_typed_cell(typed_memory, 0x44000002) == 0x89abcdef and read_typed_cell(typed_memory, 0x83000001) == 1, 'typed VM cells preserve u32 and bit descriptors')
simple_trace = trace_event(root, '08000200', 160)
simple_text = [e['text'] for e in simple_trace['events'] if e['kind'] == 'text']
check(simple_trace['termination'] == 'return' and simple_text == ['（電話だけで、\nすぐ採用されてしまった）', '（春はやっぱり\n人手不足なんだな……）'], 'event tracer resolves and returns through 08000200 dialogue')
branch_trace = trace_event(root, 'a063f380', 80, {0x3e: 1})
cross_calls = [e for e in branch_trace['events'] if e['kind'] == 'call']
check(any(e['event_id'] == 'a0500180' and e['target']['resource_file'] == '0378.sbin' for e in cross_calls), 'event tracer follows opcode 16 across SBIN resources')
choice_trace = trace_event(root, 'a08c2880', 250, text_replies={2: 1})
choice_returns = [e for e in choice_trace['events'] if e['kind'] == 'return_by_id']
choice_calls = [e for e in choice_trace['events'] if e['kind'] == 'call']
check(choice_trace['termination'] == 'return' and choice_returns == [{'kind': 'return_by_id', 'event_id': 'a08c0f80'}] and any(e['event_id'] == 'a08c0f80' and e['target']['resource_file'] == '0343.sbin' for e in choice_calls), 'event tracer applies mode-2 text choice and deferred opcode-14 handoff')
tail_ref = (sample['slot'] << 24) | (sample['script'] << 12) | sample['entry']
tail_machine = TraceMachine(lp_paths.scripts_dir(root), {'records': d}, resource_map, d['08000200'])
tail_machine.acc = tail_ref
tail_outcome = tail_machine.finish_return()
check(tail_outcome and tail_outcome[0] == 'call_ref' and tail_outcome[1] == tail_ref and tail_outcome[2]['resource_file'] == '0381.sbin', 'event tracer follows packed root-return script references')
tail_machine.write_local(0, 0x1234)
check(tail_machine.read_local(0) == 0, 'event tracer honors undeclared local cells as native no-ops')
local_machine = TraceMachine(lp_paths.scripts_dir(root), {'records': d}, resource_map, d['0b4e0000'])
local_machine.write_local(0, 0x1234)
local_machine.write_local(5, 1)
check(local_machine.read_local(0) == 0x34 and local_machine.read_local(5) == 0, 'event tracer applies declared local-cell width and bounds')
for name in ('event_id_table_kind3', 'event_id_table_kind5', 'event_id_table_kind6'):
    ids = struct.unpack('<3I', eng.table(name)); check(all('%08x' % i in d for i in ids), '%s ids %s all exist in the script directory' % (name, [hex(i) for i in ids]))
check(list(struct.unpack('<8h', eng.table('g7221_expected_bits'))) == [52, 47, 43, 37, 29, 22, 16, 0], 'G.722.1 expected_bits table = {52,47,43,37,29,22,16,0}')
check(eng.cstr('static', 0x212427d) == '姉ヶ崎', 'embedded UTF-8 string at 0x212427d is 姉ヶ崎')
es = json.load(open(os.path.join(root, 'engine', 'strings.json')))
check(any('スキスキダイスキ' in e['text'] for e in es) and any('ぁあぃいぅうぇえぉお' in e['text'] for e in es), 'embedded strings include ASR vocabulary and the kana tables (%d strings)' % len(es))
for sub in ('assets', 'engine'):
    check(os.path.isdir(os.path.join(root, sub)), 'directory %s present' % sub)
for sub in ('scripts', 'strings', 'text', 'graphics', 'models', 'sound', 'fonts'):
    check(os.path.isdir(os.path.join(X, sub)), 'export directory %s present' % sub)
# rendering data: face tables, expressions, shading, variants, camera presets
ft = json.load(open(os.path.join(root, 'engine', 'face_tables.json')))
check(len(ft['moods']) == 45 and len(ft['mouth_rows']) == 9 and ft['light_colors'] == [0x7fff, 0x4210, 0x2108, 0x7fff], 'face tables: 45 moods, 9 mouth rows, light colours')
check(all(0 <= m['brow'] <= 5 and 0 <= m['cheek'] <= 1 and 0 <= m['mouth_row'] <= 8 for m in ft['moods']), 'every mood references an existing brow/cheek/mouth row')
ex = json.load(open(os.path.join(X, 'models', 'expressions', 'expressions.json')))['heroines']
check(all(len(ex[p]['eye']) == 23 and len(ex[p]['brow']) == 6 and len(ex[p]['cheek']) == 2 and len(ex[p]['mouth']) == 12 for p in 'mrn'), 'expression textures: 23 eyes, 6 brows, 2 cheeks, 12 mouths per heroine')
check(all(os.path.exists(os.path.join(X, 'models', 'expressions', v['png'])) for p in ex for s in ex[p].values() for v in s.values()), 'all expression PNGs exist')
used_eyes = {r[c] for r in ft['eye_table'] for c in range(18) if r[c] >= 0}
check(used_eyes <= set(range(23)), 'eye table only references eye variants 0..22 (%s)' % sorted(used_eyes))
sd = json.load(open(os.path.join(X, 'models', 'shading', 'tm_model_block_base.json')))['tmbsd']
check(all(sd['m_base_%d' % i]['toon'][-1] == 0x7fff and sd['m_base_%d' % i]['toon'][0] != 0x7fff for i in range(3)), 'base toon tables are tint ramps ending in white (day/evening/night)')
cp = json.load(open(os.path.join(X, 'camera', 'presets.json')))['presets']
check(len(cp) == 150 and '1000' in cp and cp['1000']['eye'] == [0.0, 144.0, 150.0], 'camera presets: 150, preset 1000 eye (0,144,150)')
var = json.load(open(os.path.join(X, 'models', 'variants', 'variants.json')))['textures']
check(len(var) > 100, 'texture variants indexed (%d textures)' % len(var))
mt = json.load(open(os.path.join(X, 'models', 'motion_table.json')))['records']
mdir = os.path.join(X, 'models', 'motions')
check(len(mt) == 974 and all(r['stems'] and all(os.path.exists(os.path.join(mdir, x + '.json')) for x in r['stems']) for r in mt), 'motion table: 974 indices, every index resolves to exported motions of its own heroine')
check(all(r['stem'] in r['stems'] for r in mt), 'every index resolves to one exact motion stem (<p>_<NNNN>_<info0*10+variant>)')
check(all(x.startswith(r['heroine'] + '_') for r in mt for x in r['stems']), 'index ranges 0-738/739-840/841-973 match m/r/n motion prefixes')
check(all(-1 <= r.get('mood', 0) < 45 for r in mt), 'default moods within 0..44')
mi = json.load(open(os.path.join(X, 'sound', 'midi', 'index.json')))['sequences']
check(len(mi) == 152 and not any('error' in m for m in mi) and not any(m.get('warnings') for m in mi), 'all 152 sequences interpreted without errors or warnings')
check(sum(1 for m in mi if m.get('notes', 0) > 0 and os.path.exists(os.path.join(X, 'sound', 'midi', m['name'] + '.mid'))) >= 150, 'MIDI written for every non-empty sequence')
sfd = os.path.join(X, 'sound', 'soundfonts')
check(len([f for f in os.listdir(sfd) if f.endswith('.sf2')]) >= 380 and all(open(os.path.join(sfd, f), 'rb').read(12)[8:] == b'sfbk' for f in os.listdir(sfd)[:20]), 'SoundFonts written per bank (%d)' % len(os.listdir(sfd)))
fj = [json.load(open(os.path.join(X, 'fonts', n + '.json'))) for n in ('LC10_UTF8_R90', 'LC12_UTF8_R90')]
check(all(f['glyphs'] == 3833 and f['encoding'] == 'UTF-8' and f['stored_rotation_deg'] == 90 for f in fj), 'fonts: LC10/LC12 NFTR parsed (3833 glyphs, UTF-8, stored rotated 90)')
cov = set(r for f in fj[1:] for a, b, g in f['cmap_ranges'] for r in range(a, b + 1))
miss = set()
for fn in os.listdir(os.path.join(X, 'strings')):
    if fn.endswith('.json'):
        for v in json.load(open(os.path.join(X, 'strings', fn))).values(): miss |= {c for c in v if c != '\n' and ord(c) not in cov}
check(miss <= {'\x17', '\ue724'}, 'LC12 covers every character used by the game strings (except %s)' % sorted(hex(ord(c)) for c in miss))
tr = json.load(open(os.path.join(root, 'engine', 'text_rules.json')))
check(tr['speed_updates_per_char'] == [10, 8, 4, 2, 1] and tr['text_palette_rgb555'][3] == 0x1f, 'text rules: speed table and highlight colour read from the engine image')
print('ALL OK' if ok else 'PROBLEMS FOUND'); sys.exit(0 if ok else 1)
