/** Real Chromium / Rust Wasm integration. No network requests allowed after boot. */
import assert from 'node:assert/strict';
import {createServer} from 'node:http';
import {readFile, mkdir, stat} from 'node:fs/promises';
import {resolve, extname, sep} from 'node:path';
import {pathToFileURL} from 'node:url';
const root=resolve(new URL('..',import.meta.url).pathname);
const playwrightPath=process.env.PLAYWRIGHT_MODULE;
const {chromium}=await import(playwrightPath ? pathToFileURL(resolve(playwrightPath)).href : 'playwright');
const mime={'.html':'text/html','.js':'text/javascript','.css':'text/css','.wasm':'application/wasm','.json':'application/json'};
const server=createServer(async(req,res)=>{try{let path=resolve(root,'.'+decodeURIComponent(new URL(req.url,'http://localhost').pathname));if(path!==root&&!path.startsWith(root+sep))throw Error('invalid path');if((await stat(path)).isDirectory())path=resolve(path,'index.html');const bytes=await readFile(path);res.writeHead(200,{'content-type':mime[extname(path)]||'application/octet-stream'});res.end(bytes);}catch{res.writeHead(404);res.end('Not found');}});
await new Promise(resolve=>server.listen(0,'127.0.0.1',resolve));
const origin=`http://127.0.0.1:${server.address().port}`;
const browser=await chromium.launch({headless:true,...(process.env.CHROME_BIN?{executablePath:process.env.CHROME_BIN}:{}),args:['--no-sandbox']});
const context=await browser.newContext({viewport:{width:1440,height:1300}});
const errors=[],requestsAfterBoot=[];let booted=false;
await context.route('**/*',async route=>{const url=route.request().url();if(booted){requestsAfterBoot.push(url);await route.abort();}else if(!url.startsWith(origin+'/'))await route.abort();else await route.continue();});
const page=await context.newPage();page.on('pageerror',e=>errors.push(e.message));
try {
  await page.goto(origin+'/examples/browser/');
  await page.waitForFunction(()=>window.demoReady||window.demoError,{timeout:30000});
  assert.equal(await page.evaluate(()=>window.demoError),undefined);
  booted=true;
  const report=await page.evaluate(()=>{
    const d=window.computerworldDemo;
    const check=(value,message)=>{if(!value)throw Error(message);};
    const initial=d.world.stateHash();
    const snapshot=d.world.snapshot();
    const command=d.act('terminal.v1','execute',{command:'echo offline-wasm'});
    check(command.outcomes[0].success,'terminal action failed');
    check(JSON.stringify(command).includes('offline-wasm'),'terminal output missing');
    const changed=d.world.stateHash();check(changed!==initial,'action did not change state');
    d.world.restore(snapshot);check(d.world.stateHash()===initial,'snapshot state hash changed');
    const replay=d.act('terminal.v1','execute',{command:'echo offline-wasm'});
    check(d.world.stateHash()===changed,'deterministic replay mismatch');
    const fork=d.world.fork(snapshot);check(fork.stateHash()===initial,'fork state mismatch');fork.free();snapshot.free();
    const definitions=d.definition;
    for(const c of definitions.computers){d.select(c.id);check(d.act('terminal.v1','execute',{command:'pwd'}).outcomes[0].success,`computer ${c.id} unavailable`);}
    d.select(definitions.computers[0].id);
    const site=definitions.services.find(s=>s.kind==='static-site')||definitions.services[0];
    const navigation=d.navigate(`http://${site.domains[0]}/`);check(navigation.outcomes[0].success,'synthetic navigation failed');
    const denied=d.navigate('https://real-internet.invalid/');check(!denied.outcomes[0].success,'outbound network unexpectedly allowed');
    d.navigate(`http://${site.domains[0]}/`);
    const frame=d.env.render(960,560);const a=Array.from(frame.rgba);frame.free();const second=d.env.render(960,560);const b=second.rgba;check(a.length===960*560*4,'wrong frame size');check(a.every((v,i)=>v===b[i]),'render nondeterminism');check(new Set(a).size>3,'render appears blank');second.free();
    const events=d.world.trajectory();check(events.length>0,'trajectory empty');check(events.some(e=>/network|http|dns/.test(e.kind)),'network trajectory missing');
    check(typeof d.env.inspect==='undefined'&&typeof d.env.snapshot==='undefined','actor exposes owner privileges');
    d.refresh();return {computers:definitions.computers.length,services:definitions.services.length,events:events.length,rgbaBytes:a.length,stateHash:d.world.stateHash()};
  });
  await page.locator('#snapshot').click();await page.locator('#fork').click();await page.locator('#restore').click();await page.locator('#reset').click();
  assert.deepEqual(requestsAfterBoot,[],'episode made browser network requests');assert.deepEqual(errors,[],'browser errors');
  await mkdir(resolve(root,'artifacts'),{recursive:true});await page.screenshot({path:resolve(root,'artifacts/browser-demo.png'),fullPage:true});
  console.log(JSON.stringify({ok:true,networkRequestsDuringEpisode:requestsAfterBoot.length,...report},null,2));
}finally{await browser.close();await new Promise(resolve=>server.close(resolve));}
