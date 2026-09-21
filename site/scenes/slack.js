export default {
  id: 'slack', title: "A flaky test, argued out in #eng",
  summary: "northstar.slack.com is open in Edge on Windows 11 with thread chat-1 in the side pane and a reply about the flaky test half typed into it.",
  machines: [{ id: 'windows-slack', like: 'bob-windows', size: [1280, 800] }],
  open(pc) {
    // #eng with the flaky-test thread open in the right-hand pane, a reply half typed.
    pc.visit('http://northstar.slack.com/archives/eng?thread=chat-1');
    pc.click('window:0:maximize');
    pc.step('browser.v1', 'fill', { id: 'chat-1-reply-body', value: 'Pinned the runner to one core and it still fails, so it is not a race. Trying the' });
  },
};
