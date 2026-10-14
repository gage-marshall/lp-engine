"""Generated test cases for the engine's recogniser (engine/src/dlt.rs): random stroke sets (1..20 strokes, single points
and long strokes, the whole box range) and the 12 candidate codes the game's recogniser gives for them under unicorn.
The hand-drawn cases are saved from the pad (pad.py) into cases.json; these go to generated.json.
    .venv/bin/python sandbox/dlt/export_cases.py [count]"""
import json, os, random, sys
HERE = os.path.dirname(os.path.abspath(__file__))
sys.path.insert(0, HERE)
from harness import Recogniser

ROOT = os.path.dirname(os.path.dirname(HERE))
OUT = os.path.join(ROOT, 'engine/tests/data/dlt/generated.json')


def stroke(rng):
    n = rng.choice([1, 2, 3, rng.randint(4, 40)])
    a, b = rng.randint(1, 0x77), rng.randint(10, 0x80)
    pts = [(a, b)]
    for _ in range(n - 1):
        a = min(0x77, max(1, a + rng.randint(-12, 12)))
        b = min(0x80, max(10, b + rng.randint(-12, 12)))
        pts.append((a, b))
    return pts


if __name__ == '__main__':
    count = int(sys.argv[1]) if len(sys.argv) > 1 else 60
    rng = random.Random(0x4c50)
    R = Recogniser()
    cases = []
    for k in range(count):
        strokes = [stroke(rng) for _ in range(rng.choice([1, 2, 3, rng.randint(4, 20)]))]
        codes = list(R.recognise_raw(strokes))
        cases.append(dict(strokes=[[list(p) for p in s] for s in strokes], codes=codes))
    with open(OUT, 'w') as f:
        f.write('[\n' + ',\n'.join(json.dumps(c, separators=(',', ':')) for c in cases) + '\n]\n')
    print(len(cases), 'cases ->', os.path.relpath(OUT, ROOT))
