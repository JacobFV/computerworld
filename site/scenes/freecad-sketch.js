export default {
  id: 'freecad-sketch', title: "Constraint solving in a sketch",
  summary: "FreeCAD's Sketcher on Windows 11 drives an 80 × 50 outline symmetric about the origin, a ⌀24 bore and two equal ⌀8 holes 12 mm in — solved, not drawn.",
  machines: [{ id: 'windows-freecad', like: 'bob-windows', size: [1280, 800] }],
  open(pc) {
    const ui = target => pc.step('application.v1', 'shell', { target: `window:0:content:freecad:${target}` });
    // A new sketch looks straight down at 120 mm of the XY plane, centred in the 3D view.
    const at = (x, y) => pc.clickAt(Math.round(802 + x * 5.258), Math.round(407 - y * 5.258));
    // Picks accumulate in the Sketcher; a click on empty paper drops them.
    const constrain = (kind, picks, value) => {
      at(50, 32);
      for (const pick of picks) typeof pick === 'number' ? ui(`sk:element:${pick}`) : at(...pick);
      ui(`cmd:Sketcher_Constrain${kind}`);
      if (value) { pc.type(value); pc.key('Enter'); }
    };
    pc.launch('freecad');
    pc.click('window:0:maximize');
    ui('cmd:PartDesign_NewSketch'); ui('task:ok');
    ui('cmd:Sketcher_CreateRectangle'); at(-40, -25); at(40, 25); pc.key('Escape');
    ui('cmd:Sketcher_CreateCircle'); at(0, 0); at(12, 0); at(-28, -13); at(-24, -13); at(28, 13); at(32, 13); pc.key('Escape');
    // Positions first: a diameter's label sits on the centre it would hide.
    constrain('Symmetric', [[-40, -25], [40, 25], [0, 0]]);
    constrain('Symmetric', [[-28, -13], [28, 13], [0, 0]]);
    constrain('DistanceX', [[-40, -25], [-28, -13]], '12');
    constrain('DistanceY', [[-40, -25], [-28, -13]], '12');
    constrain('DistanceX', [0], '80');
    constrain('DistanceY', [1], '50');
    constrain('Equal', [5, 6]);
    constrain('Diameter', [4], '24');
    constrain('Diameter', [5], '8');
    ui('cmd:Sketcher_ViewSketch');
  },
};
