import { useMemo, useState } from 'react';
import { ChevronDown, ChevronLeft, ChevronRight, ChevronsUpDown, ChevronUp, Download, Mail, Plus, Search } from '../shared/icons';
import { members, type Member, type Role, type Status } from './data';

type SortKey = 'name' | 'role' | 'status' | 'lastActive' | 'seats';
type Sort = { key: SortKey; dir: 'asc' | 'desc' };

const PAGE_SIZE = 8;

const statusBadge: Record<Status, string> = {
  Active: 'bg-green-50 text-green-700 ring-green-600/20',
  Invited: 'bg-blue-50 text-blue-700 ring-blue-700/10',
  Suspended: 'bg-red-50 text-red-700 ring-red-600/10',
};

const statusDot: Record<Status, string> = {
  Active: 'bg-green-500',
  Invited: 'bg-blue-500',
  Suspended: 'bg-red-500',
};

const roleBadge: Record<Role, string> = {
  Owner: 'bg-purple-100 text-purple-800',
  Admin: 'bg-indigo-100 text-indigo-800',
  Editor: 'bg-gray-100 text-gray-800',
  Viewer: 'bg-gray-50 text-gray-600',
};

const columns: { key: SortKey; label: string; className?: string }[] = [
  { key: 'name', label: 'Name' },
  { key: 'role', label: 'Role' },
  { key: 'status', label: 'Status' },
  { key: 'lastActive', label: 'Last active' },
  { key: 'seats', label: 'Projects', className: 'text-right' },
];

function compare(a: Member, b: Member, key: SortKey) {
  const x = a[key];
  const y = b[key];
  return typeof x === 'number' && typeof y === 'number' ? x - y : String(x).localeCompare(String(y));
}

function SortIcon({ active, dir }: { active: boolean; dir: Sort['dir'] }) {
  if (!active) return <ChevronsUpDown className="h-3.5 w-3.5 text-gray-300 group-hover:text-gray-400" />;
  return dir === 'asc' ? <ChevronUp className="h-3.5 w-3.5 text-gray-700" /> : <ChevronDown className="h-3.5 w-3.5 text-gray-700" />;
}

export default function App() {
  const [query, setQuery] = useState('');
  const [status, setStatus] = useState<Status | 'All'>('All');
  const [sort, setSort] = useState<Sort>({ key: 'name', dir: 'asc' });
  const [page, setPage] = useState(0);
  const [selected, setSelected] = useState<Set<number>>(new Set());

  const rows = useMemo(() => {
    const q = query.trim().toLowerCase();
    const filtered = members.filter(
      (m) => (status === 'All' || m.status === status) && (!q || m.name.toLowerCase().includes(q) || m.email.toLowerCase().includes(q)),
    );
    const sorted = [...filtered].sort((a, b) => compare(a, b, sort.key));
    return sort.dir === 'asc' ? sorted : sorted.reverse();
  }, [query, status, sort]);

  const pages = Math.max(1, Math.ceil(rows.length / PAGE_SIZE));
  const current = Math.min(page, pages - 1);
  const visible = rows.slice(current * PAGE_SIZE, current * PAGE_SIZE + PAGE_SIZE);
  const allVisibleSelected = visible.length > 0 && visible.every((m) => selected.has(m.id));

  function toggleSort(key: SortKey) {
    setSort((s) => (s.key === key ? { key, dir: s.dir === 'asc' ? 'desc' : 'asc' } : { key, dir: 'asc' }));
  }

  function toggle(id: number) {
    setSelected((s) => {
      const next = new Set(s);
      if (next.has(id)) next.delete(id);
      else next.add(id);
      return next;
    });
  }

  function toggleAll() {
    setSelected((s) => {
      const next = new Set(s);
      for (const m of visible) {
        if (allVisibleSelected) next.delete(m.id);
        else next.add(m.id);
      }
      return next;
    });
  }

  const counts = {
    All: members.length,
    Active: members.filter((m) => m.status === 'Active').length,
    Invited: members.filter((m) => m.status === 'Invited').length,
    Suspended: members.filter((m) => m.status === 'Suspended').length,
  };

  return (
    <div className="min-h-screen bg-white font-sans text-gray-900">
      <div className="mx-auto max-w-6xl px-8 py-8">
        <div className="flex items-center justify-between">
          <div>
            <h1 className="text-xl font-semibold text-gray-900">Team members</h1>
            <p className="mt-1 text-sm text-gray-600">Everyone with access to the Northwind workspace, their role and when they were last seen.</p>
          </div>
          <div className="flex gap-2">
            <button className="inline-flex items-center gap-2 rounded-md bg-white px-3 py-2 text-sm font-semibold text-gray-900 shadow-sm ring-1 ring-inset ring-gray-300 hover:bg-gray-50">
              <Download className="h-4 w-4 text-gray-400" />
              Export CSV
            </button>
            <button className="inline-flex items-center gap-2 rounded-md bg-gray-900 px-3 py-2 text-sm font-semibold text-white shadow-sm hover:bg-gray-700">
              <Plus className="h-4 w-4" />
              Invite
            </button>
          </div>
        </div>

        <div className="mt-6 flex items-center justify-between gap-4">
          <div className="inline-flex rounded-lg bg-gray-100 p-1">
            {(Object.keys(counts) as (keyof typeof counts)[]).map((s) => (
              <button
                key={s}
                data-status={s}
                onClick={() => {
                  setStatus(s);
                  setPage(0);
                }}
                className={`rounded-md px-3 py-1.5 text-sm font-medium ${status === s ? 'bg-white text-gray-900 shadow-sm' : 'text-gray-500 hover:text-gray-700'}`}
              >
                {s}
                <span className={`ml-1.5 rounded-full px-1.5 text-xs ${status === s ? 'bg-gray-100 text-gray-700' : 'text-gray-400'}`}>{counts[s]}</span>
              </button>
            ))}
          </div>
          <div className="relative w-72">
            <Search className="pointer-events-none absolute inset-y-0 left-3 my-auto h-4 w-4 text-gray-400" />
            <input
              id="filter"
              value={query}
              onChange={(e) => {
                setQuery(e.target.value);
                setPage(0);
              }}
              placeholder="Filter by name or email"
              className="block w-full rounded-md border-0 py-2 pl-9 pr-3 text-sm ring-1 ring-inset ring-gray-300 placeholder:text-gray-400 focus:ring-2 focus:ring-inset focus:ring-gray-900"
            />
          </div>
        </div>

        {selected.size > 0 && (
          <div className="mt-4 flex items-center gap-3 rounded-lg bg-gray-900 px-4 py-2 text-sm text-white">
            <span className="font-medium">{selected.size} selected</span>
            <span className="h-4 w-px bg-gray-600" />
            <button className="inline-flex items-center gap-1.5 text-gray-300 hover:text-white">
              <Mail className="h-4 w-4" />
              Email
            </button>
          </div>
        )}

        <div className="mt-4 max-h-[480px] overflow-auto rounded-lg ring-1 ring-gray-200">
          <table className="min-w-full border-separate border-spacing-0 text-sm">
            <thead>
              <tr>
                <th className="sticky top-0 z-10 w-12 border-b border-gray-200 bg-gray-50/95 px-4 py-3 text-left">
                  <input type="checkbox" aria-label="Select all" checked={allVisibleSelected} onChange={toggleAll} className="h-4 w-4 rounded border-gray-300" />
                </th>
                {columns.map((c) => (
                  <th key={c.key} className={`sticky top-0 z-10 border-b border-gray-200 bg-gray-50/95 px-4 py-3 font-semibold text-gray-900 ${c.className ?? 'text-left'}`}>
                    <button data-sort={c.key} onClick={() => toggleSort(c.key)} className="group inline-flex items-center gap-1">
                      {c.label}
                      <SortIcon active={sort.key === c.key} dir={sort.dir} />
                    </button>
                  </th>
                ))}
              </tr>
            </thead>
            <tbody>
              {visible.map((m) => (
                <tr key={m.id} className={selected.has(m.id) ? 'bg-gray-50' : 'hover:bg-gray-50'}>
                  <td className="border-b border-gray-100 px-4 py-3">
                    <input type="checkbox" data-row={m.id} checked={selected.has(m.id)} onChange={() => toggle(m.id)} className="h-4 w-4 rounded border-gray-300" />
                  </td>
                  <td className="whitespace-nowrap border-b border-gray-100 px-4 py-3">
                    <div className="flex items-center gap-3">
                      <div className={`flex h-9 w-9 items-center justify-center rounded-full text-xs font-semibold ${m.tone}`}>
                        {m.name
                          .split(' ')
                          .map((p) => p[0])
                          .join('')}
                      </div>
                      <div>
                        <div className="font-medium text-gray-900">{m.name}</div>
                        <div className="text-gray-500">{m.email}</div>
                      </div>
                    </div>
                  </td>
                  <td className="whitespace-nowrap border-b border-gray-100 px-4 py-3">
                    <span className={`rounded px-2 py-0.5 text-xs font-medium ${roleBadge[m.role]}`}>{m.role}</span>
                  </td>
                  <td className="whitespace-nowrap border-b border-gray-100 px-4 py-3">
                    <span className={`inline-flex items-center gap-1.5 rounded-md px-2 py-1 text-xs font-medium ring-1 ring-inset ${statusBadge[m.status]}`}>
                      <span className={`h-1.5 w-1.5 rounded-full ${statusDot[m.status]}`} />
                      {m.status}
                    </span>
                  </td>
                  <td className="whitespace-nowrap border-b border-gray-100 px-4 py-3 text-gray-500">{m.lastActive}</td>
                  <td className="whitespace-nowrap border-b border-gray-100 px-4 py-3 text-right tabular-nums text-gray-900">{m.seats}</td>
                </tr>
              ))}
              {visible.length === 0 && (
                <tr>
                  <td colSpan={6} className="px-4 py-12 text-center text-gray-500">
                    No members match “{query}”.
                  </td>
                </tr>
              )}
            </tbody>
          </table>
        </div>

        <nav className="mt-4 flex items-center justify-between" aria-label="Pagination">
          <p className="text-sm text-gray-600">
            Showing <span className="font-medium text-gray-900">{rows.length === 0 ? 0 : current * PAGE_SIZE + 1}</span> to{' '}
            <span className="font-medium text-gray-900">{Math.min(rows.length, (current + 1) * PAGE_SIZE)}</span> of{' '}
            <span className="font-medium text-gray-900">{rows.length}</span> members
          </p>
          <div className="flex items-center gap-1">
            <button
              id="prev"
              disabled={current === 0}
              onClick={() => setPage(current - 1)}
              className="inline-flex h-8 w-8 items-center justify-center rounded-md ring-1 ring-inset ring-gray-300 hover:bg-gray-50 disabled:opacity-40"
              aria-label="Previous page"
            >
              <ChevronLeft className="h-4 w-4" />
            </button>
            {Array.from({ length: pages }, (_, i) => (
              <button
                key={i}
                data-page={i + 1}
                onClick={() => setPage(i)}
                className={`h-8 min-w-[2rem] rounded-md px-2 text-sm font-medium ${
                  i === current ? 'bg-gray-900 text-white' : 'text-gray-700 ring-1 ring-inset ring-gray-300 hover:bg-gray-50'
                }`}
              >
                {i + 1}
              </button>
            ))}
            <button
              id="next"
              disabled={current === pages - 1}
              onClick={() => setPage(current + 1)}
              className="inline-flex h-8 w-8 items-center justify-center rounded-md ring-1 ring-inset ring-gray-300 hover:bg-gray-50 disabled:opacity-40"
              aria-label="Next page"
            >
              <ChevronRight className="h-4 w-4" />
            </button>
          </div>
        </nav>
      </div>
    </div>
  );
}
