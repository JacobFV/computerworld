// Run after scripts/build-wasm.sh. Matches native rounded_ui_golden_and_incremental.
const { SceneRenderer } = require('../../../pkg/node/computerworld.js');
const { createHash } = require('node:crypto');
const assert = require('node:assert/strict');
const scene = {
  width: 220, height: 100, revision: 0, background: [24, 31, 46, 255],
  nodes: [
    { id: 1, bounds: {x:8,y:8,width:204,height:84}, primitive: {
      kind:'rounded_box', fill:[246,248,255,230],border:[255,255,255,255],border_width:2,radius:17
    } },
    { id: 2, bounds: {x:20,y:17,width:182,height:62}, primitive: {
      kind:'ui_text',text:'Window settings\nWiFi · λ · 09:41',size:17,color:[24,31,46,255]
    } }
  ]
};
const renderer = new SceneRenderer();
const frame = renderer.render(scene);
const hash = createHash('sha256').update(frame.rgba).digest('hex');
assert.equal(hash, 'da928cab834c47449b5d838ccd2f3338e2e318dc63237f421dd7ac3d79324cf0');
const changed = {...scene.nodes[1], primitive:{kind:'ui_text',text:'Connected\nWiFi · λ · 09:42',size:17,color:[0,0,0,255]}};
const patched = renderer.patch({base_revision:0, revision:1, operations:[{op:'upsert',value:changed}]});
const freshRenderer = new SceneRenderer();
const fresh = freshRenderer.render({...scene,revision:1,nodes:[scene.nodes[0],changed]});
assert.deepEqual(patched.rgba, fresh.rgba);
frame.free(); patched.free(); fresh.free(); renderer.free(); freshRenderer.free();
console.log(JSON.stringify({test:'gui-primitives-native-wasm-parity',sha256:hash,incremental:true}));
