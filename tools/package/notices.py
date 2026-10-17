#!/usr/bin/env python3
"""THIRD_PARTY_NOTICES.txt for a release of the game app: the Siren14 attribution, then every Rust crate the game binary
links (normal dependencies, for the target being packaged) with its licence and the licence / notice files it ships.

    notices.py OUT [--target TRIPLE]
"""
import glob
import json
import os
import subprocess
import sys

ENGINE = os.path.join(os.path.dirname(os.path.abspath(__file__)), '..', '..', 'engine')

HEADER = """LovePlus+ engine: third-party notices
=====================================

LovePlus is a trademark of Konami Digital Entertainment. This program is not affiliated with or endorsed by Konami. It
contains no part of the game: it reads the player's own copy.

Voice playback uses an ITU-T G.722.1 Annex C decoder. Siren14(TM) / G.722.1C technology licensed from Polycom(R),
Inc. Siren14 is a trademark of Polycom, Inc.

The Rust libraries below are linked into the program. Each is listed with its licence as its authors declare it,
followed by the licence and notice files it is distributed with.
"""


def main():
    if len(sys.argv) < 2:
        sys.exit(__doc__)
    out = sys.argv[1]
    cmd = ['cargo', 'metadata', '--format-version', '1', '--manifest-path', os.path.join(ENGINE, 'Cargo.toml')]
    if '--target' in sys.argv:
        cmd += ['--filter-platform', sys.argv[sys.argv.index('--target') + 1]]
    meta = json.loads(subprocess.check_output(cmd))
    pkgs = {p['id']: p for p in meta['packages']}
    nodes = {n['id']: n for n in meta['resolve']['nodes']}
    root = next(p['id'] for p in meta['packages'] if p['name'] == 'lp_engine')
    # the packages reached through normal (not dev / build) dependencies
    seen, todo = set(), [root]
    while todo:
        i = todo.pop()
        if i in seen:
            continue
        seen.add(i)
        for d in nodes[i]['deps']:
            if any(k['kind'] is None for k in d['dep_kinds']):
                todo.append(d['pkg'])
    seen.discard(root)
    crates = sorted((pkgs[i] for i in seen), key=lambda p: (p['name'], p['version']))
    lines = [HEADER]
    lines.append('\nSummary\n-------\n')
    for p in crates:
        lines.append('%s %s: %s' % (p['name'], p['version'], p.get('license') or ('see ' + (p.get('license_file') or 'below'))))
    texts = {}
    for p in crates:
        d = os.path.dirname(p['manifest_path'])
        files = sorted(f for pat in ('LICEN[CS]E*', 'COPYING*', 'NOTICE*', 'COPYRIGHT*') for f in glob.glob(os.path.join(d, pat)) if os.path.isfile(f))
        if p.get('license_file'):
            f = os.path.join(d, p['license_file'])
            if os.path.isfile(f) and f not in files:
                files.append(f)
        lines.append('\n\n' + '=' * 78 + '\n%s %s (%s)\n%s\n' % (p['name'], p['version'], p.get('license') or '', p.get('repository') or ''))
        if not files:
            lines.append('(the crate ships no licence file: its licence is %s)' % (p.get('license') or 'unknown'))
        for f in files:
            body = open(f, encoding='utf-8', errors='replace').read().strip()
            # the same text (most Apache-2.0 copies) is printed once and referred to afterwards
            if body in texts:
                lines.append('--- %s: identical to the text given for %s' % (os.path.basename(f), texts[body]))
            else:
                texts[body] = '%s %s (%s)' % (p['name'], p['version'], os.path.basename(f))
                lines.append('--- %s\n\n%s\n' % (os.path.basename(f), body))
    open(out, 'w', encoding='utf-8').write('\n'.join(lines) + '\n')
    missing = [p['name'] for p in crates if not p.get('license')]
    print('%d crates -> %s%s' % (len(crates), out, ('; no declared licence: ' + ', '.join(missing)) if missing else ''))


if __name__ == '__main__':
    main()
