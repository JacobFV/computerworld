// Browser demo blueprint: OS shells are rendered by Rust, never HTML overlays.
import {readFile, writeFile} from 'node:fs/promises';
const definition=JSON.parse(await readFile(new URL('../../worlds/company-2026/world.json',import.meta.url),'utf8'));
const profiles=[
 {id:'virtual-macos-golden-gate',name:'macOS · Golden Gate',family:'macos',home:'/Users/{user}',case_sensitive:false,shell:'posix'},
 {id:'virtual-windows-11',name:'Windows 11',family:'windows',home:'C:/Users/{user}',case_sensitive:false,shell:'powershell'},
 {id:'virtual-ubuntu-24',name:'Ubuntu 24',family:'linux',home:'/home/{user}',case_sensitive:true,shell:'posix'},
 {id:'virtual-ios-18',name:'iOS 18',family:'ios',home:'/var/mobile',case_sensitive:true,shell:'posix'},
 {id:'virtual-android-12',name:'Android 12',family:'android',home:'/data/user/{user}',case_sensitive:true,shell:'posix'}
];
definition.profiles.push(...profiles);
for(const [id,profile] of [['alice-mac',profiles[0].id],['bob-windows',profiles[1].id],['carol-ubuntu',profiles[2].id]])definition.computers.find(c=>c.id===id).profile=profile;
for(const [id,profile,address,user] of [['alice-phone',profiles[3].id,'10.0.0.15','alice'],['bob-android',profiles[4].id,'10.0.0.16','bob']]){
 definition.computers.push({id,profile,address,user,initial_files:{'notes.txt':`Mobile notes for ${user}\nRead http://mail.internal/ then update http://docs.internal/\n`},installed_apps:['browser','terminal','files','editor','desktop'],packages:['coreutils','curl','git']});
 definition.network.nodes.push({id,address,zone:'local'});
 definition.network.links.push({from:'app-server',to:id,bidirectional:true,latency_us:10,loss_per_million:0});
}
// Mail, Calendar, Messages, Documents and Contacts are native applications: `url` is the
// service each one talks to over HTTP, not a page the browser is sent to.
const nativeApps=[
 {id:'mail',label:'Mail',kind:'native',url:'http://mail.internal/',icon:'mail'},
 {id:'calendar',label:'Calendar',kind:'native',url:'http://calendar.internal/',icon:'calendar'},
 {id:'chat',label:'Messages',kind:'native',url:'http://chat.internal/',icon:'chat'},
 {id:'docs',label:'Documents',kind:'native',url:'http://docs.internal/',icon:'docs'},
 {id:'contacts',label:'Contacts',kind:'native',url:'http://mail.internal/|http://chat.internal/',icon:'contacts'},
 {id:'notes',label:'Notes',kind:'native',url:'',icon:'notes'},
 {id:'settings',label:'Settings',kind:'native',url:'',icon:'settings'},
 {id:'calculator',label:'Calculator',kind:'native',url:'',icon:'calculator'},
 {id:'clock',label:'Clock',kind:'native',url:'',icon:'clock'},
 // These four read real services on the synthetic web, or the machine's own files.
 {id:'photos',label:'Photos',kind:'native',url:'',icon:'photos'},
 {id:'music',label:'Music',kind:'native',url:'http://spotify.com/',icon:'music'},
 {id:'maps',label:'Maps',kind:'native',url:'http://maps.google.com/',icon:'maps'},
 {id:'weather',label:'Weather',kind:'native',url:'http://weather.com/',icon:'weather'},
 // Visual Studio Code edits the machine's own files and runs them in its own shell.
 {id:'code',label:'Visual Studio Code',kind:'native',url:'',icon:'code'}
];
// Phones have no desktop editor: Code is installed on the three desktop profiles only.
const desktopOnly=new Set(['code']);
const desktopProfiles=new Set(profiles.slice(0,3).map(p=>p.id));
definition.metadata={...definition.metadata,desktop_apps:nativeApps,device_presentations:{'alice-mac':'desktop','bob-windows':'desktop','carol-ubuntu':'laptop','app-server':'server','git-server':'server','alice-phone':'phone','bob-android':'phone'}};
for(const computer of definition.computers){
 if(!computer.profile.startsWith('virtual-'))continue;
 for(const app of nativeApps){
  if(desktopOnly.has(app.id)&&!desktopProfiles.has(computer.profile))continue;
  if(!computer.installed_apps.includes(app.id))computer.installed_apps.push(app.id);
 }
}
// Notes keeps its files on the machine, so every graphical device starts with the folder.
for(const computer of definition.computers){
 if(!computer.profile.startsWith('virtual-'))continue;
 computer.initial_files={...computer.initial_files,'Notes/welcome.txt':`Notes for ${computer.user}\nThese are real files under the Notes folder.\n`};
}
// A small project for Visual Studio Code to open: it opens ~/project when there is one.
for(const computer of definition.computers){
 if(!desktopProfiles.has(computer.profile))continue;
 computer.initial_files={...computer.initial_files,
  'project/README.md':'# Project\n\nA small workspace. Run `main.py` with F5, or `bash run.sh` in the terminal.\n',
  'project/main.py':'import sys\n\n\ndef greet(name: str) -> str:\n    return f"Hello, {name}!"\n\n\nif __name__ == "__main__":\n    print(greet(sys.argv[1] if len(sys.argv) > 1 else "world"))\n',
  'project/src/app.js':"// TODO: wire this to the page\nfunction add(a, b) {\n  return a + b;\n}\n\nconsole.log(`2 + 3 = ${add(2, 3)}`);\n",
  'project/run.sh':'#!/bin/bash\necho "Files in $(pwd):"\nls\n',
  'project/package.json':'{\n  "name": "project",\n  "version": "1.0.0",\n  "main": "src/app.js"\n}\n'};
}
await writeFile(new URL('./world-definition.js',import.meta.url),`// Generated by node examples/browser/build.mjs\nexport default ${JSON.stringify(definition,null,2)};\n`);
console.log(`Built embedded OS desktop world (7 devices, 5 graphical OS profiles, ${definition.services.length} services, ${nativeApps.length} native applications).`);
