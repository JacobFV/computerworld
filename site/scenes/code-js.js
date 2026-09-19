export default {
  id: 'code-js', title: 'A Node project in VS Code on Windows 11',
  machines: [{ id: 'windows-node', like: 'bob-windows', size: [1280, 800] }],
  open(pc) {
    const root = 'C:/Users/bob/stockroom';
    const write = (path, lines) => pc.step('filesystem.v1', 'write', { path: `${root}/${path}`, content: lines.join('\n') + '\n' });
    pc.sh(`mkdir -p ${root}/src ${root}/data`);
    write('package.json', [
      '{',
      '  "name": "stockroom",',
      '  "version": "0.3.0",',
      '  "description": "Reorder report for the Northstar warehouse",',
      '  "main": "src/index.js",',
      '  "scripts": { "start": "node src/index.js" }',
      '}',
    ]);
    write('data/stock.json', [
      '[',
      '  { "sku": "NS-1001", "name": "Label printer", "onHand": 4, "reorderAt": 6, "unitCost": 129.0 },',
      '  { "sku": "NS-1002", "name": "Thermal labels (roll)", "onHand": 180, "reorderAt": 60, "unitCost": 3.4 },',
      '  { "sku": "NS-1003", "name": "Barcode scanner", "onHand": 2, "reorderAt": 5, "unitCost": 84.5 },',
      '  { "sku": "NS-1004", "name": "Packing tape", "onHand": 35, "reorderAt": 40, "unitCost": 2.15 },',
      '  { "sku": "NS-1005", "name": "Pallet wrap", "onHand": 22, "reorderAt": 10, "unitCost": 17.9 }',
      ']',
    ]);
    write('src/format.js', [
      "const money = value => '$' + value.toFixed(2);",
      '',
      'const pad = (text, width) => String(text).padEnd(width);',
      '',
      'module.exports = { money, pad };',
    ]);
    write('src/reorder.js', [
      '// Anything at or below its reorder point is topped up to twice that point.',
      'function reorderList(stock) {',
      '  return stock',
      '    .filter(item => item.onHand <= item.reorderAt)',
      '    .map(item => {',
      '      const quantity = item.reorderAt * 2 - item.onHand;',
      '      return { ...item, quantity, cost: quantity * item.unitCost };',
      '    })',
      '    .sort((a, b) => b.cost - a.cost);',
      '}',
      '',
      'module.exports = { reorderList };',
    ]);
    write('src/index.js', [
      "const fs = require('fs');",
      "const path = require('path');",
      "const { reorderList } = require('./reorder');",
      "const { money, pad } = require('./format');",
      '',
      "const file = path.join(__dirname, '..', 'data', 'stock.json');",
      "const stock = JSON.parse(fs.readFileSync(file, 'utf8'));",
      'const orders = reorderList(stock);',
      '',
      'console.log(`${orders.length} of ${stock.length} items need reordering\\n`);',
      'for (const { sku, name, quantity, cost } of orders) {',
      '  console.log(pad(sku, 9) + pad(name, 18) + pad(`x${quantity}`, 6) + money(cost));',
      '}',
      'const total = orders.reduce((sum, order) => sum + order.cost, 0);',
      "console.log('\\nPurchase order total: ' + money(total));",
    ]);
    pc.launch('code', root);
    pc.click('window:0:maximize');
    pc.click('code:tree:src');
    pc.click('code:tree:src/reorder.js');
    pc.click('code:tree:src/index.js');
    pc.click('code:cmd:workbench.action.terminal.runActiveFile');
  },
};
