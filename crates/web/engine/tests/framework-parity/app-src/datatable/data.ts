export type Role = 'Owner' | 'Admin' | 'Editor' | 'Viewer';
export type Status = 'Active' | 'Invited' | 'Suspended';

export type Member = {
  id: number;
  name: string;
  email: string;
  role: Role;
  status: Status;
  lastActive: string;
  seats: number;
  tone: string;
};

const tones = ['bg-indigo-100 text-indigo-700', 'bg-amber-100 text-amber-700', 'bg-emerald-100 text-emerald-700', 'bg-rose-100 text-rose-700', 'bg-sky-100 text-sky-700'];

const rows: [string, Role, Status, string, number][] = [
  ['Lindsay Walton', 'Owner', 'Active', 'Just now', 14],
  ['Courtney Henry', 'Admin', 'Active', '3 minutes ago', 9],
  ['Tom Cook', 'Editor', 'Active', '1 hour ago', 6],
  ['Whitney Francis', 'Editor', 'Invited', 'Never', 0],
  ['Leonard Krasner', 'Viewer', 'Active', '2 hours ago', 2],
  ['Floyd Miles', 'Editor', 'Suspended', '3 weeks ago', 4],
  ['Emily Selman', 'Admin', 'Active', 'Yesterday', 11],
  ['Kristin Watson', 'Viewer', 'Active', '4 days ago', 1],
  ['Emma Dorsey', 'Editor', 'Active', '5 hours ago', 7],
  ['Alicia Bell', 'Viewer', 'Invited', 'Never', 0],
  ['Jenny Wilson', 'Editor', 'Active', '12 minutes ago', 5],
  ['Anna Roberts', 'Viewer', 'Active', 'Last week', 3],
  ['Benjamin Russel', 'Editor', 'Suspended', '2 months ago', 8],
  ['Dries Vincent', 'Admin', 'Active', '6 hours ago', 12],
  ['Hector Gibbons', 'Viewer', 'Active', 'Yesterday', 2],
  ['Jeffrey Webb', 'Editor', 'Invited', 'Never', 0],
  ['Michael Foster', 'Editor', 'Active', '20 minutes ago', 10],
  ['Rebecca Nguyen', 'Viewer', 'Active', '3 days ago', 1],
  ['Sofia Mendes', 'Editor', 'Active', '9 hours ago', 6],
  ['Victor Allen', 'Viewer', 'Suspended', '5 weeks ago', 2],
  ['Wade Cooper', 'Editor', 'Active', '2 days ago', 5],
  ['Yusuf Hassan', 'Viewer', 'Invited', 'Never', 0],
];

export const members: Member[] = rows.map(([name, role, status, lastActive, seats], i) => ({
  id: i + 1,
  name,
  email: `${name.split(' ')[0].toLowerCase()}@northwind.io`,
  role,
  status,
  lastActive,
  seats,
  tone: tones[i % tones.length],
}));
