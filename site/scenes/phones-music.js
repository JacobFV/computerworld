export default {
  id: 'phones-music', title: "Playback that runs on the world clock",
  summary: "Cold Start playing at 6:20 in Apple Music on an iPhone while Warm Cache starts in YouTube Music on a Pixel: the position comes from logical time, never the host's.",
  machines: [
    { id: 'iphone-music', like: 'alice-phone', size: [390, 844], label: "Alice's iPhone" },
    { id: 'pixel-music', like: 'bob-android', size: [412, 892], label: "Bob's Pixel" },
  ],
  open(iphone, pixel) {
    // Controls are pressed by name: the album tiles sit half off the edge of a phone.
    const press = (phone, control) => phone.step('application.v1', 'shell', { target: `window:0:content:music:${control}` });
    iphone.launch('music');
    press(iphone, 'album:cold-start');
    press(iphone, 'play:album:cold-start');
    press(iphone, 'seek:380');
    press(iphone, 'expand');
    pixel.launch('music');
    press(pixel, 'album:cold-reads');
    press(pixel, 'play:album:cold-reads@warm-cache');
  },
};
