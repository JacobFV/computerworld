export default {
  id: 'mail', title: 'Replying to a message in Outlook on Windows 11',
  machines: [{ id: 'windows-mail', like: 'bob-windows', size: [1280, 800] }],
  open(pc) {
    pc.launch('mail');
    pc.click('window:0:maximize');
    pc.click('mail:open:mail-1');
    pc.click('mail:reply');
    pc.type('Carol, the CI section is done: the release job is green on the candidate and launch.txt validates against ATLAS-2026. I will update OPS-1 once the rollback fix');
  },
};
