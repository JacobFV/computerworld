// Node executes the same generated Wasm module as the browser package.
import {readFile,writeFile} from 'node:fs/promises';
import {createRequire} from 'node:module';import {performance} from 'node:perf_hooks';
const require=createRequire(import.meta.url);const t0=performance.now();const wasm=require('../pkg/node/computerworld.js');const instantiate_ns=(performance.now()-t0)*1e6;
const definition=JSON.parse(await readFile('worlds/company-2026/world.json','utf8'));let results=[];const count=+(process.env.BENCH_SAMPLES||1000),runs=+(process.env.BENCH_RUNS||5);
for(const name of ['wasm.terminal.step','wasm.terminal.batch10','wasm.observe','wasm.reset','wasm.snapshot','wasm.fork'])for(let run=0;run<runs;run++){
 const world=new wasm.World(definition,2026);const env=world.environment({actor:'alice',machines:['alice-mac'],actions:['terminal.v1'],observations:['terminal.v1']});let samples=[];
 const action={family:'terminal.v1',op:'execute',machine:'alice-mac',payload:{command:'pwd'}};const snapshot=world.snapshot();
 for(let i=0;i<count+100;i++){const t=performance.now();switch(name){case 'wasm.terminal.step':case 'wasm.terminal.batch10':{const r=env.step(name.endsWith('batch10')?Array(10).fill(action):[action]);if(!r.outcomes.every(x=>x.success))throw Error(JSON.stringify(r));break;}case 'wasm.observe':env.observe();break;case 'wasm.reset':world.reset(2026);break;case 'wasm.snapshot':world.snapshot().free();break;case 'wasm.fork':world.fork(snapshot).free();break;}if(i>=100)samples.push((performance.now()-t)*1e6);}
 samples.sort((a,b)=>a-b);results.push({name,run,count,p50_ns:samples[Math.floor(count/2)],p95_ns:samples[Math.floor(count*.95)],mean_ns:samples.reduce((a,b)=>a+b,0)/count,samples_ns:samples});snapshot.free();env.free();world.free();
}
await writeFile(process.env.BENCH_OUTPUT||'benchmarks/results/wasm-node.json',JSON.stringify({meta:{node:process.version,instantiate_ns,note:'Node WebAssembly canonical runtime; boundary conversion included. Batch10 latency is whole ten-action batch; not individual-operation quantiles.',memory:process.memoryUsage()},results},null,2));console.log(results.map(({samples_ns,...v})=>v));
