export default {
  id: 'discord', title: "Community support in a busy channel",
  summary: "discord.com/channels/atlas-help in Safari on macOS: the transcript scrolled to its newest message, with an eyes reaction left on chat-33.",
  machines: [{ id: 'mac-discord', like: 'alice-mac', size: [1280, 800] }],
  open(pc) {
    pc.visit('http://discord.com/channels/atlas-help');
    pc.click('window:0:maximize');
    pc.click('chat-33-react-eyes');
    // Discord opens a channel at its newest message: scroll the transcript to the end.
    pc.step('pointer.v1', 'wheel', { x: 640, y: 400, width: 1280, height: 800, delta_y: 220 });
  },
};
