export type RangeKey = '7d' | '30d' | '90d';

export type Range = {
  label: string;
  phrase: string;
  revenue: string;
  orders: string;
  customers: string;
  refunds: string;
  deltas: [number, number, number, number];
  points: number[];
  labels: string[];
};

export const ranges: Record<RangeKey, Range> = {
  '7d': {
    label: 'Last 7 days',
    phrase: 'this week',
    revenue: '$18,240',
    orders: '412',
    customers: '96',
    refunds: '1.8%',
    deltas: [4.2, 2.9, -3.1, -0.4],
    points: [21, 24, 22, 28, 26, 31, 34],
    labels: ['Mon', 'Tue', 'Wed', 'Thu', 'Fri', 'Sat', 'Sun'],
  },
  '30d': {
    label: 'Last 30 days',
    phrase: 'this month',
    revenue: '$84,512',
    orders: '1,906',
    customers: '438',
    refunds: '2.1%',
    deltas: [12.4, 8.1, 5.6, -0.9],
    points: [42, 48, 45, 53, 61, 58, 66, 72],
    labels: ['Apr 1', 'Apr 5', 'Apr 9', 'Apr 13', 'Apr 17', 'Apr 21', 'Apr 25', 'Apr 29'],
  },
  '90d': {
    label: 'Last 90 days',
    phrase: 'this quarter',
    revenue: '$241,090',
    orders: '5,771',
    customers: '1,204',
    refunds: '2.4%',
    deltas: [18.9, 15.2, -2.3, 0.6],
    points: [128, 141, 136, 155, 170, 164, 188, 203, 196, 221, 236, 249],
    labels: ['W1', 'W2', 'W3', 'W4', 'W5', 'W6', 'W7', 'W8', 'W9', 'W10', 'W11', 'W12'],
  },
};

export type Order = {
  id: number;
  customer: string;
  email: string;
  avatar: string;
  date: string;
  status: 'Paid' | 'Pending' | 'Refunded' | 'Failed';
  amount: number;
};

export const orders: Order[] = [
  { id: 3210, customer: 'Olivia Martin', email: 'olivia@example.com', avatar: 'bg-violet-500', date: 'Apr 29, 2024', status: 'Paid', amount: 1999 },
  { id: 3209, customer: 'Jackson Lee', email: 'jackson@example.com', avatar: 'bg-sky-500', date: 'Apr 28, 2024', status: 'Pending', amount: 39 },
  { id: 3208, customer: 'Isabella Nguyen', email: 'isabella@example.com', avatar: 'bg-emerald-500', date: 'Apr 28, 2024', status: 'Paid', amount: 299 },
  { id: 3207, customer: 'William Kim', email: 'will@example.com', avatar: 'bg-amber-500', date: 'Apr 27, 2024', status: 'Refunded', amount: 99 },
  { id: 3206, customer: 'Sofia Davis', email: 'sofia@example.com', avatar: 'bg-rose-500', date: 'Apr 26, 2024', status: 'Failed', amount: 450.5 },
];

export const channels = [
  { name: 'Direct', value: 42, color: '#6366f1' },
  { name: 'Search', value: 28, color: '#0ea5e9' },
  { name: 'Social', value: 18, color: '#10b981' },
  { name: 'Email', value: 12, color: '#f59e0b' },
];

export const goals = [
  { name: 'Monthly revenue', current: 84, target: 100, color: 'bg-indigo-500' },
  { name: 'New customers', current: 438, target: 500, color: 'bg-emerald-500' },
  { name: 'Average order value', current: 44, target: 40, color: 'bg-sky-500' },
  { name: 'Support tickets closed', current: 172, target: 240, color: 'bg-amber-500' },
];
