export default {
  id: 'phones-music', title: 'Apple Music on an iPhone, YouTube Music on a Pixel',
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
