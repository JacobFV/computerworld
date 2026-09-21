// Four machines, one picture: the house on the hill, drawn again in each platform's own
// image editor. Bob draws it by hand in Paint, Alice redraws it with shapes in Pixelmator
// Pro, marks the same house onto a sunset photo in Photos on her phone, and Bob sketches
// it once more in Sketchbook on his Pixel.
export default {
  id: 'art-studio', title: "One brief, four drawing tools",
  summary: "Two designers draw the same house four ways — Paint on Windows 11, Pixelmator Pro on macOS, Markup over a photo on an iPhone, Sketchbook on a Pixel — so one asset can be compared across every canvas the team owns.",
  machines: [
    { id: 'windows-art', like: 'bob-windows', size: [1280, 800], label: "Bob's PC, drawing in Paint" },
    { id: 'mac-art', like: 'alice-mac', size: [1280, 800], label: "Alice's Mac, in Pixelmator Pro" },
    { id: 'iphone-art', like: 'alice-phone', size: [390, 844], label: "Alice's iPhone, marking up a photo" },
    { id: 'pixel-art', like: 'bob-android', size: [412, 892], label: "Bob's Pixel, sketching in Sketchbook" },
  ],
  open(pc, mac, iphone, pixel) {
    // ----- Windows: the house drawn by hand, in pencil, and flooded with colour --------
    {
      const ui = target => pc.step('application.v1', 'shell', { target: `window:0:content:paint:${target}` });
      // Image pixels to screen: the 960 x 540 canvas is shown at 96%.
      const at = ([x, y]) => ({ x: Math.round(179 + x * 0.96), y: Math.round(186 + y * 0.96), width: 1280, height: 800 });
      const draw = (...points) => {
        pc.step('pointer.v1', 'down', at(points[0]));
        for (const p of points.slice(1, -1)) pc.step('pointer.v1', 'move', at(p));
        pc.step('pointer.v1', 'up', at(points.at(-1)));
      };
      const flood = (color, point) => { ui('tool:fill'); ui(`fg:${color}`); pc.clickAt(at(point).x, at(point).y); };
      pc.launch('paint');
      pc.click('window:0:maximize');
      // Sky over the whole sheet, then a wobbly horizon with grass under it.
      flood('99d9ea', [12, 12]);
      ui('tool:pencil'); ui('fg:000000'); ui('set:size:5');
      draw([0, 432], [130, 424], [270, 437], [410, 428], [560, 439], [700, 425], [850, 436], [959, 430]);
      flood('22b14c', [60, 505]);
      ui('tool:pencil'); ui('fg:000000');
      // Walls, roof and chimney, each closed so the fill has somewhere to stop.
      draw([300, 430], [299, 340], [302, 250], [460, 246], [620, 251], [618, 340], [620, 430], [460, 433], [300, 430]);
      draw([268, 252], [360, 195], [460, 132], [560, 194], [652, 252], [500, 249], [340, 250], [268, 252]);
      draw([546, 190], [546, 118], [596, 116], [596, 220], [546, 190]);
      // A door and two windows.
      draw([418, 430], [416, 336], [460, 332], [502, 335], [500, 430]);
      draw([338, 300], [398, 298], [400, 358], [340, 360], [338, 300]);
      draw([520, 299], [580, 300], [582, 359], [522, 358], [520, 299]);
      flood('efe4b0', [330, 408]);
      flood('880015', [460, 200]);
      flood('b97a57', [460, 400]);
      flood('b97a57', [570, 150]);
      flood('ffc90e', [368, 328]);
      flood('ffc90e', [550, 328]);
      // Panes, a door knob, the sun and smoke, all in pencil.
      ui('tool:pencil'); ui('fg:000000'); ui('set:size:3');
      draw([368, 299], [369, 359]);
      draw([339, 329], [399, 330]);
      draw([551, 300], [550, 358]);
      draw([521, 329], [581, 330]);
      draw([488, 385], [492, 388]);
      ui('fg:ff7f27'); ui('set:size:5');
      draw(...Array.from({ length: 15 }, (_, i) => {
        const a = i * 2 * Math.PI / 14;
        return [Math.round(824 + 58 * Math.sin(a)), Math.round(124 - 58 * Math.cos(a))];
      }));
      flood('fff200', [824, 124]);
      ui('tool:pencil'); ui('fg:ff7f27'); ui('set:size:5');
      for (const [dx, dy] of [[0, -1], [1, -1], [1, 0], [1, 1], [0, 1], [-1, 1], [-1, 0], [-1, -1]])
        draw([824 + dx * 72, 124 + dy * 72], [824 + dx * 104, 124 + dy * 104]);
      ui('fg:c3c3c3'); ui('set:size:6');
      draw([572, 112], [556, 84], [588, 62], [566, 34], [596, 14]);
    }

    // ----- macOS: the same house again, this time with the shape tools ----------------
    {
      const ui = target => mac.step('application.v1', 'shell', { target: `window:0:content:pixelmator:${target}` });
      // Image pixels to screen: the 800 x 600 image is shown at 97%.
      const at = ([x, y]) => ({ x: Math.round(232 + x * 0.97), y: Math.round(126 + y * 0.97), width: 1280, height: 800 });
      const drag = (from, to) => { mac.step('pointer.v1', 'down', at(from)); mac.step('pointer.v1', 'up', at(to)); };
      const shape = (kind, fill, from, to) => { ui(`shape:${kind}`); ui(`bg:${fill}`); drag(from, to); };
      mac.launch('pixelmator');
      mac.click('window:0:maximize');
      ui('dialog:new-image'); ui('set:width:800'); ui('set:height:600'); ui('apply');
      ui('fg:cfe9ff'); ui('bg:6cb6e8'); ui('tool:gradient'); drag([400, 0], [400, 430]);
      ui('tool:shape'); ui('fill-style:fill');
      shape('rectangle', '7cc36a', [0, 415], [799, 599]);
      shape('ellipse', 'ffd93d', [640, 40], [760, 160]);
      shape('rectangle', '8a5a3b', [452, 120], [488, 210]);
      shape('rectangle', 'f2e0b6', [230, 260], [570, 470]);
      shape('triangle', 'c0392b', [190, 262], [610, 132]);   // dragged upwards, eaves to ridge
      shape('rectangle', '8a5a3b', [370, 370], [430, 470]);
      shape('rectangle', 'ffeaa0', [268, 300], [336, 360]);
      shape('rectangle', 'ffeaa0', [464, 300], [532, 360]);
      ui('fill-style:outline'); ui('fg:6b4a32'); ui('set:size:3');
      shape('line', 'ffffff', [302, 300], [302, 360]);
      shape('line', 'ffffff', [268, 330], [336, 330]);
      shape('line', 'ffffff', [498, 300], [498, 360]);
      shape('line', 'ffffff', [464, 330], [532, 330]);
      ui('fill-style:fill'); ui('tool:shape');
      shape('ellipse', '4f9e46', [90, 430], [180, 490]);
      shape('ellipse', '4f9e46', [640, 440], [740, 500]);
      shape('ellipse', '62b455', [130, 415], [200, 470]);
      // A curl of smoke, left half-drawn with the brush in hand.
      ui('tool:brush'); ui('fg:dfe6ea'); ui('set:size:9');
      mac.step('pointer.v1', 'down', at([470, 112]));
      for (const p of [[452, 78], [486, 52], [462, 22]]) mac.step('pointer.v1', 'move', at(p));
      mac.step('pointer.v1', 'up', at([492, 4]));
    }

    // ----- iPhone: the house drawn onto a photo, in Markup ----------------------------
    {
      // Nothing ships in Pictures; an APNG clip is also a valid PNG of its first frame.
      iphone.sh('mkdir -p ~/Pictures && cp ~/Movies/Sunset.apng ~/Pictures/Sunset.png');
      iphone.launch('photos');
      iphone.click('photos:open:Sunset.png');
      iphone.click('photos:begin-edit:ios');
      iphone.click('photos:edit:tab:markup');
      const ui = target => iphone.step('application.v1', 'shell', { target: `window:0:content:photos:edit:${target}` });
      // The 320 x 180 photo sits at (35, 315) on the screen.
      const at = ([x, y]) => ({ x: 35 + x, y: 315 + y, width: 390, height: 844 });
      const draw = (...points) => {
        iphone.step('pointer.v1', 'down', at(points[0]));
        for (const p of points.slice(1, -1)) iphone.step('pointer.v1', 'move', at(p));
        iphone.step('pointer.v1', 'up', at(points.at(-1)));
      };
      ui('tool:marker'); ui('color:000000'); ui('set:size:6');
      draw([42, 118], [42, 62], [138, 62], [138, 118], [42, 118]);
      draw([26, 64], [90, 22], [154, 64]);
      draw([76, 118], [76, 86], [104, 86], [104, 118]);
      ui('color:ffcc00'); ui('set:size:4');
      draw([52, 74], [68, 74], [68, 90], [52, 90], [52, 74]);
      draw([112, 74], [128, 74], [128, 90], [112, 90], [112, 74]);
    }

    // ----- Android: the same house, sketched on the phone ----------------------------
    {
      const ui = target => pixel.step('application.v1', 'shell', { target: `window:0:content:sketchbook:${target}` });
      const at = ([x, y]) => ({ x, y, width: 412, height: 892 });
      const draw = (...points) => {
        pixel.step('pointer.v1', 'down', at(points[0]));
        for (const p of points.slice(1, -1)) pixel.step('pointer.v1', 'move', at(p));
        pixel.step('pointer.v1', 'up', at(points.at(-1)));
      };
      pixel.launch('sketchbook');
      ui('dialog:new-image'); ui('set:width:360'); ui('set:height:640'); ui('apply');
      ui('tool:fill'); ui('color:bfe5f7'); draw([200, 300]);
      ui('tool:brush'); ui('color:ffd34d'); ui('set:size:64');
      draw([320, 250]);
      // The ground line stops at the walls, so the house stands on it rather than in front
      // of it; the whole outline is down before anything is flooded.
      ui('tool:pencil'); ui('color:1d1d1d'); ui('set:size:5');
      draw([22, 666], [62, 658], [104, 664]);
      draw([308, 664], [350, 656], [390, 666]);
      draw([104, 664], [102, 528], [306, 526], [308, 664], [104, 664]);
      draw([78, 528], [206, 424], [334, 528], [78, 528]);
      draw([172, 664], [170, 580], [242, 578], [244, 664]);
      draw([126, 556], [170, 556], [170, 600], [126, 600], [126, 556]);
      draw([250, 556], [286, 556], [286, 600], [250, 600], [250, 556]);
      ui('tool:fill'); ui('color:8ecf72'); draw([60, 760]);
      ui('color:f6e3b0'); draw([118, 630]);
      ui('color:c0503c'); draw([206, 490]);
      ui('color:a9714a'); draw([206, 630]);
      ui('color:ffd34d'); draw([148, 578]);
      ui('color:ffd34d'); draw([268, 578]);
      // A cloud, still under the brush when we look in.
      ui('tool:brush'); ui('color:ffffff'); ui('set:size:26');
      draw([96, 272], [128, 268], [164, 272]);
      draw([112, 252], [140, 250], [158, 258]);
    }
  },
};
