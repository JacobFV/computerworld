export default {
  id: 'gimp', title: 'Painting a sunset in GIMP on Ubuntu',
  machines: [{ id: 'ubuntu-gimp', like: 'carol-ubuntu', size: [1280, 800] }],
  open(pc) {
    const ui = target => pc.step('application.v1', 'shell', { target: `window:0:content:gimp:${target}` });
    // Image pixels to screen: the 640 x 480 image is shown at 100%.
    const at = ([x, y]) => ({ x: 341 + x, y: 224 + y, width: 1280, height: 800 });
    const drag = (...points) => {
      pc.step('pointer.v1', 'down', at(points[0]));
      for (const p of points.slice(1, -1)) pc.step('pointer.v1', 'move', at(p));
      pc.step('pointer.v1', 'up', at(points.at(-1)));
    };
    const dot = p => pc.clickAt(at(p).x, at(p).y);
    pc.launch('gimp');
    pc.click('window:0:maximize');
    ui('dialog:new-image'); ui('set:width:640'); ui('set:height:480'); ui('apply');
    // Sky, a soft glow and the sun, then the water over its lower half.
    ui('fg:2a2a72'); ui('bg:ff9d52'); ui('tool:gradient'); drag([320, 0], [320, 300]);
    ui('tool:brush'); ui('fg:ffd27a'); ui('set:hardness:0'); ui('set:opacity:45'); ui('set:size:300'); dot([320, 300]);
    ui('fg:fff0b3'); ui('set:hardness:90'); ui('set:opacity:100'); ui('set:size:120'); dot([320, 300]);
    ui('tool:select-rect'); drag([0, 300], [640, 480]);
    ui('fg:f7a35c'); ui('bg:1d2350'); ui('tool:gradient'); drag([320, 300], [320, 480]);
    // Hills along the horizon: a lasso outline, filled.
    ui('tool:lasso');
    drag([0, 265], [50, 240], [95, 258], [165, 212], [215, 265], [260, 300], [400, 300], [450, 272], [500, 282], [570, 236], [640, 268], [640, 300], [0, 300]);
    ui('fg:241a3a'); ui('tool:fill'); ui('set:tolerance:255'); dot([60, 280]);
    ui('select-none');
    // Light on the water, on its own layer.
    ui('layer:new'); ui('tool:brush'); ui('fg:ffe3a1'); ui('set:size:6'); ui('set:opacity:80');
    for (const [y, half] of [[314, 56], [334, 42], [356, 50], [384, 30], [414, 38], [448, 20]]) drag([320 - half, y], [320 + half, y + 1]);
  },
};
