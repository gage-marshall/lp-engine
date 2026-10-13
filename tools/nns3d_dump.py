"""Python side of the Rust G3D loader differential test (engine/src/bin/nns3d_dump.rs prints the same canonical text).
usage: nns3d_dump.py ASSETS_DIR > out.txt"""
import hashlib, os, struct, sys
sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import nns3d as N


def f7(x):
    s = '%.7f' % x
    return '0.0000000' if s == '-0.0000000' else s


def H(lines):
    return hashlib.sha1('\n'.join(lines).encode()).hexdigest()[:16]


def node_lines(n):
    return [n['name'], str(n['flag'])] + [f7(v) for v in n['trans']] + [f7(v) for r in n['rot'] for v in r] + [f7(v) for v in n['scale']]


def tex_digest(t0, tx):
    pal = None
    pals = {p['name']: p for p in t0['palettes']}
    if tx['name'] + '_pl' in pals: pal = pals[tx['name'] + '_pl']
    elif t0['palettes']: pal = t0['palettes'][0]
    try:
        img = N.decode_texture(t0, tx, pal)
        return hashlib.sha1(img.tobytes()).hexdigest()[:16]
    except Exception:
        return 'ERR'


def dump_bmd(d, out):
    r = N.parse_file(d)
    for m in r['models']:
        i = m['info']
        out.append('M %s nodes=%d mats=%d shapes=%d ps=%s rule=%d tm=%d fu=%d nv=%d np=%d' % (m['name'], i['num_node'], i['num_mat'], i['num_shp'], f7(i['pos_scale']), i['scaling_rule'], i['tex_mtx_mode'], i['first_unused_mtx'], i['num_vertex'], i['num_polygon']))
        out.append(' nodes %s' % H([l for n in m['nodes'] for l in node_lines(n)]))
        out.append(' mats %s' % H(['|'.join(str(x) for x in (mm['name'], mm['diffuse'], mm['ambient'], mm['polyattr'], mm['alpha'], mm['cull'], mm['tex_param'], mm['pltt_base'], mm['flag'], mm['orig_w'], mm['orig_h'], mm['tex'], mm['pal'])) for mm in m['materials']]))
        out.append(' shapes %s' % H(['%s|%d|%d|%d' % (s['name'], s['flag'], s['dl_off'], s['dl_size']) for s in m['shapes']]))
        try:
            tris = N.build_mesh(d, m, skin=True)
            lines = []
            for t in tris:
                lines.append(str(t['material']))
                for pos, nrm, uv, col, joints in t['verts']:
                    lines.append(','.join(f7(x) for x in list(pos) + list(nrm) + list(uv) + list(col)) + ';' + ','.join('%d:%s' % (a, f7(b)) for a, b in (joints or [])))
            out.append(' mesh tris=%d %s' % (len(tris), H(lines)))
        except Exception as e:
            out.append(' mesh ERR')
    t0 = r['tex0']
    if t0:
        for tx in t0['textures']:
            out.append(' tex %s %dx%d fmt=%d %s' % (tx['name'], tx['w'], tx['h'], tx['fmt'], tex_digest(t0, tx)))
        out.append(' pals %s' % H(['%s|%d' % (p['name'], p['offset']) for p in t0['palettes']]))


def dump_btx(d, out):
    t0 = N.parse_file(d)['tex0']
    for tx in t0['textures']:
        out.append(' tex %s %dx%d fmt=%d %s' % (tx['name'], tx['w'], tx['h'], tx['fmt'], tex_digest(t0, tx)))
    out.append(' pals %s' % H(['%s|%d' % (p['name'], p['offset']) for p in t0['palettes']]))


def spec_s(s):
    return '%s:%s' % (s[0], ','.join(f7(v) if isinstance(v, float) else str(v) for v in s[1:]))


def dump_bca(d, out):
    for a in N.parse_bca(d):
        tl = []
        for t in a['tags']:
            tl.append('%d|%d|%s|%s|%s' % (t['node'], t['flag'], ';'.join(spec_s(s) for s in t.get('t', [])), spec_s(t['r']) if 'r' in t else '', ';'.join(spec_s(s) for s in t.get('s', []))))
        nn = max([t['node'] for t in a['tags']] + [0]) + 1
        base = [{'name': 'n%d' % i, 'flag': 0, 'trans': (0.1 * i, 0.0, 0.0), 'rot': [[1, 0, 0], [0, 1, 0], [0, 0, 1]], 'scale': (1.0, 1.0, 1.0)} for i in range(nn)]
        samples = []
        for fr in sorted({0, a['frames'] // 3, a['frames'] // 2, max(0, a['frames'] - 1)}):
            try:
                res = N.sample_anim(a, base, fr)
                samples.append(H([l for n in res for l in node_lines(n)]))
            except Exception:
                samples.append('ERR')
        out.append(' bca %s frames=%d tracks=%s samples=%s' % (a['name'], a['frames'], H(tl), ','.join(samples)))


def dump_bva(d, out):
    for a in N.parse_bva(d):
        vis = ''.join('1' if N.visible(a, f, n) else '0' for f in sorted({0, a['frames'] // 2, max(0, a['frames'] - 1)}) for n in range(min(a['nodes'], 40)))
        out.append(' bva %s frames=%d nodes=%d bits=%s vis=%s' % (a['name'], a['frames'], a['nodes'], H([str(b) for b in a['bits']]), vis))


def bundle_entries(b):
    if len(b) < 8: return None
    n, esz = struct.unpack_from('<II', b, 0)
    if not (1 <= n <= 4096 and esz == 0x28) or 8 + n * esz > len(b): return None
    out = []
    for k in range(n):
        off, size = struct.unpack_from('<II', b, 8 + k * esz); name = b[8 + k * esz + 8:8 + k * esz + 40].split(b'\0')[0].decode('latin1')
        out.append((name, b[off:off + size]))
    return out


def main(root):
    out = []
    for r, ds, fs in os.walk(root):
        ds.sort()
        for f in sorted(fs):
            p = os.path.join(r, f); b = open(p, 'rb').read(); rel = os.path.relpath(p, root)
            if b[:4] == b'BMD0':
                out.append('F ' + rel)
                try: dump_bmd(b, out)
                except Exception as e: out.append(' ERR')
            elif b[:4] == b'BTX0':
                out.append('F ' + rel)
                try: dump_btx(b, out)
                except Exception: out.append(' ERR')
            else:
                ents = bundle_entries(b)
                if ents and any(d[:4] in (b'BCA0', b'BVA0') for _, d in ents):
                    out.append('F ' + rel)
                    for name, d in ents:
                        if d[:4] == b'BCA0':
                            out.append(' E ' + name)
                            try: dump_bca(d, out)
                            except Exception: out.append(' ERR')
                        elif d[:4] == b'BVA0':
                            out.append(' E ' + name)
                            try: dump_bva(d, out)
                            except Exception: out.append(' ERR')
    sys.stdout.write('\n'.join(out) + '\n')


main(sys.argv[1])
