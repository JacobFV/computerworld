export default {
  id: 'bank', title: "Online banking as a site, not a mock",
  summary: "northwind.example opens in Safari on macOS — a real HTML banking site served over the world's own network, with accounts and balances, before any money is moved.",
  machines: [{ id: 'mac-bank', like: 'alice-mac', size: [1280, 800] }],
  open(pc) {
    pc.visit('http://northwind.example/');
    pc.click('window:0:maximize');
  },
};
