/** Five native Rust desktop shells, exercised through actual Chromium canvas input.
 * Run after scripts/build-wasm.sh and node examples/browser/build.mjs.
 * PLAYWRIGHT_MODULE=/path/to/playwright/index.mjs CHROME_BIN=/usr/bin/google-chrome node scripts/test-desktops.mjs
 */
import assert from 'node:assert/strict';
import {createServer} from 'node:http';
import {readFile, mkdir, stat, writeFile} from 'node:fs/promises';
import {resolve, extname, sep} from 'node:path';
import {pathToFileURL} from 'node:url';
const root=resolve(new URL('..',import.meta.url).pathname);
const {chromium}=await import(process.env.PLAYWRIGHT_MODULE ? pathToFileURL(resolve(process.env.PLAYWRIGHT_MODULE)).href : 'playwright');
const mime={'.html':'text/html','.js':'text/javascript','.css':'text/css','.wasm':'application/wasm','.json':'application/json'};
const server=createServer(async(req,res)=>{try{let p=resolve(root,'.'+decodeURIComponent(new URL(req.url,'http://localhost').pathname));if(p!==root&&!p.startsWith(root+sep))throw Error('invalid path');if((await stat(p)).isDirectory())p=resolve(p,'index.html');res.writeHead(200,{'content-type':mime[extname(p)]||'application/octet-stream'});res.end(await readFile(p));}catch{res.writeHead(404);res.end('Not found');}});
await new Promise(r=>server.listen(0,'127.0.0.1',r));
const origin=`http://127.0.0.1:${server.address().port}`;
const browser=await chromium.launch({headless:true,...(process.env.CHROME_BIN?{executablePath:process.env.CHROME_BIN}:{}),args:['--no-sandbox']});
const context=await browser.newContext({viewport:{width:1600,height:1100}});
const errors=[],requestsAfterBoot=[];let booted=false;
await context.route('**/*',async route=>{const u=route.request().url();if(booted){requestsAfterBoot.push(u);await route.abort();}else if(!u.startsWith(origin+'/'))await route.abort();else await route.continue();});
const page=await context.newPage();page.on('pageerror',e=>errors.push(e.message));
const report={ok:false,devices:[],networkRequestsDuringEpisode:0};
const scene=()=>page.evaluate(()=>{const c=document.querySelector('#screen');return window.computerworldDemo.env.scene(c.width,c.height);});
async function clickTarget(target,{optional=false}={}){
  let s=await scene(),node=s.nodes.find(n=>n.interaction===target);
  if(!node&&target.startsWith('shell:launch:')&&s.nodes.some(n=>n.interaction==='shell:launcher')){await clickTarget('shell:launcher');s=await scene();node=s.nodes.find(n=>n.interaction===target);}
  if(!node&&optional)return false;
  assert.ok(node,`missing interaction ${target}`);
  const screen=page.locator('#screen');await screen.scrollIntoViewIfNeeded();
  const box=await screen.boundingBox(),size=await screen.evaluate(c=>({width:c.width,height:c.height}));
  const t=node.transform??{a:1024,b:0,c:0,d:1024,tx:0,ty:0},x=node.bounds.x+node.bounds.width/2,y=node.bounds.y+node.bounds.height/2;
  const px=(t.a*x+t.c*y)/1024+t.tx,py=(t.b*x+t.d*y)/1024+t.ty;
  await page.mouse.click(box.x+px*box.width/size.width,box.y+py*box.height/size.height);
  assert.doesNotMatch(await page.locator('#notice').innerText(),/^(Invalid|Unsupported|Permission|Action|Error)/i);
  return true;
}
async function frameMetrics(){return page.evaluate(()=>{const d=window.computerworldDemo,c=document.querySelector('#screen');const start=performance.now(),a=d.env.render(c.width,c.height),elapsed=performance.now()-start,b=d.env.render(c.width,c.height),bytes=a.rgba,other=b.rgba;let equal=bytes.length===other.length,hash=2166136261;for(let i=0;i<bytes.length;i++){if(bytes[i]!==other[i])equal=false;hash=Math.imul(hash^bytes[i],16777619)>>>0;}const result={width:a.width,height:a.height,rgbaBytes:bytes.length,deterministic:equal,pixelChecksum:hash.toString(16),renderMilliseconds:elapsed,nodes:d.env.scene(c.width,c.height).nodes.length};a.free();b.free();return result;});}
try{
  await mkdir(resolve(root,'artifacts'),{recursive:true});
  await page.goto(origin+'/examples/browser/');
  await page.waitForFunction(()=>window.demoReady||window.demoError,{timeout:60000});
  assert.equal(await page.evaluate(()=>window.demoError),undefined);booted=true;
  const devices=await page.evaluate(()=>{const d=window.computerworldDemo;return d.definition.computers.map(c=>({...c,profileName:d.definition.profiles.find(p=>p.id===c.profile)?.name,kind:d.definition.metadata?.device_presentations?.[c.id]}));});
  for(const theme of ['macos','windows','ubuntu','ios','android']){
    const device=devices.find(c=>(c.profile+' '+c.profileName).toLowerCase().includes(theme)&&!c.id.includes('server'));
    assert.ok(device,`missing ${theme} reference device`);
    await page.evaluate(id=>window.computerworldDemo.select(id),device.id);
    await page.evaluate(()=>window.computerworldDemo.act('application.v1','home'));
    const home=await frameMetrics();assert.equal(home.deterministic,true);
    assert.ok(home.nodes>20,`${theme} shell is unexpectedly sparse`);
    await page.locator('#screen').screenshot({path:resolve(root,`artifacts/desktop-${theme}-home.png`)});
    await clickTarget('shell:launch:browser');
    await clickTarget('shell:address');
    await page.keyboard.type('http://intranet.internal/');
    await page.locator('#screen').press('Enter');
    let observation=await page.evaluate(()=>window.computerworldDemo.env.observe());
    assert.match(JSON.stringify(observation),/intranet\.internal/,'native address did not navigate');
    const s=await scene(),link=s.nodes.find(n=>n.interaction&&n.semantic?.role==='link');
    assert.ok(link,`${theme} browser has no service-rendered link`);
    const beforeLink=observation.channels['browser.v1']?.[device.id]?.url;
    await clickTarget(link.interaction);
    observation=await page.evaluate(()=>window.computerworldDemo.env.observe());
    const afterLink=observation.channels['browser.v1']?.[device.id]?.url;
    assert.ok(afterLink&&afterLink!==beforeLink,`${theme} translated page link did not navigate`);
    const window=await frameMetrics();assert.equal(window.deterministic,true);
    await page.locator('#screen').screenshot({path:resolve(root,`artifacts/desktop-${theme}-window.png`)});
    const snapshot=await page.evaluate(()=>{const d=window.computerworldDemo;window.desktopQaSnapshot=d.world.snapshot();return d.world.stateHash();});
    if(!['ios','android'].includes(theme)){
      const beforeMax=(await scene()).nodes.find(n=>n.interaction==='shell:address').bounds;
      await clickTarget('shell:maximize');
      const afterMax=(await scene()).nodes.find(n=>n.interaction==='shell:address').bounds;
      assert.notDeepEqual(afterMax,beforeMax,'maximize did not change window geometry');
      await clickTarget('shell:minimize');
      assert.ok(!(await scene()).nodes.some(n=>n.interaction==='shell:address'),'minimized browser is still visible');
      await clickTarget('shell:launch:browser');
      assert.ok((await scene()).nodes.some(n=>n.interaction==='shell:address'),'minimized browser did not restore');
      await clickTarget('shell:close');
      assert.ok(!(await scene()).nodes.some(n=>n.interaction==='shell:address'),'closed browser is still visible');
    }else {await clickTarget('shell:home');assert.ok(!(await scene()).nodes.some(n=>n.interaction==='shell:address'),'phone home did not hide browser');}
    await page.evaluate(expected=>{const d=window.computerworldDemo,s=window.desktopQaSnapshot;d.world.restore(s);if(d.world.stateHash()!==expected)throw Error('desktop snapshot restore mismatch');const fork=d.world.fork(s);if(fork.stateHash()!==expected)throw Error('desktop fork mismatch');fork.free();s.free();delete window.desktopQaSnapshot;d.refresh();},snapshot);
    await clickTarget('shell:launch:terminal');
    await clickTarget('terminal-input',{optional:true});
    await page.keyboard.type(`echo qa-${theme}`);await page.locator('#screen').press('Enter');
    observation=await page.evaluate(()=>window.computerworldDemo.env.observe());
    assert.match(JSON.stringify(observation),new RegExp(`qa-${theme}`),'terminal keyboard output missing');
    report.devices.push({id:device.id,profile:device.profile,home,window,nativeBrowserAddress:true,offsetPageHitTest:true,terminalKeyboard:true,snapshotFork:true,windowControls:!['ios','android'].includes(theme)});
  }
  assert.equal(new Set(report.devices.map(d=>d.home.pixelChecksum)).size,5,'OS shells must render distinctly');
  await page.evaluate(()=>{const d=window.computerworldDemo;d.select(d.definition.computers.find(c=>c.profile.includes('macos')).id);d.refresh();});
  await page.screenshot({path:resolve(root,'artifacts/desktop-world-console.png'),fullPage:true});
  assert.deepEqual(requestsAfterBoot,[],'episode made real browser network requests');assert.deepEqual(errors,[],'browser errors');
  report.ok=true;report.networkRequestsDuringEpisode=requestsAfterBoot.length;
  await writeFile(resolve(root,'artifacts/desktop-verification.json'),JSON.stringify(report,null,2)+'\n');
  console.log(JSON.stringify(report,null,2));
}finally{await browser.close();await new Promise(r=>server.close(r));}
