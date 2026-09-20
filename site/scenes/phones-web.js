export default {
  id: 'phones-web', title: 'Safari on an iPhone, Chrome on a Pixel',
  summary: 'Deterministic_simulation in Safari on the iPhone, a reddit.com thread opened in Chrome on the Pixel',
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
