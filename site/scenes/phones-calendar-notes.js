export default {
  id: 'phones-calendar-notes', title: 'Calendar on an iPhone, Notes on a Pixel',
  summary: 'Nine events around the Atlas 1.0 release; "Seattle trip, 21-24 Sept" open on the Pixel',
  machines: [
    { id: 'iphone-calendar', like: 'alice-phone', size: [390, 844], label: "Alice's iPhone" },
    { id: 'pixel-notes', like: 'bob-android', size: [412, 892], label: "Bob's Pixel" },
  ],
  open(iphone, pixel) {
    const cal = control => iphone.step('application.v1', 'shell', { target: `window:0:content:cal:${control}` });
    // Days count from the world's first morning, Thursday 17 September.
    const event = (day, hour, hours, title) => {
      cal(`day:${day}`); cal('new'); iphone.type(title);
      for (let h = 9; h < hour; h++) cal('later');
      for (let h = 1; h < hours; h++) cal('longer');
      cal('save');
    };
    iphone.launch('calendar');
    event(1, 11, 1, 'Dentist');
    event(4, 9, 3, 'Atlas 1.0 release');
    event(7, 15, 1, 'Release retro');
    event(0, 10, 1, 'Atlas standup');
    event(0, 11, 1, 'Replay viewer review');
    event(0, 12, 1, 'Lunch with Carol');
    event(0, 14, 2, 'Launch walkthrough');
    event(0, 16, 1, 'Budget with Bob');
    event(0, 18, 1, 'Climbing gym');
    pixel.step('filesystem.v1', 'write', { path: '/data/user/bob/Notes/Seattle trip.txt', content: [
      'Seattle trip, 21-24 Sept',
      '',
      'Mon  Fly in 10:40, train to Hotel Cascade',
      '     Atlas 1.0 release at HQ, 9 to noon',
      'Tue  Lunch at Harborline Coffee with Alice',
      '     Quill & Kettle Books: gift for Carol',
      'Wed  Ninth & Pike Market before the retro',
      'Thu  Release retro 15:00, fly home 19:05',
      '',
      'Pack: rain jacket (rain through Monday),',
      'badge, laptop charger, the good headphones',
      '',
      'Ask Alice: is the utilities bill right?',
    ].join('\n') + '\n' });
    pixel.launch('notes');
    pixel.click('notes:open:Seattle trip.txt');
  },
};
