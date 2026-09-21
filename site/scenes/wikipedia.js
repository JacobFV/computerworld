export default {
  id: 'wikipedia', title: "A long article, laid out and scrolled",
  summary: "en.wikipedia.org/wiki/Deterministic_simulation in a browser on Ubuntu, scrolled into the body — real HTML and CSS through the engine's own layout.",
  machines: [{ id: 'ubuntu-wikipedia', like: 'carol-ubuntu', size: [1280, 800] }],
  open(pc) {
    pc.visit('http://en.wikipedia.org/wiki/Deterministic_simulation');
    pc.click('window:0:maximize');
    pc.step('browser.v1', 'scroll', { y: 300 });
  },
};
