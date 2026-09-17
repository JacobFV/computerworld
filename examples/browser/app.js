import init, { World } from '../../pkg/web/computerworld.js';
import initialDefinition from './world-definition.js';
const $ = id => document.getElementById(id);
const pretty = value => JSON.stringify(value, (_, v) => typeof v === 'bigint' ? v.toString() : v, 2);
let world, saved, machine, scrollY = 0, environments = new Map(), definition = initialDefinition;
let presentations = {...initialDefinition.metadata?.device_presentations}, savedPresentation, savedSessions;
const seed = 2026;
const kind = id => presentations[id] ?? (id.includes('server') ? 'server' : 'desktop');
const dimensions = id => kind(id)==='phone' ? [390,780] : [960,640];
const config = id => ({actor: definition.computers.find(c => c.id === id).user,machines:[id],actions:['terminal.v1','browser.v1','keyboard.v1','pointer.v1','application.v1','filesystem.v1','http.v1'],observations:['terminal.v1','semantic.v1','browser.v1'],action_budget:1000000});
const env = () => environments.get(machine);
function sessions(ids) {
  for (const environment of environments.values()) environment.free();
  environments = new Map(definition.computers.map(c => [c.id, ids?.has(c.id) ? world.session(ids.get(c.id)) : world.environment(config(c.id))]));
}
function announce(text) { $('notice').textContent = text; }
function protect(fn) { return (...args) => {try {return fn(...args);} catch(error) {announce(String(error));}}; }
function paint(environment, canvas, width, height) {
  const frame = environment.render(width,height);
  canvas.width=width;canvas.height=height;
  canvas.getContext('2d').putImageData(new ImageData(new Uint8ClampedArray(frame.rgba),width,height),0,0);
  frame.free();
}
function refresh() {
  if (!env()) return;
  const observation = env().observe();
  $('observation').textContent = pretty(observation);
  const browserUrl = observation.channels['browser.v1']?.[machine]?.url;
  if (browserUrl) $('url').value = browserUrl;
  const terminal = observation.channels['terminal.v1']?.[machine];
  $('terminal-output').textContent = terminal ? (terminal.stdout ?? pretty(terminal)) : 'Ready. Run a command or use the screen.';
  paint(env(),$('screen'),...dimensions(machine));
  for (const c of definition.computers) {
    const card = [...document.querySelectorAll('[data-machine]')].find(el=>el.dataset.machine===c.id);
    const e = environments.get(c.id);
    if (!card || !e) continue;
    if (kind(c.id)==='server') {
      const t=e.observe().channels['terminal.v1']?.[c.id];
      card.querySelector('pre').textContent=`${c.id}\n${c.address}\n> ${t?.stdout || 'Console ready\nNo display attached'}`;
    } else {
      const preview=card.querySelector('canvas');
      if(c.id===machine){preview.width=$('screen').width;preview.height=$('screen').height;preview.getContext('2d').drawImage($('screen'),0,0);preview.dataset.rendered='true';}
      else if(!preview.dataset.rendered){paint(e,preview,...dimensions(c.id));preview.dataset.rendered='true';}
    }
  }
  const events = world.trajectory();
  $('tick').textContent = events.length;
  $('event-count').textContent = `(${events.length})`;
  $('trajectory').textContent = pretty(events.slice(-100));
  $('hash').textContent = world.stateHash().slice(0,16);
  return observation;
}
function act(family, op, payload = {}) {
  const result = env().step([{family,op,machine,payload}]);
  const failure = result.outcomes.find(outcome => !outcome.success);
  announce(failure ? `${failure.error?.code}: ${failure.error?.message}` : `Updated ${machine}.`);
  refresh();return result;
}
function navigate(url) { scrollY = 0; $('url').value = url; return act('browser.v1','navigate',{url}); }
function select(id) {
  if(!environments.has(id)) return;
  machine=id;scrollY=0;
  const c=definition.computers.find(c=>c.id===id);
  $('machine-name').textContent=id;
  $('device-meta').textContent=`${kind(id)==='phone'?'Phone · ':kind(id)==='server'?'Headless · ':''}${definition.profiles.find(p=>p.id===c.profile)?.name ?? c.profile} · ${c.address} · ${c.user}`;
  $('display-shell').className=`display-shell ${kind(id)}`;
  $('screen').setAttribute('aria-label',kind(id)==='server'?`Remote console for ${id}`:`Interactive screen for ${id}`);
  $('pointer-focus').textContent=kind(id)==='phone'?'☝ Touchscreen':'↖ Mouse';
  const hosts=definition.services.some(s=>s.node===(c.node||c.id));
  $('remove-device').disabled=hosts;
  $('remove-device').title=hosts?'This computer hosts services; move those services before removing it.':'Remove selected computer';
  $('navigation').hidden=kind(id)==='server';
  document.querySelectorAll('[data-machine]').forEach(b=>{b.classList.toggle('active',b.dataset.machine===id);b.setAttribute('aria-pressed',String(b.dataset.machine===id));});
  refresh();requestAnimationFrame(drawLinks);
}
function drawLinks() {
  const map=$('network-map'),scale=Number($('zoom').value)/100,base=map.getBoundingClientRect();
  const elements=[...map.querySelectorAll('[data-node]')];
  const positions=new Map(elements.map(el=>{const r=el.getBoundingClientRect();return [el.dataset.node,{x:(r.x-base.x+r.width/2)/scale,y:(r.y-base.y+r.height/2)/scale}];}));
  $('links').replaceChildren();
  for(const link of definition.network.links){const a=positions.get(link.from),b=positions.get(link.to);if(!a||!b)continue;
    const p=document.createElementNS('http://www.w3.org/2000/svg','path');
    p.setAttribute('d',`M${a.x} ${a.y} C${a.x} ${(a.y+b.y)/2},${b.x} ${(a.y+b.y)/2},${b.x} ${b.y}`);
    if(link.from===machine||link.to===machine)p.setAttribute('class','selected');
    const title=document.createElementNS('http://www.w3.org/2000/svg','title');title.textContent=`${link.from} ${link.bidirectional?'↔':'→'} ${link.to} · ${link.latency_us} μs`;p.append(title);$('links').append(p);
  }
}
function buildMap() {
  definition=world.definition();
  $('machines').replaceChildren();$('sites').replaceChildren();
  for(const c of definition.computers){
    const button=document.createElement('button');button.className=`device-card ${kind(c.id)}`;button.dataset.machine=c.id;button.dataset.node=c.node||c.id;button.setAttribute('aria-label',`Control ${c.id}, ${kind(c.id)}`);
    const display=document.createElement('div');display.className='mini-screen';display.append(document.createElement(kind(c.id)==='server'?'pre':'canvas'));if(kind(c.id)==='server')display.firstChild.className='server-lines';
    const foot=document.createElement('div');foot.className='monitor-foot';
    const peripherals=document.createElement('div');peripherals.className='mini-peripherals';peripherals.textContent=kind(c.id)==='phone'?'Touch · text input':kind(c.id)==='server'?'● ●  Remote console':'⌨  ▰';
    const title=document.createElement('span');title.className='card-title';title.textContent=c.id;
    const subtitle=document.createElement('small');subtitle.textContent=`${definition.profiles.find(p=>p.id===c.profile)?.name ?? c.profile} · ${c.address}`;
    button.append(display,foot,peripherals,title,subtitle);button.onclick=protect(()=>select(c.id));$('machines').append(button);
  }
  const represented=new Set(definition.computers.map(c=>c.node||c.id));
  for(const s of definition.services){
    const node=definition.network.nodes.find(n=>n.id===s.node);
    const b=document.createElement('button');b.className=`service-card ${node?.zone==='internet'?'public':''}`;
    if(!represented.has(s.node)){b.dataset.node=s.node;represented.add(s.node);}
    const title=document.createElement('strong');title.textContent=`◇ ${s.domains[0]??s.id}`;
    const subtitle=document.createElement('small');subtitle.textContent=`${s.kind} · ${node?.zone==='internet'?'synthetic internet':s.node}`;
    b.append(title,subtitle);b.onclick=protect(()=>navigate(`http://${s.domains[0]??node?.address}/`));$('sites').append(b);
  }
  $('device-count').textContent=definition.computers.length;$('service-count').textContent=definition.services.length;
  $('topology-summary').textContent=`${definition.network.nodes.length} nodes · ${definition.network.links.length} links`;
  requestAnimationFrame(drawLinks);
}
function nextIdentity() {let n=1;while(definition.computers.some(c=>c.id===`device-${n}`))n++;return `device-${n}`;}
function openAdd() {
  $('device-id').value=nextIdentity();$('form-error').textContent='';$('device-link').replaceChildren();
  const isolated=document.createElement('option');isolated.value='';isolated.textContent='Isolated (no links)';$('device-link').append(isolated);
  for(const n of definition.network.nodes){const o=document.createElement('option');o.value=n.id;o.textContent=`${n.id} · ${n.address}`;$('device-link').append(o);}
  $('device-link').value=definition.network.nodes.find(n=>n.id==='app-server')?.id??definition.network.nodes[0]?.id??'';
  $('device-dialog').showModal();
}
function addDevice({id,profile,user,type='desktop',connectTo='app-server'}) {
  if(profile==='virtual-ios-18'||profile==='virtual-android-12')type='phone';
  let octet=20;const used=new Set(definition.network.nodes.map(n=>n.address));while(used.has(`10.0.2.${octet}`)&&octet<255)octet++;
  if(octet===255)throw Error('No address available in the demo subnet.');
  const address=`10.0.2.${octet}`;
  const computer={id,profile,address,user,initial_files:{'notes.txt':`Welcome ${user}.\nInternal site: http://intranet.internal/\n`},installed_apps:['terminal','browser','editor','files','desktop',...(profile.startsWith('virtual-')?['mail','calendar','chat','docs']:[])],packages:['coreutils','git','curl']};
  const node={id,address,zone:'local'};
  const links=connectTo?[{from:connectTo,to:id,bidirectional:true,latency_us:10,loss_per_million:0}]:[];
  world.addComputer(computer,node,links);presentations[id]=type;definition=world.definition();environments.set(id,world.environment(config(id)));
  if(type==='server'){environments.get(id).step([{family:'application.v1',op:'launch',machine:id,payload:{kind:'terminal'}}]);}
  buildMap();select(id);announce(`Added ${id}${connectTo?` connected to ${connectTo}`:' with no network links'}.`);
}
function removeDevice(id=machine) {
  world.removeComputer(id);environments.get(id)?.free();environments.delete(id);delete presentations[id];buildMap();
  if(definition.computers.length)select(definition.computers[0].id);
  else {machine=undefined;$('screen').getContext('2d').clearRect(0,0,$('screen').width,$('screen').height);$('machine-name').textContent='No devices';$('device-meta').textContent='Add a device to begin.';$('remove-device').disabled=true;}
  announce(`Removed ${id}. Other devices and services retain their state.`);
}
document.querySelectorAll('[data-app]').forEach(b=>b.onclick=protect(()=>{act('application.v1','launch',{kind:b.dataset.app});document.querySelectorAll('[data-app]').forEach(x=>x.classList.toggle('active',x===b));}));
$('home-screen').onclick=protect(()=>act('application.v1','home'));
$('expand-screen').onclick=()=>{const active=document.querySelector('.console-layout').classList.toggle('focused-device');$('expand-screen').textContent=active?'Show network':'Expand desktop';$('expand-screen').setAttribute('aria-expanded',String(active));requestAnimationFrame(drawLinks);};
$('navigation').onsubmit=protect(e=>{e.preventDefault();navigate($('url').value);});
$('terminal').onsubmit=protect(e=>{e.preventDefault();act('terminal.v1','execute',{command:$('command').value});$('command').value='';});
let pointerGesture=null, queuedPointer=null, pointerFrame=0;
function pointerCoordinates(e) {const r=$('screen').getBoundingClientRect(),[width,height]=dimensions(machine);return {x:Math.round((e.clientX-r.left)*width/r.width),y:Math.round((e.clientY-r.top)*height/r.height),width,height,button:e.button<0?0:e.button,pointer_type:e.pointerType||'mouse'};}
function pointerStep(op,payload,full=false) {
  const result=env().step([{family:'pointer.v1',op,machine,payload}]);
  const failure=result.outcomes.find(outcome=>!outcome.success);
  if(failure)announce(`${failure.error?.code}: ${failure.error?.message}`);
  const cursor=result.outcomes[0]?.value?.cursor;if(cursor)$('screen').style.cursor=cursor;
  if(full)refresh();else paint(env(),$('screen'),...dimensions(machine));
  return result;
}
function flushPointer() {pointerFrame=0;if(queuedPointer&&env()){const p=queuedPointer;queuedPointer=null;if(p.machine===machine){delete p.machine;pointerStep('move',p);}}}
$('screen').onpointerdown=protect(e=>{if(!env())return;e.preventDefault();$('screen').focus();$('screen').setPointerCapture(e.pointerId);pointerGesture={id:e.pointerId,machine};pointerStep('down',pointerCoordinates(e));});
$('screen').onpointermove=protect(e=>{if(pointerGesture&&pointerGesture.id!==e.pointerId)return;if(!pointerGesture&&e.pointerType!=='mouse')return;queuedPointer={...pointerCoordinates(e),machine};if(!pointerFrame)pointerFrame=requestAnimationFrame(protect(flushPointer));});
$('screen').onpointerup=protect(e=>{if(!pointerGesture||pointerGesture.id!==e.pointerId)return;if(pointerFrame)cancelAnimationFrame(pointerFrame);flushPointer();pointerStep('up',pointerCoordinates(e),true);pointerGesture=null;$('screen').releasePointerCapture(e.pointerId);});
$('screen').onpointercancel=protect(e=>{if(!pointerGesture)return;if(pointerFrame)cancelAnimationFrame(pointerFrame);queuedPointer=null;pointerStep('cancel',pointerCoordinates(e),true);pointerGesture=null;});
$('screen').ondblclick=protect(e=>{e.preventDefault();pointerStep('double_click',pointerCoordinates(e),true);});
$('screen').oncontextmenu=e=>e.preventDefault();

$('screen').addEventListener('wheel',protect(e=>{e.preventDefault();scrollY=Math.max(0,scrollY+Math.round(e.deltaY));act('browser.v1','scroll',{y:scrollY});}),{passive:false});
$('screen').onkeydown=protect(e=>{if(e.key==='Tab'&&!e.altKey&&!e.metaKey)return;e.preventDefault();const prefix=e.metaKey?'Meta+':e.ctrlKey?'Ctrl+':e.altKey?'Alt+':'';const key=prefix+e.key;act('keyboard.v1',!prefix&&e.key.length===1?'type':'key',!prefix&&e.key.length===1?{text:e.key}:{key});});
$('keyboard-focus').onclick=()=>{$('text-entry').focus();announce('Text is sent to the focused field in the active application.');};
$('typing').onsubmit=protect(e=>{e.preventDefault();act('keyboard.v1','type',{text:$('text-entry').value});$('text-entry').value='';});
$('pointer-focus').onclick=()=>{$('screen').focus();announce(kind(machine)==='phone'?'Tap the screen to interact.':'Click the screen to interact.');};
$('enter-key').onclick=protect(()=>act('keyboard.v1','key',{key:'Enter'}));$('back-key').onclick=protect(()=>act('keyboard.v1','key',{key:'Backspace'}));
$('snapshot').onclick=protect(()=>{saved?.free();saved=world.snapshot();savedPresentation={...presentations};savedSessions=new Map([...environments].map(([id,e])=>[id,e.id]));$('restore').disabled=false;$('fork').disabled=false;announce('Saved devices, services, topology and actor sessions.');});
function restored() {definition=world.definition();presentations={...savedPresentation};sessions(savedSessions);buildMap();select(environments.has(machine)?machine:definition.computers[0]?.id);}
$('restore').onclick=protect(()=>{world.restore(saved);restored();announce('Restored saved world, including its devices and connections.');});
$('fork').onclick=protect(()=>{const fork=world.fork(saved);world.free();world=fork;restored();announce('Now controlling an independent fork of the saved world.');});
$('reset').onclick=protect(()=>{world.reset(seed);definition=world.definition();presentations={...initialDefinition.metadata?.device_presentations};sessions();buildMap();select(definition.computers[0]?.id);announce('Reset to the original world and seed 2026.');});
$('add-device').onclick=openAdd;$('close-dialog').onclick=()=>$('device-dialog').close();
$('device-kind').onchange=()=>{if($('device-kind').value==='phone')$('device-profile').value='virtual-ios-18';else if($('device-kind').value==='server')$('device-profile').value='ubuntu';else $('device-profile').value='virtual-ubuntu-24';$('device-help').textContent=$('device-kind').value==='phone'?'A touch-driven home screen and applications using the chosen synthetic mobile OS profile.':$('device-kind').value==='server'?'A computer without a monitor, controlled through its remote console.':'An independent filesystem, processes and network identity.';};
$('device-profile').onchange=()=>{if(['virtual-ios-18','virtual-android-12'].includes($('device-profile').value))$('device-kind').value='phone';};
$('device-form').onsubmit=e=>{e.preventDefault();try{addDevice({id:$('device-id').value,profile:$('device-profile').value,user:$('device-user').value,type:$('device-kind').value,connectTo:$('device-link').value});$('device-dialog').close();}catch(error){$('form-error').textContent=String(error);}};
$('remove-device').onclick=protect(()=>removeDevice());
$('zoom').oninput=()=>{const value=Number($('zoom').value);$('zoom-label').value=`${value}%`;$('network-map').style.zoom=value/100;requestAnimationFrame(drawLinks);};
new ResizeObserver(()=>requestAnimationFrame(drawLinks)).observe($('network-map'));
try {
  await init();world=new World(initialDefinition,seed);definition=world.definition();sessions();
  for(const c of definition.computers){if(kind(c.id)==='server')environments.get(c.id).step([{family:'application.v1',op:'launch',machine:c.id,payload:{kind:'terminal'}}]);}
  buildMap();select(definition.computers[0].id);$('loading').hidden=true;$('status').textContent='● Running locally';
  window.computerworldDemo={get world(){return world;},get env(){return env();},get machine(){return machine;},get definition(){return definition;},select,act,navigate,refresh,addDevice,removeDevice,buildMap};window.demoReady=true;
} catch(error){$('loading').textContent=`Runtime could not start: ${error}`;$('status').textContent='Initialization failed';console.error(error);window.demoError=String(error);}
