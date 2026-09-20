export default {
  id: 'gdocs', title: 'Google Docs in Edge on Windows',
  summary: 'docs.google.com/documents/atlas-launch in Edge — the same launch plan, this time in a browser',
  machines: [{ id: 'windows-gdocs', like: 'bob-windows', size: [1280, 800] }],
  open(pc) {
    pc.visit('http://docs.google.com/documents/atlas-launch');
    pc.click('window:0:maximize');
  },
};
