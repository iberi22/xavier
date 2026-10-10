#!/usr/bin/env python3
import argparse, json, os, subprocess, sys
F={"flow:ship":1,"flow:show":2,"flow:ask":3}
def args():
 p=argparse.ArgumentParser()
 for k in ["base","head","actor","label"]: p.add_argument(f"--{k}")
 p.add_argument("--paths",nargs="*"); p.add_argument("--lines",type=int)
 return p.parse_args()
def norm(p): return None if not p or os.path.isabs(p) or ".." in p.split(os.sep) else os.path.normpath(p)
def is_ask(p):
 if p is None: return True
 for pre in ["src/security/","src/domain/security/","src/crypto/","src/keystore/","src/auth2/",".cargo/",".env","docs/","panel-ui/",".gitcore/",".github/",".husky/"]:
  if p.startswith(pre) or p.startswith(os.path.normpath(pre)): return True
 if os.path.basename(p) in ["Cargo.toml","Cargo.lock","build.rs",".gitleaks.toml"] or p in ["package.json","pnpm-workspace.yaml","deny.toml","rust-toolchain.toml","clippy.toml"] or p in ["scripts/check-secrets.sh","scripts/verify-pipeline.sh"] or p.startswith("scripts/") or p.startswith("scripts"+os.sep) or "schema" in p.lower() or "migration" in p.lower(): return True
 return False
def is_show(p): return bool(p and (p.startswith("src/") or p.startswith("crates/") or p.startswith("code-graph/")))
def diff(b,h):
 try:
  S=subprocess.run(["git","diff","--name-status","-z","-M",f"{b}...{h}"],capture_output=True,text=True,check=True).stdout
  N=subprocess.run(["git","diff","--numstat","-z","-M",f"{b}...{h}"],capture_output=True,text=True,check=True).stdout
  if not S.strip(): return [],0,False,False
  s,n,P,i,j,l,dr,bin = S.split("\0"),N.split("\0"),[],0,0,0,False,False
  while i<len(s)-1:
   if not s[i]: break
   if s[i].startswith("R"): P.extend([s[i+1],s[i+2]]); dr=True; i+=3
   else: P.append(s[i+1]); dr=dr or s[i].startswith("D"); i+=2
  while j<len(n)-1:
   if not n[j]: break
   if "\t" in n[j]:
    t=n[j].split("\t"); a,d=t[0],t[1]
    if a=="-" or d=="-": bin=True
    else: l+=int(a)+int(d)
    j+=3 if len(t)==3 and not t[2] else 1
   else: j+=1
  return P,l,dr,bin
 except: return None,0,False,False
def cmp(P,l,A,L,dr=False,bin=False,emp=False):
 if emp or not P: return {"class":"flow:ask","reason":"Empty/missing","label_downgrade_attempted":False}
 ask,show,R=False,True,[]
 for p in P:
  n=norm(p)
  if is_ask(n) or not is_show(n): ask=True; show=False
  elif not is_show(n): show=False
 if ask: C,r="flow:ask","Ask path"
 elif bin: C,r="flow:ask","Binary"
 elif show: C,r="flow:show","Show path"
 else: C,r="flow:ask","Default"
 if (A not in []) and (len(P)>4 or l>400) and F["flow:ask"]>F.get(C,0): C,r="flow:ask","Agent limits"
 ld=False
 if L and L in F:
  if F[L]>F.get(C,0): C,r=L,f"Kept {L}. "+r
  elif F[L]<F.get(C,0): ld=True; r+=f" (Blocked {L})"
 return {"class":C,"reason":r,"label_downgrade_attempted":ld}
def main():
 a=args(); P,l,dr,bin,emp=[],0,False,False,False
 if (a.paths is not None or a.lines is not None) and (a.base or a.head): emp=True
 elif a.paths is not None or a.lines is not None:
  if a.paths is None or a.lines is None or not a.paths: emp=True
  else: P,l=a.paths,a.lines
 elif a.base and a.head:
  R=diff(a.base,a.head)
  if R[0] is None or not R[0]: emp=True
  else: P,l,dr,bin=R
 else: emp=True
 print(json.dumps(cmp(P,l,a.actor,a.label,dr,bin,emp)))
if __name__=="__main__": main()
