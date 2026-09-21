export default {
  id: 'assistant', title: "Asking an assistant about your own world",
  summary: "claude.ai opens in Firefox on Ubuntu; a suggestion is taken, then “Who owns the Atlas launch?” is typed into the composer — an assistant site inside the simulation.",
  machines: [{ id: 'ubuntu-claude', like: 'carol-ubuntu', size: [1280, 800] }],
  open(pc) {
    pc.visit('http://claude.ai/');
    pc.click('window:0:maximize');
    pc.click('suggestion-2');
    pc.step('browser.v1', 'fill', { id: 'composer-message', value: 'Who owns the Atlas launch?' });
    pc.step('browser.v1', 'submit', { id: 'composer' });
  },
};
