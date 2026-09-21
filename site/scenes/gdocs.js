export default {
  id: 'gdocs', title: "The same document, this time on the web",
  summary: "The launch plan again, opened from docs.google.com in Edge on Windows 11 — a web app parsed, cascaded and laid out by the engine's own browser.",
  machines: [{ id: 'windows-gdocs', like: 'bob-windows', size: [1280, 800] }],
  open(pc) {
    pc.visit('http://docs.google.com/documents/atlas-launch');
    pc.click('window:0:maximize');
  },
};
