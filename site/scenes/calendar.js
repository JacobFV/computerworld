export default {
  id: 'calendar', title: 'Launch week in Calendar on macOS',
  machines: [{ id: 'mac-calendar', like: 'alice-mac', size: [1280, 800] }],
  open(pc) {
    const cal = command => pc.step('application.v1', 'shell', { target: `window:0:content:cal:${command}` });
    // Day 0 is today, Thursday 17 September; new events start at 9:00 and last an hour.
    const event = (day, hour, hours, title) => {
      cal(`day:${day}`); cal('new');
      for (let h = 9; h < hour; h++) cal('later');
      for (let h = 1; h < hours; h++) cal('longer');
      pc.type(title); cal('save');
    };
    pc.launch('calendar');
    pc.click('window:0:maximize');
    event(0, 14, 1, 'Pricing page with Carol');
    event(1, 10, 1, 'Rollback rehearsal');
    event(1, 15, 1, 'Proofread announcement');
    event(2, 8, 2, 'Long run');
    event(4, 9, 3, 'Dress rehearsal: RC1');
    event(4, 13, 1, 'Install guide walkthrough');
    event(5, 9, 1, 'Release code call');
    event(5, 10, 2, 'Atlas 1.0 launch');
    event(5, 13, 4, 'On call in #incidents');
    event(6, 11, 1, 'Launch retro');
    event(7, 10, 1, 'Customer call: Halden');
    event(7, 15, 2, 'Docs cleanup');
    event(8, 12, 1, 'Team lunch');
    event(11, 10, 1, 'Plan 1.0.1');
    event(12, 14, 1, '1:1 with Bob');
    event(13, 16, 1, 'Quarter close');
    cal('month');
    pc.click('cal:event:event-1');
  },
};
