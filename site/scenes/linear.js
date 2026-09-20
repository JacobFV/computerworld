export default {
  id: 'linear', title: 'Linear in Firefox on Ubuntu',
  summary: 'linear.app/projects/OPS · "Dry-run the rollback before launch day" filed, back on the board',
  machines: [{ id: 'ubuntu-linear', like: 'carol-ubuntu', size: [1280, 800] }],
  open(pc) {
    pc.visit('http://linear.app/projects/OPS');
    pc.click('window:0:maximize');
    pc.step('browser.v1', 'fill', { id: 'new-issue-title', value: 'Dry-run the rollback before launch day' });
    pc.step('browser.v1', 'fill', { id: 'new-issue-body', value: 'Roll staging back to 0.9 and forward again, and time both directions.' });
    pc.step('browser.v1', 'submit', { id: 'new-issue' });
    pc.click('back');
  },
};
