export default {
  id: 'paint', title: 'A drawing in Paint on Windows',
  summary: 'A 960 × 540 sheet · house, tree, sun and clouds in filled shapes, two birds in pencil',
  machines: [{ id: 'windows-paint', like: 'bob-windows', size: [1280, 800] }],
  open(pc) {
    const ui = target => pc.step('application.v1', 'shell', { target: `window:0:content:paint:${target}` });
    // Image pixels to screen: the 960 x 540 canvas is shown at 96%.
    const at = ([x, y]) => ({ x: Math.round(179 + x * 0.96), y: Math.round(186 + y * 0.96), width: 1280, height: 800 });
    const drag = (...points) => {
      pc.step('pointer.v1', 'down', at(points[0]));
      for (const p of points.slice(1, -1)) pc.step('pointer.v1', 'move', at(p));
      pc.step('pointer.v1', 'up', at(points.at(-1)));
    };
    // A shape is outlined in Color 1 and filled with Color 2.
    const shape = (kind, outline, fill, from, to) => { ui(`shape:${kind}`); ui(`fg:${outline}`); ui(`bg:${fill}`); drag(from, to); };
    pc.launch('paint');
    pc.click('window:0:maximize');
    ui('tool:fill'); ui('fg:99d9ea'); pc.clickAt(at([10, 10]).x, at([10, 10]).y);
    ui('fill:solid');
    shape('rectangle', '22b14c', '22b14c', [0, 400], [959, 539]);
    shape('ellipse', 'ffc90e', 'fff200', [780, 40], [880, 140]);
    shape('ellipse', 'ffffff', 'ffffff', [110, 70], [230, 120]);
    shape('ellipse', 'ffffff', 'ffffff', [180, 50], [310, 110]);
    shape('rectangle', '000000', 'efe4b0', [300, 260], [560, 440]);
    shape('triangle', '000000', '880015', [270, 150], [590, 260]);
    shape('rectangle', '000000', 'b97a57', [400, 340], [460, 440]);
    shape('rectangle', '000000', 'c8bfe7', [325, 300], [375, 350]);
    shape('rectangle', '000000', 'c8bfe7', [485, 300], [535, 350]);
    shape('rectangle', '000000', 'b97a57', [722, 330], [752, 445]);
    shape('ellipse', '000000', 'b5e61d', [665, 210], [810, 350]);
    ui('tool:pencil'); ui('fg:000000');
    drag([600, 90], [615, 75], [630, 90], [645, 75], [660, 90]);
    drag([520, 50], [532, 38], [544, 50], [556, 38], [568, 50]);
  },
};
