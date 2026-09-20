export default {
  id: 'freecad-part', title: 'A Part Design body in FreeCAD on macOS',
  summary: 'An 80 × 50 plate padded 10 mm under a ⌀28 boss padded 26, bored ⌀16, four ⌀7 holes',
  machines: [{ id: 'mac-freecad', like: 'alice-mac', size: [1280, 800] }],
  open(pc) {
    const ui = target => pc.step('application.v1', 'shell', { target: `window:0:content:freecad:${target}` });
    // A new sketch looks straight down at 120 mm of the XY plane, centred in the 3D view.
    const at = (x, y) => pc.clickAt(Math.round(802 + x * 4.958), Math.round(399 - y * 4.958));
    const enter = text => { pc.type(text); pc.key('Enter'); };
    const sketch = (tool, ...points) => {
      ui('cmd:PartDesign_NewSketch'); ui('task:ok'); ui(`cmd:Sketcher_Create${tool}`);
      for (const [x, y] of points) at(x, y);
      pc.key('Escape');
    };
    pc.launch('freecad');
    pc.click('window:0:maximize');
    // A bored boss: two circles on the origin, padded 26 mm.
    sketch('Circle', [0, 0], [14, 0], [0, 0], [8, 0]);
    ui('sk:close');
    ui('cmd:PartDesign_Pad'); ui('field:task:Length'); enter('26'); ui('task:ok');
    // The plate under it: 80 x 50 with the bore and four mounting holes, padded 10 mm.
    sketch('Rectangle', [-40, -25], [40, 25]);
    ui('sk:element:0'); ui('cmd:Sketcher_ConstrainDistanceX'); enter('80');
    ui('sk:element:1'); ui('cmd:Sketcher_ConstrainDistanceY'); enter('50');
    ui('cmd:Sketcher_CreateCircle');
    for (const [x, y] of [[0, 0], [-30, -15], [30, -15], [-30, 15], [30, 15]]) { at(x, y); at(x + (x ? 3.5 : 8), y); }
    pc.key('Escape'); ui('sk:close');
    ui('cmd:PartDesign_Pad'); ui('task:ok');
    ui('cmd:Std_ViewIsometric'); ui('cmd:Std_ViewFitAll');
    ui('tree-toggle:Pad'); ui('tree-toggle:Pad001'); ui('tree:Sketch001');
  },
};
