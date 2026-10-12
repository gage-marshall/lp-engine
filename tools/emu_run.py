"""Run all script commands (stage 1) + their task descriptors through TaskList_TickTaskMain (stage 2).
usage: emu_run.py OUT.json ov,ov,...   (overlays to load, e.g. 2,41)"""
import sys,json,csv,struct
sys.path.insert(0,'tools')
import emu_core as E
out=sys.argv[1]; ovs=[int(x) for x in sys.argv[2].split(',') if x]
emu=E.Emu(ovs)
use={r['cmd']:r['argc_histogram'] for r in csv.DictReader(open('fid/script_cmd_usage.csv'))}
cmds=[r['cmd'] for r in csv.DictReader(open('fid/script_commands.csv'))]
RUNMAIN=0x205a8f4; DESC=0x2330000
res={}
def best_argc(c):
    h=use.get(c,'')
    if not h: return 0
    return max(((int(x.split(':')[0]),int(x.split(':')[1])) for x in h.split()),key=lambda t:t[1])[0]
for c in cmds:
    ci=int(c,16); res[c]={}
    for argc in range(0,9):
        cands=[]
        score=lambda ev:sum(1 for e in ev if e[0] in ('call','ext','task','str','enter'))-5*any(e[0]=='fault' for e in ev)
        for gate in ('all',set()):
            best_for_gate=None
            for rd in (0,1):
                emu.setup_script_ctx(ci,argc,'int')
                ev,_=emu.run(RUNMAIN,regs=(E.REF,),native_depth=1,ret_default=rd,gate_loaded=gate)
                if best_for_gate is None or score(ev)>score(best_for_gate): best_for_gate=ev
            cands.append(best_for_gate)
        res[c]['%dint'%argc]=max(cands,key=score)
        if any(e[0]=='gate' for e in cands[0]+cands[1]):
            res[c]['%dgate'%argc]={'loaded':cands[0],'notloaded':cands[1]}
    for argc in {best_argc(c)}:
        emu.setup_script_ctx(ci,argc,'str')
        ev,_=emu.run(RUNMAIN,regs=(E.REF,),native_depth=1,gate_loaded='all')
        res[c]['%dstr'%argc]=ev
    # stage 2: task descriptors from the best-argc int run
    b=best_argc(c); st2=[]
    for e in res[c]['%dint'%b]:
        if e[0]=='task':
            for rd in (0,1):
                emu.setup_script_ctx(ci,b,'int')
                emu.uc.mem_write(DESC,b''.join(struct.pack('<I',w) for w in e[3]))
                ev2,ret=emu.run(E.TASK,regs=(DESC,0,0,0),native_depth=0,ret_default=rd,gate_loaded='all')
                st2.append({'kind':e[3][0],'ret_default':rd,'ret':ret,'events':ev2})
    res[c]['stage2']=st2
json.dump(res,open(out,'w'))
print(len(res),'commands; overlays',ovs)
