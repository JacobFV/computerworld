export default {
  id: 'kdenlive', title: 'Cutting clips in Kdenlive on Ubuntu',
  machines: [{ id: 'ubuntu-kdenlive', like: 'carol-ubuntu', size: [1280, 800] }],
  open(pc) {
    const ui = target => pc.step('application.v1', 'shell', { target: `window:0:content:video:${target}` });
    pc.launch('kdenlive');
    pc.click('window:0:maximize');
    ui('import');
    for (const file of ['Countdown.apng', 'Sunset.apng', 'Color Bars.apng', 'Countdown Beeps.wav', 'Music Bed.wav']) ui(`pick:${file}`);
    ui('close-sheet');
    // Media ids follow the import order from 3; clips from 8.
    for (const id of [3, 4, 5, 6, 7]) ui(`append:${id}`);
    ui('zoom-fit:1090');
    ui('clip:8:40:0'); ui('add-transition:cross_dissolve');
    ui('clip:9:40:0'); ui('add-transition:dip_to_black');
    ui('seek:84'); ui('add-title:lower'); ui('title-text');
    for (let i = 0; i < 9; i++) pc.key('Backspace');
    pc.type('Golden hour, Pier 14');
    ui('clip:9:40:0'); ui('set:fade-in:12'); ui('seek:130');
  },
};
