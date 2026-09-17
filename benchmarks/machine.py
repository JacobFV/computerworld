import json,subprocess,platform,pathlib,hashlib,gzip,os

def cmd(args):
 try:return subprocess.check_output(args,text=True,stderr=subprocess.STDOUT).strip()
 except Exception as e:return str(e)
files=['Cargo.lock','worlds/company-2026/world.json','crates/render/assets/DejaVuSansMono.ttf','pkg/web/computerworld_bg.wasm','pkg/web/computerworld.js','benchmarks/runner/src/bin/world.rs','benchmarks/runner/src/main.rs','benchmarks/browser-render.mjs','benchmarks/predecessor.mjs']
assets={}
for p in files:
 if pathlib.Path(p).exists():
  b=pathlib.Path(p).read_bytes();assets[p]={'bytes':len(b),'gzip_bytes':len(gzip.compress(b,mtime=0)),'sha256':hashlib.sha256(b).hexdigest()}
meta={'revision':cmd(['git','rev-parse','HEAD']),'rustc':cmd(['rustc','-Vv']),'kernel':platform.uname()._asdict(),'cpu':cmd(['lscpu']),'affinity':cmd(['taskset','-pc',str(os.getpid())]),'clock':cmd(['date','--iso-8601=seconds']),'browser':cmd(['google-chrome','--version']),'assets':assets,'sampling':'Native pinned CPU0 (Cortex-X925); browser Chrome process tree inherits CPU0 when run under taskset. Five independent runs; all raw operation samples in sibling JSONs.'}
pathlib.Path('benchmarks/results/machine.json').write_text(json.dumps(meta,indent=2))
