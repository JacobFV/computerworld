export default {
  id: 'imovie', title: 'A short film coming together in iMovie on macOS',
  summary: 'Three clips over a music bed · cross dissolve, "Golden hour, Pier 14" at 80, Ken Burns',
  machines: [{ id: 'mac-imovie', like: 'alice-mac', size: [1280, 800] }],
  open(pc) {
    const ui = target => pc.step('application.v1', 'shell', { target: `window:0:content:video:${target}` });
    pc.launch('imovie');
    pc.click('window:0:maximize');
    ui('import');
    for (const file of ['Countdown.apng', 'Sunset.apng', 'Color Bars.apng', 'Music Bed.wav']) ui(`pick:${file}`);
    ui('close-sheet');
    // iMovie adds at the playhead. Media ids follow the import order from 3; clips from 7.
    for (const id of [3, 4, 5]) { ui('end'); ui(`append:${id}`); }
    ui('seek:72'); ui('append:6');
    ui('zoom-fit:1278');
    ui('clip:7:50:0'); ui('add-transition:cross_dissolve');
    ui('seek:80'); ui('add-title:lower'); ui('title-text');
    for (let i = 0; i < 9; i++) pc.key('Backspace');
    pc.type('Golden hour, Pier 14');
    ui('clip:8:50:0'); ui('ken-burns'); ui('seek:120');
  },
};
