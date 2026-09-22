const fs=require('node:fs'), assert=require('node:assert/strict'), crypto=require('node:crypto');
const path=require('node:path'), root=path.resolve(__dirname,'../..');
const output=process.argv[2] || path.join(root,'target/binding-checks/desktop.json');
fs.mkdirSync(path.dirname(output),{recursive:true});
const {World}=require(path.join(root,'pkg/node/computerworld.js'));
const def=JSON.parse(fs.readFileSync(path.join(root,'worlds/company-2026/world.json'),'utf8'));
def.metadata.desktop_themes={'alice-mac':'virtual-windows-11'};
const world=new World(def,42), env=world.environment({actor:'alice',machines:['alice-mac'],actions:['terminal.v1','filesystem.v1','browser.v1','pointer.v1','keyboard.v1','application.v1','http.v1'],observations:['terminal.v1','semantic.v1']});
const step=(family,op,payload)=>assert(env.step([{family,op,machine:'alice-mac',payload}]).outcomes[0].success);
const click=(id)=>{const node=env.scene(960,640).nodes.find(n=>n.interaction===id);assert(node,id);step('pointer.v1','click',{x:node.bounds.x+Math.floor(node.bounds.width/2),y:node.bounds.y+Math.floor(node.bounds.height/2),width:960,height:640});};
click('shell:launch:terminal');step('keyboard.v1','type',{text:'echo desktop-pixel-parity'});step('keyboard.v1','key',{key:'Enter'});
const maximize = env.scene(960,640).nodes.find(n=>/^window:\d+:maximize$/.test(n.interaction ?? ''));
assert(maximize,'expected a window maximize control');click(maximize.interaction);
const frame=env.render(960,640);const pixelHash=crypto.createHash('sha256').update(frame.rgba).digest('hex');frame.free();
const variants=[];
for(const theme of ['virtual-macos-golden-gate','virtual-windows-11','virtual-ubuntu-24','virtual-ios-18','virtual-android-12']) {
  const definition={...def,metadata:{...def.metadata,desktop_themes:{'alice-mac':theme}}};
  const runtime=new World(definition,42), actor=runtime.environment({actor:'alice',machines:['alice-mac'],actions:['application.v1'],observations:['semantic.v1']});
  const width=theme==='virtual-ios-18'||theme==='virtual-android-12'?390:960, height=theme==='virtual-ios-18'||theme==='virtual-android-12'?844:640;
  const image=actor.render(width,height), hash=crypto.createHash('sha256').update(image.rgba).digest('hex');image.free();
  variants.push({theme,definition,session:actor.id,snapshot:runtime.exportSnapshot(),stateHash:runtime.stateHash(),pixelHash:hash,width,height});
}
fs.writeFileSync(output,JSON.stringify({definition:def,session:env.id,snapshot:world.exportSnapshot(),stateHash:world.stateHash(),pixelHash,variants}));
console.log(JSON.stringify({desktopPixelHash:pixelHash,stateHash:world.stateHash(),variants:variants.map(({theme,pixelHash})=>({theme,pixelHash}))}));
