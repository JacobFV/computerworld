// The business side of the same company, on one set of figures: the Q3 budget
// (Budget.xlsx), the mail Bob sends Carol about it, and the September power bill
// paid at the bank. The quarter comes to $8,738.85, September to $2,898.65, and the
// September utilities line — $131.10 — is the payment on Alice's statement.
export default {
  id: 'office-team', title: 'Three machines, one set of figures',
  machines: [
    { id: 'office-windows-excel', like: 'bob-windows', size: [1280, 800], label: "Bob's PC · the quarter in Excel" },
    { id: 'office-ubuntu-mail', like: 'carol-ubuntu', size: [1280, 800], label: "Carol's ThinkPad · replying about the figures" },
    { id: 'office-mac-bank', like: 'alice-mac', size: [1280, 800], label: "Alice's Mac · the power bill at the bank" },
  ],
  open(pc, thinkpad, mac) {
    // The mail window is opened and closed first, so the workbook is window 1.
    const sheet = command => pc.step('application.v1', 'shell', { target: `window:1:content:sheet:${command}` });
    const field = name => pc.step('application.v1', 'shell', { target: `window:0:content:mail:field:${name}` });

    // Bob mails Carol the quarter out of the workbook, then goes back to it.
    pc.launch('mail');
    pc.click('mail:compose');
    pc.type('carol');
    field('subject');
    pc.type('Q3 comes to $8,738.85');
    field('body');
    pc.type('Carol, Budget.xlsx is closed for the quarter. July $2,909.55, August $2,930.65, September $2,898.65, ' +
      'so Q3 is $8,738.85 and the monthly average $2,912.95. Rent is $5,550 of it, 63.5% of the quarter. ' +
      'The one line still outstanding is September utilities, $131.10 to Cascade Power. Alice is paying it today.');
    pc.step('application.v1', 'shell', { target: 'window:0:content:mail:send' });
    pc.click('window:0:close');

    pc.launch('spreadsheet', 'C:/Users/bob/Documents/Budget.xlsx');
    pc.click('window:1:maximize');
    sheet('select:A1:D7');
    sheet('chart:column');
    sheet('zoom:120');
    sheet('select:D3');

    // Carol reads it and is half way through her reply.
    thinkpad.launch('mail');
    thinkpad.click('window:0:maximize');
    thinkpad.click('mail:open:mail-2');
    thinkpad.click('mail:reply');
    thinkpad.type('Thanks Bob. $8,738.85 against the $9,000 we set for Q3, so we come in $261.15 under. ' +
      'I will sign off September once the $131.10 to Cascade Power shows on the statement. One question about rent:');

    // Alice pays that line, from the account it comes out of.
    mac.visit('http://northwind.example/transfers');
    mac.click('window:0:maximize');
    mac.step('browser.v1', 'fill', { id: 'pay-account', value: 'chk-4417' });
    mac.step('browser.v1', 'fill', { id: 'pay-payee', value: 'cascade-power' });
    mac.step('browser.v1', 'fill', { id: 'pay-amount', value: '13110' });
    mac.step('browser.v1', 'submit', { id: 'pay' });
    mac.visit('http://northwind.example/statements/chk-4417/all');
  },
};
