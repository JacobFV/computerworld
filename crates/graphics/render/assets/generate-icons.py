"""Rebuild original vector icon artwork (requires cairosvg); no runtime dependency.
Original artwork released under repository MIT license. Yaru PNGs are separate.
"""
from pathlib import Path
import cairosvg
import sys
OUT=Path(__file__).parent/'icons'
colors={'files':('#75ceff','#0677d8'),'browser':('#4abdfc','#0560d8'),'terminal':('#444952','#17191e'),'docs':('#fdfefe','#d6e3f0'),'mail':('#40c4ff','#086ee9'),'calendar':('#fff','#e7edf4'),'chat':('#6ae268','#16ae39'),'settings':('#aeb4c0','#697383'),'camera':('#e3e6eb','#abb3bc'),'photos':('#fff','#eff3f7'),'phone':('#6cdf65','#0fa834'),'store':('#47baff','#0877ee'),'launcher':('#dfdef2','#7b8196'),'trash':('#e3edf4','#9eb0bf'),'notes':('#ffdc6e','#f5a521'),'contacts':('#e6bd93','#a9713d'),'clock':('#5b6472','#232831'),'calculator':('#9aa4b2','#4e5765'),'music':('#ff8a9b','#e2374f'),'maps':('#84dfa6','#1c9a55'),'weather':('#8fd0ff','#2d7ccd'),'code':('#ffffff','#e6ecf3')}
shapes={
'files':'<path d="M18 33h29l9 10h54v52a9 9 0 0 1-9 9H27a9 9 0 0 1-9-9Z" fill="#e8a333"/><path d="M18 46h92v49a9 9 0 0 1-9 9H27a9 9 0 0 1-9-9Z" fill="url(#folder)"/><path d="M28 57h71" stroke="#fff0be" stroke-width="3"/>',
'browser':'<circle cx="64" cy="64" r="44" fill="#e9f8ff"/><circle cx="64" cy="64" r="39" fill="#118adc"/><g stroke="#fff" stroke-width="2"><path d="M64 28v8m0 56v8M28 64h8m56 0h8M39 39l6 6m38 38 6 6M39 89l6-6m38-38 6-6"/></g><path d="m82 43-11 28-27 14 12-28Z" fill="#fff"/><path d="m82 43-11 28-15-14Z" fill="#ff555a"/>',
'terminal':'<rect x="17" y="23" width="94" height="82" rx="8" fill="#20242a" stroke="#747d88" stroke-width="2"/><path d="m32 45 18 16-18 16m27 2h26" stroke="#fff" stroke-width="6" stroke-linejoin="round" fill="none"/>',
'docs':'<path d="M35 17h45l20 20v74H35Z" fill="#fff" stroke="#b2bfd0" stroke-width="2"/><path d="M80 17v20h20" fill="#dbe8f5"/><path d="M46 50h41M46 61h41M46 72h41M46 83h27" stroke="#5893c4" stroke-width="4"/><path d="m72 95 26-39 9 6-26 39-13 7Z" fill="#ffbb48"/><path d="m68 108 4-13 9 7Z" fill="#465365"/>',
'mail':'<rect x="19" y="33" width="90" height="65" rx="8" fill="#f7fbff"/><path d="m23 91 30-30m53 30L76 61" stroke="#a8c9e9" stroke-width="3"/><path d="m22 38 42 33 42-33" fill="none" stroke="#2c8eda" stroke-width="4"/>',
'calendar':'<rect x="19" y="22" width="90" height="86" rx="8" fill="#fff"/><path d="M27 22h74a8 8 0 0 1 8 8v17H19V30a8 8 0 0 1 8-8" fill="#fa5454"/><path d="M40 16v15m48-15v15" stroke="#d0d7de" stroke-width="5" stroke-linecap="round"/><text x="64" y="91" text-anchor="middle" fill="#253044" font-family="DejaVu Sans" font-size="43">17</text>',
'chat':'<path d="M108 59c0 23-19 39-45 39-5 0-10-1-14-2L29 107l4-22C12 63 21 30 53 23c28-6 55 9 55 36Z" fill="#fff"/>',
'settings':'<g fill="#dce1e9" stroke="#515c6d" stroke-width="3"><path d="m55 16 18 0 4 13 11 6 14-3 9 16-10 10v12l10 10-9 16-14-3-11 6-4 13H55l-4-13-11-6-14 3-9-16 10-10V58L17 48l9-16 14 3 11-6Z"/><circle cx="64" cy="64" r="27" fill="#788390"/><circle cx="64" cy="64" r="17" fill="#cbd4de"/></g>',
'camera':'<path d="M25 36h20l7-11h25l7 11h19a9 9 0 0 1 9 9v51a9 9 0 0 1-9 9H25a9 9 0 0 1-9-9V45a9 9 0 0 1 9-9Z" fill="#48505a"/><circle cx="64" cy="69" r="28" fill="#8e99a9"/><circle cx="64" cy="69" r="22" fill="#182333"/><circle cx="64" cy="69" r="14" fill="#29547a"/><circle cx="59" cy="64" r="6" fill="#5ca0d4"/><circle cx="99" cy="47" r="4" fill="#f4ce68"/>',
'phone':'<path d="M35 23c-8 5-12 19-8 32 8 27 28 46 48 50 13 3 26-3 28-10l-22-20-12 11C55 77 46 67 42 54l12-10Z" fill="#fff"/>',
'store':'<path d="m41 89 30-53m-11 0 30 53M32 74h64" stroke="#fff" stroke-width="10" stroke-linecap="round"/>',
'launcher':''.join(f'<rect x="{25+i%3*29}" y="{25+i//3*29}" width="22" height="22" rx="6" fill="{c}"/>' for i,c in enumerate(['#41b5f2','#50c768','#ff9863','#9475ef','#e66992','#f5c646','#62b0da','#799ad5','#53c4b7'])),
'trash':'<path d="M32 36h64l-5 72H37Z" fill="#d7e1e9" stroke="#8499aa" stroke-width="3"/><path d="M27 30h74M48 30V20h32v10M49 46v48m15-48v48m15-48v48" fill="none" stroke="#7d92a5" stroke-width="5"/>',
'photos':''.join(f'<ellipse cx="64" cy="40" rx="15" ry="25" fill="{c}" opacity=".85" transform="rotate({i*45} 64 64)"/>' for i,c in enumerate(['#ffb32c','#ff692d','#f14e79','#bb64bb','#7279d5','#419ede','#51bb9a','#b8d94f'])),
'notes':'<rect x="26" y="17" width="76" height="94" rx="9" fill="#fffdf1" stroke="#dcc687" stroke-width="2"/><path d="M26 26a9 9 0 0 1 9-9h58a9 9 0 0 1 9 9v13H26Z" fill="#f2a52b"/><path d="M44 56h40M44 71h40M44 86h25" stroke="#cdb578" stroke-width="5" stroke-linecap="round"/><path d="M45 11v15m38-15v15" stroke="#cfd6dd" stroke-width="5" stroke-linecap="round"/>',
'contacts':'<rect x="26" y="17" width="76" height="94" rx="9" fill="#fff" stroke="#b5c1cf" stroke-width="2"/><path d="M35 17h9v94h-9a9 9 0 0 1-9-9V26a9 9 0 0 1 9-9Z" fill="#8a6a49"/><circle cx="72" cy="52" r="15" fill="#5e7fa4"/><path d="M50 93a22 22 0 0 1 44 0Z" fill="#5e7fa4"/><path d="M102 36h11M102 56h11M102 76h11" stroke="#e8834a" stroke-width="6" stroke-linecap="round"/>',
'clock':'<circle cx="64" cy="64" r="46" fill="#2c323c"/><circle cx="64" cy="64" r="40" fill="#fbfdff"/><path d="M64 28v7m0 58v7M28 64h7m58 0h7" stroke="#2c323c" stroke-width="5" stroke-linecap="round"/><path d="M64 38v26h20" fill="none" stroke="#2c323c" stroke-width="6" stroke-linecap="round" stroke-linejoin="round"/><circle cx="64" cy="64" r="5" fill="#f0553f"/>',
'calculator':'<rect x="27" y="16" width="74" height="96" rx="10" fill="#eef2f7" stroke="#97a3b2" stroke-width="2"/><rect x="37" y="26" width="54" height="22" rx="5" fill="#2b323d"/><path d="M46 40h28" stroke="#6fe08d" stroke-width="5" stroke-linecap="round"/>'+''.join(f'<rect x="{37+i%3*20}" y="{58+i//3*18}" width="14" height="12" rx="3" fill="{"#f5a13c" if i%3==2 else "#ccd6e2"}"/>' for i in range(9)),
'music':'<path d="M50 89V38m46 39V27" stroke="#fff" stroke-width="8" stroke-linecap="round"/><path d="M46 33l54-13v17L46 50Z" fill="#fff"/><ellipse cx="39" cy="89" rx="15" ry="12" fill="#fff"/><ellipse cx="85" cy="77" rx="15" ry="12" fill="#fff"/><path d="M46 44l54-13v6L46 50Z" fill="#ffd9de"/>',
'maps':'<path d="M18 36 47 25v70L18 106Z" fill="#fff"/><path d="M47 25l34 11v70L47 95Z" fill="#dcefe3"/><path d="M81 36l29-11v70l-29 11Z" fill="#fff"/><path d="M18 36 47 25v70L18 106Zm63 0 29-11v70l-29 11Z" fill="none" stroke="#bcd8c7" stroke-width="2"/><path d="M33 63h58" stroke="#f2c14b" stroke-width="5" stroke-linecap="round"/><path d="M64 30a17 17 0 0 0-17 17c0 13 17 32 17 32s17-19 17-32a17 17 0 0 0-17-17Z" fill="#e8453c"/><circle cx="64" cy="47" r="7" fill="#fff"/>',
'weather':'<circle cx="50" cy="46" r="19" fill="#ffd451"/><g stroke="#ffd451" stroke-width="6" stroke-linecap="round"><path d="M50 15v-8M19 46h-8M28 24l-6-6M72 24l6-6M28 68l-6 6"/></g><path d="M48 99a20 20 0 0 1 1-40 27 27 0 0 1 50 7 17 17 0 0 1-4 33Z" fill="#fff"/><path d="M48 99a20 20 0 0 1 1-40 27 27 0 0 1 50 7 17 17 0 0 1-4 33Z" fill="none" stroke="#dce9f4" stroke-width="2"/>',
'code':'<polygon points="16.2,49.4 24.5,42.2 89.0,92.1 89.0,109.8" fill="#0065a9" stroke="#0065a9" stroke-width="3" stroke-linejoin="round"/><polygon points="16.2,78.6 24.5,85.8 89.0,35.9 89.0,18.2" fill="#007acc" stroke="#007acc" stroke-width="3" stroke-linejoin="round"/><polygon points="84.8,15.1 111.8,28.6 111.8,99.4 84.8,112.9" fill="#1f9cf0" stroke="#1f9cf0" stroke-width="3" stroke-linejoin="round"/><polygon points="84.8,15.1 91.0,18.2 91.0,109.8 84.8,112.9" fill="#000" opacity=".16"/>',
}
def platform_icons():
 for platform in ['macos','windows','ios','android']:
  for name,(a,b) in colors.items():
   defs=f'<defs><linearGradient id="bg" x2="0" y2="1"><stop stop-color="{a}"/><stop offset="1" stop-color="{b}"/></linearGradient><linearGradient id="folder" x2="0" y2="1"><stop stop-color="#ffe88b"/><stop offset="1" stop-color="#f7ba40"/></linearGradient></defs>'
   bg='<rect x="5" y="6" width="118" height="118" rx="27" fill="#000" opacity=".14"/><rect x="5" y="3" width="118" height="118" rx="27" fill="url(#bg)"/><rect x="6" y="4" width="116" height="116" rx="26" fill="none" stroke="#fff" stroke-opacity=".32"/>'
   if platform=='ios': bg='<rect x="5" y="4" width="118" height="118" rx="27" fill="url(#bg)"/>'
   if platform=='android': bg='<circle cx="64" cy="64" r="60" fill="url(#bg)"/>'
   if platform=='windows': bg=''
   art=shapes[name]
   if name=='files' and platform == 'macos':
    art='<path d="M23 17h82v94H23Z" fill="#69baff"/><path d="M64 17h41v94H59l9-52H56Z" fill="#e4f3ff"/><path d="M45 39v8m39-8v8M38 77q25 24 52-1" stroke="#173e66" stroke-width="4" fill="none" stroke-linecap="round"/>'
   if name=='files' and platform=='ios':
    bg='<rect x="5" y="4" width="118" height="118" rx="27" fill="#fff"/>'
    art='<path d="M21 39h32l9 10h45v46H21Z" fill="#0876ed"/><path d="M21 39h32l9 10h45v11H21Z" fill="#3b9af9"/>'
   if name=='browser' and platform=='windows':
    art='<path d="M109 80c-5 24-37 38-62 22C17 84 18 56 35 35c20-25 63-15 73 12 4 12-1 21-13 25-13 5-38-5-42 9-3 13 38 20 56-1Z" fill="#12b7ba"/><path d="M109 80c-27 12-53 1-55-16-2-17 20-29 35-22-17-21-49-14-61 11-13 28 6 55 34 57 23 1 41-12 47-30Z" fill="#1578df"/><path d="M35 35c26-26 59-10 64 11-22-14-48-1-48 19-16 0-26-13-16-30Z" fill="#48dab7"/>'
   if name=='browser' and platform=='android':
    art='<circle cx="64" cy="64" r="47" fill="#f0c742"/><path d="M64 17a47 47 0 0 1 41 24H64L42 79 23 47a47 47 0 0 1 41-30" fill="#e95446"/><path d="M23 47 46 87h43a47 47 0 0 1-25 24 47 47 0 0 1-41-64" fill="#45a666"/><circle cx="64" cy="64" r="23" fill="#fff"/><circle cx="64" cy="64" r="19" fill="#388ed5"/>'
   if name=='code' and platform in ('macos','windows'):
    bg=''  # Visual Studio Code ships its mark bare, with no tile behind it.
   svg=f'<svg xmlns="http://www.w3.org/2000/svg" width="128" height="128" viewBox="0 0 128 128">{defs}{bg}{art}</svg>'
   (OUT/f'{platform}-{name}.svg').write_text(svg)
   cairosvg.svg2png(bytestring=svg.encode(),write_to=str(OUT/f'{platform}-{name}.png'),output_width=128,output_height=128)

 # Visual Studio Code on Ubuntu is its own mark, not a Yaru icon, drawn at Yaru's 256 px.
 svg=f'<svg xmlns="http://www.w3.org/2000/svg" width="128" height="128" viewBox="0 0 128 128">{shapes["code"]}</svg>'
 (OUT/'ubuntu-code.svg').write_text(svg)
 cairosvg.svg2png(bytestring=svg.encode(),write_to=str(OUT/'ubuntu-code.png'),output_width=256,output_height=256)

# Platform-specific image editors: each exists on one platform only, so each is drawn
# once, in that platform's icon idiom. Original artwork (MIT), not vendor logos.
# `generate-icons.py --editors` rebuilds just these.
EDITORS={
 # Windows 11 Paint: a pale palette with four paint wells and a slanted brush.
 ('windows','paint',128):'<path d="M64 14C33 14 12 36 12 62c0 25 19 44 42 44 9 0 12-5 11-11-1-8 4-12 11-12h12c16 0 28-11 28-27 0-24-24-42-52-42Z" fill="#f4f1ea" stroke="#c9c2b3" stroke-width="3"/><circle cx="36" cy="58" r="9" fill="#e5383b"/><circle cx="50" cy="35" r="9" fill="#f7b32b"/><circle cx="76" cy="32" r="9" fill="#2a9d8f"/><circle cx="96" cy="50" r="9" fill="#3a86ff"/><path d="M118 20 80 76" stroke="#8a5a2b" stroke-width="7" stroke-linecap="round"/><path d="M83 71c-8-2-15 3-17 10-2 8-6 12-12 14 13 5 30 1 33-12 1-5-1-10-4-12Z" fill="#1f6fd1"/>',
 # Preview: two photo prints, one tilted, under a loupe.
 ('macos','preview',128):'<rect x="5" y="6" width="118" height="118" rx="27" fill="#000" opacity=".14"/><rect x="5" y="3" width="118" height="118" rx="27" fill="url(#bg)"/><g transform="rotate(-10 58 64)"><rect x="22" y="30" width="62" height="50" rx="3" fill="#fff" stroke="#c7ced8" stroke-width="2"/><rect x="28" y="36" width="50" height="34" fill="#8fd0ff"/><path d="M28 70 44 52l10 10 8-7 16 15Z" fill="#3aa35c"/></g><rect x="44" y="44" width="62" height="50" rx="3" fill="#fff" stroke="#c7ced8" stroke-width="2"/><rect x="50" y="50" width="50" height="34" fill="#ffd08a"/><circle cx="88" cy="60" r="6" fill="#fff4c2"/><path d="M50 84 66 66l10 10 8-7 16 15Z" fill="#e0703a"/><circle cx="60" cy="86" r="17" fill="#e8f4ff" fill-opacity=".55" stroke="#3b4656" stroke-width="5"/><path d="m72 98 14 14" stroke="#3b4656" stroke-width="8" stroke-linecap="round"/>',
 # Pixelmator Pro: a white squircle with a spectrum-gradient swirl of three petals.
 ('macos','pixelmator',128):'<defs><linearGradient id="px" x1="0" y1="0" x2="1" y2="1"><stop stop-color="#ff4f7b"/><stop offset=".35" stop-color="#ffb13d"/><stop offset=".65" stop-color="#38c7ff"/><stop offset="1" stop-color="#7a5cff"/></linearGradient></defs><rect x="5" y="6" width="118" height="118" rx="27" fill="#000" opacity=".14"/><rect x="5" y="3" width="118" height="118" rx="27" fill="#fbfbfd"/><rect x="6" y="4" width="116" height="116" rx="26" fill="none" stroke="#000" stroke-opacity=".08"/><g fill="url(#px)"><path d="M64 26c16 0 26 12 22 28-8-8-20-10-32-6 0-12 4-22 10-22Z"/><path d="M98 78c-8 14-24 18-36 8 11-3 19-12 22-24 10 4 18 10 14 16Z"/><path d="M34 82c-8-14-2-30 14-34-3 11 0 23 8 32-8 6-18 8-22 2Z"/></g><circle cx="64" cy="64" r="9" fill="#fff"/>',
 # GIMP: a grey-brown creature's face with round eyes, holding a brush.
 ('ubuntu','gimp',256):'<rect x="16" y="20" width="224" height="224" rx="48" fill="#000" opacity=".16"/><rect x="16" y="14" width="224" height="224" rx="48" fill="#5c5543"/><rect x="16" y="14" width="224" height="112" rx="48" fill="#6f6752"/><path d="M52 150c0-44 34-78 76-78s76 34 76 78c0 32-30 52-76 52s-76-20-76-52Z" fill="#8a7f63"/><path d="M60 92 44 44l42 30Zm136 0 16-48-42 30Z" fill="#8a7f63"/><circle cx="100" cy="126" r="24" fill="#fff"/><circle cx="156" cy="126" r="24" fill="#fff"/><circle cx="108" cy="130" r="10" fill="#222"/><circle cx="148" cy="130" r="10" fill="#222"/><ellipse cx="128" cy="168" rx="16" ry="9" fill="#3a3528"/><path d="M150 176 222 104" stroke="#e8c26a" stroke-width="10" stroke-linecap="round"/><path d="M222 104 236 90" stroke="#b33a2c" stroke-width="12" stroke-linecap="round"/>',
 # Pinta: a paintbrush crossing a small palette, on a Yaru-like rounded square.
 ('ubuntu','pinta',256):'<rect x="16" y="20" width="224" height="224" rx="48" fill="#000" opacity=".16"/><rect x="16" y="14" width="224" height="224" rx="48" fill="#f2f0ec"/><path d="M128 50c-50 0-86 34-86 76 0 38 28 62 62 62 14 0 18-8 16-18-2-12 6-18 18-18h22c26 0 46-18 46-44 0-34-34-58-78-58Z" fill="#fff" stroke="#b9b2a3" stroke-width="5"/><circle cx="86" cy="122" r="14" fill="#e01b24"/><circle cx="106" cy="84" r="14" fill="#f6d32d"/><circle cx="150" cy="80" r="14" fill="#33d17a"/><circle cx="182" cy="106" r="14" fill="#3584e4"/><path d="M226 44 150 150" stroke="#865e3c" stroke-width="13" stroke-linecap="round"/><path d="M155 143c-14-4-28 5-31 18-4 14-11 21-22 25 24 9 55 2 60-22 2-9-2-18-7-21Z" fill="#9141ac"/>',
 # Sketchbook: an orange-red circle with a white pencil swoosh.
 ('android','sketchbook',128):'<circle cx="64" cy="64" r="60" fill="#ef5a2e"/><path d="M28 88c14-30 34-44 50-38 12 5 2 22-10 26-10 3-4 14 12 10 12-3 20-12 24-18" fill="none" stroke="#fff" stroke-width="8" stroke-linecap="round" stroke-linejoin="round"/><path d="m92 30 10 10-30 30-13 3 3-13Z" fill="#fff"/>',
 # KiCad, on the three desktops: a circuit board carrying an IC, gold traces and vias.
 ('macos','kicad',128):'<defs><linearGradient id="kc" x2="0" y2="1"><stop stop-color="#6f8fe8"/><stop offset="1" stop-color="#2b47a6"/></linearGradient></defs><rect x="5" y="6" width="118" height="118" rx="27" fill="#000" opacity=".14"/><rect x="5" y="3" width="118" height="118" rx="27" fill="url(#kc)"/><rect x="6" y="4" width="116" height="116" rx="26" fill="none" stroke="#fff" stroke-opacity=".32"/><g transform="translate(8 8) scale(.875)"><rect x="18" y="22" width="92" height="84" rx="10" fill="#1d7a48" stroke="#0f5230" stroke-width="3"/><path d="M18 50h26M18 78h26M84 50h26M84 78h26M56 22v20M72 22v20M56 86v20M72 86v20" stroke="#e8c14a" stroke-width="4"/><rect x="42" y="42" width="44" height="44" rx="4" fill="#23272e" stroke="#0b0d10" stroke-width="2"/><circle cx="50" cy="50" r="3" fill="#9aa3ad"/><path d="M50 70l8-8 6 10 10-16" fill="none" stroke="#5fd18a" stroke-width="3" stroke-linecap="round" stroke-linejoin="round"/><circle cx="30" cy="36" r="5" fill="#e8c14a"/><circle cx="30" cy="36" r="2" fill="#0f5230"/><circle cx="98" cy="94" r="5" fill="#e8c14a"/><circle cx="98" cy="94" r="2" fill="#0f5230"/><circle cx="98" cy="34" r="5" fill="#e8c14a"/><circle cx="98" cy="34" r="2" fill="#0f5230"/></g>',
 ('windows','kicad',128):'<rect x="18" y="22" width="92" height="84" rx="10" fill="#1d7a48" stroke="#0f5230" stroke-width="3"/><path d="M18 50h26M18 78h26M84 50h26M84 78h26M56 22v20M72 22v20M56 86v20M72 86v20" stroke="#e8c14a" stroke-width="4"/><rect x="42" y="42" width="44" height="44" rx="4" fill="#23272e" stroke="#0b0d10" stroke-width="2"/><circle cx="50" cy="50" r="3" fill="#9aa3ad"/><path d="M50 70l8-8 6 10 10-16" fill="none" stroke="#5fd18a" stroke-width="3" stroke-linecap="round" stroke-linejoin="round"/><circle cx="30" cy="36" r="5" fill="#e8c14a"/><circle cx="30" cy="36" r="2" fill="#0f5230"/><circle cx="98" cy="94" r="5" fill="#e8c14a"/><circle cx="98" cy="94" r="2" fill="#0f5230"/><circle cx="98" cy="34" r="5" fill="#e8c14a"/><circle cx="98" cy="34" r="2" fill="#0f5230"/>',
 ('ubuntu','kicad',256):'<defs><linearGradient id="kc" x2="0" y2="1"><stop stop-color="#5a7fe0"/><stop offset="1" stop-color="#2f4bb0"/></linearGradient></defs><rect x="16" y="20" width="224" height="224" rx="48" fill="#000" opacity=".16"/><rect x="16" y="14" width="224" height="224" rx="48" fill="url(#kc)"/><g transform="translate(24 20) scale(1.62)"><rect x="18" y="22" width="92" height="84" rx="10" fill="#1d7a48" stroke="#0f5230" stroke-width="3"/><path d="M18 50h26M18 78h26M84 50h26M84 78h26M56 22v20M72 22v20M56 86v20M72 86v20" stroke="#e8c14a" stroke-width="4"/><rect x="42" y="42" width="44" height="44" rx="4" fill="#23272e" stroke="#0b0d10" stroke-width="2"/><circle cx="50" cy="50" r="3" fill="#9aa3ad"/><path d="M50 70l8-8 6 10 10-16" fill="none" stroke="#5fd18a" stroke-width="3" stroke-linecap="round" stroke-linejoin="round"/><circle cx="30" cy="36" r="5" fill="#e8c14a"/><circle cx="30" cy="36" r="2" fill="#0f5230"/><circle cx="98" cy="94" r="5" fill="#e8c14a"/><circle cx="98" cy="94" r="2" fill="#0f5230"/><circle cx="98" cy="34" r="5" fill="#e8c14a"/><circle cx="98" cy="34" r="2" fill="#0f5230"/></g>',
}
def editors():
 for (platform,name,size),art in EDITORS.items():
  a,b=('#f3f6fa','#d9e1ea') if name=='preview' else ('#fff','#eee')
  defs=f'<defs><linearGradient id="bg" x2="0" y2="1"><stop stop-color="{a}"/><stop offset="1" stop-color="{b}"/></linearGradient></defs>'
  svg=f'<svg xmlns="http://www.w3.org/2000/svg" width="{size}" height="{size}" viewBox="0 0 {size} {size}">{defs}{art}</svg>'
  (OUT/f'{platform}-{name}.svg').write_text(svg)
  cairosvg.svg2png(bytestring=svg.encode(),write_to=str(OUT/f'{platform}-{name}.png'),output_width=size,output_height=size)

# FreeCAD, on the three desktops: an isometric machined block with a bored hole and a
# red sketch profile on its top face. Original artwork (MIT), not FreeCAD's logo.
FREECAD_ART='<path d="M24 50 64 30 104 50 104 92 64 112 24 92Z" fill="#5b6b7e"/><path d="M24 50 64 30 104 50 64 70Z" fill="#e3e9f0"/><path d="M24 50 64 70 64 112 24 92Z" fill="#a7b6c8"/><path d="M64 70 104 50 104 92 64 112Z" fill="#74879d"/><ellipse cx="64" cy="50" rx="15" ry="7.5" fill="#34404e"/><path d="M49 50a15 7.5 0 0 0 30 0" fill="#23303c"/><path d="M34 50 64 35 94 50 64 65Z" fill="none" stroke="#e03131" stroke-width="3.5" stroke-linejoin="round"/><circle cx="64" cy="35" r="4" fill="#e03131"/><circle cx="94" cy="50" r="4" fill="#e03131"/><circle cx="34" cy="50" r="4" fill="#e03131"/><circle cx="64" cy="65" r="4" fill="#e03131"/><path d="M24 50 64 70 104 50M64 70v42" fill="none" stroke="#2b3542" stroke-width="2" stroke-linejoin="round" opacity=".55"/>'
def freecad():
 tiles={
  'macos':('<defs><linearGradient id="bg" x2="0" y2="1"><stop stop-color="#fbfcfe"/><stop offset="1" stop-color="#dfe6ee"/></linearGradient></defs><rect x="5" y="6" width="118" height="118" rx="27" fill="#000" opacity=".14"/><rect x="5" y="3" width="118" height="118" rx="27" fill="url(#bg)"/><rect x="6" y="4" width="116" height="116" rx="26" fill="none" stroke="#fff" stroke-opacity=".32"/>',128),
  'windows':('',128),
  'ubuntu':('<rect x="8" y="10" width="112" height="112" rx="24" fill="#000" opacity=".16"/><rect x="8" y="7" width="112" height="112" rx="24" fill="#f6f5f4"/>',256),
 }
 for platform,(bg,size) in tiles.items():
  svg=f'<svg xmlns="http://www.w3.org/2000/svg" width="{size}" height="{size}" viewBox="0 0 128 128">{bg}{FREECAD_ART}</svg>'
  (OUT/f'{platform}-freecad.svg').write_text(svg)
  cairosvg.svg2png(bytestring=svg.encode(),write_to=str(OUT/f'{platform}-freecad.png'),output_width=size,output_height=size)

# Spreadsheets and database clients, each in its platform's idiom: Excel's mark (bare on
# Windows, over a soft shadow on the Mac), Numbers' bar chart on a green tile, Sheets'
# folded green page on Android, DB Browser for SQLite's stacked cylinder and TablePlus's
# ruled table on an amber squircle. Original artwork (MIT), not vendor logos. Ubuntu's
# LibreOffice Calc icon is Yaru's own and is copied, not drawn. `--office` rebuilds these.
EXCEL_MARK='<rect x="38" y="16" width="78" height="96" rx="8" fill="#21a366"/><path d="M77 16h31a8 8 0 0 1 8 8v24H77Z" fill="#33c481"/><path d="M77 80h39v24a8 8 0 0 1-8 8H77Z" fill="#107c41"/><path d="M38 48h78M38 80h78M77 16v96" stroke="#185c37" stroke-opacity=".35" stroke-width="2"/><rect x="12" y="36" width="56" height="56" rx="7" fill="#107c41"/><rect x="12" y="36" width="56" height="56" rx="7" fill="none" stroke="#0b5a2f" stroke-width="2"/><path d="m28 51 24 26m0-26-24 26" stroke="#fff" stroke-width="8" stroke-linecap="round"/>'
NUMBERS_ART='<rect x="28" y="70" width="16" height="34" rx="3" fill="#fff"/><rect x="50" y="46" width="16" height="58" rx="3" fill="#fff"/><rect x="72" y="58" width="16" height="46" rx="3" fill="#e8f7e1"/><rect x="94" y="30" width="16" height="74" rx="3" fill="#fff"/><path d="M22 106h92" stroke="#fff" stroke-opacity=".7" stroke-width="3" stroke-linecap="round"/>'
CYLINDER='<path d="M26 34v60c0 9 17 16 38 16s38-7 38-16V34Z" fill="#2f78b7"/><path d="M26 54c0 9 17 16 38 16s38-7 38-16M26 74c0 9 17 16 38 16s38-7 38-16" fill="none" stroke="#9fd0f5" stroke-width="3"/><ellipse cx="64" cy="34" rx="38" ry="16" fill="#62aee8"/><ellipse cx="64" cy="34" rx="30" ry="11" fill="#9fd0f5"/><rect x="70" y="72" width="44" height="40" rx="4" fill="#fff" stroke="#23557f" stroke-width="3"/><path d="M70 85h44M70 98h44M85 72v40M100 72v40" stroke="#23557f" stroke-width="2"/><path d="M70 72h44v13H70Z" fill="#cfe6f8"/>'
OFFICE={
 ('windows','spreadsheet',128):EXCEL_MARK,
 ('macos','excel',128):'<g transform="translate(0 3)" opacity=".18"><rect x="38" y="16" width="78" height="96" rx="8"/><rect x="12" y="36" width="56" height="56" rx="7"/></g>'+EXCEL_MARK,
 ('macos','spreadsheet',128):'<defs><linearGradient id="nb" x2="0" y2="1"><stop stop-color="#4fd46a"/><stop offset="1" stop-color="#1c9f3c"/></linearGradient></defs><rect x="5" y="6" width="118" height="118" rx="27" fill="#000" opacity=".14"/><rect x="5" y="3" width="118" height="118" rx="27" fill="url(#nb)"/><rect x="6" y="4" width="116" height="116" rx="26" fill="none" stroke="#fff" stroke-opacity=".32"/>'+NUMBERS_ART,
 ('ios','spreadsheet',128):'<defs><linearGradient id="nb" x2="0" y2="1"><stop stop-color="#4fd46a"/><stop offset="1" stop-color="#1c9f3c"/></linearGradient></defs><rect x="5" y="4" width="118" height="118" rx="27" fill="url(#nb)"/>'+NUMBERS_ART,
 ('android','spreadsheet',128):'<circle cx="64" cy="64" r="60" fill="#fff"/><circle cx="64" cy="64" r="60" fill="none" stroke="#dadce0" stroke-width="2"/><path d="M38 20h38l20 20v68a4 4 0 0 1-4 4H38a4 4 0 0 1-4-4V24a4 4 0 0 1 4-4Z" fill="#0f9d58"/><path d="M76 20v16a4 4 0 0 0 4 4h16Z" fill="#87ceac"/><rect x="45" y="58" width="40" height="36" rx="2" fill="#fff"/><path d="M45 70h40M45 82h40M61 58v36" stroke="#0f9d58" stroke-width="4"/>',
 ('windows','database',128):CYLINDER,
 ('ubuntu','database',256):'<rect x="16" y="20" width="224" height="224" rx="48" fill="#000" opacity=".16"/><rect x="16" y="14" width="224" height="224" rx="48" fill="#eef2f6"/><rect x="16" y="14" width="224" height="112" rx="48" fill="#f7f9fb"/><g transform="translate(24 20) scale(1.62)">'+CYLINDER+'</g>',
 ('macos','database',128):'<defs><linearGradient id="tp" x2="0" y2="1"><stop stop-color="#ffcc4d"/><stop offset="1" stop-color="#f29a1f"/></linearGradient></defs><rect x="5" y="6" width="118" height="118" rx="27" fill="#000" opacity=".14"/><rect x="5" y="3" width="118" height="118" rx="27" fill="url(#tp)"/><rect x="6" y="4" width="116" height="116" rx="26" fill="none" stroke="#fff" stroke-opacity=".32"/><rect x="24" y="30" width="80" height="68" rx="8" fill="#fff"/><path d="M24 38a8 8 0 0 1 8-8h64a8 8 0 0 1 8 8v12H24Z" fill="#3b4453"/><path d="M24 66h80M24 82h80M52 50v48M78 50v48" stroke="#c9ced6" stroke-width="3"/><circle cx="34" cy="40" r="3" fill="#ff5f57"/><circle cx="43" cy="40" r="3" fill="#febc2e"/><circle cx="52" cy="40" r="3" fill="#28c840"/>',
}
def office():
 for (platform,name,size),art in OFFICE.items():
  svg=f'<svg xmlns="http://www.w3.org/2000/svg" width="{size}" height="{size}" viewBox="0 0 {size} {size}">{art}</svg>'
  (OUT/f'{platform}-{name}.svg').write_text(svg)
  cairosvg.svg2png(bytestring=svg.encode(),write_to=str(OUT/f'{platform}-{name}.png'),output_width=size,output_height=size)

# Video editors, one per platform in its own icon idiom. Original artwork (MIT), not
# vendor logos: Clipchamp's violet clip with a play mark (bare, as Windows shows it),
# iMovie's star over a film frame on a violet squircle (the Mac and the phone), Kdenlive's
# clapper over a timeline on a Yaru-style tile, and the Android editor's scissors under a
# play mark in a teal circle. `--video` rebuilds just these.
IMOVIE_ART='<defs><linearGradient id="im" x2="0" y2="1"><stop stop-color="#b98cff"/><stop offset="1" stop-color="#5b2bd6"/></linearGradient><linearGradient id="st" x2="0" y2="1"><stop stop-color="#fff6c8"/><stop offset="1" stop-color="#ffc93a"/></linearGradient></defs>'
IMOVIE_BODY='<rect x="26" y="52" width="76" height="50" rx="8" fill="#261a4a"/><path d="M26 64h76M26 90h76" stroke="#6a55a8" stroke-width="3"/><g fill="#ecdcff">'+''.join(f'<rect x="{31+i*14}" y="55" width="8" height="6" rx="1"/><rect x="{31+i*14}" y="93" width="8" height="6" rx="1"/>' for i in range(5))+'</g><path d="M54 70v14l12-7Z" fill="#fff"/><path d="m64 16 7 15 16 2-12 11 3 16-14-8-14 8 3-16-12-11 16-2Z" fill="url(#st)" stroke="#e0a100" stroke-width="2" stroke-linejoin="round"/>'
VIDEO={
 ('windows','clipchamp',128):'<defs><linearGradient id="cc" x1="0" y1="0" x2="1" y2="1"><stop stop-color="#a98bff"/><stop offset=".55" stop-color="#6d4dff"/><stop offset="1" stop-color="#2f6bff"/></linearGradient></defs><path d="M30 18h54l26 26v58a10 10 0 0 1-10 10H30a10 10 0 0 1-10-10V28a10 10 0 0 1 10-10Z" fill="url(#cc)"/><path d="M84 18v20a6 6 0 0 0 6 6h20" fill="#c9b8ff"/><path d="M50 50v38l32-19Z" fill="#fff"/><path d="M30 100h56" stroke="#e6ddff" stroke-width="4" stroke-linecap="round" opacity=".7"/>',
 ('macos','imovie',128):IMOVIE_ART+'<rect x="5" y="6" width="118" height="118" rx="27" fill="#000" opacity=".14"/><rect x="5" y="3" width="118" height="118" rx="27" fill="url(#im)"/><rect x="6" y="4" width="116" height="116" rx="26" fill="none" stroke="#fff" stroke-opacity=".32"/>'+IMOVIE_BODY,
 ('ios','imovie',128):IMOVIE_ART+'<rect x="5" y="4" width="118" height="118" rx="27" fill="url(#im)"/>'+IMOVIE_BODY,
 ('ubuntu','kdenlive',256):'<defs><linearGradient id="kd" x2="0" y2="1"><stop stop-color="#3a4250"/><stop offset="1" stop-color="#1c2129"/></linearGradient></defs><rect x="16" y="20" width="224" height="224" rx="48" fill="#000" opacity=".16"/><rect x="16" y="14" width="224" height="224" rx="48" fill="url(#kd)"/><g transform="translate(24 20) scale(1.62)"><path d="M22 44 94 26l4 14-72 18Z" fill="#e8eef5"/><path d="m34 41 8 12m12-17 8 12m12-17 8 12" stroke="#1d99f3" stroke-width="5"/><rect x="22" y="58" width="84" height="44" rx="4" fill="#e8eef5"/><rect x="28" y="66" width="40" height="10" rx="2" fill="#1d99f3"/><rect x="50" y="82" width="46" height="10" rx="2" fill="#27ae60"/><path d="M60 60v42" stroke="#da4453" stroke-width="3"/></g>',
 ('android','videoeditor',128):'<defs><linearGradient id="ve" x2="0" y2="1"><stop stop-color="#3ee8cf"/><stop offset="1" stop-color="#009b8a"/></linearGradient></defs><circle cx="64" cy="64" r="60" fill="url(#ve)"/><rect x="30" y="30" width="68" height="52" rx="10" fill="#0d2b2a"/><path d="M56 44v24l20-12Z" fill="#fff"/><g stroke="#fff" stroke-width="5" stroke-linecap="round"><path d="M44 104l20-18M84 104 64 86"/></g><circle cx="42" cy="106" r="7" fill="none" stroke="#fff" stroke-width="4"/><circle cx="86" cy="106" r="7" fill="none" stroke="#fff" stroke-width="4"/>',
}
def video():
 for (platform,name,size),art in VIDEO.items():
  svg=f'<svg xmlns="http://www.w3.org/2000/svg" width="{size}" height="{size}" viewBox="0 0 {size} {size}">{art}</svg>'
  (OUT/f'{platform}-{name}.svg').write_text(svg)
  cairosvg.svg2png(bytestring=svg.encode(),write_to=str(OUT/f'{platform}-{name}.png'),output_width=size,output_height=size)

if '--video' in sys.argv:
 video()
 sys.exit(0)
if '--freecad' in sys.argv:
 freecad()
 sys.exit(0)
if '--office' in sys.argv:
 office()
 sys.exit(0)
if '--editors' not in sys.argv:
 platform_icons()
editors()
freecad()
office()
video()
