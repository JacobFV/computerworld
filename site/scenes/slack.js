export default {
  id: 'slack', title: 'Slack in Edge on Windows',
  machines: [{ id: 'windows-slack', like: 'bob-windows', size: [1280, 800] }],
  open(pc) {
    pc.visit('http://northstar.slack.com/archives/eng');
    pc.click('window:0:maximize');
    pc.step('browser.v1', 'fill', { id: 'chat-1-reply-text', value: 'Pinned the runner to one core and it still fails, so it is not a race. Trying the' });
  },
};
