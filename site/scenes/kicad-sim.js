export default {
  id: 'kicad-sim', title: "SPICE simulation on the schematic",
  summary: "KiCad on Windows 11 runs a transient analysis over oscillator.kicad_sch — 50 µs steps out to 80 ms — and reads V(out) and V(cap) under a cursor.",
  machines: [{ id: 'windows-kicad', like: 'bob-windows', size: [1280, 800] }],
  open(pc) {
    const ui = (w, target) => pc.step('application.v1', 'shell', { target: `window:${w}:content:kicad:${target}` });
    // Sheet positions are in mils; after two zoom steps the canvas shows 157 px per 1000.
    const px = (x, y) => [36 + Math.floor((x - 2002) * 0.157), 96 + Math.floor((y - 2064) * 0.157)];
    const at = (x, y) => pc.clickAt(...px(x, y));
    const place = (tool, id, x, y, key) => {
      ui(1, `sch:tool:${tool}`); ui(1, `dlg:pick:${id}`); ui(1, 'dlg:ok');
      if (key) pc.key(key);
      at(x, y);
    };
    const wire = (...points) => { for (const p of points) at(...p); };
    pc.launch('kicad');
    ui(0, 'pm:new'); ui(0, 'field:name'); for (let i = 0; i < 8; i++) pc.key('Backspace');
    pc.type('oscillator'); ui(0, 'dlg:ok');
    ui(0, 'pm:launch:sch');
    pc.click('window:1:maximize');
    pc.step('pointer.v1', 'move', { x: 640, y: 400, width: 1280, height: 800 });
    pc.key('F1'); pc.key('F1');
    place('symbol', 'Timer:NE555P', 5000, 3500);
    place('symbol', 'Device:R', 6200, 2900);
    place('symbol', 'Device:R', 6200, 3750);
    place('symbol', 'Device:R', 5700, 2550);
    place('symbol', 'Device:C', 6200, 4250);
    place('symbol', 'Device:LED', 5700, 2950, 'r');
    place('symbol', 'Simulation_SPICE:VDC', 3800, 2700);
    place('power', 'power:+5V', 5000, 2300);
    place('power', 'power:GND', 5000, 3900);
    place('power', 'power:GND', 6200, 4400);
    place('power', 'power:GND', 3800, 2900);
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
    wire([3800, 2500], [3800, 2300], [4300, 2300]);
    ui(1, 'sch:tool:noconnect'); at(4600, 3500);
    ui(1, 'sch:tool:select');
    for (const [x, y, name] of [[5550, 3300, 'out'], [6000, 4000, 'cap']]) {
      ui(1, 'sch:tool:label'); at(x, y); pc.type(name); ui(1, 'dlg:ok');
    }
    ui(1, 'sch:tool:select');
    at(6200, 4250); ui(1, 'sch:properties'); ui(1, 'field:Value');
    for (let i = 0; i < 4; i++) pc.key('Backspace');
    pc.type('1u'); ui(1, 'dlg:ok');
    at(7000, 5000); ui(1, 'sch:zoom:objects'); ui(1, 'sch:save');
    ui(1, 'sch:simulator');
    pc.click('window:2:maximize');
    ui(2, 'sim:settings'); ui(2, 'dlg:tab:3');
    for (const [field, value] of [['Time step', '50u'], ['Final time', '80m']]) {
      ui(2, `field:${field}`);
      for (let i = 0; i < 3; i++) pc.key('Backspace');
      pc.type(value);
    }
    ui(2, 'dlg:ok'); ui(2, 'sim:run'); ui(2, 'sim:signals');
    ui(2, 'dlg:signal:V(out)'); ui(2, 'dlg:signal:V(cap)'); ui(2, 'dlg:ok');
    ui(2, 'sim:cursor:0');
  },
};
