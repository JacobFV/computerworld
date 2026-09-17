// Run with Node's tsx loader from the pinned SCE checkout (see README).
import { performance } from 'node:perf_hooks';
import { mkdtemp, writeFile, rm } from 'node:fs/promises';
import { tmpdir, cpus, release, arch } from 'node:os';
import { join, resolve } from 'node:path';
import { pathToFileURL } from 'node:url';
import { execFileSync } from 'node:child_process';
const source=resolve(process.env.SCE_SOURCE || '/tmp/computerworld-sources/synthetic-computer-environment');
const {SimulationRuntime}=await import(pathToFileURL(join(source,'packages/kernel/src/simulation.ts')));
const {seed2026Blueprint}=await import(pathToFileURL(join(source,'ecosystems/seed-2026/src/index.ts')));
const root=await mkdtemp(join(tmpdir(),'cw-baseline-'));
const count=Number(process.env.BENCH_SAMPLES||1000), runs=Number(process.env.BENCH_RUNS||5);
const results=[];
async function bench(name,fn,n=count){for(let i=0;i<100;i++)await fn(i); for(let run=0;run<runs;run++){let samples=[];for(let i=0;i<n;i++){const a=performance.now();await fn(i);samples.push((performance.now()-a)*1e6);}const sorted=[...samples].sort((a,b)=>a-b);results.push({name,run,count:n,p50_ns:sorted[Math.floor(n*.5)],p95_ns:sorted[Math.floor(n*.95)],mean_ns:samples.reduce((a,b)=>a+b,0)/n,samples_ns:samples});}}
try{
 const initStart=performance.now();const r=new SimulationRuntime({topology:seed2026Blueprint,stateRoot:root,runId:'baseline'}); await r.initialize();const initialize_ns=(performance.now()-initStart)*1e6;
 await bench('predecessor.terminal.pwd',async()=>{const x=await r.execute('ubuntu-dev','pwd');if(x.exitCode!==0)throw Error(x.stderr)});
 await bench('predecessor.terminal.pipe',async()=>{const x=await r.execute('ubuntu-dev','echo benchmark | cat');if(!x.stdout.includes('benchmark'))throw Error('pipe failed')});
 await bench('predecessor.filesystem.write_read',async i=>{const x=await r.execute('ubuntu-dev',`echo value-${i} > /tmp/bench.txt; cat /tmp/bench.txt`);if(!x.stdout.includes(`value-${i}`))throw Error('write/read failed')});
 await bench('predecessor.network.http',async()=>{const x=await r.http('win-workstation','http://intranet.seed.local:8080/');if(x.status!==200)throw Error('http failed')});
 const meta={initialize_ns,timestamp:new Date().toISOString(),commit:execFileSync('git',['rev-parse','HEAD'],{cwd:source,encoding:'utf8'}).trim(),node:process.version,cpu:cpus()[0].model,os:release(),arch:arch(),rss_bytes:process.memoryUsage().rss,note:'Persistent SCE runtime; default tracing. SCE VFS persists through host filesystem. Samples exclude initialization and disposal.'};
 await writeFile(process.env.BENCH_OUTPUT||'benchmarks/results/predecessor.json',JSON.stringify({meta,results},null,2));
 console.log(JSON.stringify(results.map(({samples_ns,...x})=>x),null,2));
}finally{await rm(root,{recursive:true,force:true})}
