export default {
  id: 'discord', title: 'Discord in Safari on macOS',
  machines: [{ id: 'mac-discord', like: 'alice-mac', size: [1280, 800] }],
  open(pc) {
    pc.visit('http://discord.com/channels/atlas-help');
    pc.click('window:0:maximize');
    pc.click('chat-33-react-eyes');
  },
};
