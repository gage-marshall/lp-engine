"""Where things live in the layer-0 content tree (docs/CONTENT_LAYOUT.md); no exported/converted data is needed here."""
import json, os, struct

def scripts_dir(content):
    return os.path.join(content, 'assets', 'arc', 'tmx_event.arc')

def load_resource_map(content):
    return json.load(open(os.path.join(content, 'engine', 'script_resource_map.json')))

def load_directory(content):
    """master event directory (script with >1000 id records), built straight from the decrypted SBIN"""
    sd = scripts_dir(content)
    for f in sorted(os.listdir(sd)):
        if not f.endswith('.sbin'):
            continue
        d = open(os.path.join(sd, f), 'rb').read()
        h = struct.unpack('>11I', d[8:0x34])
        if h[10] > 1000:
            recs = [struct.unpack('>II', d[h[9] + 8 * k:h[9] + 8 * k + 8]) for k in range(h[10])]
            return {'script_file': f, 'records': {'%08x' % i: {'slot': (r >> 24) & 0xf, 'script': (r >> 12) & 0xfff, 'entry': r & 0xfff} for i, r in recs}}
    raise ValueError('no master directory script in ' + sd)
