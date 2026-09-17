import json,subprocess,platform,pathlib,hashlib,gzip,os

def cmd(args):
 try:return subprocess.check_output(args,text=True,stderr=subprocess.STDOUT).strip()
 except Exception as e:return str(e)
files=['Cargo.lock','worlds/company-2026/world.json','crates/render/assets/DejaVuSansMono.ttf','pkg/web/computerworld_bg.wasm','pkg/web/computerworld.js','benchmarks/runner/src/bin/world.rs','benchmarks/runner/src/main.rs','benchmarks/browser-render.mjs','benchmarks/predecessor.mjs','target/release/world','target/release/cw-benchmarks','target/release/many_worlds']
assets={}
for p in files:
 if pathlib.Path(p).exists():
  b=pathlib.Path(p).read_bytes();assets[p]={'bytes':len(b),'gzip_bytes':len(gzip.compress(b,mtime=0)),'sha256':hashlib.sha256(b).hexdigest()}
meta={'revision':cmd(['git','rev-parse','HEAD']),'worktree_status':cmd(['git','status','--short']),'diff_sha256':hashlib.sha256(cmd(['git','diff','--binary']).encode()).hexdigest(),'rustc':cmd(['rustc','-Vv']),'kernel':platform.uname()._asdict(),'cpu':cmd(['lscpu']),'affinity':cmd(['taskset','-pc',str(os.getpid())]),'clock':cmd(['date','--iso-8601=seconds']),'browser':cmd(['google-chrome','--version']),'assets':assets,'pinned_cpu_capacity':pathlib.Path('/sys/devices/system/cpu/cpu19/cpu_capacity').read_text().strip(),'pinned_cpu_max_khz':pathlib.Path('/sys/devices/system/cpu/cpu19/cpufreq/cpuinfo_max_freq').read_text().strip(),'sampling':'Native pinned CPU19 (Cortex-X925); browser Chrome process tree inherits CPU19 when run under taskset. Five independent runs; all raw operation samples in sibling JSONs.'}
pathlib.Path('benchmarks/results/machine.json').write_text(json.dumps(meta,indent=2))
