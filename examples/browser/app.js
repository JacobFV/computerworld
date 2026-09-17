import init, { World } from '../../pkg/web/computerworld.js';
import definition from './world-definition.js';
const $ = id => document.getElementById(id);
const pretty = value => JSON.stringify(value, null, 2);
let world, saved, machine, environments = new Map();
const seed = 2026;
const config = id => ({actor: `demo-${id}`,machines:[id],actions:['terminal.v1','browser.v1','keyboard.v1','pointer.v1','application.v1','filesystem.v1','http.v1'],observations:['terminal.v1','semantic.v1'],action_budget:1000000});
const env = () => environments.get(machine);
function sessions() {
  for (const environment of environments.values()) environment.free();
  environments = new Map(definition.computers.map(c => [c.id, world.environment(config(c.id))]));
}
function announce(text) { $('notice').textContent = text; }
function refresh() {
  const observation = env().observe();
  $('observation').textContent = pretty(observation);
  const terminal = observation.channels['terminal.v1']?.[machine];
  if (terminal) $('terminal-output').textContent = terminal.stdout ?? pretty(terminal);
  const frame = env().render(960, 560);
  const pixels = new Uint8ClampedArray(frame.rgba);
  $('screen').getContext('2d').putImageData(new ImageData(pixels, frame.width, frame.height),0,0);
  frame.free();
  const events = world.trajectory();
  $('tick').textContent = events.length;
  $('event-count').textContent = `(${events.length})`;
  $('trajectory').textContent = pretty(events.slice(-100));
  $('hash').textContent = world.stateHash().slice(0,12);
  return observation;
}
function act(family, op, payload = {}) {
  const result = env().step([{family,op,machine,payload}]);
  const failure = result.outcomes.find(outcome => !outcome.success);
  announce(failure ? `${failure.error?.code}: ${failure.error?.message}` : `${family} / ${op} completed in the Rust runtime.`);
  refresh();
  return result;
}
function navigate(url) { $('url').value = url; return act('browser.v1','navigate',{url}); }
function select(id) {
  machine = id;
  $('machine-name').textContent = `${id} · ${definition.computers.find(c => c.id === id).profile}`;
  document.querySelectorAll('[data-machine]').forEach(button => button.classList.toggle('active',button.dataset.machine === id));
  refresh();
}
function protect(fn) { return (...args) => {try {return fn(...args);} catch(error) {announce(String(error)); console.error(error);}}; }
document.querySelectorAll('[data-app]').forEach(button => button.onclick=protect(()=>act('application.v1','launch',{kind:button.dataset.app})));
$('navigation').onsubmit = protect(event => {event.preventDefault();navigate($('url').value);});
$('terminal').onsubmit = protect(event => {event.preventDefault();act('terminal.v1','execute',{command:$('command').value});$('command').value='';});
$('screen').onclick = protect(event => {
  const bounds = event.currentTarget.getBoundingClientRect();
  act('pointer.v1','click',{x:Math.floor((event.clientX-bounds.left)*960/bounds.width),y:Math.floor((event.clientY-bounds.top)*560/bounds.height),width:960,height:560});
  $('screen').focus();
});
$('screen').onkeydown = protect(event => {
  if (event.ctrlKey || event.metaKey || event.altKey) return;
  if (event.key === 'Tab') return;
  event.preventDefault();
  act('keyboard.v1',event.key.length===1?'type':'key',event.key.length===1?{text:event.key}:{key:event.key});
});
$('snapshot').onclick = protect(() => {saved?.free();saved=world.snapshot();$('restore').disabled=false;$('fork').disabled=false;announce(`Snapshot saved: ${world.stateHash().slice(0,12)}.`);});
$('restore').onclick = protect(() => {world.restore(saved);refresh();announce('World restored, including services, computers and actor sessions.');});
$('fork').onclick = protect(() => {
  const fork = world.fork(saved);
  const ids = new Map([...environments].map(([computer, environment]) => [computer, environment.id]));
  for (const environment of environments.values()) environment.free();
  environments.clear();world.free();world=fork;
  environments = new Map([...ids].map(([computer,id]) => [computer,world.session(id)]));refresh();announce('Now controlling an independent fork of the saved world.');
});
$('reset').onclick = protect(() => {world.reset(seed);refresh();announce(`World reset with seed ${seed}.`);});
try {
  await init();
  world = new World(definition, seed);
  sessions();
  for (const computer of definition.computers) {
    const button = document.createElement('button');button.dataset.machine=computer.id;
    button.textContent=computer.id;const subtitle=document.createElement('small');subtitle.textContent=computer.profile;button.append(subtitle);
    button.onclick=protect(()=>select(computer.id));$('machines').append(button);
  }
  for (const service of definition.services) {
    if (!service.domains.length) continue;
    const button=document.createElement('button');button.textContent=service.domains[0];button.onclick=protect(()=>navigate(`http://${service.domains[0]}/`));$('sites').append(button);
  }
  select(definition.computers[0].id);
  const first = definition.services.find(s => s.kind === 'static-site') ?? definition.services[0];
  if (first?.domains.length) navigate(`http://${first.domains[0]}/`);
  $('loading').hidden=true;$('status').textContent='Local runtime ready';
  // Deliberate owner-only test/dev surface. It is never the handle given to an agent.
  window.computerworldDemo = {get world(){return world;},get env(){return env();},get machine(){return machine;},select,act,navigate,refresh,definition};
  window.demoReady=true;
} catch(error) {$('loading').textContent=`Runtime could not start: ${error}`;$('status').textContent='Initialization failed';console.error(error);window.demoError=String(error);}
