// Homepage diagram frames: a real P2 counterexample and a non-merging branch.
import {readFileSync, writeFileSync, mkdirSync} from 'node:fs';
import {createRequire} from 'node:module';
import {encodePNG} from './dom-to-site/png.mjs';
const require = createRequire(import.meta.url);
const {World} = require('../pkg/node/computerworld.js');
const report = JSON.parse(readFileSync(new URL('../site/data/policies.json', import.meta.url)));
const world = new World(JSON.parse(readFileSync(new URL('../worlds/company-2026/world.json', import.meta.url))), 7n);
const env = world.environment({actor:'alice', machines:['alice-mac'], actions:['application.v1','pointer.v1','browser.v1'], observations:['semantic.v1']});
const step = action => {
  const result = env.step([{...action, machine:'alice-mac'}]).outcomes[0];
  if (!result.success) throw new Error(JSON.stringify(result.error));
  return result;
};
const out = new URL('../site/media/branches/', import.meta.url);
mkdirSync(out, {recursive:true});
const save = name => {const frame=env.render(1440,900);writeFileSync(new URL(name+'.png',out),encodePNG(frame));frame.free();};
step({family:'application.v1',op:'launch',payload:{kind:'browser',argument:'http://github.com/northstar/atlas/pulls'}});
step({family:'application.v1',op:'maximize',payload:{window:0}});
save('checkpoint');
const controls = report.policies.find(p=>p.id==='P2').counterexample.actions.map(a=>a.control);
step({family:'browser.v1',op:'click',payload:{id:controls[0]}});save('review');
const checkpoint=world.snapshot();
step({family:'browser.v1',op:'click',payload:{id:controls[1]}});save('merged');
world.restore(checkpoint);
step({family:'browser.v1',op:'navigate',payload:{url:'http://github.com/northstar/atlas/pulls'}});save('unmerged');
console.log('Rendered checkpoint, review, merged, and unmerged frames.');
