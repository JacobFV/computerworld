"""Persistent PyO3 call overhead; no Python simulation or Node subprocess."""
import json,os,time,platform
from pathlib import Path
import computerworld
count=int(os.environ.get('BENCH_SAMPLES','1000')); runs=int(os.environ.get('BENCH_RUNS','5')); results=[]
definition=json.loads(Path('worlds/company-2026/world.json').read_text())
for name in ['python.terminal.step','python.terminal.batch10','python.observe','python.reset','python.snapshot','python.fork']:
 for run in range(runs):
  world=computerworld.World(definition,2026)
  env=world.environment({'actor':'alice','machines':['alice-mac'],'actions':['terminal.v1'],'observations':['terminal.v1']})
  action={'family':'terminal.v1','op':'execute','machine':'alice-mac','payload':{'command':'pwd'}}; snapshot=world.snapshot(); samples=[]
  for i in range(count+100):
   t=time.perf_counter_ns()
   if name in ['python.terminal.step','python.terminal.batch10']:
    result=env.step([action]*(10 if name.endswith('batch10') else 1)); assert all(x['success'] for x in result['outcomes'])
   elif name=='python.observe':env.observe()
   elif name=='python.reset':world.reset(2026)
   elif name=='python.snapshot':world.snapshot()
   elif name=='python.fork':world.fork(snapshot)
   elapsed=time.perf_counter_ns()-t
   if i>=100:samples.append(elapsed)
  ordered=sorted(samples); results.append(dict(name=name,run=run,count=count,p50_ns=ordered[count//2],p95_ns=ordered[count*95//100],mean_ns=sum(samples)/count,samples_ns=samples))
Path(os.environ.get('BENCH_OUTPUT','benchmarks/results/python.json')).write_text(json.dumps({'meta':{'python':platform.python_version(),'note':'PyO3 canonical Rust runtime with Python conversion. Batch10 is batch latency.'},'results':results},indent=2))
print([{k:v for k,v in x.items() if k!='samples_ns'} for x in results])
