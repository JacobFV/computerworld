export default {
  id: 'kicad-schematic', title: "Schematic capture from an empty sheet",
  summary: "An NE555P astable wired up in KiCad on macOS — C1 at 10 µF, the LED resistor at 470 Ω — and blinker.kicad_sch saved.",
  machines: [{ id: 'mac-kicad', like: 'alice-mac', size: [1280, 800] }],
  open(pc) {
    const ui = (w, target) => pc.step('application.v1', 'shell', { target: `window:${w}:content:kicad:${target}` });
    // Sheet positions are in mils; after two zoom steps the canvas shows 141 px per 1000.
    const px = (x, y) => [36 + Math.floor((x - 1565) * 0.141), 128 + Math.floor((y - 2016) * 0.141)];
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
    pc.click('window:1:maximize');
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
    at(7000, 5000);
    ui(1, 'sch:zoom:objects'); ui(1, 'sch:save');
  },
};
