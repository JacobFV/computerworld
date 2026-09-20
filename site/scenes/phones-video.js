export default {
  id: 'phones-video', title: 'iMovie on an iPhone, a video editor on a Pixel',
  summary: '"Golden hour at the bay" cut on the iPhone, "Atlas 1.0 launch night" on the Pixel',
  machines: [
    { id: 'iphone-imovie', like: 'alice-phone', size: [390, 844], label: "Alice's iPhone" },
    { id: 'pixel-video', like: 'bob-android', size: [412, 892], label: "Bob's Pixel" },
  ],
  open(iphone, pixel) {
    const edit = (phone, kind, clips, music, title, frame) => {
      const press = control => phone.step('application.v1', 'shell', { target: `window:0:content:video:${control}` });
      const add = (clip, where) => { press(where); press('import'); press(`pick-add:${clip}`); };
      phone.launch(kind);
      // The picker closes after each pick, and a clip lands at the playhead.
      clips.forEach(clip => add(clip, 'end'));
      add(music, 'start');
      press('start'); press('add-title:lower'); press('title-text');
      for (const _ of 'Your Name') phone.key('Backspace'); // the placeholder
      phone.type(title);
      press('deselect'); press(`seek:${frame}`);
    };
    edit(iphone, 'imovie', ['Sunset.apng', 'Countdown.apng'], 'Music Bed.wav', 'Golden hour at the bay', 20);
    edit(pixel, 'videoeditor', ['Countdown.apng', 'Sunset.apng', 'Color Bars.apng'], 'Countdown Beeps.wav', 'Atlas 1.0 launch night', 40);
  },
};
