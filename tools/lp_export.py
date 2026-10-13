#!/usr/bin/env python3
"""LovePlus+ exporter (layer 2): content tree -> common formats and translation interfaces.

    lp_export.py CONTENT_DIR EXPORT_DIR [--only stage,stage,...]

The engine does not read anything this writes.  These are tools for people: PNG/glTF/WAV/MIDI/SoundFont conversions,
the per-script string tables used as the translation interface, and a few JSON indexes.  Everything is derived from the
layer-0 tree (CONTENT_DIR) and can be regenerated at any time.

Stages: scripts, strings, text, graphics, nns2d, nns3d, sound, fonts
Output layout:
  scripts/directory.json   master event directory: event_id -> {slot, script, entry}
  strings/NNNN.json        per-script dialogue string table {byte_offset: text}
  text/NAME.json           system/UI/mail text resources [group][sub][line]
  graphics/                GTD images as PNG; graphics/nns = NitroSystem 2D (PNG + cells.json + anim.json)
  models/                  glTF binary models, motions, shading, expressions, variants, motion_table_copy
  camera/presets.json
  sound/                   waves/ streams/ banks/ sequences/ midi/ soundfonts/ index.json
  fonts/                   NFTR fonts as PNG + JSON
"""
import json, os, struct, sys, shutil

HERE = os.path.dirname(os.path.abspath(__file__))
sys.path.insert(0, HERE)
from txrc import parse as txrc_parse

STAGES = ['scripts', 'strings', 'text', 'graphics', 'nns2d', 'nns3d', 'sound', 'fonts']


def validate_script_directory(dirdoc, resource_map, scripts_dir):
    """Ensure every event id resolves to an extracted SBIN entry point."""
    entry_counts = {}
    failures = []
    for event_id, ref in dirdoc['records'].items():
        pool = resource_map['slots'].get(str(ref['slot']))
        if pool is None or ref['script'] >= len(pool['entries']):
            failures.append('%s has invalid slot/script %d/%d' % (event_id, ref['slot'], ref['script']))
            continue
        resource = pool['entries'][ref['script']]
        if resource not in entry_counts:
            path = os.path.join(scripts_dir, '%04d.sbin' % resource)
            try:
                data = open(path, 'rb').read(12)
                entry_counts[resource] = struct.unpack_from('>I', data, 8)[0]
            except (OSError, struct.error):
                entry_counts[resource] = None
        if entry_counts[resource] is None or ref['entry'] >= entry_counts[resource]:
            failures.append('%s resolves to %04d.sbin entry %d' % (event_id, resource, ref['entry']))
    if failures:
        raise ValueError('unresolved script directory references: ' + '; '.join(failures[:5]))
    return len(entry_counts)


def script_strings(d, h):
    """sequential length-prefixed UTF-8 strings of the script string table -> {offset: text}"""
    base = h[1]
    secs = sorted(x for x in h[1:] if x > base) + [len(d)]
    end = secs[0]
    out = {}; p = base
    while p < end and p + 1 < len(d):
        n = d[p]; q = p + 1
        if n & 0x80: n = d[p + 1] + 0x80; q = p + 2
        if q + n > end: break
        try: s = d[q:q + n].decode('utf-8')
        except UnicodeDecodeError: break
        out[p - base] = s
        p = q + n
    return out


def stage_scripts(C, X, st):
    src = C('assets', 'arc', 'tmx_event.arc')
    resource_map = json.load(open(C('engine', 'script_resource_map.json')))
    dirdoc = None
    for f in sorted(os.listdir(src)):
        if not f.endswith('.sbin'): continue
        d = open(os.path.join(src, f), 'rb').read()
        h = struct.unpack('>11I', d[8:0x34])
        if h[10] > 1000:   # the master directory script
            recs = [struct.unpack('>II', d[h[9] + 8 * k:h[9] + 8 * k + 8]) for k in range(h[10])]
            dirdoc = {'script_file': f, 'records': {'%08x' % i: {'slot': (r >> 24) & 0xf, 'script': (r >> 12) & 0xfff, 'entry': r & 0xfff} for i, r in recs}}
    os.makedirs(X('scripts'), exist_ok=True)
    resolved = validate_script_directory(dirdoc, resource_map, src)
    json.dump(dirdoc, open(X('scripts', 'directory.json'), 'w'), indent=0)
    st['scripts'] = {'directory_records': len(dirdoc['records']), 'resolved_script_files': resolved}


def stage_strings(C, X, st):
    src = C('assets', 'arc', 'tmx_event.arc')
    os.makedirs(X('strings'), exist_ok=True); n = 0
    for f in sorted(os.listdir(src)):
        if not f.endswith('.sbin'): continue
        d = open(os.path.join(src, f), 'rb').read()
        s = script_strings(d, struct.unpack('>11I', d[8:0x34]))
        if s:
            json.dump({str(k): v for k, v in s.items()}, open(X('strings', f.replace('.sbin', '.json')), 'w'), ensure_ascii=False, indent=0)
            n += len(s)
    st['strings'] = {'strings': n}


def stage_text(C, X, st):
    os.makedirs(X('text'), exist_ok=True); n = lines = 0
    tdir = C('assets', 'text')
    for f in sorted(os.listdir(tdir)):
        if not f.endswith('.txrc'): continue
        groups = txrc_parse(open(os.path.join(tdir, f), 'rb').read(), decrypted=True)
        doc = [[[l.decode('utf-8', 'replace') for l in sub] for sub in g] for g in groups]
        json.dump(doc, open(X('text', f.replace('.txrc', '.json')), 'w'), ensure_ascii=False)
        n += 1; lines += sum(len(s) for g in doc for s in g)
    st['text'] = {'files': n, 'lines': lines}


def stage_graphics(C, X, st):
    from gtd2png import gtd
    n = 0
    for root, _, fs in os.walk(C('assets')):
        for f in fs:
            if not f.endswith('.bin'): continue
            p = os.path.join(root, f); d = open(p, 'rb').read()
            if len(d) < 32 or d[0] != 4 or d[1] not in (4, 5): continue
            w, h_, po, pl, px = struct.unpack('<HHHHI', d[4:16])
            if po != 16 or 16 + pl + px != len(d) or not (0 < w <= 512 and 0 < h_ <= 512): continue
            try:
                img, _ = gtd(d)
                dst = X('graphics', os.path.relpath(p, C('assets'))[:-4] + '.png'); os.makedirs(os.path.dirname(dst), exist_ok=True)
                img.convert('RGBA').save(dst); n += 1
            except Exception:
                pass
    st['graphics'] = {'gtd_png': n}


def stage_nns2d(C, X, st):
    import nns2d_export
    n, fail = nns2d_export.export_tree(C('assets'), X('graphics', 'nns'))
    st['nns2d'] = {'resource_sets': n, 'failures': len(fail)}


def stage_nns3d(C, X, st):
    import nns3d_export
    st3, fail3 = nns3d_export.export_tree(C('assets'), X('models'))
    adir = C('assets', 'arc', 'async3d.arc')
    nmot, motbytes = nns3d_export.export_motions(adir, X('models', 'motions')) if os.path.isdir(adir) else (0, 0)
    wardrobe = nns3d_export.build_wardrobe(X('models'))
    n_sd, n_pl = nns3d_export.export_shading(C('assets', 'arc'), X('models', 'shading'))
    n_expr = nns3d_export.export_expressions(C('assets', 'arc'), X('models', 'expressions'))
    n_var = nns3d_export.export_variants(C('assets', 'arc'), X('models', 'variants'))
    os.makedirs(X('camera'), exist_ok=True); n_cam = nns3d_export.export_camera_presets(C('assets', 'arc'), X('camera', 'presets.json'))
    shutil.copy(C('engine', 'motion_table.json'), X('models', 'motion_table.json'))
    ft = json.load(open(C('engine', 'face_tables.json')))
    xp = X('models', 'expressions', 'expressions.json')
    if os.path.exists(xp):                      # flag eye variants no mood/column references (22 is a numbered debug texture)
        xj = json.load(open(xp)); used = {v for r in ft['eye_table'] for v in r if v >= 0}
        for h in xj['heroines'].values():
            for k, v in h.get('eye', {}).items(): v['unused'] = int(k) not in used
        json.dump(xj, open(xp, 'w'), indent=1)
    st['nns3d'] = {'models': st3.get('models', 0), 'triangles': st3.get('triangles', 0), 'animations': st3.get('animations', 0), 'failures': len(fail3),
                   'heroine_motions': nmot, 'wardrobe_outfits_hairs': wardrobe, 'shading_tables': n_sd, 'palettes': n_pl,
                   'expression_textures': n_expr, 'variant_textures': n_var, 'camera_presets': n_cam}


def stage_sound(C, X, st):
    import sdat, sseq, sf2
    sdir = X('sound'); os.makedirs(sdir, exist_ok=True)
    st['sdat'] = sdat.export(C('assets', 'sys', 'sound_data.sdat'), sdir)
    st['sseq'] = sseq.export_all(sdir, os.path.join(sdir, 'midi'))
    st['soundfonts'] = sf2.export_all(sdir, os.path.join(sdir, 'soundfonts'))


def stage_fonts(C, X, st):
    import nftr
    st['fonts'] = {'fonts': nftr.export_fonts(C('assets'), X('fonts'))}


def main():
    args = [a for a in sys.argv[1:] if not a.startswith('--')]
    if len(args) < 2:
        print(__doc__); return 1
    only = STAGES
    if '--only' in sys.argv:
        only = sys.argv[sys.argv.index('--only') + 1].split(',')
        args = [a for a in args if a != ','.join(only)]
    content, out = args[:2]
    C = lambda *a: os.path.join(content, *a)
    X = lambda *a: os.path.join(out, *a)
    os.makedirs(out, exist_ok=True)
    st = {}
    for name in only:
        globals()['stage_' + name](C, X, st)
        print(name, 'done', flush=True)
    mp = X('export_manifest.json')
    old = json.load(open(mp)) if os.path.exists(mp) else {}
    old.update(st)
    json.dump(old, open(mp, 'w'), indent=1)
    print(json.dumps(st, indent=1))
    return 0


if __name__ == '__main__':
    sys.exit(main())
