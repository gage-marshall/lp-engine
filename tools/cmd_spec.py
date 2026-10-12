"""Summarize fid/emu_cmds.json + usage into fid/script_command_spec.csv"""
import json,csv,collections
em=json.load(open('fid/emu_cmds.json'))
use={r['cmd']:r for r in csv.DictReader(open('fid/script_cmd_usage.csv'))}
def tagname(v):
    if 0x1001<=v<=0x1008: return 'a%d'%(v-0x1001)
    if 0x80001001<=v<=0x80001008: return 's%d'%(v-0x80001001)
    return None
def fmt(v):
    t=tagname(v)
    if t: return t
    if v==0xffffffff: return '-1'
    if v==0x7000: return 'CHAR'
    return hex(v) if v>9 else str(v)
def summarize(evs):
    parts=[]
    for k,d,n,a in evs:
        if k=='task':
            kind=a[0]; cnt=a[1]&0xff
            words=[fmt(x) for x in a[1:12]]
            while words and words[-1]=='0': words.pop()
            parts.append('TASK kind=%#x desc[1..]=%s'%(kind,' '.join(words)))
        elif k=='call': parts.append('%s(%s)'%(n,','.join(fmt(x) for x in a[:3])))
        elif k=='ext': parts.append('%s(%s)'%(n,','.join(fmt(x) for x in a[:3])))
        elif k=='str': parts.append('str')
        elif k=='char': parts.append('charlookup')
        elif k=='fault': parts.append('FAULT '+n)
        elif k=='enter': parts.append('>'+n)
    return ' ; '.join(p for p in parts if not p.startswith('>')) or '(none) '+' '.join(p for p in parts if p.startswith('>'))
rows=[]; faults=0; empty=0
for c,v in em.items():
    u=use.get(c); h=u['argc_histogram'] if u else ''
    best=None
    if h:
        best=max(((int(x.split(':')[0]),int(x.split(':')[1])) for x in h.split()),key=lambda t:t[1])[0]
    best=best if best is not None else 0
    evs=v['%dint'%best]
    if any(e[0]=='fault' for e in evs): faults+=1
    s=summarize(evs)
    if s.startswith('(none)'): empty+=1
    rows.append((c,int(u['uses']) if u else 0,best,h,s))
rows.sort(key=lambda r:-r[1])
with open('fid/script_command_spec.csv','w') as f:
    w=csv.writer(f); w.writerow(['cmd','uses','argc','argc_hist','emulated_effect'])
    for r in rows: w.writerow(r)
print(len(rows),'cmds; faults',faults,'; no external effect traced',empty)
for r in rows[:40]: print(r[0],r[1],'argc',r[2],'|',r[4][:150])
