export default {
  id: 'kicad-board', title: "PCB layout, routing and copper pours",
  summary: "blinker.kicad_pcb on Ubuntu: a 43 × 23 mm Edge.Cuts outline, signals routed on F.Cu, the reset net on B.Cu, and a GND pour over the rest.",
  machines: [{ id: 'ubuntu-kicad', like: 'carol-ubuntu', size: [1280, 800] }],
  open(pc) {
    const ui = (w, target) => pc.step('application.v1', 'shell', { target: `window:${w}:content:kicad:${target}` });
    // Sheet positions are in mils; after two zoom steps the canvas shows 159 px per 1000.
    const px = (x, y) => [104 + Math.floor((x - 1987) * 0.159), 136 + Math.floor((y - 1691) * 0.159)];
    const at = (x, y) => pc.clickAt(...px(x, y));
    const place = (tool, id, x, y, key) => {
      ui(1, `sch:tool:${tool}`); ui(1, `dlg:pick:${id}`); ui(1, 'dlg:ok');
      if (key) pc.key(key);
      at(x, y);
    };
    const wire = (...points) => { for (const p of points) at(...p); };
    pc.launch('kicad');
    ui(0, 'pm:new'); ui(0, 'field:name'); for (let i = 0; i < 8; i++) pc.key('Backspace');
    pc.type('blinker'); ui(0, 'dlg:ok');
    ui(0, 'pm:launch:sch');
    pc.step('application.v1', 'maximize', {}); pc.step('application.v1', 'close', { window: 0 });
    pc.step('pointer.v1', 'move', { x: 640, y: 400, width: 1280, height: 800 });
    pc.key('F1'); pc.key('F1');
    place('symbol', 'Timer:NE555P', 5000, 3500);
    place('symbol', 'Device:R', 6200, 2900);
    place('symbol', 'Device:R', 6200, 3750);
    place('symbol', 'Device:R', 5700, 2550);
    place('symbol', 'Device:C', 6200, 4250);
    place('symbol', 'Device:LED', 5700, 2950, 'r');
    place('symbol', 'Connector:Conn_01x02', 3500, 2300, 'y');
    place('power', 'power:+5V', 5000, 2300);
    place('power', 'power:GND', 5000, 3900);
    place('power', 'power:GND', 6200, 4400);
    place('power', 'power:GND', 3900, 2600);
    ui(1, 'sch:tool:wire');
    wire([5000, 2300], [5000, 3100]);
    wire([4600, 3700], [4300, 3700], [4300, 2300], [5000, 2300]);
    wire([5700, 2400], [5700, 2300], [5000, 2300]);
    wire([6200, 2750], [6200, 2300], [5700, 2300]);
    wire([5700, 2700], [5700, 2800]);
    wire([5700, 3100], [5700, 3300], [5400, 3300]);
    wire([6200, 3050], [6200, 3600]);
    wire([5400, 3500], [6200, 3500]);
    wire([6200, 3900], [6200, 4100]);
    wire([5400, 3700], [5800, 3700], [5800, 4000], [6200, 4000]);
    wire([4600, 3300], [4450, 3300], [4450, 4300], [5800, 4300], [5800, 4000]);
    wire([3700, 2300], [4300, 2300]);
    wire([3700, 2400], [3900, 2400], [3900, 2600]);
    ui(1, 'sch:tool:noconnect'); at(4600, 3500);
    ui(1, 'sch:tool:select');
    // Values through each symbol's properties dialog.
    for (const [x, y, value] of [[6200, 4250, '10u'], [5700, 2550, '470']]) {
      at(x, y); ui(1, 'sch:properties');
      ui(1, 'field:Value');
      for (let i = 0; i < 4; i++) pc.key('Backspace');
      pc.type(value); ui(1, 'dlg:ok');
    }
    ui(1, 'sch:update-pcb'); ui(2, 'dlg:apply'); ui(2, 'dlg:ok');
    pc.step('application.v1', 'maximize', {});
    // Every open KiCad frame makes each step slower; the project manager and schematic have done their part.
    pc.step('application.v1', 'close', { window: 1 });
    // Board positions are in mm; the new board opens at 13 px per mm.
    const mm = (x, y) => ({ x: 104 + Math.floor((x - 16.619) * 13), y: 136 + Math.floor((y - 6.774) * 13), width: 1280, height: 800 });
    const move = (x, y, dx, dy) => {
      pc.step('pointer.v1', 'down', mm(x + 0.5, y)); pc.step('pointer.v1', 'up', mm(x + 0.5 + dx, y + dy));
    };
    const route = (...points) => { for (const p of points) pc.clickAt(mm(...p).x, mm(...p).y); };
    // Drag each footprint from where Update PCB dropped it (its pad 1) by a whole-grid offset.
    move(21.05, 21.5, 41.75, 10.5);
    move(58.46, 21.62, -0.5, 5);
    move(44.2, 21.62, 13.75, -0.5);
    move(31.05, 23.25, 12.25, 11.75); pc.key('r'); pc.key('r');
    move(39.35, 21.8, -5.25, 2.25);
    move(21.1, 33.55, 25, -9.5);
    move(72.72, 21.62, -35.75, 0.5); pc.key('Shift+R');
    ui(2, 'pcb:layer:Edge.Cuts'); ui(2, 'pcb:tool:rect'); route([30, 16], [73, 39]);
    // Pad to pad on the front, the reset line on the back, then a ground pour under it all.
    ui(2, 'pcb:layer:F.Cu'); ui(2, 'pcb:tool:route');
    route([34.1, 24.05], [36.97, 22.12]);
    route([36.97, 22.12], [39, 20], [49.75, 20], [53.72, 24.05]);
    route([53.72, 24.05], [57.95, 21.12]);
    route([53.72, 26.59], [57.96, 26.62]);
    route([68.11, 21.12], [65.5, 23.75], [60.75, 23.75], [57.96, 26.62]);
    route([68.12, 26.62], [62.8, 32]);
    route([62.8, 32], [53.72, 29.13]);
    route([46.1, 26.59], [53.72, 29.13]);
    route([46.1, 29.13], [43.3, 35]);
    route([40.76, 35], [36.97, 32.28]);
    route([34.1, 26.59], [46.1, 24.05]);
    ui(2, 'pcb:layer:B.Cu');
    route([53.72, 24.05], [46.1, 31.67]);
    ui(2, 'pcb:tool:zone'); route([31, 17], [72, 17], [72, 38], [31, 38]);
    pc.step('pointer.v1', 'double_click', mm(31, 38));
    ui(2, 'dlg:net:2'); ui(2, 'dlg:ok');
    ui(2, 'pcb:layer:F.Cu'); ui(2, 'pcb:tool:select'); ui(2, 'pcb:zoom:fit'); ui(2, 'pcb:save');
  },
};
