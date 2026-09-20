export default {
  id: 'phones-photos', title: 'Photos on an iPhone, Sketchbook on a Pixel',
  summary: 'Sunset.png under the vivid-warm filter, the same view drawn 360 × 640 on the Pixel',
  machines: [
    { id: 'iphone-photos', like: 'alice-phone', size: [390, 844], label: "Alice's iPhone" },
    { id: 'pixel-sketch', like: 'bob-android', size: [412, 892], label: "Bob's Pixel" },
  ],
  open(iphone, pixel) {
    // Nothing ships in Pictures; an APNG clip is also a valid PNG of its first frame.
    iphone.sh('mkdir -p ~/Pictures && cp ~/Movies/Sunset.apng ~/Pictures/Sunset.png && cp ~/Movies/Countdown.apng ~/Pictures/Countdown.png && cp "$HOME/Movies/Color Bars.apng" ~/Pictures/Bars.png');
    iphone.launch('photos');
    iphone.click('photos:open:Sunset.png');
    iphone.click('photos:begin-edit:ios');
    iphone.click('photos:edit:tab:filters');
    iphone.click('photos:edit:preset:vivid-warm');
    pixel.launch('sketchbook');
    const [width, height] = [412, 892];
    const pen = (op, [x, y]) => pixel.step('pointer.v1', op, { x, y, width, height });
    const stroke = points => { pen('down', points[0]); points.slice(1).forEach(at => pen('move', at)); pen('up', points.at(-1)); };
    const set = control => pixel.step('application.v1', 'shell', { target: `window:0:content:sketchbook:${control}` });
    set('dialog:new-image'); set('set:width:360'); set('set:height:640'); set('apply');
    set('tool:fill'); set('color:fde3b5'); stroke([[200, 300]]);
    set('tool:brush'); set('color:f7b733'); set('set:size:96'); stroke([[286, 360]]);
    // Two ridges, each drawn edge to edge and then flooded below.
    set('color:8a6fa8'); set('set:size:5');
    stroke([[10, 520], [70, 470], [120, 505], [190, 410], [250, 490], [300, 450], [400, 540]]);
    set('tool:fill'); stroke([[200, 700]]);
    set('tool:brush'); set('color:3d3b6e');
    stroke([[10, 640], [90, 570], [150, 620], [230, 540], [310, 630], [360, 600], [400, 650]]);
    stroke([[92, 258], [116, 272], [140, 256]]);
    stroke([[160, 312], [178, 322], [196, 310]]);
    set('tool:fill'); stroke([[200, 780]]);
    set('tool:brush'); set('color:f7b733'); // left ready to draw
  },
};
