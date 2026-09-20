export default {
  id: 'x', title: 'Posting on X in Safari on macOS',
  summary: 'x.com · "Replay caught a bug today…" posted, then back to the home timeline it now sits at the top of',
  machines: [{ id: 'mac-x', like: 'alice-mac', size: [1280, 800] }],
  open(pc) {
    pc.visit('http://x.com/');
    pc.click('window:0:maximize');
    pc.step('browser.v1', 'fill', { id: 'compose-text', value: 'Replay caught a bug today that three of us had stared at for a week. Determinism pays rent.' });
    pc.step('browser.v1', 'submit', { id: 'compose' });
    pc.click('nav-home');
  },
};
