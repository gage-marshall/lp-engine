import csv,collections,sys
fid={}
for l in open('fid/matches_Rom.arm.txt'):
    a,n,s=l.split(); fid[int(a,16)]=n[13:] if n.startswith('FID_conflict:') else n
mine={}
for r in csv.DictReader(open(sys.argv[1] if len(sys.argv)>1 else 'fid/objmatch_rom.csv')):
    if r['kind'] in('unique','resolved','gapfill','fuzzy') and (int(r['addr'].split(';')[0],16)+int(r['symoff']) not in mine) and r['symbol']:
        mine[int(r['addr'],16)+int(r['symoff'])]=(r['symbol'],r['kind'])
agree=[a for a in fid if a in mine and mine[a][0]==fid[a]]
dis=[(hex(a),fid[a],mine[a]) for a in fid if a in mine and mine[a][0]!=fid[a]]
fidonly=[(hex(a),fid[a]) for a in fid if a not in mine]
print('mine',len(mine),'fid',len(fid),'agree',len(agree),'disagree',len(dis),'fid-only',len(fidonly))
print('disagree:',dis[:12]); print('fid-only:',fidonly[:12])
