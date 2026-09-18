/* Real wasm execution; no browser, subprocess simulator, or network needed. */
const assert = require('node:assert/strict');
const fs = require('node:fs');
const path = require('node:path');
const { World, SceneRenderer } = require('../pkg/node/computerworld.js');
const definition = JSON.parse(fs.readFileSync(path.join(__dirname,'../worlds/company-2026/world.json'),'utf8'));
const config = {actor:'alice',machines:['alice-mac'],actions:['terminal.v1','browser.v1'],observations:['terminal.v1','semantic.v1']};
const actions = [{family:'terminal.v1',op:'execute',machine:'alice-mac',payload:{command:'echo binding-parity'}}];
const world = new World(definition, 42);
const env = world.environment(config);
const before = world.snapshot();
const initialHash = world.stateHash();
const result = env.step(actions);
assert(result.outcomes[0].success, JSON.stringify(result));
assert(JSON.stringify(result).includes('binding-parity'));
const afterHash = world.stateHash();
const portable = world.exportSnapshot();
world.restore(before);
assert.deepEqual(env.step(actions), result);
assert.equal(world.stateHash(), afterHash);
const fork = world.fork(world.snapshot());
assert.equal(fork.stateHash(), afterHash);
assert.deepEqual(fork.session(env.id).observe(), env.observe());
const gui = fork.environment({actor:'alice',machines:['alice-mac'],actions:['browser.v1','pointer.v1'],observations:['semantic.v1']});
assert(gui.step([{family:'browser.v1',op:'navigate',machine:'alice-mac',payload:{url:'http://intranet.internal/'}}]).outcomes[0].success);
const link = gui.scene(960,560).nodes.find(n => n.semantic?.role === 'link' && n.interaction);
assert(link, 'expected an interactive intranet link');
const click = gui.step([{family:'pointer.v1',op:'click',machine:'alice-mac',payload:{x:link.bounds.x+2,y:link.bounds.y+2,width:960,height:560}}]);
assert(click.outcomes[0].success, JSON.stringify(click));
const imported = new World(definition, 0);
imported.importSnapshot(portable);
assert.equal(imported.stateHash(), afterHash);
assert.equal(env.inspect, undefined);
assert.equal(env.snapshot, undefined);
assert.equal(env.step([{family:'private.v1',op:'inspect',machine:'alice-mac'}]).outcomes[0].success, false);
const frame = env.render(320,240);
assert.equal(frame.rgba.length,320*240*4);
assert(frame.rgba instanceof Uint8Array);
assert(world.trajectory().length > 0);
assert.throws(()=>env.step('invalid'));
const renderer = new SceneRenderer();
const scene = {width:32,height:32,revision:0,nodes:[{id:1,bounds:{x:2,y:2,width:10,height:10},primitive:{kind:'box',fill:[20,30,40,255],border:null,border_width:0}}]};
const firstFrame = renderer.render(scene);
const changed = {...scene.nodes[0],primitive:{...scene.nodes[0].primitive,fill:[100,120,140,255]}};
const patched = renderer.patch({base_revision:0,revision:1,operations:[{op:'upsert',value:changed}]});
const fresh = new SceneRenderer().render({...scene,revision:1,nodes:[changed]});
assert.deepEqual(patched.rgba,fresh.rgba);
assert.notDeepEqual(firstFrame.rgba,patched.rgba);
assert.throws(()=>renderer.patch({base_revision:0,revision:1,operations:[]}));
const fixture = {width:160,height:90,revision:0,background:[10,20,30,255],nodes:[
  {id:1,bounds:{x:3,y:4,width:100,height:70},primitive:{kind:'box',fill:[100,200,30,128],border:null,border_width:0}},
  {id:2,bounds:{x:8,y:8,width:130,height:65},primitive:{kind:'text',text:'Hello world\nDeterministic λ',size:16,color:[255,255,255,255]}}
]};
const fixtureHash = require('node:crypto').createHash('sha256').update(renderer.render(fixture).rgba).digest('hex');
assert.equal(fixtureHash,'01455c4eaa6c1eca6900b545f69bba35ad9428fb66275a41df86268ae3595ec6');
const metadata = JSON.parse('{"__proto__":{"polluted":true}}');
const dataWorld = new World({...definition,metadata},0);
assert.equal(dataWorld.definition().metadata.__proto__.polluted,true);
assert.equal({}.polluted,undefined);
assert.throws(()=>new World(definition,-1));
assert.throws(()=>new World(definition,1.5));
// Exact serialized object roundtrip, including arbitrary metadata and actions.
const exactMetadata = {unsigned:18446744073709551615n,signed:-9223372036854775808n,fraction:0.25,array:[1,2n]};
const exactWorld = new World({...definition,metadata:exactMetadata},42);
const exactDefinition = exactWorld.definition();
assert.equal(exactDefinition.metadata.unsigned,exactMetadata.unsigned);
assert.equal(exactDefinition.metadata.signed,exactMetadata.signed);
assert.equal(new World(exactDefinition,42).stateHash(),exactWorld.stateHash());
const exactActor = exactWorld.environment(config);
assert(exactActor.step([{...actions[0],payload:{command:'echo exact',exact:exactMetadata.unsigned}}]).outcomes[0].success);
const exactPortable = exactWorld.exportSnapshot();
assert(exactPortable.includes('18446744073709551615'));
const exactImported = new World(exactDefinition,0);
exactImported.importSnapshot(exactPortable);
assert.equal(exactImported.stateHash(),exactWorld.stateHash());
for (const invalid of [Number.MAX_SAFE_INTEGER+1,-Number.MAX_SAFE_INTEGER-1,NaN,Infinity,-Infinity,undefined,()=>0,Symbol('x'),new Date(),new Map(),18446744073709551616n,-9223372036854775809n]) {
  assert.throws(()=>new World({...definition,metadata:{invalid}},0));
}
const cyclic = {}; cyclic.self = cyclic;
assert.throws(()=>new World({...definition,metadata:cyclic},0));
let tooDeep = null;
for(let i=0;i<130;i++) tooDeep = {value:tooDeep};
assert.throws(()=>new World({...definition,metadata:tooDeep},0));
const maxSeed = new World(definition, '18446744073709551615');
assert.equal(maxSeed.stateHash(),new World(definition,18446744073709551615n).stateHash());
world.reset(42);
assert.equal(world.stateHash(),initialHash);
assert.deepEqual(env.observe(),world.session(env.id).observe());
// Device lifecycle uses the same persistent Rust world and preserves other sessions.
const dynamic = new World(definition,42);
const computer = {...definition.computers.find(c=>c.id==='carol-ubuntu'),id:'test-phone',node:'test-phone',address:'10.0.0.77'};
const node = {id:'test-phone',address:'10.0.0.77',zone:'local'};
const links = [{from:'test-phone',to:'alice-mac',bidirectional:true,latency_us:0,loss_per_million:0}];
dynamic.addComputer(computer,node,links);
assert(dynamic.definition().computers.some(c=>c.id==='test-phone'));
const phone = dynamic.environment({...config,actor:computer.user,machines:['test-phone']});
assert(phone.step([{family:'terminal.v1',op:'execute',machine:'test-phone',payload:{command:'echo mobile'}}]).outcomes[0].success);
const topologyCheckpoint = dynamic.exportSnapshot();
const topologyHash = dynamic.stateHash();
const topologySnapshot = dynamic.snapshot();
dynamic.removeComputer('test-phone');
assert(!dynamic.definition().computers.some(c=>c.id==='test-phone'));
dynamic.restore(topologySnapshot);
assert.equal(dynamic.stateHash(),topologyHash);
assert(dynamic.session(phone.id).observe());
dynamic.reset(42);
assert(!dynamic.definition().computers.some(c=>c.id==='test-phone'));
assert.equal(phone.addComputer,undefined);
assert.equal(phone.removeComputer,undefined);
// Complex scripts: Hebrew to Khmer are in the module; CJK, emoji and four larger
// scripts are the on-demand font pack. Before the pack is installed those glyphs are boxes; once
// it is, the frame must equal the native renderer's pinned hash for the same scene
// (crates/render/src/script_tests.rs, multi_script_scene_is_pinned).
const { installFont, fontPackStatus } = require('../pkg/node/computerworld.js');
const scriptsScene = JSON.parse(fs.readFileSync(path.join(__dirname,'../crates/render/tests/scripts-scene.json'),'utf8'));
const sha256 = bytes => require('node:crypto').createHash('sha256').update(bytes).digest('hex');
const nativeScriptsHash = 'a5c6132a7f8f967f99cb79df655841f8e57bf9393f370894fe509840c91e1f1f';
const scriptRenderer = new SceneRenderer();
const beforePack = sha256(scriptRenderer.render(scriptsScene).rgba);
assert.notEqual(beforePack, nativeScriptsHash, 'CJK/emoji drew without the pack');
const packBefore = fontPackStatus();
// Exactly the files the scene needs: SC/KR and their bold, the Traditional Chinese,
// Japanese and (bold) Korean locale forms, both emoji faces and the pack scripts.
assert.deepEqual([...packBefore.missing].sort(), ['noto-color-emoji.ttf','noto-emoji.ttf','noto-ethiopic.ttf','noto-gujarati.ttf','noto-myanmar.ttf','noto-sans-jp.ttf','noto-sans-kr-bold.ttf','noto-sans-kr-han-bold.ttf','noto-sans-kr.ttf','noto-sans-sc-bold.ttf','noto-sans-sc.ttf','noto-sans-tc.ttf','noto-sinhala.ttf']);
assert.throws(()=>installFont(new Uint8Array([1,2,3])));
for (const file of packBefore.files) {
  const bytes = fs.readFileSync(path.join(__dirname,'../pkg/node',file.path));
  assert.equal(sha256(bytes), file.sha256);
  assert.equal(installFont(bytes), file.file);
}
assert.deepEqual(fontPackStatus().missing, []);
// The retained renderer notices the new pack and repaints its cached text.
assert.equal(sha256(scriptRenderer.render(scriptsScene).rgba), nativeScriptsHash);
assert.equal(sha256(new SceneRenderer().render(scriptsScene).rgba), nativeScriptsHash);
if (process.argv[2]) fs.writeFileSync(process.argv[2],JSON.stringify({hash:afterHash,snapshot:portable,topologyHash,topologyCheckpoint}));
console.log(JSON.stringify({runtime:'wasm',hash:afterHash,rgba_bytes:frame.rgba.length}));
