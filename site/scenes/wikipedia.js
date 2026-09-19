export default {
  id: 'wikipedia', title: 'Reading Wikipedia on Ubuntu',
  machines: [{ id: 'ubuntu-wikipedia', like: 'carol-ubuntu', size: [1280, 800] }],
  open(pc) {
    pc.visit('http://en.wikipedia.org/wiki/Deterministic_simulation');
    pc.click('window:0:maximize');
    pc.step('browser.v1', 'scroll', { y: 300 });
  },
};
