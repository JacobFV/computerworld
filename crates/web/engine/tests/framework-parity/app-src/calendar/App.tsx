import { useLayoutEffect, useMemo, useRef, useState, type FormEvent } from 'react';
import {
  Bell,
  Calendar,
  Check,
  ChevronDown,
  ChevronLeft,
  ChevronRight,
  CircleCheck,
  Clock,
  Globe,
  Menu,
  Plus,
  Search,
  Settings,
  Video,
  X,
} from '../shared/icons';

// ---------------------------------------------------------------------------
// Data. The app is pinned to one day so it renders the same every time: "today"
// is Thursday 24 September 2026 and "now" is 11:40 AM.
// ---------------------------------------------------------------------------

const TODAY = new Date(2026, 8, 24);
const NOW_HOURS = 11 + 40 / 60;
const HOUR = 52; // px per hour row in the week grid

type CalendarId = 'work' | 'team' | 'personal' | 'focus' | 'holidays';

type CalendarDef = {
  id: CalendarId;
  name: string;
  dot: string;
  check: string;
  block: string;
  sub: string;
  fill: string;
};

const calendars: CalendarDef[] = [
  {
    id: 'work',
    name: 'Work',
    dot: 'bg-indigo-500',
    check: 'border-indigo-500 bg-indigo-500',
    block: 'border-indigo-500 bg-indigo-50 text-indigo-900 hover:bg-indigo-100',
    sub: 'text-indigo-600',
    fill: 'bg-indigo-500',
  },
  {
    id: 'team',
    name: 'Team events',
    dot: 'bg-sky-500',
    check: 'border-sky-500 bg-sky-500',
    block: 'border-sky-500 bg-sky-50 text-sky-900 hover:bg-sky-100',
    sub: 'text-sky-600',
    fill: 'bg-sky-500',
  },
  {
    id: 'personal',
    name: 'Personal',
    dot: 'bg-emerald-500',
    check: 'border-emerald-500 bg-emerald-500',
    block: 'border-emerald-500 bg-emerald-50 text-emerald-900 hover:bg-emerald-100',
    sub: 'text-emerald-600',
    fill: 'bg-emerald-500',
  },
  {
    id: 'focus',
    name: 'Focus time',
    dot: 'bg-amber-500',
    check: 'border-amber-500 bg-amber-500',
    block: 'border-amber-500 bg-amber-50 text-amber-900 hover:bg-amber-100',
    sub: 'text-amber-600',
    fill: 'bg-amber-500',
  },
  {
    id: 'holidays',
    name: 'Holidays',
    dot: 'bg-rose-500',
    check: 'border-rose-500 bg-rose-500',
    block: 'border-rose-500 bg-rose-50 text-rose-900 hover:bg-rose-100',
    sub: 'text-rose-600',
    fill: 'bg-rose-500',
  },
];

const calendarById = Object.fromEntries(calendars.map((c) => [c.id, c])) as Record<CalendarId, CalendarDef>;

type CalEvent = {
  id: number;
  title: string;
  date: string; // yyyy-mm-dd
  calendar: CalendarId;
  start?: number; // hours from midnight; absent for all-day events
  end?: number;
  location?: string;
};

// ---------------------------------------------------------------------------
// Date helpers
// ---------------------------------------------------------------------------

const MONTHS = ['January', 'February', 'March', 'April', 'May', 'June', 'July', 'August', 'September', 'October', 'November', 'December'];
const WEEKDAYS = ['Sun', 'Mon', 'Tue', 'Wed', 'Thu', 'Fri', 'Sat'];

function addDays(d: Date, n: number) {
  return new Date(d.getFullYear(), d.getMonth(), d.getDate() + n);
}

function startOfWeek(d: Date) {
  return addDays(d, -d.getDay());
}

function dateKey(d: Date) {
  return `${d.getFullYear()}-${String(d.getMonth() + 1).padStart(2, '0')}-${String(d.getDate()).padStart(2, '0')}`;
}

function sameDay(a: Date, b: Date) {
  return dateKey(a) === dateKey(b);
}

function monthGrid(year: number, month: number) {
  const first = startOfWeek(new Date(year, month, 1));
  return Array.from({ length: 42 }, (_, i) => addDays(first, i));
}

function isoWeek(d: Date) {
  const t = new Date(Date.UTC(d.getFullYear(), d.getMonth(), d.getDate()));
  const day = t.getUTCDay() || 7;
  t.setUTCDate(t.getUTCDate() + 4 - day);
  const yearStart = Date.UTC(t.getUTCFullYear(), 0, 1);
  return Math.ceil(((t.getTime() - yearStart) / 86400000 + 1) / 7);
}

function formatTime(h: number, withSuffix = true) {
  const hour = Math.floor(h);
  const min = Math.round((h - hour) * 60);
  const h12 = hour % 12 === 0 ? 12 : hour % 12;
  const suffix = hour < 12 || hour === 24 ? 'AM' : 'PM';
  return `${h12}:${String(min).padStart(2, '0')}${withSuffix ? ` ${suffix}` : ''}`;
}

function formatRange(start: number, end: number) {
  const isAm = (h: number) => h < 12 || h >= 24;
  const sameHalf = isAm(start) === isAm(end);
  return `${formatTime(start, !sameHalf)} – ${formatTime(end)}`;
}

function shortTime(h: number) {
  const hour = Math.floor(h);
  const min = Math.round((h - hour) * 60);
  const h12 = hour % 12 === 0 ? 12 : hour % 12;
  return `${h12}${min ? `:${String(min).padStart(2, '0')}` : ''}${hour < 12 ? 'am' : 'pm'}`;
}

function hourLabel(h: number) {
  if (h === 0) return '';
  const h12 = h % 12 === 0 ? 12 : h % 12;
  return `${h12} ${h < 12 ? 'AM' : 'PM'}`;
}

// ---------------------------------------------------------------------------
// Seed events for September 2026
// ---------------------------------------------------------------------------

function seedEvents(): CalEvent[] {
  const list: Omit<CalEvent, 'id'>[] = [];
  // Daily standup on every September weekday except Labor Day.
  for (let d = 1; d <= 30; d++) {
    const date = new Date(2026, 8, d);
    const wd = date.getDay();
    if (wd === 0 || wd === 6 || d === 7) continue;
    list.push({ title: 'Daily standup', date: dateKey(date), calendar: 'team', start: 9, end: 9.5, location: 'Zoom' });
  }
  list.push(
    { title: 'Welcome lunch for Jonas', date: '2026-08-31', calendar: 'team', start: 12, end: 13 },
    { title: 'Q4 planning kickoff', date: '2026-09-01', calendar: 'work', start: 10, end: 11.5 },
    { title: 'Dentist', date: '2026-09-03', calendar: 'personal', start: 16, end: 17 },
    { title: 'Labor Day', date: '2026-09-07', calendar: 'holidays' },
    { title: 'Board deck review', date: '2026-09-10', calendar: 'work', start: 14, end: 15 },
    { title: 'Team offsite', date: '2026-09-11', calendar: 'team' },
    { title: 'Design critique', date: '2026-09-14', calendar: 'team', start: 15, end: 16 },
    { title: 'Quarterly business review', date: '2026-09-17', calendar: 'work', start: 11, end: 12.5 },
    { title: 'The National — Greek Theatre', date: '2026-09-18', calendar: 'personal', start: 20, end: 22 },

    // The week of 20 September.
    { title: 'Long run', date: '2026-09-20', calendar: 'personal', start: 8, end: 9.5, location: 'Crissy Field' },
    { title: 'Meal prep', date: '2026-09-20', calendar: 'personal', start: 16, end: 17.5 },
    { title: 'Design review: onboarding', date: '2026-09-21', calendar: 'work', start: 10, end: 11.5, location: 'Room 4B' },
    { title: 'Lunch with Priya', date: '2026-09-21', calendar: 'personal', start: 12.5, end: 13.5, location: 'Tartine' },
    { title: 'Deep work', date: '2026-09-21', calendar: 'focus', start: 14, end: 16 },
    { title: 'Autumn equinox', date: '2026-09-22', calendar: 'holidays' },
    { title: '1:1 with Marcus', date: '2026-09-22', calendar: 'work', start: 11, end: 12 },
    { title: 'Sprint planning', date: '2026-09-22', calendar: 'team', start: 13, end: 14.5, location: 'Zoom' },
    { title: 'Yoga', date: '2026-09-22', calendar: 'personal', start: 17.5, end: 18.5 },
    { title: 'Focus: pricing page', date: '2026-09-23', calendar: 'focus', start: 9.5, end: 11.5 },
    { title: 'Customer call — Acme', date: '2026-09-23', calendar: 'work', start: 14, end: 15, location: 'Google Meet' },
    { title: 'Hiring sync', date: '2026-09-23', calendar: 'team', start: 15.5, end: 16.25 },
    { title: 'Roadmap review', date: '2026-09-24', calendar: 'work', start: 10, end: 11, location: 'Room 2A' },
    { title: 'Interview: frontend', date: '2026-09-24', calendar: 'team', start: 10.5, end: 11.5, location: 'Zoom' },
    { title: 'Lunch & learn', date: '2026-09-24', calendar: 'team', start: 13, end: 14, location: 'Atrium' },
    { title: 'Deep work', date: '2026-09-24', calendar: 'focus', start: 15, end: 17 },
    { title: 'Demo day prep', date: '2026-09-25', calendar: 'work', start: 11, end: 12 },
    { title: 'Team retro', date: '2026-09-25', calendar: 'team', start: 16, end: 17, location: 'Room 4B' },
    { title: 'Dinner at Nopa', date: '2026-09-25', calendar: 'personal', start: 19, end: 21 },
    { title: 'Farmers market', date: '2026-09-26', calendar: 'personal', start: 10, end: 11.5, location: 'Ferry Building' },

    { title: 'v2.4 release', date: '2026-09-29', calendar: 'work', start: 10, end: 11 },
    { title: 'Month-end close', date: '2026-09-30', calendar: 'work', start: 16, end: 17 },
    { title: 'Weekend in Tahoe', date: '2026-10-02', calendar: 'personal' },
  );
  return list.map((e, i) => ({ ...e, id: i + 1 }));
}

function sortEvents(list: CalEvent[]) {
  return [...list].sort((a, b) => {
    if (a.start === undefined && b.start !== undefined) return -1;
    if (b.start === undefined && a.start !== undefined) return 1;
    return (a.start ?? 0) - (b.start ?? 0) || (b.end ?? 0) - (a.end ?? 0) || a.id - b.id;
  });
}

// Lays a day's timed events out side by side where they overlap: events are
// grouped into clusters of transitive overlap, and each event takes the first
// column in its cluster that is free at its start.
type Placed = { event: CalEvent; col: number; cols: number };

function layoutDay(events: CalEvent[]): Placed[] {
  const sorted = sortEvents(events.filter((e) => e.start !== undefined));
  const placed: Placed[] = [];
  let cluster: Placed[] = [];
  let columnsEnd: number[] = [];
  let clusterEnd = -1;

  const flush = () => {
    for (const p of cluster) p.cols = columnsEnd.length;
    placed.push(...cluster);
    cluster = [];
    columnsEnd = [];
  };

  for (const event of sorted) {
    if (event.start! >= clusterEnd) flush();
    let col = columnsEnd.findIndex((end) => end <= event.start!);
    if (col === -1) {
      col = columnsEnd.length;
      columnsEnd.push(event.end!);
    } else {
      columnsEnd[col] = event.end!;
    }
    cluster.push({ event, col, cols: 1 });
    clusterEnd = Math.max(clusterEnd, event.end!);
  }
  flush();
  return placed;
}

// ---------------------------------------------------------------------------
// Pieces
// ---------------------------------------------------------------------------

function MiniMonth({ anchor, onPick }: { anchor: Date; onPick: (d: Date) => void }) {
  const [shown, setShown] = useState({ year: anchor.getFullYear(), month: anchor.getMonth() });
  const days = monthGrid(shown.year, shown.month);
  const weekStart = dateKey(startOfWeek(anchor));

  function shift(n: number) {
    const d = new Date(shown.year, shown.month + n, 1);
    setShown({ year: d.getFullYear(), month: d.getMonth() });
  }

  return (
    <div>
      <div className="flex items-center justify-between px-1">
        <span className="text-sm font-semibold text-gray-900">
          {MONTHS[shown.month]} {shown.year}
        </span>
        <div className="flex items-center">
          <button onClick={() => shift(-1)} className="rounded p-1 text-gray-400 hover:bg-gray-100 hover:text-gray-700" aria-label="Previous month">
            <ChevronLeft className="h-4 w-4" />
          </button>
          <button onClick={() => shift(1)} className="rounded p-1 text-gray-400 hover:bg-gray-100 hover:text-gray-700" aria-label="Next month">
            <ChevronRight className="h-4 w-4" />
          </button>
        </div>
      </div>
      <div className="mt-2 grid grid-cols-7 text-center text-[11px] font-medium text-gray-400">
        {WEEKDAYS.map((w) => (
          <span key={w} className="py-1">
            {w[0]}
          </span>
        ))}
      </div>
      <div className="grid grid-cols-7 gap-y-0.5 text-center text-xs">
        {days.map((d, i) => {
          const inWeek = dateKey(startOfWeek(d)) === weekStart;
          const isToday = sameDay(d, TODAY);
          const outside = d.getMonth() !== shown.month;
          const rounded = inWeek ? (i % 7 === 0 ? 'rounded-l-md' : i % 7 === 6 ? 'rounded-r-md' : '') : '';
          return (
            <div key={dateKey(d)} className={`py-0.5 ${inWeek ? 'bg-indigo-50' : ''} ${rounded}`}>
              <button
                onClick={() => onPick(d)}
                className={`mx-auto flex h-7 w-7 items-center justify-center rounded-full ${
                  isToday
                    ? 'bg-indigo-600 font-semibold text-white'
                    : outside
                      ? 'text-gray-300 hover:bg-gray-100'
                      : inWeek
                        ? 'font-medium text-indigo-700 hover:bg-indigo-100'
                        : 'text-gray-700 hover:bg-gray-100'
                }`}
              >
                {d.getDate()}
              </button>
            </div>
          );
        })}
      </div>
    </div>
  );
}

function CalendarToggle({ cal, on, onToggle }: { cal: CalendarDef; on: boolean; onToggle: () => void }) {
  return (
    <button
      id={`cal-toggle-${cal.id}`}
      role="checkbox"
      aria-checked={on}
      onClick={onToggle}
      className="flex w-full items-center gap-3 rounded-md px-2 py-1.5 text-left text-sm text-gray-700 hover:bg-gray-50"
    >
      <span className={`flex h-4 w-4 shrink-0 items-center justify-center rounded border-2 ${on ? cal.check : 'border-gray-300 bg-white'}`}>
        {on && <Check className="h-3 w-3 text-white" />}
      </span>
      <span className={`flex-1 ${on ? '' : 'text-gray-400'}`}>{cal.name}</span>
    </button>
  );
}

function EventBlock({ placed }: { placed: Placed }) {
  const { event, col, cols } = placed;
  const cal = calendarById[event.calendar];
  const duration = event.end! - event.start!;
  const short = duration <= 0.5;
  // Overlapping events cascade: each later one is indented and sits on top, and all
  // but the last reach past their share of the column.
  const left = (col / cols) * 60;
  const width = col === cols - 1 ? 100 - left : Math.min((100 / cols) * 1.7, 100 - left);
  return (
    <button
      data-event={event.id}
      className={`absolute overflow-hidden rounded-md border-l-[3px] px-2 text-left shadow-sm ring-1 ring-white ${cal.block} ${short ? 'py-0.5' : 'py-1'}`}
      style={{
        top: event.start! * HOUR + 1,
        height: duration * HOUR - 2,
        left: `calc(${left}% + 2px)`,
        width: `calc(${width}% - 4px)`,
        zIndex: 1 + col,
      }}
    >
      {short ? (
        <p className="truncate text-[11px] leading-4">
          <span className="font-semibold">{event.title}</span>
          <span className={`ml-1 ${cal.sub}`}>{formatTime(event.start!)}</span>
        </p>
      ) : (
        <>
          <p className="truncate text-xs font-semibold leading-4">{event.title}</p>
          <p className={`truncate text-[11px] leading-4 ${cal.sub}`}>{formatRange(event.start!, event.end!)}</p>
          {event.location && duration >= 1 && <p className={`truncate text-[11px] leading-4 ${cal.sub} opacity-80`}>{event.location}</p>}
        </>
      )}
    </button>
  );
}

function WeekView({ anchor, events }: { anchor: Date; events: CalEvent[] }) {
  const scroller = useRef<HTMLDivElement>(null);
  const days = Array.from({ length: 7 }, (_, i) => addDays(startOfWeek(anchor), i));
  const hours = Array.from({ length: 24 }, (_, h) => h);

  useLayoutEffect(() => {
    // Open on the working day rather than midnight.
    if (scroller.current) scroller.current.scrollTop = 7.5 * HOUR;
  }, []);

  const byDay = days.map((d) => events.filter((e) => e.date === dateKey(d)));

  return (
    <div ref={scroller} className="flex-1 overflow-y-auto" id="week-scroller">
      <div className="sticky top-0 z-20 border-b border-gray-200 bg-white">
        <div className="grid grid-cols-[4rem_repeat(7,minmax(0,1fr))]">
          <div className="flex items-end justify-end pb-2 pr-2 text-[10px] font-medium text-gray-400">GMT-7</div>
          {days.map((d) => {
            const isToday = sameDay(d, TODAY);
            return (
              <div key={dateKey(d)} className="flex flex-col items-center border-l border-gray-100 py-2">
                <span className={`text-[11px] font-semibold uppercase tracking-wide ${isToday ? 'text-indigo-600' : 'text-gray-500'}`}>
                  {WEEKDAYS[d.getDay()]}
                </span>
                <span
                  className={`mt-0.5 flex h-8 w-8 items-center justify-center rounded-full text-lg ${
                    isToday ? 'bg-indigo-600 font-semibold text-white' : 'font-medium text-gray-900'
                  }`}
                >
                  {d.getDate()}
                </span>
              </div>
            );
          })}
        </div>
        <div className="grid grid-cols-[4rem_repeat(7,minmax(0,1fr))] border-t border-gray-100">
          <div className="flex items-center justify-end pr-2 text-[10px] font-medium text-gray-400">all-day</div>
          {byDay.map((list, i) => (
            <div key={i} className="min-h-[28px] space-y-0.5 border-l border-gray-100 p-0.5">
              {list
                .filter((e) => e.start === undefined)
                .map((e) => (
                  <div key={e.id} className={`truncate rounded px-1.5 py-0.5 text-[11px] font-semibold text-white ${calendarById[e.calendar].fill}`}>
                    {e.title}
                  </div>
                ))}
            </div>
          ))}
        </div>
      </div>

      <div className="grid grid-cols-[4rem_repeat(7,minmax(0,1fr))]">
        <div className="relative">
          {hours.map((h) => (
            <div key={h} className="relative" style={{ height: HOUR }}>
              {h > 0 && <span className="absolute -top-2 right-2 text-[11px] text-gray-400">{hourLabel(h)}</span>}
            </div>
          ))}
        </div>
        {days.map((d, i) => {
          const isToday = sameDay(d, TODAY);
          return (
            <div key={dateKey(d)} className={`relative border-l border-gray-100 ${isToday ? 'bg-indigo-50/40' : ''}`}>
              {hours.map((h) => (
                <div key={h} className="border-t border-gray-100" style={{ height: HOUR }} />
              ))}
              {layoutDay(byDay[i]).map((p) => (
                <EventBlock key={p.event.id} placed={p} />
              ))}
              {isToday && (
                <div className="pointer-events-none absolute inset-x-0 z-10 flex items-center" style={{ top: NOW_HOURS * HOUR - 5 }}>
                  <span className="-ml-[5px] h-2.5 w-2.5 rounded-full bg-rose-500" />
                  <span className="h-0.5 flex-1 bg-rose-500" />
                </div>
              )}
            </div>
          );
        })}
      </div>
    </div>
  );
}

function MonthView({ anchor, events, onPickDay }: { anchor: Date; events: CalEvent[]; onPickDay: (d: Date) => void }) {
  // As many week rows as the month spans: five for September 2026.
  const first = new Date(anchor.getFullYear(), anchor.getMonth(), 1);
  const daysInMonth = new Date(anchor.getFullYear(), anchor.getMonth() + 1, 0).getDate();
  const rows = Math.ceil((first.getDay() + daysInMonth) / 7);
  const days = monthGrid(anchor.getFullYear(), anchor.getMonth()).slice(0, rows * 7);
  return (
    <div className="flex flex-1 flex-col" id="month-grid">
      <div className="grid grid-cols-7 border-b border-gray-200">
        {WEEKDAYS.map((w) => (
          <div key={w} className="border-l border-gray-100 py-2 text-center text-[11px] font-semibold uppercase tracking-wide text-gray-500 first:border-l-0">
            {w}
          </div>
        ))}
      </div>
      <div className="grid min-h-0 flex-1 grid-cols-7" style={{ gridTemplateRows: `repeat(${rows}, minmax(0, 1fr))` }}>
        {days.map((d, i) => {
          const list = sortEvents(events.filter((e) => e.date === dateKey(d)));
          const outside = d.getMonth() !== anchor.getMonth();
          const isToday = sameDay(d, TODAY);
          const visible = list.length > 3 ? list.slice(0, 2) : list;
          return (
            <div
              key={dateKey(d)}
              className={`min-h-0 overflow-hidden border-b border-gray-100 p-1.5 ${i % 7 === 0 ? '' : 'border-l'} ${outside ? 'bg-gray-50/70' : 'bg-white'}`}
            >
              <button
                onClick={() => onPickDay(d)}
                className={`flex h-6 min-w-[1.5rem] items-center justify-center rounded-full px-1 text-xs ${
                  isToday ? 'bg-indigo-600 font-semibold text-white' : outside ? 'text-gray-400 hover:bg-gray-100' : 'font-medium text-gray-700 hover:bg-gray-100'
                }`}
              >
                {d.getDate() === 1 ? `${MONTHS[d.getMonth()].slice(0, 3)} 1` : d.getDate()}
              </button>
              <div className="mt-1 space-y-0.5">
                {visible.map((e) => {
                  const cal = calendarById[e.calendar];
                  return e.start === undefined ? (
                    <div key={e.id} className={`truncate rounded px-1.5 py-px text-[11px] font-semibold text-white ${cal.fill}`}>
                      {e.title}
                    </div>
                  ) : (
                    <div key={e.id} className="flex items-center gap-1.5 rounded px-1 py-px text-[11px] text-gray-700 hover:bg-gray-100">
                      <span className={`h-1.5 w-1.5 shrink-0 rounded-full ${cal.dot}`} />
                      <span className="shrink-0 text-gray-500">{shortTime(e.start)}</span>
                      <span className="truncate font-medium">{e.title}</span>
                    </div>
                  );
                })}
                {list.length > visible.length && (
                  <div className="px-1 text-[11px] font-semibold text-gray-500">{list.length - visible.length} more</div>
                )}
              </div>
            </div>
          );
        })}
      </div>
    </div>
  );
}

function SelectField({ id, label, value, onChange, options }: { id: string; label: string; value: string; onChange: (v: string) => void; options: { value: string; label: string }[] }) {
  return (
    <div>
      <label htmlFor={id} className="block text-xs font-medium text-gray-600">
        {label}
      </label>
      <div className="relative mt-1">
        <select
          id={id}
          value={value}
          onChange={(e) => onChange(e.target.value)}
          className="block w-full appearance-none rounded-lg border border-gray-300 bg-white py-2 pl-3 pr-8 text-sm text-gray-900 shadow-sm focus:border-indigo-500 focus:outline-none focus:ring-2 focus:ring-indigo-500/30"
        >
          {options.map((o) => (
            <option key={o.value} value={o.value}>
              {o.label}
            </option>
          ))}
        </select>
        <ChevronDown className="pointer-events-none absolute right-2.5 top-2.5 h-4 w-4 text-gray-400" />
      </div>
    </div>
  );
}

type Draft = { title: string; date: string; start: number; end: number; calendar: CalendarId };

function NewEventModal({ days, onClose, onSave }: { days: Date[]; onClose: () => void; onSave: (d: Draft) => void }) {
  const defaultDay = days.find((d) => sameDay(d, TODAY)) ?? days[1];
  const [draft, setDraft] = useState<Draft>({ title: '', date: dateKey(defaultDay), start: 12, end: 13, calendar: 'work' });
  const [touched, setTouched] = useState(false);
  const error = touched && !draft.title.trim() ? 'Add a title for the event.' : null;

  const slots = Array.from({ length: 48 }, (_, i) => i / 2);
  const startOptions = slots.map((h) => ({ value: String(h), label: formatTime(h) }));
  const endOptions = slots.filter((h) => h > draft.start).concat([24]).map((h) => ({ value: String(h), label: h === 24 ? '12:00 AM' : formatTime(h) }));

  function setStart(v: string) {
    const start = Number(v);
    setDraft((d) => ({ ...d, start, end: d.end <= start ? Math.min(start + 1, 24) : d.end }));
  }

  function submit(e: FormEvent) {
    e.preventDefault();
    setTouched(true);
    if (!draft.title.trim()) return;
    onSave({ ...draft, title: draft.title.trim() });
  }

  return (
    <div className="fixed inset-0 z-50 flex items-center justify-center p-4">
      <div className="absolute inset-0 bg-gray-900/40 backdrop-blur-sm" onClick={onClose} />
      <form onSubmit={submit} role="dialog" aria-modal="true" aria-labelledby="new-event-heading" className="relative w-full max-w-md rounded-2xl bg-white shadow-2xl ring-1 ring-gray-900/5">
        <div className="flex items-center justify-between border-b border-gray-100 px-6 py-4">
          <h2 id="new-event-heading" className="text-base font-semibold text-gray-900">
            New event
          </h2>
          <button type="button" onClick={onClose} className="rounded-md p-1 text-gray-400 hover:bg-gray-100" aria-label="Close">
            <X className="h-5 w-5" />
          </button>
        </div>
        <div className="space-y-4 px-6 py-5">
          <div>
            <label htmlFor="event-title" className="block text-xs font-medium text-gray-600">
              Title
            </label>
            <input
              id="event-title"
              autoComplete="off"
              value={draft.title}
              onChange={(e) => setDraft((d) => ({ ...d, title: e.target.value }))}
              placeholder="e.g. Coffee with Sam"
              className={`mt-1 block w-full rounded-lg border px-3 py-2 text-sm shadow-sm focus:outline-none focus:ring-2 ${
                error ? 'border-rose-300 focus:ring-rose-500/30' : 'border-gray-300 focus:border-indigo-500 focus:ring-indigo-500/30'
              }`}
            />
            {error && <p className="mt-1.5 text-xs text-rose-600">{error}</p>}
          </div>
          <div className="grid grid-cols-[1.3fr_1fr_1fr] gap-3">
            <SelectField
              id="event-day"
              label="Day"
              value={draft.date}
              onChange={(v) => setDraft((d) => ({ ...d, date: v }))}
              options={days.map((d) => ({ value: dateKey(d), label: `${WEEKDAYS[d.getDay()]}, ${MONTHS[d.getMonth()].slice(0, 3)} ${d.getDate()}` }))}
            />
            <SelectField id="event-start" label="Starts" value={String(draft.start)} onChange={setStart} options={startOptions} />
            <SelectField id="event-end" label="Ends" value={String(draft.end)} onChange={(v) => setDraft((d) => ({ ...d, end: Number(v) }))} options={endOptions} />
          </div>
          <div>
            <span className="block text-xs font-medium text-gray-600">Calendar</span>
            <div className="mt-1.5 flex flex-wrap gap-2">
              {calendars
                .filter((c) => c.id !== 'holidays')
                .map((c) => (
                  <button
                    key={c.id}
                    type="button"
                    data-calendar-option={c.id}
                    onClick={() => setDraft((d) => ({ ...d, calendar: c.id }))}
                    className={`inline-flex items-center gap-1.5 rounded-full border px-3 py-1 text-xs font-medium ${
                      draft.calendar === c.id ? 'border-indigo-500 bg-indigo-50 text-indigo-700' : 'border-gray-200 text-gray-600 hover:bg-gray-50'
                    }`}
                  >
                    <span className={`h-2 w-2 rounded-full ${c.dot}`} />
                    {c.name}
                  </button>
                ))}
            </div>
          </div>
          <div className="flex items-center gap-2 rounded-lg bg-gray-50 px-3 py-2 text-xs text-gray-500">
            <Globe className="h-4 w-4 shrink-0 text-gray-400" />
            Pacific Time — San Francisco (GMT-7)
          </div>
        </div>
        <div className="flex justify-end gap-3 rounded-b-2xl bg-gray-50 px-6 py-4">
          <button type="button" onClick={onClose} className="rounded-lg px-4 py-2 text-sm font-medium text-gray-700 hover:bg-gray-100">
            Cancel
          </button>
          <button type="submit" id="save-event" className="rounded-lg bg-indigo-600 px-4 py-2 text-sm font-medium text-white shadow-sm hover:bg-indigo-500">
            Save event
          </button>
        </div>
      </form>
    </div>
  );
}

// ---------------------------------------------------------------------------
// App
// ---------------------------------------------------------------------------

type View = 'week' | 'month';

export default function App() {
  const [events, setEvents] = useState<CalEvent[]>(seedEvents);
  const [anchor, setAnchor] = useState(TODAY);
  const [view, setView] = useState<View>('week');
  const [visible, setVisible] = useState<Record<CalendarId, boolean>>({ work: true, team: true, personal: true, focus: true, holidays: true });
  const [showModal, setShowModal] = useState(false);
  const [toast, setToast] = useState<string | null>(null);

  const shown = useMemo(() => events.filter((e) => visible[e.calendar]), [events, visible]);
  const weekDays = Array.from({ length: 7 }, (_, i) => addDays(startOfWeek(anchor), i));

  const upNext = sortEvents(shown.filter((e) => e.date === dateKey(TODAY) && e.start !== undefined && e.start >= NOW_HOURS)).slice(0, 2);

  function step(n: number) {
    if (view === 'week') setAnchor((a) => addDays(a, 7 * n));
    else setAnchor((a) => new Date(a.getFullYear(), a.getMonth() + n, 1));
  }

  function save(d: Draft) {
    setEvents((es) => [...es, { id: Math.max(...es.map((e) => e.id)) + 1, title: d.title, date: d.date, start: d.start, end: d.end, calendar: d.calendar }]);
    setVisible((v) => ({ ...v, [d.calendar]: true }));
    setShowModal(false);
    const [y, m, day] = d.date.split('-').map(Number);
    const date = new Date(y, m - 1, day);
    setToast(`${d.title} · ${WEEKDAYS[date.getDay()]}, ${MONTHS[date.getMonth()].slice(0, 3)} ${date.getDate()}, ${formatRange(d.start, d.end)}`);
  }

  const first = weekDays[0];
  const last = weekDays[6];
  const title =
    view === 'month'
      ? `${MONTHS[anchor.getMonth()]} ${anchor.getFullYear()}`
      : first.getMonth() === last.getMonth()
        ? `${MONTHS[first.getMonth()]} ${first.getFullYear()}`
        : `${MONTHS[first.getMonth()].slice(0, 3)} – ${MONTHS[last.getMonth()].slice(0, 3)} ${last.getFullYear()}`;

  return (
    <div className="flex h-screen flex-col overflow-hidden bg-white font-sans text-gray-900">
      <header className="flex h-14 shrink-0 items-center gap-4 border-b border-gray-200 px-4">
        <button className="rounded-md p-2 text-gray-500 hover:bg-gray-100" aria-label="Menu">
          <Menu className="h-5 w-5" />
        </button>
        <div className="flex items-center gap-2">
          <span className="flex h-8 w-8 items-center justify-center rounded-lg bg-gradient-to-br from-indigo-500 to-violet-600 text-white shadow-sm">
            <Calendar className="h-4 w-4" />
          </span>
          <span className="text-base font-semibold tracking-tight">Cadence</span>
        </div>
        <div className="relative ml-8">
          <Search className="absolute left-3 top-2.5 h-4 w-4 text-gray-400" />
          <input placeholder="Search events, people, rooms" className="w-80 rounded-lg border border-gray-200 bg-gray-50 py-2 pl-9 pr-3 text-sm placeholder:text-gray-400 focus:bg-white focus:outline-none" />
        </div>
        <div className="ml-auto flex items-center gap-1">
          <button className="rounded-md p-2 text-gray-500 hover:bg-gray-100" aria-label="Settings">
            <Settings className="h-5 w-5" />
          </button>
          <button className="relative rounded-md p-2 text-gray-500 hover:bg-gray-100" aria-label="Notifications">
            <Bell className="h-5 w-5" />
            <span className="absolute right-2 top-2 h-2 w-2 rounded-full bg-rose-500 ring-2 ring-white" />
          </button>
          <span className="ml-2 inline-flex h-8 w-8 items-center justify-center rounded-full bg-gradient-to-br from-pink-500 to-orange-400 text-xs font-semibold text-white">MC</span>
        </div>
      </header>

      <div className="flex min-h-0 flex-1">
        <aside className="flex w-64 shrink-0 flex-col gap-6 overflow-y-auto border-r border-gray-200 p-4">
          <button
            id="new-event"
            onClick={() => setShowModal(true)}
            className="inline-flex items-center justify-center gap-2 rounded-lg bg-indigo-600 px-4 py-2.5 text-sm font-medium text-white shadow-sm hover:bg-indigo-500"
          >
            <Plus className="h-4 w-4" />
            New event
          </button>

          <MiniMonth anchor={anchor} onPick={(d) => setAnchor(d)} />

          <div>
            <div className="flex items-center justify-between px-2">
              <h3 className="text-xs font-semibold uppercase tracking-wide text-gray-500">My calendars</h3>
              <button className="rounded p-0.5 text-gray-400 hover:bg-gray-100 hover:text-gray-700" aria-label="Add calendar">
                <Plus className="h-4 w-4" />
              </button>
            </div>
            <div className="mt-2 space-y-0.5">
              {calendars.map((c) => (
                <CalendarToggle key={c.id} cal={c} on={visible[c.id]} onToggle={() => setVisible((v) => ({ ...v, [c.id]: !v[c.id] }))} />
              ))}
            </div>
          </div>

          <div className="mt-auto rounded-xl border border-gray-200 p-3">
            <div className="flex items-center gap-1.5 text-xs font-semibold uppercase tracking-wide text-gray-500">
              <Clock className="h-3.5 w-3.5" />
              Up next today
            </div>
            {upNext.length === 0 ? (
              <p className="mt-2 text-sm text-gray-500">Nothing else today.</p>
            ) : (
              <ul className="mt-2 space-y-2">
                {upNext.map((e) => (
                  <li key={e.id} className="flex gap-2.5">
                    <span className={`mt-1 h-8 w-1 shrink-0 rounded-full ${calendarById[e.calendar].dot}`} />
                    <div className="min-w-0">
                      <p className="truncate text-sm font-medium text-gray-900">{e.title}</p>
                      <p className="flex items-center gap-1 truncate text-xs text-gray-500">
                        {formatRange(e.start!, e.end!)}
                        {e.location && <span className="text-gray-400">· {e.location}</span>}
                      </p>
                    </div>
                  </li>
                ))}
              </ul>
            )}
            {upNext[0]?.location === 'Zoom' && (
              <button className="mt-3 inline-flex w-full items-center justify-center gap-1.5 rounded-md bg-gray-900 px-3 py-1.5 text-xs font-medium text-white hover:bg-gray-800">
                <Video className="h-3.5 w-3.5" />
                Join call
              </button>
            )}
          </div>
        </aside>

        <main className="flex min-w-0 flex-1 flex-col">
          <div className="flex shrink-0 items-center gap-3 border-b border-gray-200 px-6 py-3">
            <button
              id="today"
              onClick={() => setAnchor(TODAY)}
              className="rounded-lg border border-gray-200 px-3 py-1.5 text-sm font-medium text-gray-700 shadow-sm hover:bg-gray-50"
            >
              Today
            </button>
            <div className="flex items-center">
              <button id="prev" onClick={() => step(-1)} className="rounded-md p-1.5 text-gray-500 hover:bg-gray-100" aria-label="Previous">
                <ChevronLeft className="h-5 w-5" />
              </button>
              <button id="next" onClick={() => step(1)} className="rounded-md p-1.5 text-gray-500 hover:bg-gray-100" aria-label="Next">
                <ChevronRight className="h-5 w-5" />
              </button>
            </div>
            <h2 className="text-xl font-semibold text-gray-900">{title}</h2>
            {view === 'week' && <span className="rounded-full bg-gray-100 px-2 py-0.5 text-xs font-medium text-gray-600">Week {isoWeek(weekDays[1])}</span>}
            <div className="ml-auto inline-flex rounded-lg bg-gray-100 p-1" role="tablist">
              {(['week', 'month'] as View[]).map((v) => (
                <button
                  key={v}
                  id={`view-${v}`}
                  role="tab"
                  aria-selected={view === v}
                  onClick={() => setView(v)}
                  className={`rounded-md px-3 py-1 text-sm font-medium capitalize ${view === v ? 'bg-white text-gray-900 shadow-sm' : 'text-gray-500 hover:text-gray-700'}`}
                >
                  {v}
                </button>
              ))}
            </div>
          </div>

          {view === 'week' ? (
            <WeekView anchor={anchor} events={shown} />
          ) : (
            <MonthView
              anchor={anchor}
              events={shown}
              onPickDay={(d) => {
                setAnchor(d);
                setView('week');
              }}
            />
          )}
        </main>
      </div>

      {showModal && <NewEventModal days={weekDays} onClose={() => setShowModal(false)} onSave={save} />}

      {toast && (
        <div className="fixed bottom-6 right-6 z-40 flex w-96 items-start gap-3 rounded-xl bg-white p-4 shadow-lg ring-1 ring-black/5" role="status">
          <CircleCheck className="h-5 w-5 shrink-0 text-emerald-500" />
          <div className="min-w-0 flex-1">
            <p className="text-sm font-medium text-gray-900">Event created</p>
            <p className="mt-0.5 truncate text-sm text-gray-500">{toast}</p>
          </div>
          <button onClick={() => setToast(null)} className="rounded p-0.5 text-gray-400 hover:bg-gray-100" aria-label="Dismiss">
            <X className="h-4 w-4" />
          </button>
        </div>
      )}
    </div>
  );
}
