/** Real Chromium input acceptance for retained Rust OS shells.
 * Build fresh Wasm/demo first. No network is permitted after boot.
 */
import assert from 'node:assert/strict';
import {createServer} from 'node:http';
import {readFile,mkdir,stat,writeFile} from 'node:fs/promises';
import {resolve,extname,sep} from 'node:path';
import {pathToFileURL} from 'node:url';
const root=resolve(new URL('..',import.meta.url).pathname);
const {chromium}=await import(process.env.PLAYWRIGHT_MODULE?pathToFileURL(resolve(process.env.PLAYWRIGHT_MODULE)).href:'playwright');
const mime={'.html':'text/html','.js':'text/javascript','.css':'text/css','.wasm':'application/wasm','.json':'application/json'};
const server=createServer(async(req,res)=>{try{let p=resolve(root,'.'+decodeURIComponent(new URL(req.url,'http://localhost').pathname));if(p!==root&&!p.startsWith(root+sep))throw Error('invalid path');if((await stat(p)).isDirectory())p=resolve(p,'index.html');res.writeHead(200,{'content-type':mime[extname(p)]||'application/octet-stream'});res.end(await readFile(p));}catch{res.writeHead(404);res.end('Not found');}});
await new Promise(r=>server.listen(0,'127.0.0.1',r));
const origin=`http://127.0.0.1:${server.address().port}`;
const browser=await chromium.launch({headless:true,...(process.env.CHROME_BIN?{executablePath:process.env.CHROME_BIN}:{}),args:['--no-sandbox']});
const context=await browser.newContext({viewport:{width:1800,height:1250}});
let booted=false;const requests=[],errors=[];
await context.route('**/*',async route=>{const u=route.request().url();if(booted){requests.push(u);await route.abort();}else if(u.startsWith(origin+'/'))await route.continue();else await route.abort();});
const page=await context.newPage();page.on('pageerror',e=>errors.push(e.message));
const report={ok:false,devices:[],networkRequestsDuringEpisode:0,visualAssessment:'Screenshots require reference review; no pixel-similarity score claimed for differing scenes.'};
const scene=()=>page.evaluate(()=>{const c=document.querySelector('#screen');return window.computerworldDemo.env.scene(c.width,c.height);});
const state=()=>page.evaluate(()=>{const d=window.computerworldDemo;return JSON.parse(d.world.exportSnapshot()).sessions[d.env.id].machines[d.machine];});
const act=(family,op,payload={})=>page.evaluate(({family,op,payload})=>window.computerworldDemo.act(family,op,payload),{family,op,payload});
async function target(action){const s=await scene();const n=s.nodes.findLast(n=>n.interaction===action);assert.ok(n,`missing interaction ${action}`);const t=n.transform??{a:1024,b:0,c:0,d:1024,tx:0,ty:0},x=n.bounds.x+n.bounds.width/2,y=n.bounds.y+n.bounds.height/2;return{x:(t.a*x+t.c*y)/1024+t.tx,y:(t.b*x+t.d*y)/1024+t.ty};}
async function screenPoint(p){const c=page.locator('#screen');await c.scrollIntoViewIfNeeded();const b=await c.boundingBox(),s=await c.evaluate(c=>({width:c.width,height:c.height}));return{x:b.x+p.x*b.width/s.width,y:b.y+p.y*b.height/s.height};}
async function click(action){const p=await screenPoint(await target(action));await page.mouse.click(p.x,p.y);assert.doesNotMatch(await page.locator('#notice').innerText(),/^(Invalid|Unsupported|Permission|Action|Error)/i);}
async function drag(action,delta,absolute=false){const start=await target(action),end=absolute?delta:{x:start.x+delta.x,y:start.y+delta.y},a=await screenPoint(start),b=await screenPoint(end);await page.mouse.move(a.x,a.y);await page.mouse.down();await page.mouse.move(b.x,b.y,{steps:8});await page.mouse.up();assert.equal((await state()).desktop.pointer_capture,null,'released gesture retains pointer capture');}
async function swipe(start,end){const a=await screenPoint(start),b=await screenPoint(end);await page.mouse.move(a.x,a.y);await page.mouse.down();await page.mouse.move(b.x,b.y,{steps:8});await page.mouse.up();}
/** Going home on a phone: Android presses its Home button, iPhone swipes up from the
 * home indicator — short, because a long swipe is the App Switcher. */
async function goHome(theme,size){
  if(theme!=='ios'){await click('shell:home');return;}
  await swipe({x:size.w/2,y:size.h-20},{x:size.w/2,y:size.h-20-Math.floor(size.h/5)});
}
async function shot(name){await page.locator('#screen').screenshot({path:resolve(root,`artifacts/overhaul-${name}.png`)});}
async function launch(kind){await act('application.v1','launch',{kind});const s=await state();return s.desktop.focused;}
async function assertSnapshot(){const before=await state();await page.evaluate(()=>{const d=window.computerworldDemo;window.overhaulSnapshot=d.world.snapshot();window.overhaulHash=d.world.stateHash();});await act('application.v1','home');await page.evaluate(()=>{const d=window.computerworldDemo,s=window.overhaulSnapshot;d.world.restore(s);if(d.world.stateHash()!==window.overhaulHash)throw Error('restore hash mismatch');const f=d.world.fork(s);if(f.stateHash()!==window.overhaulHash)throw Error('fork hash mismatch');f.free();s.free();d.refresh();});assert.deepEqual(await state(),before,'snapshot lost window/application state');}
try{
 await mkdir(resolve(root,'artifacts'),{recursive:true});await page.goto(origin+'/examples/browser/');await page.waitForFunction(()=>window.demoReady||window.demoError,{timeout:120000});assert.equal(await page.evaluate(()=>window.demoError),undefined);booted=true;
 const devices=await page.evaluate(()=>window.computerworldDemo.definition.computers);
 for(const theme of (process.env.OVERHAUL_THEMES?.split(',')??['macos','windows','ubuntu','ios','android'])){
  const dev=devices.find(c=>c.profile.includes(theme)&&!c.id.includes('server'));assert.ok(dev,`missing ${theme}`);await page.evaluate(id=>window.computerworldDemo.select(id),dev.id);await act('application.v1','home');await shot(`${theme}-home`);
  const checks=[];
  const launcherNodes=(await scene()).nodes;
  const launcher=launcherNodes.some(n=>n.interaction==='shell:launcher')?'shell:launcher':launcherNodes.some(n=>n.interaction==='shell:search')?'shell:search':null;
  if(launcher){await click(launcher);await shot(`${theme}-launcher`);await act('application.v1','home');checks.push('native launcher opens');}
  if(!['ios','android'].includes(theme)){
   const id=await launch('terminal');const dragTarget=`window:${id}:drag`;
   await drag(dragTarget,{x:-25,y:35});let previous=(await state()).desktop.windows[id].frame;assert.ok(previous,'drag must persist explicit frame');
   for(const delta of [{x:50,y:20},{x:-35,y:-15},{x:20,y:30}]){await drag(dragTarget,delta);const next=(await state()).desktop.windows[id].frame;assert.ok(Math.abs(next.x-previous.x-delta.x)<=1,'horizontal drag displacement');assert.ok(Math.abs(next.y-previous.y-delta.y)<=1,'vertical drag displacement');previous=next;}checks.push('real pointer window dragging at four positions');
   for(const edge of ['e','s','se','nw']){const before=(await state()).desktop.windows[id].frame;await drag(`window:${id}:resize:${edge}`,{x:edge==='s'?0:edge==='nw'?12:-12,y:edge==='e'?0:edge==='nw'?10:-10});const after=(await state()).desktop.windows[id].frame;assert.notDeepEqual(after,before,`${edge} resize ineffective`);}checks.push('edge and corner resizing');
   const restored=(await state()).desktop.windows[id].frame;await click(`window:${id}:maximize`);assert.equal((await state()).desktop.windows[id].maximized,true);await click(`window:${id}:maximize`);assert.deepEqual((await state()).desktop.windows[id].frame,restored);checks.push('maximize restores exact geometry');
   for(const side of ['left','right']){const size=await page.locator('#screen').evaluate(c=>({w:c.width,h:c.height}));await drag(dragTarget,{x:side==='left'?1:size.w-1,y:Math.floor(size.h/3)},true);assert.equal((await state()).desktop.windows[id].snapped,side);}checks.push('left and right drag snapping');
   await click(`window:${id}:minimize`);assert.equal((await state()).desktop.windows[id].minimized,true);await click(`window:${id}:focus`);assert.equal((await state()).desktop.windows[id].minimized,false);checks.push('per-window minimize and taskbar restore');
   const second=await launch('editor');assert.notEqual(second,id);await click(`window:${id}:focus`);let s=await state();assert.equal(s.desktop.focused,id);assert.equal(s.desktop.stacking.at(-1),id);await click(`window:${second}:focus`);checks.push('overlapping independent windows focus and stack');
   const firstBrowser=await launch('browser');await act('browser.v1','navigate',{url:'http://intranet.internal/'});const secondBrowser=await launch('browser');await act('browser.v1','navigate',{url:'http://guide.example/'});assert.notEqual(firstBrowser,secondBrowser);await act('application.v1','focus',{window:firstBrowser});assert.match(JSON.stringify((await state()).browser),/intranet\.internal/);await act('application.v1','focus',{window:secondBrowser});assert.match(JSON.stringify((await state()).browser),/guide\.example/);checks.push('independent browser window histories');
   await shot(`${theme}-multiwindow`);await assertSnapshot();checks.push('snapshot and fork preserve full geometry and app state');
  }else{
   await click('shell:launch:browser');await act('browser.v1','navigate',{url:'http://intranet.internal/'});await shot(`${theme}-browser`);const size=await page.locator('#screen').evaluate(c=>({w:c.width,h:c.height}));await goHome(theme,size);assert.equal((await state()).desktop.focused,null);checks.push(theme==='ios'?'upward home gesture from the home indicator':'navigation bar Home button');
   // The app drawer is reached differently on each phone: Android swipes up from the
   // hotseat — above the navigation bar, which is buttons, not gesture area — and iOS
   // swipes left past the last home page into the App Library.
   if(theme==='android'){await swipe({x:size.w/2,y:size.h-70},{x:size.w/2,y:size.h-200});}
   else{await swipe({x:size.w-30,y:Math.floor(size.h/2)},{x:30,y:Math.floor(size.h/2)});}
   assert.equal((await state()).desktop.launcher_open,true);await shot(`${theme}-drawer`);await goHome(theme,size);
   await swipe({x:size.w/2,y:30},{x:size.w/2,y:200});assert.ok((await state()).desktop.panel,'downward gesture did not open notification/control panel');await shot(`${theme}-shade`);await goHome(theme,size);checks.push('upward app drawer and downward system panel gestures');
   // The App Switcher: a long swipe up from the iPhone's home indicator, and the
   // Recents button on Android's navigation bar.
   await launch('browser');
   if(theme==='android'){await click('shell:overview');}
   else{await swipe({x:size.w/2,y:size.h-25},{x:size.w/2,y:Math.floor(size.h/2)-30});}
   assert.equal((await state()).desktop.panel,'overview');await shot(`${theme}-recents`);await goHome(theme,size);checks.push(theme==='ios'?'long upward recent applications gesture':'navigation bar Recents button');
   const nodes=(await scene()).nodes;const recents=nodes.find(n=>['shell:recents','shell:overview'].includes(n.interaction));if(recents){await click(recents.interaction);assert.equal((await state()).desktop.panel,'overview');await shot(`${theme}-recents`);await goHome(theme,size);checks.push('mobile recent applications');}
  }
  // These are native applications now, not browser aliases: the window must be the app
 // itself, and the browser must not have been navigated on its behalf.
 for(const kind of ['mail','calendar','messages','docs','notes','contacts','settings','calculator','clock']){
  const window=await launch(kind);const s=await state();
  const opened=s.desktop.windows[String(window)];
  assert.ok(opened,`${kind} opened no window`);
  assert.equal(opened.state.type,'native',`${kind} did not open a native window`);
  assert.equal(opened.state.app,kind,`${kind} opened the wrong application`);
  assert.equal(s.browser_visible,false,`${kind} opened the browser instead of an application`);
  checks.push(`${kind} is a native application`);
  if(kind==='mail')await shot(`${theme}-mail`);
  if(kind==='calendar')await shot(`${theme}-calendar`);
 }
  await assertSnapshot();
  report.devices.push({id:dev.id,theme,checks});console.log(`${theme}: ${checks.length} interaction checks passed`);
 }
 assert.deepEqual(errors,[]);assert.deepEqual(requests,[],'real network requests after boot');report.ok=true;
} catch(error){report.error=error.stack;await page.screenshot({path:resolve(root,'artifacts/overhaul-failure.png'),fullPage:true}).catch(()=>{});throw error;}
finally{report.networkRequestsDuringEpisode=requests.length;report.browserErrors=errors;await writeFile(resolve(root,'artifacts/overhaul-verification.json'),JSON.stringify(report,null,2)+'\n');console.log(JSON.stringify(report,null,2));await browser.close();await new Promise(r=>server.close(r));}
