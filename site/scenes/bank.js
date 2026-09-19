export default {
  id: 'bank', title: 'Northwind online banking in Safari on macOS',
  machines: [{ id: 'mac-bank', like: 'alice-mac', size: [1280, 800] }],
  open(pc) {
    pc.visit('http://northwind.example/');
    pc.click('window:0:maximize');
  },
};
