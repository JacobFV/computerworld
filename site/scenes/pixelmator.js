export default {
  id: 'pixelmator', title: 'Designing a poster in Pixelmator Pro on macOS',
  summary: 'An 800 × 600 poster · NORTHSTAR at 72 pt over "LAUNCH NIGHT · OCTOBER 9", a radial night sky',
  machines: [{ id: 'mac-pixelmator', like: 'alice-mac', size: [1280, 800] }],
  open(pc) {
    const ui = target => pc.step('application.v1', 'shell', { target: `window:0:content:pixelmator:${target}` });
    // Image pixels to screen: the 800 x 600 image is shown at 97%.
    const at = ([x, y]) => ({ x: Math.round(232 + x * 0.97), y: Math.round(126 + y * 0.97), width: 1280, height: 800 });
    const drag = (from, to) => { pc.step('pointer.v1', 'down', at(from)); pc.step('pointer.v1', 'up', at(to)); };
    pc.launch('pixelmator');
    pc.click('window:0:maximize');
    ui('dialog:new-image'); ui('set:width:800'); ui('set:height:600'); ui('apply');
    ui('fg:24357a'); ui('bg:070b1d'); ui('tool:gradient'); ui('gradient-shape:radial'); drag([400, 230], [400, 640]);
    // Shapes fill with the background colour: one gold star, a scatter of pale ones.
    ui('tool:shape'); ui('shape:star'); ui('fill-style:fill'); ui('bg:f6c453'); drag([290, 60], [510, 280]);
    ui('bg:c9d4ff');
    for (const [x, y, r] of [[120, 110, 14], [200, 250, 9], [650, 90, 12], [700, 240, 8]]) drag([x - r, y - r], [x + r, y + r]);
    ui('tool:text'); ui('color:ffffff'); ui('set:font-size:72');
    pc.clickAt(at([180, 350]).x, at([180, 350]).y); pc.type('NORTHSTAR'); pc.key('Enter');
    ui('color:c9d4ff'); ui('set:font-size:28');
    pc.clickAt(at([194, 470]).x, at([194, 470]).y); pc.type('LAUNCH NIGHT  ·  OCTOBER 9'); pc.key('Enter');
  },
};
