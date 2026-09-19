export default {
  id: 'x', title: 'Posting on X in Safari on macOS',
  machines: [{ id: 'mac-x', like: 'alice-mac', size: [1280, 800] }],
  open(pc) {
    pc.visit('http://x.com/');
    pc.click('window:0:maximize');
    pc.step('browser.v1', 'fill', { id: 'compose-text', value: 'Replay caught a bug today that three of us had stared at for a week. Determinism pays rent.' });
    pc.step('browser.v1', 'submit', { id: 'compose' });
    pc.click('nav-home');
  },
};
