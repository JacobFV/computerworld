export default {
  id: 'github', title: "Code review on a working forge",
  summary: "github.com/northstar/atlas/pull/15 in Safari on macOS — the sort-refs pull request that closes issue 14, with its reviews, checks and diff.",
  machines: [{ id: 'mac-github', like: 'alice-mac', size: [1280, 800] }],
  open(pc) {
    pc.visit('http://github.com/northstar/atlas/pull/15');
    pc.click('window:0:maximize');
  },
};
