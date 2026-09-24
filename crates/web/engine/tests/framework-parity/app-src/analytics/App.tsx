import { useState } from 'react';
import {
  ArrowDownRight,
  ArrowUpRight,
  Bell,
  Calendar,
  ChartColumn,
  Check,
  ChevronDown,
  DollarSign,
  Download,
  Ellipsis,
  Folder,
  LayoutDashboard,
  Package,
  Search,
  Settings,
  ShoppingCart,
  Sparkles,
  TrendingDown,
  TrendingUp,
  Users,
} from '../shared/icons';
import { ranges, orders, channels, goals, type RangeKey, type Order } from './data';

const nav = [
  { label: 'Dashboard', icon: LayoutDashboard, active: true },
  { label: 'Reports', icon: ChartColumn },
  { label: 'Customers', icon: Users },
  { label: 'Orders', icon: ShoppingCart, count: 12 },
  { label: 'Products', icon: Package },
  { label: 'Settings', icon: Settings },
];

const teams = [
  { name: 'Growth', color: 'bg-indigo-500' },
  { name: 'Retention', color: 'bg-emerald-500' },
  { name: 'Partnerships', color: 'bg-amber-500' },
];

function Sidebar() {
  return (
    <aside className="fixed inset-y-0 left-0 flex w-64 flex-col border-r border-gray-200 bg-white">
      <div className="flex h-16 items-center gap-2 px-6">
        <div className="flex h-8 w-8 items-center justify-center rounded-lg bg-gradient-to-br from-indigo-500 to-violet-600 text-white shadow-sm">
          <Sparkles className="h-4 w-4" />
        </div>
        <span className="text-lg font-semibold tracking-tight text-gray-900">Lumen</span>
      </div>
      <nav className="flex-1 space-y-1 px-3 py-4">
        {nav.map(({ label, icon: Icon, active, count }) => (
          <a
            key={label}
            href="#"
            className={`group flex items-center gap-3 rounded-lg px-3 py-2 text-sm font-medium transition-colors ${
              active ? 'bg-indigo-50 text-indigo-700' : 'text-gray-600 hover:bg-gray-50 hover:text-gray-900'
            }`}
          >
            <Icon className={`h-5 w-5 ${active ? 'text-indigo-600' : 'text-gray-400 group-hover:text-gray-500'}`} />
            <span className="flex-1">{label}</span>
            {count !== undefined && (
              <span className="rounded-full bg-gray-100 px-2 py-0.5 text-xs font-medium text-gray-600">{count}</span>
            )}
          </a>
        ))}
        <div className="pt-6">
          <p className="px-3 text-xs font-semibold uppercase tracking-wider text-gray-400">Teams</p>
          <div className="mt-2 space-y-1">
            {teams.map((t) => (
              <a key={t.name} href="#" className="flex items-center gap-3 rounded-lg px-3 py-2 text-sm text-gray-600 hover:bg-gray-50">
                <span className={`h-2 w-2 rounded-full ${t.color}`} />
                {t.name}
              </a>
            ))}
          </div>
        </div>
      </nav>
      <div className="border-t border-gray-200 p-4">
        <div className="flex items-center gap-3">
          <div className="flex h-9 w-9 items-center justify-center rounded-full bg-gradient-to-br from-pink-500 to-orange-400 text-sm font-semibold text-white">
            AR
          </div>
          <div className="min-w-0 flex-1">
            <p className="truncate text-sm font-medium text-gray-900">Ava Reyes</p>
            <p className="truncate text-xs text-gray-500">ava@lumen.app</p>
          </div>
          <Folder className="h-4 w-4 text-gray-400" />
        </div>
      </div>
    </aside>
  );
}

function Header() {
  return (
    <header className="sticky top-0 z-10 flex h-16 items-center gap-4 border-b border-gray-200 bg-white/80 px-8 backdrop-blur">
      <div className="relative w-full max-w-md">
        <Search className="pointer-events-none absolute left-3 top-1/2 h-4 w-4 -translate-y-1/2 text-gray-400" />
        <input
          type="search"
          placeholder="Search orders, customers…"
          className="w-full rounded-lg border border-gray-200 bg-gray-50 py-2 pl-9 pr-3 text-sm text-gray-900 placeholder:text-gray-400 focus:border-indigo-500 focus:bg-white focus:outline-none focus:ring-2 focus:ring-indigo-500/20"
        />
      </div>
      <div className="ml-auto flex items-center gap-3">
        <button className="relative rounded-full p-2 text-gray-500 hover:bg-gray-100" aria-label="Notifications">
          <Bell className="h-5 w-5" />
          <span className="absolute right-1.5 top-1.5 h-2 w-2 rounded-full bg-rose-500 ring-2 ring-white" />
        </button>
        <div className="h-8 w-px bg-gray-200" />
        <div className="flex h-8 w-8 items-center justify-center rounded-full bg-gradient-to-br from-pink-500 to-orange-400 text-xs font-semibold text-white">
          AR
        </div>
      </div>
    </header>
  );
}

function RangePicker({ value, onChange }: { value: RangeKey; onChange: (r: RangeKey) => void }) {
  const [open, setOpen] = useState(false);
  return (
    <div className="relative">
      <button
        aria-label="Date range"
        onClick={() => setOpen((o) => !o)}
        className="inline-flex items-center gap-2 rounded-lg border border-gray-200 bg-white px-3 py-2 text-sm font-medium text-gray-700 shadow-sm hover:bg-gray-50"
      >
        <Calendar className="h-4 w-4 text-gray-400" />
        {ranges[value].label}
        <ChevronDown className={`h-4 w-4 text-gray-400 transition-transform ${open ? 'rotate-180' : ''}`} />
      </button>
      {open && (
        <div className="absolute right-0 z-20 mt-2 w-52 origin-top-right rounded-lg bg-white p-1 shadow-lg ring-1 ring-black/5">
          {(Object.keys(ranges) as RangeKey[]).map((key) => (
            <button
              key={key}
              data-range={key}
              onClick={() => {
                onChange(key);
                setOpen(false);
              }}
              className={`flex w-full items-center justify-between rounded-md px-3 py-2 text-left text-sm ${
                key === value ? 'bg-indigo-50 text-indigo-700' : 'text-gray-700 hover:bg-gray-50'
              }`}
            >
              {ranges[key].label}
              {key === value && <Check className="h-4 w-4" />}
            </button>
          ))}
        </div>
      )}
    </div>
  );
}

function Kpi({ label, value, delta, icon: Icon, tint }: {
  label: string;
  value: string;
  delta: number;
  icon: (p: { className?: string }) => JSX.Element;
  tint: string;
}) {
  const up = delta >= 0;
  return (
    <div className="rounded-xl border border-gray-200 bg-white p-5 shadow-sm">
      <div className="flex items-center justify-between">
        <div className={`flex h-10 w-10 items-center justify-center rounded-lg ${tint}`}>
          <Icon className="h-5 w-5" />
        </div>
        <span
          className={`inline-flex items-center gap-1 rounded-full px-2 py-0.5 text-xs font-medium ${
            up ? 'bg-emerald-50 text-emerald-700' : 'bg-rose-50 text-rose-700'
          }`}
        >
          {up ? <TrendingUp className="h-3 w-3" /> : <TrendingDown className="h-3 w-3" />}
          {up ? '+' : ''}
          {delta.toFixed(1)}%
        </span>
      </div>
      <p className="mt-4 text-sm text-gray-500">{label}</p>
      <p className="mt-1 text-2xl font-semibold tracking-tight text-gray-900">{value}</p>
    </div>
  );
}

const W = 640;
const H = 240;
const PAD = { top: 16, right: 16, bottom: 28, left: 44 };

function RevenueChart({ points, labels }: { points: number[]; labels: string[] }) {
  const max = Math.ceil(Math.max(...points) / 10) * 10;
  const innerW = W - PAD.left - PAD.right;
  const innerH = H - PAD.top - PAD.bottom;
  const x = (i: number) => PAD.left + (i * innerW) / (points.length - 1);
  const y = (v: number) => PAD.top + innerH - (v / max) * innerH;
  const line = points.map((v, i) => `${i === 0 ? 'M' : 'L'}${x(i).toFixed(1)},${y(v).toFixed(1)}`).join(' ');
  const area = `${line} L${x(points.length - 1).toFixed(1)},${PAD.top + innerH} L${PAD.left},${PAD.top + innerH} Z`;
  const ticks = [0, 0.25, 0.5, 0.75, 1].map((t) => Math.round(max * t));
  const last = points.length - 1;
  return (
    <svg viewBox={`0 0 ${W} ${H}`} className="h-60 w-full" role="img" aria-label="Revenue over time">
      <defs>
        <linearGradient id="revenue-fill" x1="0" y1="0" x2="0" y2="1">
          <stop offset="0%" stopColor="#6366f1" stopOpacity={0.25} />
          <stop offset="100%" stopColor="#6366f1" stopOpacity={0} />
        </linearGradient>
      </defs>
      {ticks.map((t) => (
        <g key={t}>
          <line x1={PAD.left} x2={W - PAD.right} y1={y(t)} y2={y(t)} stroke="#e5e7eb" strokeDasharray="4 4" />
          <text x={PAD.left - 8} y={y(t) + 4} textAnchor="end" fontSize="11" fill="#6b7280">
            ${t}k
          </text>
        </g>
      ))}
      {labels.map((l, i) => (
        <text key={l} x={x(i)} y={H - 8} textAnchor="middle" fontSize="11" fill="#6b7280">
          {l}
        </text>
      ))}
      <path d={area} fill="url(#revenue-fill)" />
      <path d={line} fill="none" stroke="#6366f1" strokeWidth={2.5} strokeLinejoin="round" strokeLinecap="round" />
      <circle cx={x(last)} cy={y(points[last])} r={5} fill="#fff" stroke="#6366f1" strokeWidth={2.5} />
    </svg>
  );
}

function ChannelBars() {
  const max = Math.max(...channels.map((c) => c.value));
  return (
    <svg viewBox="0 0 280 200" className="h-52 w-full" role="img" aria-label="Sales by channel">
      <line x1="0" x2="280" y1="170" y2="170" stroke="#e5e7eb" />
      {channels.map((c, i) => {
        const h = (c.value / max) * 140;
        const bx = 16 + i * 66;
        return (
          <g key={c.name}>
            <rect x={bx} y={170 - h} width="36" height={h} rx="6" fill={c.color} />
            <text x={bx + 18} y={162 - h} textAnchor="middle" fontSize="11" fontWeight="600" fill="#374151">
              {c.value}%
            </text>
            <text x={bx + 18} y="188" textAnchor="middle" fontSize="11" fill="#6b7280">
              {c.name}
            </text>
          </g>
        );
      })}
    </svg>
  );
}

const statusStyles: Record<Order['status'], string> = {
  Paid: 'bg-emerald-50 text-emerald-700 ring-emerald-600/20',
  Pending: 'bg-amber-50 text-amber-700 ring-amber-600/20',
  Refunded: 'bg-gray-50 text-gray-600 ring-gray-500/20',
  Failed: 'bg-rose-50 text-rose-700 ring-rose-600/20',
};

function OrdersTable() {
  return (
    <div className="overflow-hidden rounded-xl border border-gray-200 bg-white shadow-sm">
      <div className="flex items-center justify-between px-5 py-4">
        <h2 className="text-base font-semibold text-gray-900">Recent orders</h2>
        <button className="rounded-md p-1 text-gray-400 hover:bg-gray-100" aria-label="More">
          <Ellipsis className="h-5 w-5" />
        </button>
      </div>
      <table className="min-w-full divide-y divide-gray-200 text-sm">
        <thead className="bg-gray-50">
          <tr>
            {['Order', 'Customer', 'Date', 'Status', 'Amount'].map((h) => (
              <th
                key={h}
                className={`px-5 py-3 text-xs font-medium uppercase tracking-wide text-gray-500 ${h === 'Amount' ? 'text-right' : 'text-left'}`}
              >
                {h}
              </th>
            ))}
          </tr>
        </thead>
        <tbody className="divide-y divide-gray-100">
          {orders.map((o) => (
            <tr key={o.id} className="hover:bg-gray-50">
              <td className="whitespace-nowrap px-5 py-3 font-medium text-gray-900">#{o.id}</td>
              <td className="px-5 py-3">
                <div className="flex items-center gap-3">
                  <div className={`flex h-8 w-8 items-center justify-center rounded-full text-xs font-semibold text-white ${o.avatar}`}>
                    {o.customer
                      .split(' ')
                      .map((p) => p[0])
                      .join('')}
                  </div>
                  <div>
                    <p className="font-medium text-gray-900">{o.customer}</p>
                    <p className="text-xs text-gray-500">{o.email}</p>
                  </div>
                </div>
              </td>
              <td className="whitespace-nowrap px-5 py-3 text-gray-500">{o.date}</td>
              <td className="px-5 py-3">
                <span className={`inline-flex items-center rounded-full px-2 py-0.5 text-xs font-medium ring-1 ring-inset ${statusStyles[o.status]}`}>
                  {o.status}
                </span>
              </td>
              <td className="whitespace-nowrap px-5 py-3 text-right font-medium tabular-nums text-gray-900">
                ${o.amount.toFixed(2)}
              </td>
            </tr>
          ))}
        </tbody>
      </table>
    </div>
  );
}

function Donut({ percent }: { percent: number }) {
  const r = 52;
  const c = 2 * Math.PI * r;
  return (
    <svg viewBox="0 0 128 128" className="h-32 w-32 -rotate-90">
      <circle cx="64" cy="64" r={r} fill="none" stroke="#eef2ff" strokeWidth="12" />
      <circle
        cx="64"
        cy="64"
        r={r}
        fill="none"
        stroke="#6366f1"
        strokeWidth="12"
        strokeLinecap="round"
        strokeDasharray={`${((percent / 100) * c).toFixed(1)} ${c.toFixed(1)}`}
      />
    </svg>
  );
}

function Reports() {
  return (
    <div className="grid grid-cols-3 gap-6">
      <div className="col-span-2 rounded-xl border border-gray-200 bg-white p-6 shadow-sm">
        <h2 className="text-base font-semibold text-gray-900">Quarterly goals</h2>
        <p className="mt-1 text-sm text-gray-500">Progress toward the targets set in January.</p>
        <ul className="mt-6 space-y-5">
          {goals.map((g) => (
            <li key={g.name}>
              <div className="flex items-center justify-between text-sm">
                <span className="font-medium text-gray-700">{g.name}</span>
                <span className="tabular-nums text-gray-500">
                  {g.current} / {g.target}
                </span>
              </div>
              <div className="mt-2 h-2 overflow-hidden rounded-full bg-gray-100">
                <div className={`h-full rounded-full ${g.color}`} style={{ width: `${Math.min(100, (g.current / g.target) * 100)}%` }} />
              </div>
            </li>
          ))}
        </ul>
      </div>
      <div className="flex flex-col items-center rounded-xl border border-gray-200 bg-white p-6 shadow-sm">
        <h2 className="self-start text-base font-semibold text-gray-900">Retention</h2>
        <div className="relative mt-6">
          <Donut percent={72} />
          <div className="absolute inset-0 flex flex-col items-center justify-center">
            <span className="text-2xl font-semibold text-gray-900">72%</span>
            <span className="text-xs text-gray-500">30-day</span>
          </div>
        </div>
        <p className="mt-6 text-center text-sm text-gray-500">
          Up <span className="font-medium text-emerald-600">4.1 points</span> since last quarter.
        </p>
      </div>
    </div>
  );
}

const tabs = ['Overview', 'Reports', 'Customers'] as const;
type Tab = (typeof tabs)[number];

export default function App() {
  const [range, setRange] = useState<RangeKey>('30d');
  const [tab, setTab] = useState<Tab>('Overview');
  const data = ranges[range];

  return (
    <div className="min-h-screen bg-gray-50 font-sans text-gray-900 antialiased">
      <Sidebar />
      <div className="pl-64">
        <Header />
        <main className="px-8 py-8">
          <div className="flex items-end justify-between">
            <div>
              <h1 className="text-2xl font-semibold tracking-tight text-gray-900">Overview</h1>
              <p className="mt-1 text-sm text-gray-500">Here's what happened with your store {data.phrase}.</p>
            </div>
            <div className="flex items-center gap-3">
              <RangePicker value={range} onChange={setRange} />
              <button className="inline-flex items-center gap-2 rounded-lg bg-indigo-600 px-4 py-2 text-sm font-medium text-white shadow-sm hover:bg-indigo-500">
                <Download className="h-4 w-4" />
                Export
              </button>
            </div>
          </div>

          <div className="mt-6 border-b border-gray-200">
            <nav className="-mb-px flex gap-6">
              {tabs.map((t) => (
                <button
                  key={t}
                  data-tab={t}
                  onClick={() => setTab(t)}
                  className={`border-b-2 px-1 pb-3 text-sm font-medium ${
                    tab === t ? 'border-indigo-600 text-indigo-600' : 'border-transparent text-gray-500 hover:border-gray-300 hover:text-gray-700'
                  }`}
                >
                  {t}
                </button>
              ))}
            </nav>
          </div>

          {tab === 'Overview' && (
            <>
              <div className="mt-6 grid grid-cols-4 gap-6">
                <Kpi label="Revenue" value={data.revenue} delta={data.deltas[0]} icon={DollarSign} tint="bg-indigo-50 text-indigo-600" />
                <Kpi label="Orders" value={data.orders} delta={data.deltas[1]} icon={ShoppingCart} tint="bg-sky-50 text-sky-600" />
                <Kpi label="New customers" value={data.customers} delta={data.deltas[2]} icon={Users} tint="bg-emerald-50 text-emerald-600" />
                <Kpi label="Refund rate" value={data.refunds} delta={data.deltas[3]} icon={Package} tint="bg-amber-50 text-amber-600" />
              </div>

              <div className="mt-6 grid grid-cols-3 gap-6">
                <div className="col-span-2 rounded-xl border border-gray-200 bg-white p-6 shadow-sm">
                  <div className="flex items-start justify-between">
                    <div>
                      <h2 className="text-base font-semibold text-gray-900">Revenue</h2>
                      <p className="mt-1 flex items-center gap-1 text-sm text-gray-500">
                        {data.deltas[0] >= 0 ? (
                          <ArrowUpRight className="h-4 w-4 text-emerald-500" />
                        ) : (
                          <ArrowDownRight className="h-4 w-4 text-rose-500" />
                        )}
                        {data.revenue} {data.phrase}
                      </p>
                    </div>
                    <div className="flex items-center gap-2 text-xs text-gray-500">
                      <span className="h-2 w-2 rounded-full bg-indigo-500" />
                      Net sales
                    </div>
                  </div>
                  <div className="mt-4">
                    <RevenueChart points={data.points} labels={data.labels} />
                  </div>
                </div>
                <div className="rounded-xl border border-gray-200 bg-white p-6 shadow-sm">
                  <h2 className="text-base font-semibold text-gray-900">Sales by channel</h2>
                  <p className="mt-1 text-sm text-gray-500">Share of orders</p>
                  <div className="mt-4">
                    <ChannelBars />
                  </div>
                </div>
              </div>

              <div className="mt-6">
                <OrdersTable />
              </div>
            </>
          )}
          {tab === 'Reports' && (
            <div className="mt-6">
              <Reports />
            </div>
          )}
          {tab === 'Customers' && (
            <div className="mt-6 rounded-xl border border-dashed border-gray-300 bg-white p-12 text-center">
              <Users className="mx-auto h-10 w-10 text-gray-300" />
              <h2 className="mt-3 text-sm font-semibold text-gray-900">No segments yet</h2>
              <p className="mt-1 text-sm text-gray-500">Create a segment to group customers by behaviour.</p>
            </div>
          )}
        </main>
      </div>
    </div>
  );
}
