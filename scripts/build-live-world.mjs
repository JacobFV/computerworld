/** The world the live machines on the project site run in.
 *
 * The reference company (worlds/company-2026/world.json) is five computers on three OS
 * profiles. This adds the five graphical OS profiles, the two phones, the native
 * applications each platform ships and the documents and media they open, and writes the
 * result to site/world-definition.js, which site/live.js downloads as text and each of
 * its workers parses for itself. OS shells are rendered by Rust, never HTML overlays.
 *
 *   node scripts/build-live-world.mjs      # or scripts/build-content.sh, which runs it
 */
import {readFile, writeFile} from 'node:fs/promises';
const definition=JSON.parse(await readFile(new URL('../worlds/company-2026/world.json',import.meta.url),'utf8'));
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
 {id:'messages',label:'Messages',kind:'native',url:'http://messages.internal/',icon:'messages'},
 {id:'docs',label:'Documents',kind:'native',url:'http://docs.internal/',icon:'docs'},
 {id:'contacts',label:'Contacts',kind:'native',url:'http://mail.internal/|http://messages.internal/',icon:'contacts'},
 {id:'notes',label:'Notes',kind:'native',url:'',icon:'notes'},
 {id:'settings',label:'Settings',kind:'native',url:'',icon:'settings'},
 {id:'calculator',label:'Calculator',kind:'native',url:'',icon:'calculator'},
 {id:'clock',label:'Clock',kind:'native',url:'',icon:'clock'},
 // These four read real services on the synthetic web, or the machine's own files.
 {id:'photos',label:'Photos',kind:'native',url:'',icon:'photos'},
 // Apple Music, Media Player and Rhythmbox read spotify.com's catalogue; Android's music
 // player is YouTube Music, so it reads music.youtube.com.
 {id:'music',label:'Music',kind:'native',url:'http://spotify.com/',urls:{android:'http://music.youtube.com/'},icon:'music'},
 {id:'maps',label:'Maps',kind:'native',url:'http://maps.google.com/',icon:'maps'},
 {id:'weather',label:'Weather',kind:'native',url:'http://weather.com/',icon:'weather'},
 // Visual Studio Code edits the machine's own files and runs them in its own shell.
 {id:'code',label:'Visual Studio Code',kind:'native',url:'',icon:'code'},
 // FreeCAD models parts parametrically and reads and writes CAD files on the machine.
 {id:'freecad',label:'FreeCAD',kind:'native',url:'',icon:'freecad'},
 // KiCad designs circuits and boards in the machine's ~/Documents/KiCad folder.
 {id:'kicad',label:'KiCad',kind:'native',url:'',icon:'kicad'}
];
// Phones have no desktop editor, CAD or EDA: Code, FreeCAD and KiCad are on the three
// desktops only.
const desktopOnly=new Set(['code','freecad','kicad']);
const desktopProfiles=new Set(profiles.slice(0,3).map(p=>p.id));
// The reference world states what each of its computers is (worlds/company-2026/world.json
// metadata.device_presentations); this adds its two phones to that, never restates it.
definition.metadata={...definition.metadata,desktop_apps:nativeApps,device_presentations:{...definition.metadata.device_presentations,'alice-phone':'phone','bob-android':'phone'}};
for(const computer of definition.computers){
 if(!computer.profile.startsWith('virtual-'))continue;
 for(const app of nativeApps){
  if(desktopOnly.has(app.id)&&!desktopProfiles.has(computer.profile))continue;
  if(!computer.installed_apps.includes(app.id))computer.installed_apps.push(app.id);
 }
}
// Image editors are each platform's own, so each machine gets its platform's: Paint on
// Windows, Preview and Pixelmator Pro on macOS, GIMP and Pinta on Ubuntu, Sketchbook on
// Android. iOS and Android edit photos inside Photos itself.
const imageEditors={'virtual-windows-11':['paint'],'virtual-macos-golden-gate':['preview','pixelmator'],'virtual-ubuntu-24':['gimp','pinta'],'virtual-android-12':['sketchbook'],'virtual-ios-18':[]};
const editorApps=[
 {id:'paint',label:'Paint',kind:'native',url:'',icon:'paint'},
 {id:'preview',label:'Preview',kind:'native',url:'',icon:'preview'},
 {id:'pixelmator',label:'Pixelmator Pro',kind:'native',url:'',icon:'pixelmator'},
 {id:'gimp',label:'GNU Image Manipulation Program',kind:'native',url:'',icon:'gimp'},
 {id:'pinta',label:'Pinta',kind:'native',url:'',icon:'pinta'},
 {id:'sketchbook',label:'Sketchbook',kind:'native',url:'',icon:'sketchbook'}
];
// Spreadsheets and SQLite clients are each platform's own too: Excel on Windows (and as
// a second spreadsheet on the Mac), Numbers on macOS and iOS, LibreOffice Calc on Ubuntu
// and Sheets on Android; DB Browser for SQLite on Windows and Ubuntu, TablePlus on macOS.
// The `spreadsheet` kind is whichever of those the platform ships.
const officeInstalls={'virtual-windows-11':['spreadsheet','database'],'virtual-macos-golden-gate':['spreadsheet','excel','database'],'virtual-ubuntu-24':['spreadsheet','database'],'virtual-android-12':['spreadsheet'],'virtual-ios-18':['spreadsheet']};
const officeApps=[
 {id:'spreadsheet',label:'Spreadsheet',kind:'native',url:'',icon:'spreadsheet'},
 {id:'excel',label:'Microsoft Excel',kind:'native',url:'',icon:'excel'},
 {id:'database',label:'SQLite Database',kind:'native',url:'',icon:'database'}
];
definition.metadata.desktop_apps=[...nativeApps,...editorApps,...officeApps];
for(const computer of definition.computers){for(const id of imageEditors[computer.profile]||[])if(!computer.installed_apps.includes(id))computer.installed_apps.push(id);}
for(const computer of definition.computers){for(const id of officeInstalls[computer.profile]||[])if(!computer.installed_apps.includes(id))computer.installed_apps.push(id);}
// Documents for them to open: a workbook, a CSV export and a SQLite database, written by
// the engines themselves (crates/sheet/tests/samples.rs, crates/sql/tests/samples.rs)
// and checked in under worlds/company-2026/files. Binary files travel base64-encoded.
const sample=name=>readFile(new URL(`../worlds/company-2026/files/${name}`,import.meta.url));
const budget=(await sample('Budget.xlsx')).toString('base64');
const inventory=(await sample('Inventory.db')).toString('base64');
const sales=(await sample('Sales.csv')).toString('utf8');
for(const computer of definition.computers){
 if(!computer.profile.startsWith('virtual-'))continue;
 computer.initial_files={...computer.initial_files,'Documents/Sales.csv':sales};
 computer.initial_binary_files={...computer.initial_binary_files,'Documents/Budget.xlsx':budget,...(desktopProfiles.has(computer.profile)?{'Documents/Inventory.db':inventory}:{})};
}
// Video editors are each platform's own: Clipchamp on Windows, iMovie on the Mac and the
// iPhone, Kdenlive on Ubuntu and the Android editor. Each machine's movies folder holds
// the sample clips and sounds the engine generates (crates/video/tests/samples.rs):
// Animated PNG movies and WAV audio, checked in under worlds/company-2026/files.
const videoApps=[
 {id:'clipchamp',label:'Clipchamp',kind:'native',url:'',icon:'clipchamp'},
 {id:'imovie',label:'iMovie',kind:'native',url:'',icon:'imovie'},
 {id:'kdenlive',label:'Kdenlive',kind:'native',url:'',icon:'kdenlive'},
 {id:'videoeditor',label:'Video Editor',kind:'native',url:'',icon:'videoeditor'}
];
const videoInstalls={'virtual-windows-11':'clipchamp','virtual-macos-golden-gate':'imovie','virtual-ubuntu-24':'kdenlive','virtual-ios-18':'imovie','virtual-android-12':'videoeditor'};
const movieFolder={'virtual-windows-11':'Videos','virtual-ubuntu-24':'Videos'};
const sampleMedia=['Countdown.apng','Color Bars.apng','Sunset.apng','Countdown Beeps.wav','Music Bed.wav'];
const media=Object.fromEntries(await Promise.all(sampleMedia.map(async name=>[name,(await sample(name)).toString('base64')])));
definition.metadata.desktop_apps=[...definition.metadata.desktop_apps,...videoApps];
for(const computer of definition.computers){
 const id=videoInstalls[computer.profile];
 if(!id)continue;
 if(!computer.installed_apps.includes(id))computer.installed_apps.push(id);
 const folder=movieFolder[computer.profile]||'Movies';
 computer.initial_binary_files={...computer.initial_binary_files,...Object.fromEntries(sampleMedia.map(name=>[`${folder}/${name}`,media[name]]))};
}
// Notes keeps its files on the machine, so every graphical device starts with the folder.
for(const computer of definition.computers){
 if(!computer.profile.startsWith('virtual-'))continue;
 computer.initial_files={...computer.initial_files,'Notes/welcome.txt':`Notes for ${computer.user}\nThese are real files under the Notes folder.\n`};
}
// ~/project (a Python package), ~/code (a Node service), ~/Notes, ~/Documents/{Parts,KiCad,datasheets}
// and ~/bin come from the file trees under worlds/company-2026/home, seeded into world.json by
// the copy: blocks in worlds/company-2026/world.yml; nothing is added here.
await writeFile(new URL('../site/world-definition.js',import.meta.url),`// Generated by node scripts/build-live-world.mjs\nexport default ${JSON.stringify(definition,null,2)};\n`);
console.log(`Built the live site world (7 devices, 5 graphical OS profiles, ${definition.services.length} services, ${nativeApps.length+editorApps.length+officeApps.length+videoApps.length} native applications).`);
