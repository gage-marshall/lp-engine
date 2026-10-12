"""Combine emu_cmds2.json into fid/script_command_spec2.csv and fid/task_kinds.csv"""
import json,csv,sys,collections
sys.path.insert(0,'tools')
import emu_core as E
names=dict(E.LAB); names.update(E.OVLAB.get(2,{})); names.update(E.OVLAB.get(41,{}))
r=json.load(open('fid/emu_cmds2.json'))
use={x['cmd']:x for x in csv.DictReader(open('fid/script_cmd_usage.csv'))}
def best(c):
    h=use.get(c,{}).get('argc_histogram','')
    return max(((int(x.split(':')[0]),int(x.split(':')[1])) for x in h.split()),key=lambda t:t[1])[0] if h else 0
def fv(v,fnarg=False):
    if 0x1001<=v<=0x1008: return 'a%d'%(v-0x1001)
    if 0x80001001<=v<=0x80001008: return 's%d'%(v-0x80001001)
    if v==0xffffffff: return '-1'
    if v==0x7000: return 'CHAR'
    if fnarg and v in names: return names[v]
    if v in names and v>=0x2000000: return names[v]
    return hex(v) if v>9 else str(v)
def evs(es,skip_enter=True):
    o=[]
    if skip_enter and not any(e[0] in ('call','ext','str','char','task','fault') for e in es):
        skip_enter=False
    for k,d,n,a in es:
        if k=='task': continue
        if k=='enter' and (skip_enter or n.startswith('Script')): continue
        if k=='gate': continue
        if k=='fault': o.append('FAULT'); continue
        if k=='str': o.append('str'); continue
        if k=='char': o.append('charid'); continue
        args=[fv(x,fnarg=(n=='Input_SetSlotParams')) for x in a[:3]]
        while args and args[-1]=='0': args.pop()
        o.append('%s(%s)'%(n,','.join(args)))
    return o
kinds=collections.defaultdict(lambda:{'cmds':set(),'calls':None,'ret':set(),'faults':0})
rows=[]
for c,v in r.items():
    b=best(c); s1=v['%dint'%b]
    parts=evs(s1)
    tasks=[e for e in s1 if e[0]=='task']
    t2=[]
    for t in tasks:
        k=t[3][0]
        # take ret_default 0 run
        cand=[s for s in v['stage2'] if s['kind']==k]
        if cand:
            s=max(cand,key=lambda s:sum(1 for e in s['events'] if e[0] in ('call','ext')))
            if True:
                e2=evs(s['events']); kinds[k]['cmds'].add(c)
                if kinds[k]['calls'] is None or len(e2)>len(kinds[k]['calls']): kinds[k]['calls']=e2
                desc=' '.join(fv(x) for x in t[3][1:10]).rstrip(' 0')
                t2.append('TASK%#x[%s]'%(k,desc))
                parts+= [x for x in e2 if x!='FAULT']
    eff=' ; '.join(t2+[p for p in parts if not p.startswith('TASK')][:10])
    g=v.get('%dgate'%b)
    if g:
        gid=[e[3][0] for e in g['loaded'] if e[0]=='gate'][:1]
        eff='GATE ov%s: loaded=[%s] | not-loaded=[%s]'%(gid[0] if gid else '?',' ; '.join(evs(g['loaded'])[:5]),' ; '.join(evs(g['notloaded'])[:5]))
    rows.append((c,int(use[c]['uses']) if c in use else 0,b,eff))
rows.sort(key=lambda x:-x[1])
with open('fid/script_command_spec2.csv','w') as f:
    w=csv.writer(f); w.writerow(['cmd','uses','argc','effect']); w.writerows(rows)
with open('fid/task_kinds.csv','w') as f:
    w=csv.writer(f); w.writerow(['kind','commands','effects'])
    for k in sorted(kinds): w.writerow([hex(k),' '.join(sorted(kinds[k]['cmds'])),' ; '.join(kinds[k]['calls'] or [])])
nf=sum(1 for c in r if any(e[0]=='fault' for e in r[c]['%dint'%best(c)]))
none=sum(1 for x in rows if not x[3])
print(len(rows),'cmds;',len(kinds),'task kinds; stage-1 faulting',nf,'; empty effect',none)
