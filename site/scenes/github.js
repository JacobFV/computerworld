export default {
  id: 'github', title: 'A GitHub pull request in Safari on macOS',
  machines: [{ id: 'mac-github', like: 'alice-mac', size: [1280, 800] }],
  open(pc) {
    pc.visit('http://github.com/northstar/atlas/pull/15');
    pc.click('window:0:maximize');
  },
};
