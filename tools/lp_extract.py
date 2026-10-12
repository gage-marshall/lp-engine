#!/usr/bin/env python3
"""LovePlus+ one-shot content extractor.

    lp_extract.py ROM.nds OUTDIR

Reads the user's ROM once and writes everything a reimplementation needs into OUTDIR (the "content tree"). Nothing from the
ROM needs to be kept afterwards. The engine loads OUTDIR, optionally overlaid by OUTDIR/../patch/ with the same relative
paths (translations etc.), so no ROM surgery is ever required.

Layout:
  manifest.json            ROM identity (sha1, game code), extractor version, per-stage counts
  scripts/NNNN.sbin        decrypted event scripts (big-endian VM bytecode, see docs/SCRIPT_VM.md)
  scripts/directory.json   master directory: event_id -> {slot, script, entry}
  strings/NNNN.json        per-script dialogue string table {byte_offset: text}  <- translation target
  text/NAME.json           system/UI/mail text resources [group][sub][line]       <- translation target
  voice/                   Siren14Voice.spp + .vtb (clips), lipsync.dat/_add.dat
  sound/                   waves/*.wav, streams/*.wav (decoded, loop points in smpl chunks), banks/*.json (instruments), sequences/*.sseq, index.json
  tables/g7221.pack        lookup tables for the voice decoder, dumped from the game's ARM9 (LPTB pack)
  graphics/  ...           character art as PNG (custom GTD format); graphics/nns/ = NitroSystem 2D (screens, sprite cells, animations)
  models/                  NitroSystem 3D models as glTF binary (.glb, bind pose, textures embedded)
  assets/                  every other NitroFS file; game archives unpacked into directories (LZ decompressed)
"""
import hashlib, json, os, struct, sys, shutil, collections
HERE = os.path.dirname(os.path.abspath(__file__))
sys.path.insert(0, HERE)
from blz_decompress import blz_decode
from lp_decrypt import decrypt
import unpack_arc
from txrc import parse as txrc_parse

VERSION = 1
GAME_CODE = b'BOXJ'


def nitrofs(rom):
    fnt_off, fnt_sz, fat_off, fat_sz = struct.unpack('<4I', rom[0x40:0x50])
    fat = [struct.unpack('<2I', rom[fat_off + i * 8:fat_off + i * 8 + 8]) for i in range(fat_sz // 8)]

    def walk(did, path):
        ent = struct.unpack('<IHH', rom[fnt_off + (did & 0xfff) * 8:fnt_off + (did & 0xfff) * 8 + 8])
        p = fnt_off + ent[0]; fid = ent[1]
        while True:
            t = rom[p]; p += 1
            if t == 0: break
            n = t & 0x7f; name = rom[p:p + n].decode('latin1'); p += n
            if t & 0x80:
                sub = struct.unpack('<H', rom[p:p + 2])[0]; p += 2
                yield from walk(sub, path + name + '/')
            else:
                yield path + name, fat[fid]; fid += 1
    return list(walk(0xf000, ''))


def arm9_static(rom):
    off, _entry, ram, size = struct.unpack('<4I', rom[0x20:0x30])
    raw = rom[off:off + size]
    k = raw.find(struct.pack('<I', 0xDEC00621))          # nitrocode marker inside ModuleParams
    if k < 0: return raw
    comp_end = struct.unpack('<I', raw[k - 8:k - 4])[0]  # compressed_static_end (RAM address); 0 = not compressed
    if comp_end == 0: return raw
    return blz_decode(raw, comp_end - ram)


def arm9_autoloads(rom, static):
    """split ITCM/DTCM initial images out of the decompressed static module (ModuleParams autoload list)"""
    off, _e, ram, size = struct.unpack('<4I', rom[0x20:0x30])
    k = static.find(struct.pack('<I', 0xDEC00621))
    lst_s, lst_e, auto_start = struct.unpack('<3I', static[k - 28:k - 16])
    out = {}; p = auto_start - ram
    for a in range(lst_s, lst_e, 12):
        addr, sz, bss = struct.unpack('<3I', static[a - ram:a - ram + 12])
        out['itcm' if addr == 0x1ff8000 else 'dtcm' if addr == 0x27e0000 else '%08x' % addr] = (addr, static[p:p + sz], bss)
        p += sz
    return out


def overlays(rom):
    fat_off, _ = struct.unpack_from('<II', rom, 0x48)
    tab_off, tab_sz = struct.unpack_from('<II', rom, 0x50)
    res = []
    for i in range(tab_sz // 32):
        oid, ram, size, bss, s0, s1, fid, comp = struct.unpack_from('<8I', rom, tab_off + 32 * i)
        fo, fe = struct.unpack_from('<II', rom, fat_off + 8 * fid)
        raw = rom[fo:fe]; csize, flag = comp & 0xFFFFFF, comp >> 24
        data = blz_decode(raw, csize if csize and csize <= len(raw) else len(raw)) if flag & 1 else raw
        res.append(dict(id=oid, ram=ram, size=size, bss=bss, sinit=[s0, s1], data=data))
    return res


# --- table pack for the voice decoder (LPTB): u32 magic, u32 count, count*(u32 addr,u32 len), then data ---
G7221_RANGES = [(0x20fbc00, 0x80), (0x2112e00, 0x3600)]


def write_pack(path, arm9, ranges):
    base = 0x2000000
    hdr = b'LPTB' + struct.pack('<I', len(ranges)) + b''.join(struct.pack('<II', a, n) for a, n in ranges)
    with open(path, 'wb') as f:
        f.write(hdr)
        for a, n in ranges: f.write(arm9[a - base:a - base + n])


def _jp_ok(t):
    kana = lambda c: '\u3041' <= c <= '\u30ff'
    if len(t) < 2 or any((ord(c) < 0x20 and c not in '\n\r\t') or 0xe000 <= ord(c) <= 0xf8ff for c in t): return False
    jp = sum(kana(c) or '\u4e00' <= c <= '\u9fff' or '\uff00' <= c <= '\uffef' for c in t)
    return jp >= 2 and sum(kana(c) for c in t) >= 1


def engine_strings(images, catalog):
    """Japanese strings embedded in the game's data (kana tables, default names, speech-recognition vocabulary...).
    Scanned inside the catalog's data regions (never code); utf-8 first, cp932 fallback, strict Japanese-text filter."""
    out = []
    for t in catalog['tables']:
        if t['module'] not in images or t['kind'] == 'pointer_table': continue
        base, img = images[t['module']]
        a = int(t['addr'], 16) - base; blob = img[a:a + t['size_est']]
        i = 0
        while i < len(blob):
            j = blob.find(b'\0', i)
            if j < 0: j = len(blob)
            b = blob[i:j]
            if len(b) >= 4 and any(c >= 0x80 for c in b):
                for enc in ('utf-8', 'cp932'):
                    try: txt = b.decode(enc)
                    except UnicodeDecodeError: continue
                    if _jp_ok(txt): out.append({'module': t['module'], 'addr': '0x%07x' % (base + a + i), 'encoding': enc, 'text': txt})
                    break
            i = j + 1
    return out


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


def main():
    if len(sys.argv) < 3:
        print(__doc__); return 1
    rom_path, out = sys.argv[1], sys.argv[2]
    rom = open(rom_path, 'rb').read()
    if rom[0x0c:0x10] != GAME_CODE:
        print('error: not a LovePlus+ (BOXJ) ROM, game code is', rom[0x0c:0x10]); return 1
    os.makedirs(out, exist_ok=True)
    man = {'extractor_version': VERSION, 'rom': {'game_code': GAME_CODE.decode(), 'title': rom[:12].rstrip(b'\0').decode(),
           'sha1': hashlib.sha1(rom).hexdigest(), 'size': len(rom)}, 'stages': {}}
    P = lambda *a: os.path.join(out, *a)

    # 1) NitroFS -> assets (archives unpacked)
    files = nitrofs(rom); n_arc = n_raw = 0
    stats_before = collections.Counter(unpack_arc.stats)
    for path, (s, e) in files:
        data = rom[s:e]
        if path.startswith('sys/Siren14Voice') or path.startswith('lipsync/'):
            dst = P('voice', os.path.basename(path)); os.makedirs(os.path.dirname(dst), exist_ok=True)
            open(dst, 'wb').write(data); n_raw += 1; continue
        if path == 'sys/sound_data.sdat':
            os.makedirs(P('sound'), exist_ok=True); open(P('sound', 'sound_data.sdat'), 'wb').write(data); n_raw += 1; continue
        if data[:4] == b'\x18\x56\x01\x00':
            unpack_arc.unpack(data, P('assets', path)); n_arc += 1
        else:
            dst = P('assets', path); os.makedirs(os.path.dirname(dst), exist_ok=True); open(dst, 'wb').write(data); n_raw += 1
    man['stages']['nitrofs'] = {'files': len(files), 'archives_unpacked': n_arc, 'raw_files': n_raw,
                                'entries_by_type': {k: v - stats_before.get(k, 0) for k, v in unpack_arc.stats.items()}}

    # 2) scripts (tmx_event.arc was unpacked as .sbin files; decrypt into scripts/, strings/ and directory)
    src = P('assets', 'arc', 'tmx_event.arc')
    os.makedirs(P('scripts'), exist_ok=True); os.makedirs(P('strings'), exist_ok=True)
    nscr = nstr = 0; dirdoc = None
    for f in sorted(os.listdir(src)):
        if not f.endswith('.sbin'): continue
        d = decrypt(open(os.path.join(src, f), 'rb').read())
        open(P('scripts', f), 'wb').write(d); nscr += 1
        h = struct.unpack('>11I', d[8:0x34])
        st = script_strings(d, h)
        if st:
            json.dump({str(k): v for k, v in st.items()}, open(P('strings', f.replace('.sbin', '.json')), 'w'), ensure_ascii=False, indent=0)
            nstr += len(st)
        if h[10] > 1000:   # the master directory script
            recs = [struct.unpack('>II', d[h[9] + 8 * k:h[9] + 8 * k + 8]) for k in range(h[10])]
            dirdoc = {'script_file': f, 'records': {'%08x' % i: {'slot': (r >> 24) & 0xf, 'script': (r >> 12) & 0xfff, 'entry': r & 0xfff} for i, r in recs}}
    if dirdoc: json.dump(dirdoc, open(P('scripts', 'directory.json'), 'w'), indent=0)
    shutil.rmtree(src)   # raw encrypted copies not needed
    man['stages']['scripts'] = {'scripts': nscr, 'strings': nstr, 'directory_records': len(dirdoc['records']) if dirdoc else 0}

    # 3) text resources
    os.makedirs(P('text'), exist_ok=True); ntxt = nlines = 0
    tdir = P('assets', 'text')
    for f in sorted(os.listdir(tdir)):
        if not f.endswith('.txrc'): continue
        groups = txrc_parse(open(os.path.join(tdir, f), 'rb').read())
        doc = [[[l.decode('utf-8', 'replace') for l in sub] for sub in g] for g in groups]
        json.dump(doc, open(P('text', f.replace('.txrc', '.json')), 'w'), ensure_ascii=False)
        ntxt += 1; nlines += sum(len(s) for g in doc for s in g)
    shutil.rmtree(tdir)
    man['stages']['text'] = {'files': ntxt, 'lines': nlines}

    # 4) graphics: convert GTD images to PNG alongside (originals removed)
    from gtd2png import gtd
    ngtd = 0
    for root, _, fs in os.walk(P('assets')):
        for f in fs:
            if not f.endswith('.bin'): continue
            p = os.path.join(root, f); d = open(p, 'rb').read()
            if len(d) < 32 or d[0] != 4 or d[1] not in (4, 5): continue
            w, h_, po, pl, px = struct.unpack('<HHHHI', d[4:16])
            if po != 16 or 16 + pl + px != len(d) or not (0 < w <= 512 and 0 < h_ <= 512): continue
            try:
                img, _ = gtd(d)
                rel = os.path.relpath(p, P('assets'))[:-4] + '.png'
                dst = P('graphics', rel); os.makedirs(os.path.dirname(dst), exist_ok=True)
                img.convert('RGBA').save(dst); os.remove(p); ngtd += 1
            except Exception:
                pass
    man['stages']['graphics'] = {'gtd_png': ngtd}

    # 4b) NitroSystem 2D resources (NCGR/NCLR/NSCR/NCER/NANR) -> PNG + JSON under graphics/nns (raw files stay in assets/)
    import nns2d_export
    n2d, fail2d = nns2d_export.export_tree(P('assets'), P('graphics', 'nns'))
    man['stages']['nns2d'] = {'resource_sets': n2d, 'failures': len(fail2d)}

    # 4c) NitroSystem 3D models (NSBMD + NSBTX) -> glTF binary under models/ (bind pose; raw files stay in assets/)
    import nns3d_export
    st3, fail3 = nns3d_export.export_tree(P('assets'), P('models'))
    nmot, motbytes = nns3d_export.export_motions(P('assets', 'arc', 'async3d.arc'), P('models', 'motions')) if os.path.isdir(P('assets', 'arc', 'async3d.arc')) else (0, 0)
    wardrobe = nns3d_export.build_wardrobe(P('models'))
    n_sd, n_pl = nns3d_export.export_shading(P('assets', 'arc'), P('models', 'shading'))
    n_expr = nns3d_export.export_expressions(P('assets', 'arc'), P('models', 'expressions'))
    n_var = nns3d_export.export_variants(P('assets', 'arc'), P('models', 'variants'))
    os.makedirs(P('camera'), exist_ok=True); n_cam = nns3d_export.export_camera_presets(P('assets', 'arc'), P('camera', 'presets.json'))
    man['stages']['nns3d'] = {'models': st3.get('models', 0), 'triangles': st3.get('triangles', 0), 'animations': st3.get('animations', 0), 'failures': len(fail3),
                              'heroine_motions': nmot, 'heroine_motion_json_bytes': motbytes, 'wardrobe_outfits_hairs': wardrobe,
                              'shading_tables': n_sd, 'palettes': n_pl, 'expression_textures': n_expr, 'variant_textures': n_var, 'camera_presets': n_cam}

    # 4d) sound archive -> decoded waves/streams (WAV), banks (JSON), sequences (raw SSEQ), index.json under sound/
    import sdat
    ss = sdat.export(P('sound', 'sound_data.sdat'), P('sound'))
    man['stages']['sdat'] = ss
    import sseq
    man['stages']['sseq'] = sseq.export_all(P('sound'), P('sound', 'midi'))
    import sf2
    man['stages']['soundfonts'] = sf2.export_all(P('sound'), P('sound', 'soundfonts'))

    # 5) voice-decoder tables from the game's ARM9
    arm9 = arm9_static(rom)
    n_mt = nns3d_export.export_motion_table(P('assets', 'arc'), P('models', 'motion_table.json'), arm9)
    man['stages']['nns3d']['motion_table_records'] = n_mt
    os.makedirs(P('tables'), exist_ok=True)
    write_pack(P('tables', 'g7221.pack'), arm9, G7221_RANGES)
    man['stages']['tables'] = {'g7221.pack_ranges': [[hex(a), n] for a, n in G7221_RANGES]}
    import face_tables
    os.makedirs(P('engine'), exist_ok=True); ft = face_tables.extract(arm9); json.dump(ft, open(P('engine', 'face_tables.json'), 'w'), indent=1)
    man['stages']['face_tables'] = {'moods': len(ft['moods']), 'mouth_rows': len(ft['mouth_rows'])}
    import textlayout, nftr
    json.dump(textlayout.extract_rules(arm9), open(P('engine', 'text_rules.json'), 'w'), indent=1, ensure_ascii=False)
    man['stages']['fonts'] = {'fonts': nftr.export_fonts(P('assets'), P('fonts'))}
    xp = P('models', 'expressions', 'expressions.json')
    if os.path.exists(xp):                      # flag eye variants no mood/column references (22 is a numbered debug texture)
        xj = json.load(open(xp)); used = {v for r in ft['eye_table'] for v in r if v >= 0}
        for h in xj['heroines'].values():
            for k, v in h.get('eye', {}).items(): v['unused'] = int(k) not in used
        json.dump(xj, open(xp, 'w'), indent=1)

    # 6) engine images: everything the game code and its embedded data tables consist of (static, ITCM, DTCM, 44 overlays)
    eng = P('engine'); os.makedirs(eng, exist_ok=True); mm = {}
    open(os.path.join(eng, 'static.bin'), 'wb').write(arm9); mm['static'] = {'base': 0x2000000, 'size': len(arm9), 'sha1': hashlib.sha1(arm9).hexdigest()}
    for name, (addr, blob, bss) in arm9_autoloads(rom, arm9).items():
        if name in ('itcm', 'dtcm'):
            open(os.path.join(eng, name + '.bin'), 'wb').write(blob); mm[name] = {'base': addr, 'size': len(blob), 'bss': bss, 'sha1': hashlib.sha1(blob).hexdigest()}
    ovs = overlays(rom)
    for o in ovs:
        open(os.path.join(eng, 'ov%02d.bin' % o['id']), 'wb').write(o['data'])
        mm['ov%02d' % o['id']] = {'base': o['ram'], 'size': len(o['data']), 'bss': o['bss'], 'sinit': o['sinit'], 'sha1': hashlib.sha1(o['data']).hexdigest()}
    cat = os.path.join(HERE, '..', 'data', 'engine_tables.json')
    if os.path.exists(cat): shutil.copy(cat, os.path.join(eng, 'tables.json'))
    json.dump(mm, open(os.path.join(eng, 'memory_map.json'), 'w'), indent=1)
    nstr_engine = 0
    if os.path.exists(cat):
        catalog = json.load(open(cat))
        imgs = {'static': (0x2000000, arm9)}
        for name, (addr, blob, bss) in arm9_autoloads(rom, arm9).items():
            if name in ('itcm', 'dtcm'): imgs[name] = (addr, blob)
        for o in ovs: imgs['ov%02d' % o['id']] = (o['ram'], o['data'])
        es = engine_strings(imgs, catalog); nstr_engine = len(es)
        json.dump(es, open(os.path.join(eng, 'strings.json'), 'w'), ensure_ascii=False, indent=0)
    man['stages']['engine'] = {'images': len(mm), 'bytes': sum(v['size'] for v in mm.values()), 'catalog': os.path.exists(cat), 'embedded_strings': nstr_engine}

    json.dump(man, open(P('manifest.json'), 'w'), indent=1)
    print(json.dumps(man['stages'], indent=1))
    return 0


if __name__ == '__main__':
    sys.exit(main())
