"""A drawing pad in the browser for the game's handwriting recogniser (harness.Recogniser).

The pad is the writing box as the player sees it (the DS held sideways): 120 x 120 points. The points are stored the way
RunProfileDlt 0x217bcd0 stores the touch samples: a touch is taken only inside the box (screen 0x74 < x < 0xec,
9 < y < 0x81) and only when it moves more than 2 px on some axis from the last one taken; it is stored as
a = y - 9, b = 0x81 - (x - 0x74). In the player's view a runs left to right and b top to bottom, so pad (u, v) is
a = u + 1, b = v + 10. Limits: 0x400 points, 0x32 strokes. The game recognises after 15 idle frames; the pad does
it 250 ms after the pen lifts.
    .venv/bin/python sandbox/dlt/pad.py   then open http://localhost:8765"""
import json, os, sys
from http.server import BaseHTTPRequestHandler, HTTPServer
sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
from harness import Recogniser

PAGE = r"""<!doctype html><meta charset=utf-8><title>DLT pad</title>
<style>
body{font:16px system-ui,sans-serif;background:#f4f1ea;color:#222;margin:24px}
canvas{background:#fff;border:1px solid #888;touch-action:none;cursor:crosshair;image-rendering:pixelated}
#c span{display:inline-block;min-width:1.6em;font-size:28px;text-align:center;margin:2px;padding:2px 4px;
 border:1px solid #ccc;background:#fff;border-radius:4px}
#c span:first-child{border-color:#c33}
button{font-size:16px;margin-right:8px}
#info{color:#666;font-size:13px;margin-top:6px}
</style>
<p>Draw in the box; candidates appear when you lift the pen. Strokes add up until you clear.</p>
<canvas id=p width=360 height=360></canvas>
<div><button id=clr>clear</button><button id=undo>undo stroke</button><button id=save>save as test case</button>
<input id=note placeholder="what you wrote (optional)" size=24></div>
<div id=c></div><div id=info></div>
<script>
const S=3, cv=document.getElementById('p'), g=cv.getContext('2d');
let strokes=[], cur=null, last=null, timer=null;
function grid(){g.clearRect(0,0,360,360);g.strokeStyle='#eee';g.lineWidth=1;
 g.beginPath();g.moveTo(180,0);g.lineTo(180,360);g.moveTo(0,180);g.lineTo(360,180);g.stroke();
 g.strokeStyle='#222';g.lineWidth=4;g.lineCap='round';g.lineJoin='round';
 for(const s of strokes){g.beginPath();s.forEach(([a,b],i)=>{const x=(a-1)*S,y=(b-10)*S;i?g.lineTo(x,y):g.moveTo(x,y)});
  if(s.length==1)g.lineTo((s[0][0]-1)*S+.1,(s[0][1]-10)*S);g.stroke();}}
function pt(e){const r=cv.getBoundingClientRect();const u=Math.floor((e.clientX-r.left)/r.width*120),
 v=Math.floor((e.clientY-r.top)/r.height*120);if(u<0||u>118||v<0||v>118)return null;return [u+1,v+10];}
function take(p){if(!p)return;if(last&&Math.abs(p[0]-last[0])<=2&&Math.abs(p[1]-last[1])<=2)return;
 if(strokes.reduce((n,s)=>n+s.length,0)>=0x400)return;cur.push(p);last=p;grid();}
cv.onpointerdown=e=>{if(strokes.length>=0x32)return;cv.setPointerCapture(e.pointerId);clearTimeout(timer);
 cur=[];strokes.push(cur);take(pt(e));};
cv.onpointermove=e=>{if(cur)take(pt(e));};
cv.onpointerup=e=>{if(!cur)return;if(!cur.length)strokes.pop();cur=null;last=null;timer=setTimeout(go,250);};
async function go(){if(!strokes.length){document.getElementById('c').innerHTML='';return;}
 const t=performance.now();const r=await fetch('/rec',{method:'POST',body:JSON.stringify(strokes)});const j=await r.json();
 document.getElementById('c').innerHTML=j.error?('<b>'+j.error+'</b>'):j.cands.map(c=>'<span>'+(c||'·')+'</span>').join('');
 document.getElementById('info').textContent=strokes.length+' strokes, '+strokes.reduce((n,s)=>n+s.length,0)+
  ' points, '+Math.round(performance.now()-t)+' ms';}
document.getElementById('clr').onclick=()=>{strokes=[];grid();go();};
document.getElementById('undo').onclick=()=>{strokes.pop();grid();go();};
document.getElementById('save').onclick=async()=>{if(!strokes.length)return;
 const r=await fetch('/save',{method:'POST',body:JSON.stringify({strokes,note:document.getElementById('note').value})});
 const j=await r.json();document.getElementById('info').textContent=j.error||('saved case #'+j.n+' to '+j.path);};
grid();
</script>"""

R = Recogniser()
ROOT = os.path.dirname(os.path.dirname(os.path.dirname(os.path.abspath(__file__))))
CASES = os.path.join(ROOT, 'engine/tests/data/dlt/cases.json')


def save(strokes, note):
    """append a test case: the strokes and the 12 raw candidate codes the recogniser gives for them"""
    try:
        strokes = [[[int(a), int(b)] for a, b in s] for s in strokes]
        codes = list(R.recognise_raw([[tuple(p) for p in s] for s in strokes]))
        cases = json.load(open(CASES)) if os.path.exists(CASES) else []
        cases.append(dict(note=note, strokes=strokes, codes=codes))
        os.makedirs(os.path.dirname(CASES), exist_ok=True)
        with open(CASES, 'w') as f:
            f.write('[\n' + ',\n'.join(json.dumps(c, ensure_ascii=False, separators=(',', ':')) for c in cases) + '\n]\n')
        return dict(n=len(cases), path=os.path.relpath(CASES, ROOT))
    except Exception as e:
        return dict(error=str(e))


class H(BaseHTTPRequestHandler):
    def log_message(self, *a):
        pass

    def _send(self, body, ctype):
        b = body.encode('utf8')
        self.send_response(200)
        self.send_header('Content-Type', ctype)
        self.send_header('Content-Length', str(len(b)))
        self.end_headers()
        self.wfile.write(b)

    def do_GET(self):
        self._send(PAGE, 'text/html; charset=utf-8')

    def do_POST(self):
        body = json.loads(self.rfile.read(int(self.headers['Content-Length'])))
        if self.path == '/save':
            return self._send(json.dumps(save(body['strokes'], body.get('note', '')), ensure_ascii=False),
                              'application/json')
        strokes = body
        try:
            out = dict(cands=R.recognise([[tuple(p) for p in s] for s in strokes]))
            print(len(strokes), 'strokes ->', ' '.join(c or '·' for c in out['cands']), flush=True)
        except Exception as e:
            out = dict(error=str(e))
        self._send(json.dumps(out, ensure_ascii=False), 'application/json')


if __name__ == '__main__':
    port = int(sys.argv[1]) if len(sys.argv) > 1 else 8765
    print('http://localhost:%d' % port, flush=True)
    srv = HTTPServer(('127.0.0.1', port), H)
    srv.serve_forever()
