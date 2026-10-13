#!/usr/bin/env python3
"""LovePlus+ one-shot content extractor (layer 0: ROM -> files).

    lp_extract.py ROM.nds OUTDIR

Reads the user's ROM once and writes the game's own files into OUTDIR (the "content tree").  Nothing from the ROM is needed
afterwards.  The engine loads OUTDIR, optionally overlaid by a `patch/` tree with the same relative paths.

Layer 0 does only what the ROM's containers require, and never interprets content:
  * NitroFS files are written at their ROM paths under assets/; game archives (0x00015618 header) are unpacked one level into
    directories (LZ decompressed);
  * reversible obfuscation is removed: the SBIN/TXRC stream cipher (assets/arc/tmx_event.arc/*.sbin, assets/text/*.txrc),
    and BLZ compression of the ARM9 and the overlays;
  * data that lives inside the game's code (no file exists for it) is dumped to engine/: code images, memory map, table catalog,
    embedded strings, face/text/motion tables, the script resource pool map, decoder tables.
Conversions to common formats (PNG, glTF, WAV, JSON strings, ...) are layer 2: see tools/lp_export.py.

Layout:
  manifest.json            ROM identity (sha1, game code), extractor version, per-stage counts
  assets/                  every NitroFS file at its ROM path, archives unpacked, cipher removed
  engine/                  static.bin itcm.bin dtcm.bin ov00..ov43.bin, memory_map.json, tables.json, strings.json,
                           face_tables.json, text_rules.json, motion_table.json, bgm_tables.json, script_resource_map.json, g7221.pack
"""
import hashlib, json, os, struct, sys, shutil, collections
HERE = os.path.dirname(os.path.abspath(__file__))
sys.path.insert(0, HERE)
from blz_decompress import blz_decode
from lp_decrypt import decrypt
import unpack_arc

VERSION = 3
GAME_CODE = b'BOXJ'

# The script VM encodes a target as (slot << 24) | (script_id << 12) |
# entry_index.  Script_LoadScriptFile indexes one of these four static pools to
# turn script_id into an entry number in tmx_event.arc.  These are initialized
# ARM9 data, not data in tmx_bbin.arc.
SCRIPT_RESOURCE_POOL_RECORDS = 0x02128868


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


def script_resource_map(arm9):
    """Read the static (slot, script_id) -> tmx_event.arc entry mapping.

    Each eight-byte pool record is ``u32 table_address, u32 entry_count``;
    the table itself is a u16 array of archive entry indices.  Keeping this
    mapping in the extracted content makes directory references usable without
    consulting the original ARM9 image at runtime.
    """
    base = 0x02000000

    def span(addr, size):
        start = addr - base
        if start < 0 or size < 0 or start + size > len(arm9):
            raise ValueError('script resource pool outside decompressed ARM9: %#x + %#x' % (addr, size))
        return start

    rec = span(SCRIPT_RESOURCE_POOL_RECORDS, 4 * 8)
    slots = {}
    for slot in range(4):
        table, count = struct.unpack_from('<II', arm9, rec + slot * 8)
        start = span(table, count * 2)
        slots[str(slot)] = {'table_address': '0x%08x' % table,
                            'entries': list(struct.unpack_from('<%dH' % count, arm9, start)) if count else []}
    return {'pool_records_address': '0x%08x' % SCRIPT_RESOURCE_POOL_RECORDS, 'slots': slots}


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

    # 1) NitroFS -> assets (archives unpacked one level, LZ decompressed)
    files = nitrofs(rom); n_arc = n_raw = 0
    stats_before = collections.Counter(unpack_arc.stats)
    for path, (s, e) in files:
        data = rom[s:e]
        if data[:4] == b'\x18\x56\x01\x00':
            unpack_arc.unpack(data, P('assets', path)); n_arc += 1
        else:
            dst = P('assets', path); os.makedirs(os.path.dirname(dst), exist_ok=True); open(dst, 'wb').write(data); n_raw += 1
    man['stages']['nitrofs'] = {'files': len(files), 'archives_unpacked': n_arc, 'raw_files': n_raw,
                                'entries_by_type': {k: v - stats_before.get(k, 0) for k, v in unpack_arc.stats.items()}}

    # 2) remove the stream cipher in place (SBIN event scripts, TXRC text resources)
    n_sbin = n_txrc = 0
    for root, _, fs in os.walk(P('assets')):
        for f in fs:
            p = os.path.join(root, f)
            if f.endswith('.sbin') or f.endswith('.txrc'):
                raw = open(p, 'rb').read()
                if raw[:4] in (b'SBIN', b'TXRC'):
                    open(p, 'wb').write(decrypt(raw))
                    if f.endswith('.sbin'): n_sbin += 1
                    else: n_txrc += 1
    man['stages']['decrypt'] = {'sbin': n_sbin, 'txrc': n_txrc}

    # 3) data embedded in the game's code
    arm9 = arm9_static(rom)
    eng = P('engine'); os.makedirs(eng, exist_ok=True); mm = {}
    open(os.path.join(eng, 'static.bin'), 'wb').write(arm9); mm['static'] = {'base': 0x2000000, 'size': len(arm9), 'sha1': hashlib.sha1(arm9).hexdigest()}
    autos = arm9_autoloads(rom, arm9)
    for name, (addr, blob, bss) in autos.items():
        if name in ('itcm', 'dtcm'):
            open(os.path.join(eng, name + '.bin'), 'wb').write(blob); mm[name] = {'base': addr, 'size': len(blob), 'bss': bss, 'sha1': hashlib.sha1(blob).hexdigest()}
    ovs = overlays(rom)
    for o in ovs:
        open(os.path.join(eng, 'ov%02d.bin' % o['id']), 'wb').write(o['data'])
        mm['ov%02d' % o['id']] = {'base': o['ram'], 'size': len(o['data']), 'bss': o['bss'], 'sinit': o['sinit'], 'sha1': hashlib.sha1(o['data']).hexdigest()}
    json.dump(mm, open(os.path.join(eng, 'memory_map.json'), 'w'), indent=1)
    cat = os.path.join(HERE, '..', 'data', 'engine_tables.json')
    nstr_engine = 0
    if os.path.exists(cat):
        shutil.copy(cat, os.path.join(eng, 'tables.json'))
        catalog = json.load(open(cat))
        imgs = {'static': (0x2000000, arm9)}
        for name, (addr, blob, bss) in autos.items():
            if name in ('itcm', 'dtcm'): imgs[name] = (addr, blob)
        for o in ovs: imgs['ov%02d' % o['id']] = (o['ram'], o['data'])
        es = engine_strings(imgs, catalog); nstr_engine = len(es)
        json.dump(es, open(os.path.join(eng, 'strings.json'), 'w'), ensure_ascii=False, indent=0)
    man['stages']['engine'] = {'images': len(mm), 'bytes': sum(v['size'] for v in mm.values()), 'catalog': os.path.exists(cat), 'embedded_strings': nstr_engine}

    resource_map = script_resource_map(arm9)
    json.dump(resource_map, open(os.path.join(eng, 'script_resource_map.json'), 'w'), indent=1)
    man['stages']['script_resource_map'] = {slot: len(pool['entries']) for slot, pool in resource_map['slots'].items()}
    write_pack(os.path.join(eng, 'g7221.pack'), arm9, G7221_RANGES)
    man['stages']['g7221.pack'] = {'ranges': [[hex(a), n] for a, n in G7221_RANGES]}
    import face_tables, textlayout, nns3d_export, audio_tables
    ft = face_tables.extract(arm9); json.dump(ft, open(os.path.join(eng, 'face_tables.json'), 'w'), indent=1)
    man['stages']['face_tables'] = {'moods': len(ft['moods']), 'mouth_rows': len(ft['mouth_rows'])}
    bt = audio_tables.extract(arm9); json.dump(bt, open(os.path.join(eng, 'bgm_tables.json'), 'w'), indent=1)
    man['stages']['bgm_tables'] = {'by_slot': bt['by_slot']}
    json.dump(textlayout.extract_rules(arm9), open(os.path.join(eng, 'text_rules.json'), 'w'), indent=1, ensure_ascii=False)
    n_mt = nns3d_export.export_motion_table(P('assets', 'arc'), os.path.join(eng, 'motion_table.json'), arm9)
    man['stages']['motion_table'] = {'records': n_mt}

    json.dump(man, open(P('manifest.json'), 'w'), indent=1)
    print(json.dumps(man['stages'], indent=1))
    return 0


if __name__ == '__main__':
    sys.exit(main())
