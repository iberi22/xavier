import importlib.util, json, os, subprocess, sys, tempfile, unittest
script_path = os.path.join(os.path.dirname(__file__), "classify-change.py")
spec = importlib.util.spec_from_file_location("c", script_path)
c = importlib.util.module_from_spec(spec)
sys.modules["c"] = c
spec.loader.exec_module(c)

class T(unittest.TestCase):
 def test_table(self):
  cases = [
   (["x.txt"],10,"human",None,False,False,False,"flow:ask",False),
   (["src/a.rs","crates/b.rs"],20,"agent",None,False,False,False,"flow:show",False),
   (["src/a.rs","src/b.rs","src/c.rs","src/d.rs","src/e.rs"],10,"agent",None,False,False,False,"flow:ask",False),
   (["src/a.rs"],401,"agent",None,False,False,False,"flow:ask",False),
   (["src/crypto/mod.rs"],10,"agent",None,False,False,False,"flow:ask",False),
   (["crates/Cargo.toml"],10,"human",None,False,False,False,"flow:ask",False),
   (["docs/a.md"],10,"human",None,False,False,False,"flow:ask",False),
   (["src/../Cargo.toml"],10,"human",None,False,False,False,"flow:ask",False),
   ([".env.example"],2,"agent",None,False,False,False,"flow:ask",False),
   (["src/a.rs","src/b.rs"],20,"agent",None,True,False,False,"flow:show",False),
   (["src/a.rs","docs/a.md"],20,"agent",None,True,False,False,"flow:ask",False),
   (["src/a.png"],0,"human",None,False,True,False,"flow:ask",False),
   ([],0,"human",None,False,False,True,"flow:ask",False),
   (["docs/a.md"],10,"agent","flow:ship",False,False,False,"flow:ask",True),
   (["src/a.rs"],10,"agent","flow:ask",False,False,False,"flow:ask",False),
  ]
  for P,l,a,L,dr,bin,emp,E_c,E_ld in cases:
   with self.subTest(P=P,l=l,a=a,L=L,dr=dr,bin=bin,emp=emp):
    r = c.compute_classification(P,l,a,L,dr,bin,emp)
    self.assertEqual(r["class"], E_c)
    self.assertEqual(r["label_downgrade_attempted"], E_ld)

class TCLI(unittest.TestCase):
 def setUp(self):
  self.d = tempfile.TemporaryDirectory()
  self.r = self.d.name
  subprocess.run(["git","init","-b","main"],cwd=self.r,check=True,capture_output=True)
  subprocess.run(["git","config","user.name","X"],cwd=self.r,check=True)
  subprocess.run(["git","config","user.email","x@x"],cwd=self.r,check=True)
  subprocess.run(["git","commit","--allow-empty","-m","I"],cwd=self.r,check=True)
 def tearDown(self): self.d.cleanup()
 def cli(self,*a):
  return json.loads(subprocess.run([sys.executable,os.path.abspath(script_path)]+list(a),cwd=self.r,capture_output=True,text=True).stdout.strip())
 def test_cli(self):
  self.assertEqual(self.cli("--base","HEAD~1","--head","HEAD","--paths","a","--lines","1")["class"],"flow:ask")
  subprocess.run(["git","checkout","-b","f"],cwd=self.r,check=True)
  self.assertEqual(self.cli("--base","main","--head","f")["class"],"flow:ask")

  with open(os.path.join(self.r,"a.bin"),"wb") as f: f.write(b'\x00\x01')
  subprocess.run(["git","add","a.bin"],cwd=self.r,check=True)
  subprocess.run(["git","commit","-m","B"],cwd=self.r,check=True)
  self.assertEqual(self.cli("--base","HEAD~1","--head","HEAD")["class"],"flow:ask")

  os.makedirs(os.path.join(self.r,"src"),exist_ok=True)
  with open(os.path.join(self.r,"src","a.txt"),"w") as f: f.write("a\n")
  subprocess.run(["git","add","src/a.txt"],cwd=self.r,check=True)
  subprocess.run(["git","commit","-m","T"],cwd=self.r,check=True)
  subprocess.run(["git","mv","src/a.txt","src/b.txt"],cwd=self.r,check=True)
  subprocess.run(["git","commit","-m","R"],cwd=self.r,check=True)
  self.assertEqual(self.cli("--base","HEAD~1","--head","HEAD")["class"],"flow:show")

  self.assertEqual(self.cli("--base","unknown_ref","--head","HEAD")["class"],"flow:ask")

  # Three-dot empty (Head behind/at merge base, main ahead)
  subprocess.run(["git","checkout","main"],cwd=self.r,check=True)
  subprocess.run(["git","commit","--allow-empty","-m","AHEAD"],cwd=self.r,check=True)
  self.assertEqual(self.cli("--base","main","--head","f")["class"],"flow:ask")

  # CLI sub-process for Ask paths
  cases = [("src/security/x.rs","ask"), ("crates/a/Cargo.toml","ask"), ("src/../Cargo.toml","ask")]
  for p, _ in cases:
   d = os.path.join(self.r, os.path.dirname(p))
   os.makedirs(d, exist_ok=True)
   with open(os.path.join(self.r, p), "w") as f: f.write("1\n")
   subprocess.run(["git","add",p],cwd=self.r,check=True)
   subprocess.run(["git","commit","-m","A"],cwd=self.r,check=True)
   self.assertEqual(self.cli("--base","HEAD~1","--head","HEAD")["class"],"flow:ask")

if __name__=='__main__': unittest.main()
