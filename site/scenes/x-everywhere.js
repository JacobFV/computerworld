// One account, two devices. Alice writes a thread on x.com in Safari on the Mac, and her
// phone — the same person, on the same world's x.com — is open on the timeline that
// thread has just landed in, with its first post at the top. The two machines share a
// world, so the post the Mac makes is the post the phone loads; nothing is staged twice.
//
// x.com is laid out for a desktop — a 275px rail, a 600px column of posts and a 350px
// column of cards, with no narrow layout — so the phone shows the middle of that page,
// which is what a phone really does with a site like it.
export default {
  id: 'x-everywhere', title: 'The same X account on a Mac and an iPhone',
  machines: [
    { id: 'mac-x-thread', like: 'alice-mac', size: [1280, 800], label: "Alice's Mac, writing a thread on X" },
    { id: 'iphone-x-thread', like: 'alice-phone', size: [390, 844], label: "Alice's iPhone, the same timeline" },
  ],
  open(mac, iphone) {
    const fill = (id, value) => mac.step('browser.v1', 'fill', { id, value });
    mac.visit('http://x.com/');
    mac.click('window:0:maximize');
    fill('compose-text', 'The flicker only happened on CI, only in the afternoon, and never once on my machine. ' +
      'Three of us had stared at it for a week. 1/3');
    // Posting lands on the new post's own page, where the box under it continues the thread.
    mac.step('browser.v1', 'submit', { id: 'compose' });
    fill('reply-text', 'Replaying the run from its seed put it on screen in forty seconds: same world, same tick, ' +
      'same flicker, every time. The bug was a hash map we were iterating. 2/3');
    mac.step('browser.v1', 'submit', { id: 'reply' });
    // The third part is still being typed when we look over her shoulder.
    fill('reply-text', 'Nine lines to fix. The week is the part I want back. 3/');
    // Her phone, on the home timeline the thread opens: 1/3 is the post at the top of it.
    iphone.visit('http://x.com/');
    iphone.step('browser.v1', 'scroll', { y: 150 });
  },
};
