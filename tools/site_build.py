#!/usr/bin/env python3
"""Builds the guide site: each page in site/pages/ is a body fragment that starts with a JSON header comment
(<!--{"title": ..., "description": ..., "nav": "guide" | "map", "chapter": N}-->); this wraps it in the shared head,
bar and footer, adds the previous / next chapter links, and checks every link to the function map
(symbols.html#Name or #module-address) against the symbol map, so a guide never names a function that isn't there.

    site_build.py SYMBOLS.json OUT_DIR
"""
import html
import json
import os
import re
import sys

ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
PAGES = os.path.join(ROOT, 'site', 'pages')

# the chapters in order: (number, file, title)
CHAPTERS = [
    (1, 'machine.html', 'The machine'),
    (2, 'structure.html', 'The shape of the game'),
    (3, 'scripts.html', 'Scripts and events'),
    (4, 'time.html', 'Real time'),
    (5, 'heroines.html', 'The heroines'),
    (6, 'save.html', 'The save'),
    (7, 'drawing.html', 'Drawing'),
    (8, 'sound.html', 'Sound'),
    (9, 'text.html', 'Text'),
    (10, 'hardware.html', 'The hardware edges'),
    (11, 'formats.html', 'The files'),
    (12, 'engine.html', 'Inside lp-engine'),
    (13, 'open.html', "What we don't know yet"),
]

HEAD = '''<!doctype html>
<html lang="en">
<head>
<meta charset="utf-8">
<meta name="viewport" content="width=device-width, initial-scale=1, viewport-fit=cover">
<title>{title}</title>
<meta name="description" content="{description}">
<link rel="icon" href="img/icon_256.png">
<link rel="preconnect" href="https://fonts.googleapis.com">
<link rel="preconnect" href="https://fonts.gstatic.com" crossorigin>
<link rel="stylesheet" href="https://fonts.googleapis.com/css2?family=IBM+Plex+Mono:wght@400;500&family=IBM+Plex+Sans:ital,wght@0,400;0,600;1,400&family=Zen+Maru+Gothic:wght@700&display=swap">
<link rel="stylesheet" href="style.css">{extra_head}
</head>
<body>
<header class="bar"><div class="bar-inner">
  <a class="brand" href="./"><span class="heart">♥</span> lp-engine</a>
  <nav>
    <a href="./"{cur_guide}>How it works</a>
    <a href="symbols.html"{cur_map}>Function map</a>
    <a href="https://github.com/gage-marshall/lp-engine">GitHub</a>
    <a href="https://github.com/gage-marshall/lp-engine/releases">Download</a>
  </nav>
</div></header>
'''
FOOT = '''
<footer class="foot"><div>lp-engine is MIT licensed. LovePlus is a trademark of Konami Digital Entertainment; this
project is not affiliated with or endorsed by Konami or Nintendo.</div></footer>{extra_body}
</body>
</html>
'''


def chapter_nav(n):
    i = n - 1
    prev = CHAPTERS[i - 1] if i > 0 else None
    nxt = CHAPTERS[i + 1] if i + 1 < len(CHAPTERS) else None
    parts = ['<nav class="chapnav" aria-label="Chapters">']
    parts.append('<a class="prev" href="%s"><span>← Chapter %d</span>%s</a>' % (prev[1], prev[0], html.escape(prev[2])) if prev else '<a class="prev" href="./"><span>← Contents</span>All chapters</a>')
    parts.append('<a class="next" href="%s"><span>Chapter %d →</span>%s</a>' % (nxt[1], nxt[0], html.escape(nxt[2])) if nxt else '<a class="next" href="./"><span>Contents →</span>All chapters</a>')
    parts.append('</nav>')
    return '\n'.join(parts)


def main():
    syms_path, out = sys.argv[1], sys.argv[2]
    d = json.load(open(syms_path))
    names = {s[3] for s in d['symbols']}
    keys = {'%s-%08x' % (s[0], s[1]) for s in d['symbols']}
    bad = []
    built = 0
    for fn in sorted(os.listdir(PAGES)):
        if not fn.endswith('.html'):
            continue
        src = open(os.path.join(PAGES, fn), encoding='utf-8').read()
        m = re.match(r'<!--(\{.*?\})-->\s*', src, re.S)
        if not m:
            sys.exit('%s: no header comment' % fn)
        meta = json.loads(m.group(1))
        body = src[m.end():]
        for target in re.findall(r'href="symbols\.html#([^"]+)"', body):
            if target not in names and target not in keys:
                bad.append('%s: symbols.html#%s' % (fn, target))
        if 'chapter' in meta:
            body = body.replace('<!--chapnav-->', chapter_nav(meta['chapter']))
        page = HEAD.format(title=html.escape(meta['title']), description=html.escape(meta['description'], quote=True),
                           extra_head=meta.get('head', ''), cur_guide=' aria-current="page"' if meta.get('nav') == 'guide' else '',
                           cur_map=' aria-current="page"' if meta.get('nav') == 'map' else '') + body + FOOT.format(extra_body=meta.get('body_end', ''))
        open(os.path.join(out, fn), 'w', encoding='utf-8').write(page)
        built += 1
    if bad:
        sys.exit('links to functions that are not in the map:\n  ' + '\n  '.join(bad))
    print('%d pages, function links checked' % built)


if __name__ == '__main__':
    main()
