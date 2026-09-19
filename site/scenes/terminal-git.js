export default {
  id: 'terminal-git', title: 'A git session in Terminal on Ubuntu',
  machines: [{ id: 'ubuntu-terminal', like: 'carol-ubuntu', size: [1280, 800] }],
  open(pc) {
    pc.launch('terminal');
    pc.click('window:0:maximize');
    [
      'cd ~/project && git init',
      'git add . && git commit -m "Import the project"',
      'git switch -c greeting',
      "sed -i 's/world/Northstar/' main.py && python3 main.py",
      'git status',
      'git commit -am "Greet Northstar by name"',
      'git log',
      "awk -F, 'NR > 1 { sold[$2] += $4 } END { for (r in sold) print r, sold[r] }' ~/Documents/Sales.csv | sort -k2 -nr",
      'sqlite3 ~/Documents/Inventory.db "SELECT sku, name, stock FROM low_stock"',
    ].forEach(line => { pc.type(line); pc.key('Enter'); });
  },
};
