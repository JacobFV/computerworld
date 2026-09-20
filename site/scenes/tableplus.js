export default {
  id: 'tableplus', title: 'Browsing an inventory database in TablePlus on macOS',
  summary: 'Inventory.db twice · orders by quantity, products by stock, the lowest cell picked',
  machines: [{ id: 'mac-tableplus', like: 'alice-mac', size: [1280, 800] }],
  open(pc) {
    const drag = (x0, y0, x1, y1) => [['down', x0, y0], ['move', x1, y1], ['up', x1, y1]]
      .forEach(([op, x, y]) => pc.step('pointer.v1', op, { x, y, width: 1280, height: 800 }));
    // Two windows on the same database, each narrowed to its table by the right edge.
    pc.launch('database', '/Users/alice/Documents/Inventory.db');
    drag(1055, 400, 780, 400);
    pc.click('db:table:orders');
    pc.click('db:sort:2'); pc.click('db:sort:2');
    pc.launch('database', '/Users/alice/Documents/Inventory.db');
    drag(1083, 400, 800, 400);
    drag(440, 115, 880, 190);
    pc.click('db:table:products');
    pc.click('db:tab:browse');
    pc.click('db:sort:5');
    pc.click('db:cell:0:5');
  },
};
