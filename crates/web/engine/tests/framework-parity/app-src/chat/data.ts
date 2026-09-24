export const me = 'me';

export type Message = { from: string; text: string; time: string };

export type Conversation = {
  id: string;
  name: string;
  initials: string;
  color: string;
  online: boolean;
  unread: number;
  messages: Message[];
};

const short = (from: string, text: string, time: string): Message[] => [{ from, text, time }];

export const conversations: Conversation[] = [
  {
    id: 'priya',
    name: 'Priya Raman',
    initials: 'PR',
    color: 'bg-rose-500',
    online: true,
    unread: 2,
    messages: [
      { from: 'priya', text: 'Morning! Did you get a chance to look at the onboarding flow?', time: '09:12' },
      { from: me, text: 'Yes, going through it now. The first two screens feel great.', time: '09:15' },
      { from: me, text: 'The permissions step is where I got lost, though.', time: '09:15' },
      { from: 'priya', text: 'Same feedback from the usability sessions. Four of six people stalled there.', time: '09:20' },
      { from: 'priya', text: 'I was thinking we split it: ask for notifications later, only when they first follow someone.', time: '09:21' },
      { from: me, text: 'That makes sense. Contextual permission prompts convert much better anyway.', time: '09:30' },
      { from: 'priya', text: 'Exactly. I mocked up a version, sending the link in a sec.', time: '09:32' },
      { from: me, text: 'Perfect, I can review before standup.', time: '09:33' },
      { from: 'priya', text: 'Here it is — the prototype is on the second page of the file.', time: '10:05' },
      { from: 'priya', text: 'Let me know what you think about the illustration on the last step too!', time: '10:06' },
    ],
  },
  { id: 'tom', name: 'Tom Becker', initials: 'TB', color: 'bg-sky-500', online: true, unread: 0, messages: [{ from: 'tom', text: 'Deploy is green, shipping at 3.', time: '09:58' }, { from: me, text: 'Great, thanks for the heads-up!', time: '10:01' }] },
  { id: 'design', name: 'Design team', initials: 'DT', color: 'bg-violet-500', online: false, unread: 5, messages: short('lena', 'Lena: Updated the icon set in the library', '09:47') },
  { id: 'amara', name: 'Amara Okoye', initials: 'AO', color: 'bg-emerald-500', online: false, unread: 0, messages: short(me, 'Sounds good, see you Thursday', 'Yesterday') },
  { id: 'lucas', name: 'Lucas Moreau', initials: 'LM', color: 'bg-amber-500', online: true, unread: 1, messages: short('lucas', 'Can you share the Q2 numbers?', 'Yesterday') },
  { id: 'hana', name: 'Hana Sato', initials: 'HS', color: 'bg-pink-500', online: false, unread: 0, messages: short('hana', 'Thanks for the intro!', 'Mon') },
  { id: 'ops', name: 'Ops alerts', initials: 'OA', color: 'bg-slate-600', online: false, unread: 0, messages: short('ops', 'Disk usage on db-2 back under 70%', 'Mon') },
  { id: 'noah', name: 'Noah Fischer', initials: 'NF', color: 'bg-teal-500', online: false, unread: 0, messages: short(me, 'I will send the contract over tonight', 'Sun') },
  { id: 'isla', name: 'Isla MacLeod', initials: 'IM', color: 'bg-orange-500', online: true, unread: 0, messages: short('isla', 'Loved the talk, the slides were beautiful', 'Sat') },
  { id: 'ben', name: 'Ben Adeyemi', initials: 'BA', color: 'bg-indigo-500', online: false, unread: 0, messages: short('ben', 'Pushed a fix for the flaky test', 'Fri') },
  { id: 'zoe', name: 'Zoë Laurent', initials: 'ZL', color: 'bg-fuchsia-500', online: false, unread: 0, messages: short(me, 'Happy birthday!!', 'Thu') },
  { id: 'kai', name: 'Kai Nakamura', initials: 'KN', color: 'bg-cyan-500', online: false, unread: 0, messages: short('kai', 'Can we push our 1:1 to next week?', 'Wed') },
];
