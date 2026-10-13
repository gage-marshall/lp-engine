"""Build the viewer sample sheet from the Rust viewer: three heroines in default pose + an animation strip.
usage: viewer_sample.py CONTENT_DIR VIEWER_BIN -> export/viewer/sample_sheet.png"""
import os, subprocess, sys
from PIL import Image
content, binary = sys.argv[1:3]
os.makedirs('export/viewer', exist_ok=True)
def shot(name, *args, size=384):
    p = 'export/viewer/_%s.png' % name
    subprocess.run([binary, content, '--size', str(size), '--png', p, *args], check=True, capture_output=True)
    return Image.open(p).convert('RGB')
row1 = [shot('m', '--heroine', 'm', '--outfit', '17', '--zoom', '1.5', '--mood', '7'),
        shot('r', '--heroine', 'r', '--outfit', '3', '--zoom', '1.5', '--mood', '7'),
        shot('n', '--heroine', 'n', '--outfit', '3', '--zoom', '1.5', '--mood', '7')]
row2 = [shot('a%d' % i, '--heroine', 'm', '--outfit', '5', '--hair', '2', '--motion', '100', '--frame', str(i * 10), '--zoom', '1.5', '--mood', '3', '--state', '1') for i in range(3)]
sheet = Image.new('RGB', (384 * 3, 384 * 2))
for i, im in enumerate(row1 + row2): sheet.paste(im, ((i % 3) * 384, (i // 3) * 384))
sheet.save('export/viewer/sample_sheet.png'); print('wrote export/viewer/sample_sheet.png')
