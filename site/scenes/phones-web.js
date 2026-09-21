export default {
  id: 'phones-web', title: "The same web, on a small screen",
  summary: "Deterministic_simulation in Safari on an iPhone and a reddit.com thread in Chrome on a Pixel — one engine, at phone breakpoints.",
  machines: [
    { id: 'iphone-safari', like: 'alice-phone', size: [390, 844], label: "Alice's iPhone" },
    { id: 'pixel-chrome', like: 'bob-android', size: [412, 892], label: "Bob's Pixel" },
  ],
  open(iphone, pixel) {
    iphone.visit('http://en.wikipedia.org/wiki/Deterministic_simulation');
    pixel.visit('http://reddit.com/');
    pixel.click('t-5115-open');
  },
};
