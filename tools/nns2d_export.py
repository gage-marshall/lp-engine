"""Export NitroSystem 2D resources found under a directory tree to PNG/JSON.

    nns2d_export.py ASSETS_DIR OUT_DIR

Files are grouped into sets by name stem (the part after an `NNNN_` archive index) or, when archives only have indices, by order
(a set starts at each NCGR; NCLR/NSCR/NCER/NANR that follow belong to it). A set without its own palette uses the most recent NCLR in the same
directory. Output per set under OUT_DIR/<relative dir>/<stem>/: screen*.png (NSCR composites), chars.png (tile sheet),
cells/NNN.png + cells.json (OAM data), anim.json (animation sequences).
"""
import os, re, sys, json, collections, traceback
sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import nns2d as N

EXT = ('ncgr', 'nclr', 'nscr', 'ncer', 'nanr')


def groups(files):
    """files: sorted names in one directory -> list of (stem, {ext: [name,...]})"""
    named = any(re.match(r'^\d+_', f) for f in files)
    out = []
    if named:
        by = collections.OrderedDict()
        for f in files:
            stem, ext = f.rsplit('.', 1); stem = re.sub(r'^\d+_', '', stem)
            by.setdefault(stem.lower(), (stem, collections.defaultdict(list)))[1][ext.lower()].append(f)
        out = list(by.values())
    else:
        cur = None; n = 0
        for f in files:
            ext = f.rsplit('.', 1)[1].lower()
            if cur is None or (ext == 'ncgr' and cur[1].get('ncgr')):
                n += 1; cur = ('set%03d' % n, collections.defaultdict(list)); out.append(cur)
            cur[1][ext].append(f)
    return out


def oams_json(cell):
    return [N.oam_decode(*o) for o in cell['oams']]


def export_dir(src, dst, log):
    files = sorted(f for f in os.listdir(src) if f.rsplit('.', 1)[-1].lower() in EXT and os.path.isfile(os.path.join(src, f)))
    if not files: return 0
    last_pal = None; n = 0
    # palette lookup order: own set, else previous NCLR in the directory
    for stem, g in groups(files):
        rd = lambda name: open(os.path.join(src, name), 'rb').read()
        try:
            pal = N.parse_nclr(rd(g['nclr'][0])) if g.get('nclr') else last_pal
            if g.get('nclr'): last_pal = pal
            ch = N.parse_ncgr(rd(g['ncgr'][0])) if g.get('ncgr') else None
            od = os.path.join(dst, re.sub(r'[^\w.\-]', '_', stem)); made = False
            if ch is not None:
                os.makedirs(od, exist_ok=True); N.ncgr_image(ch, pal).save(os.path.join(od, 'chars.png')); made = True
                for k, f in enumerate(g.get('nscr', [])):
                    N.nscr_image(N.parse_nscr(rd(f)), ch, pal).save(os.path.join(od, 'screen%s.png' % ('' if k == 0 else k)))
                if g.get('ncer'):
                    ce = N.parse_ncer(rd(g['ncer'][0])); cells = []
                    os.makedirs(os.path.join(od, 'cells'), exist_ok=True)
                    for i, c in enumerate(ce['cells']):
                        r = N.render_cell(c, ce['mapping'], ch, pal)
                        entry = {'oams': oams_json(c), 'bounds': c['br']}
                        if r: r[0].save(os.path.join(od, 'cells', '%03d.png' % i)); entry['origin'] = list(r[1]); entry['size'] = list(r[0].size)
                        cells.append(entry)
                    json.dump({'mapping': ce['mapping'], 'cells': cells}, open(os.path.join(od, 'cells.json'), 'w'))
            if g.get('nanr'):
                os.makedirs(od, exist_ok=True); json.dump(N.parse_nanr(rd(g['nanr'][0])), open(os.path.join(od, 'anim.json'), 'w')); made = True
            n += made
        except Exception as e:
            log.append((os.path.join(src, stem), repr(e)[:100]))
    return n


def export_tree(src_root, out_root):
    log = []; total = 0
    for r, ds, fs in os.walk(src_root):
        rel = os.path.relpath(r, src_root)
        total += export_dir(r, os.path.join(out_root, rel), log)
    return total, log


if __name__ == '__main__':
    t, log = export_tree(sys.argv[1], sys.argv[2])
    print(t, 'resource sets exported;', len(log), 'failures')
    for l in log[:15]: print(' ', l)
