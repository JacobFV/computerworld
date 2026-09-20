export default {
  id: 'assistant', title: 'Asking Claude in Firefox on Ubuntu',
  summary: 'claude.ai in Firefox · a suggestion opened, then "Who owns the Atlas launch?" asked in the composer',
  machines: [{ id: 'ubuntu-claude', like: 'carol-ubuntu', size: [1280, 800] }],
  open(pc) {
    pc.visit('http://claude.ai/');
    pc.click('window:0:maximize');
    pc.click('suggestion-2');
    pc.step('browser.v1', 'fill', { id: 'composer-message', value: 'Who owns the Atlas launch?' });
    pc.step('browser.v1', 'submit', { id: 'composer' });
  },
};
