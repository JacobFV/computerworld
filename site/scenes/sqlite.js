export default {
  id: 'sqlite', title: 'Querying an inventory in DB Browser for SQLite on Ubuntu',
  machines: [{ id: 'ubuntu-sqlite', like: 'carol-ubuntu', size: [1280, 800] }],
  open(pc) {
    pc.launch('database', '/home/carol/Documents/Inventory.db');
    pc.click('window:0:maximize');
    pc.click('db:tab:execute');
    pc.click('db:sql');
    [
      '-- What sells, who supplies it, and how many weeks of stock are left?',
      'SELECT p.sku, p.name AS product, s.name AS supplier,',
      '       sum(o.quantity) AS units_sold,',
      '       round(sum(o.quantity * p.price), 2) AS revenue,',
      '       p.stock,',
      '       round(p.stock * 13.0 / sum(o.quantity), 1) AS weeks_left',
      'FROM products p',
      'JOIN suppliers s ON s.id = p.supplier_id',
      'JOIN orders o ON o.product_id = p.id',
      'GROUP BY p.id',
      'ORDER BY weeks_left;',
    ].forEach((line, i) => { if (i) pc.key('Enter'); pc.type(line); });
    pc.click('db:run');
  },
};
