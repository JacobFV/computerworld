export default {
  id: 'code-debug', title: "Stepping through Python inside the world",
  summary: "VS Code on macOS stopped on a breakpoint in ~/orders/invoice.py, inside line_total on the second order, with a watch on quantity * price * discount.",
  machines: [{ id: 'mac-debug', like: 'alice-mac', size: [1280, 800] }],
  open(pc) {
    pc.sh("mkdir -p /Users/alice/orders && printf '" + [
      'ORDERS = [',
      '    ("Northstar Cafe", "espresso beans", 12, 18.5),',
      '    ("Harbor Books", "gift cards", 40, 25.0),',
      '    ("Northstar Cafe", "oat milk", 30, 3.2),',
      '    ("Pine Street Gym", "towels", 75, 4.8),',
      ']',
      '',
      '',
      'def line_total(quantity, price, discount):',
      '    gross = quantity * price',
      '    saved = round(gross * discount, 2)',
      '    return gross - saved',
      '',
      '',
      'def invoice(orders):',
      '    totals = {}',
      '    for customer, item, quantity, price in orders:',
      '        discount = 0.1 if quantity >= 30 else 0.0',
      '        amount = line_total(quantity, price, discount)',
      '        totals[customer] = totals.get(customer, 0) + amount',
      '    return totals',
      '',
      '',
      'for name, owed in invoice(ORDERS).items():',
      '    print(name, owed)',
    ].join('\\n') + "\\n' > /Users/alice/orders/invoice.py");
    pc.launch('code', '/Users/alice/orders');
    pc.step('application.v1', 'maximize', {});
    pc.click('code:tree:invoice.py');
    pc.click('code:cmd:python.execInTerminal');
    pc.click('code:gutter:0:12');
    pc.key('F5');
    pc.click('code:cmd:workbench.action.debug.continue'); // on to the second order, the discounted one
    pc.click('code:cmd:workbench.debug.viewlet.action.addWatchExpression');
    pc.type('quantity * price * discount');
    pc.key('Enter');
    pc.click('code:panel:terminal');
  },
};
