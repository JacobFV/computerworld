// Five machines on one board: the sensor node the reference world keeps in
// ~/Documents/KiCad/sensor-node, and the enclosure lid in ~/Documents/Parts that has to
// go over it. Rev B of the board changes one part — the LED resistor R3, 330 Ω is
// blinding at 3.3 V, so it becomes 1 kΩ — and every screen here is that one change:
// Bob's schematic, the board on the bench, Alice's lid, the diff on Carol's ThinkPad,
// and the three of them talking about it on Alice's phone.
const GROUP = 'messages:open:+14155550100|+14155550101|+14155550102';

/** Send one text into the team's group thread from a desktop. */
const say = (pc, text) => {
  pc.click(GROUP);            // the desktop list keeps the thread open; clicking refetches
  pc.click('messages:compose');
  pc.type(text);
  pc.key('Enter');
};

export default {
  id: 'hardware-team', title: 'Five machines on one board',
  machines: [
    { id: 'hardware-board', like: 'carol-ubuntu', size: [1280, 800], label: 'The lab bench · the sensor-node board, DRC after the change' },
    { id: 'hardware-schematic', like: 'bob-windows', size: [1280, 800], label: "Bob's PC · the same project's schematic, R3 → 1 kΩ" },
    { id: 'hardware-enclosure', like: 'alice-mac', size: [1280, 800], label: "Alice's Mac · the lid that goes over that board" },
    { id: 'hardware-git', like: 'carol-ubuntu', size: [1280, 800], label: "Carol's ThinkPad · rev B on its way into git" },
    { id: 'hardware-phone', like: 'alice-phone', size: [390, 844], label: "Alice's iPhone · the three of them on rev B" },
  ],
  open(bench, bob, alice, carol, phone) {
    // The team talks first, from the machines they are sitting at; the phone reads the
    // thread at the end, once every message has been sent.
    for (const pc of [bob, alice, carol]) { pc.launch('messages'); pc.click(GROUP); }
    say(bob, 'R3 is 330 Ω on the sensor-node and the LED is blinding at 3.3 V. Rev B takes it to 1 kΩ.');
    say(alice, 'Fine by the lid — I am tracing the 70 × 45 board outline on the cavity floor to be sure.');
    say(carol, 'Diff is the schematic and the BOM line, nothing else. Going in as rev B.');
    say(bob, 'Board is updated from the schematic and DRC is clean, nothing unrouted.');
    say(alice, 'Lid clears the connector by 1.8 mm either way. Pilot batch of five is on.');
    for (const pc of [bob, alice, carol]) pc.step('application.v1', 'close', { window: 0 });

    // Bob: the schematic, the whole circuit on the canvas, R3 retyped from 330 to 1k.
    {
      const ui = t => bob.step('application.v1', 'shell', { target: `window:1:content:kicad:${t}` });
      bob.launch('kicad', 'sch|C:/Users/bob/Documents/KiCad/sensor-node/sensor-node.kicad_pro');
      bob.step('application.v1', 'maximize', {});
      ui('sch:zoom:objects');
      // One zoom step about the middle of the sheet: every part of the circuit stays on
      // the canvas. Sheet positions are in mils — R3 sits at 215.9 mm, 71.12 mm, which is
      // 8500, 2800 — and afterwards the canvas starts at 35, 95 and shows 162 px per
      // 1000 from 2990, 1966.
      bob.step('pointer.v1', 'move', { x: 700, y: 400, width: 1280, height: 800 });
      bob.key('F1');
      const at = (x, y) => bob.clickAt(35 + Math.round((x - 2990) * 0.162), 95 + Math.round((y - 1966) * 0.162));
      at(8500, 2800);
      ui('sch:properties');
      ui('field:Value');
      for (let i = 0; i < 3; i++) bob.key('Backspace');
      bob.type('1k');
      ui('dlg:ok');
      ui('sch:erc'); ui('dlg:run'); ui('dlg:ok');     // clean, and the status bar says so
      ui('sch:save');
      at(8500, 2800);                                  // R3 left selected, its value in hand
    }

    // The bench: the same board. Each machine here has its own disk, so Bob's one-line
    // schematic edit arrives on this one the way a pull would land it; KiCad does the rest
    // of the work — the board takes the new value from the schematic and is checked again.
    {
      const ui = t => bench.step('application.v1', 'shell', { target: `window:0:content:kicad:${t}` });
      bench.sh(`sed -i 's/"330"/"1k"/' ~/Documents/KiCad/sensor-node/sensor-node.kicad_sch`);
      bench.launch('kicad', 'pcb|/home/carol/Documents/KiCad/sensor-node/sensor-node.kicad_pro');
      bench.step('application.v1', 'maximize', {});
      ui('pcb:zoom:fit');
      ui('pcb:update'); ui('dlg:apply'); ui('dlg:ok');  // Update PCB from Schematic: R3 → 1k
      // R3's body, three millimetres along from the pad the footprint is placed by; the
      // board canvas starts at 104, 136 and shows 12 px per millimetre from −3.291,
      // −4.125 mm. Selecting it before the check leaves the part and the result together.
      bench.clickAt(104 + Math.round((13 + 3.291) * 12), 136 + Math.round((38 + 4.125) * 12));
      ui('pcb:save');
      ui('pcb:drc'); ui('dlg:run'); ui('dlg:ok');
    }

    // Alice: the lid, with the board's 70 × 45 outline sketched on the cavity floor.
    {
      const ui = t => alice.step('application.v1', 'shell', { target: `window:1:content:freecad:${t}` });
      alice.launch('freecad', '/Users/alice/Documents/Parts/enclosure-lid.FCStd.json');
      alice.click('window:1:maximize');
      ui('cmd:PartDesign_NewSketch'); ui('task:plane:DatumPlane001'); ui('task:ok');
      ui('cmd:Std_ViewFitAll');
      // The lid runs 0…120 by 0…80 mm; fitted, its origin is at 577, 548 and a
      // millimetre is 3.73 px. The board sits centred in it.
      const at = (x, y) => alice.clickAt(Math.round(577.5 + x * 3.73), Math.round(548 - y * 3.73));
      ui('cmd:Sketcher_CreateRectangle'); at(25, 17.5); at(95, 62.5); alice.key('Escape');
      ui('sk:element:0'); ui('cmd:Sketcher_ConstrainDistanceX'); alice.type('70'); alice.key('Enter');
      ui('sk:element:1'); ui('cmd:Sketcher_ConstrainDistanceY'); alice.type('45'); alice.key('Enter');
      ui('sk:close');
      ui('tree:Sketch002'); ui('prop:Label');   // the property field opens on its value
      alice.type('sensor-node board 70 × 45');
      alice.key('Enter');
      ui('tree-eye:DatumPlane'); ui('tree-eye:DatumPlane001');   // the datum planes out of the way
      ui('cmd:Std_ViewIsometric'); ui('cmd:Std_ViewFitAll');
      ui('tree:Sketch002');
    }

    // Carol: the same project under git, the change waiting to go in.
    {
      carol.sh('cd ~/Documents/KiCad/sensor-node && git init && git add . && git commit -m "sensor-node rev A"');
      carol.sh(`cd ~/Documents/KiCad/sensor-node && sed -i 's/"330"/"1k"/' sensor-node.kicad_sch sensor-node-bom.csv`);
      carol.launch('code', '/home/carol/Documents/KiCad/sensor-node');
      carol.step('application.v1', 'maximize', {});
      carol.click('code:tree:sensor-node-bom.csv');
      carol.clickAt(500, 304);                         // the R3 line of the bill of materials
      carol.click('code:activity:scm');
      carol.click('code:scm-message');
      carol.type('rev B: R3 330 Ω → 1 kΩ');
    }

    // The phone, last: every message is in by now.
    phone.launch('messages');
    phone.click(GROUP);
  },
};
