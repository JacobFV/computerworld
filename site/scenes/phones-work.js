export default {
  id: 'phones-work', title: "Desk work, finished on a phone",
  summary: "“August budget is up $21” sent from an iPhone with Budget.xlsx reopened on the August SUM, and the mail read on a Pixel.",
  machines: [
    { id: 'iphone-numbers', like: 'alice-phone', size: [390, 844], label: "Alice's iPhone" },
    { id: 'pixel-mail', like: 'bob-android', size: [412, 892], label: "Bob's Pixel" },
  ],
  open(iphone, pixel) {
    // Alice mails Bob about the budget, then goes back to the numbers; Bob reads it.
    // The keyboard covers the composer, so its fields are pressed by name.
    const press = control => iphone.step('application.v1', 'shell', { target: `window:0:content:mail:${control}` });
    iphone.launch('mail');
    iphone.click('mail:compose');
    iphone.type('bob');
    press('field:subject');
    iphone.type('August budget is up $21');
    press('field:body');
    iphone.type('Hi Bob, I went through Budget.xlsx on the train. August comes to $2,930.65 against $2,909.55 in July. ' +
      'Groceries (+$26.55), utilities (+$14.45) and transport (+$14.50) went up; entertainment came down $34.40. ' +
      'Rent and insurance did not move. Can you check the utilities bill before Friday? It looks high for the summer. Thanks, Alice');
    press('send');
    iphone.launch('spreadsheet', '/var/mobile/Documents/Budget.xlsx');
    iphone.clickAt(330, 438); // the August total, so the formula bar shows its SUM
    pixel.launch('mail');
    pixel.click('mail:open:mail-2');
  },
};
