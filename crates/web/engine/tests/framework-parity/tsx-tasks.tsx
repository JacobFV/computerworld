// The task tracker of react18-tasks.html, written as a React + TypeScript module an
// agent would write (and could run in Chrome with any React toolchain). `cw-tsx build`
// compiles it to the UI IR that cw-ui runs without a JS VM, and to the plain script
// (tsx-tasks.js) that React 18 runs as the fallback. tsx-tasks.html is the shell both
// run in; tests/framework_parity.rs checks the two against each other and Chromium.
import { useMemo, useState } from 'react';
import type { FormEvent } from 'react';
import { createRoot } from 'react-dom/client';

type Person = 'ada' | 'bo' | 'cy';
type TagName = 'bug' | 'feature' | 'chore';
type Status = 'todo' | 'doing' | 'done';
type View = 'list' | 'board';
type Filter = 'all' | 'open' | 'done';

interface Task {
  id: number;
  title: string;
  tag: TagName;
  who: Person;
  due: string;
  status: Status;
}

const PEOPLE: Record<Person, [string, string]> = {
  ada: ['AL', 'rgb(200, 90, 60)'],
  bo: ['BK', 'rgb(60, 120, 190)'],
  cy: ['CN', 'rgb(120, 90, 170)'],
};

const INITIAL: Task[] = [
  { id: 1, title: 'Checkout fails when the basket holds a gift card', tag: 'bug', who: 'ada', due: 'Sep 24', status: 'doing' },
  { id: 2, title: 'Export invoices as CSV', tag: 'feature', who: 'bo', due: 'Sep 26', status: 'todo' },
  { id: 3, title: 'Rotate the staging certificates', tag: 'chore', who: 'cy', due: 'Sep 22', status: 'done' },
  { id: 4, title: 'Search results lose their filters after paging back', tag: 'bug', who: 'bo', due: 'Sep 25', status: 'todo' },
  { id: 5, title: 'Dark mode for the settings pages', tag: 'feature', who: 'ada', due: 'Oct 02', status: 'todo' },
  { id: 6, title: 'Upgrade the build image', tag: 'chore', who: 'cy', due: 'Sep 20', status: 'done' },
];

const COLUMNS: [Status, string][] = [['todo', 'To do'], ['doing', 'In progress'], ['done', 'Done']];

function Avatar({ who }: { who: Person }) {
  const [initials, colour] = PEOPLE[who];
  return <span className="avatar" style={{ backgroundColor: colour }}>{initials}</span>;
}

function Tag({ tag }: { tag: TagName }) {
  return <span className={'tag ' + tag}>{tag}</span>;
}

function Stats({ tasks }: { tasks: Task[] }) {
  const done = tasks.filter((t) => t.status === 'done').length;
  const pct = tasks.length ? Math.round((done * 100) / tasks.length) : 0;
  return (
    <section className="stats">
      <div className="stat" id="stat-total"><div className="label">Total</div><div className="value">{tasks.length}</div></div>
      <div className="stat" id="stat-open"><div className="label">Open</div><div className="value">{tasks.length - done}</div></div>
      <div className="stat" id="stat-done">
        <div className="label">Done</div>
        <div className="value">{pct + '%'}</div>
        <div className="bar"><div style={{ width: pct + '%' }} /></div>
      </div>
    </section>
  );
}

interface RowProps {
  task: Task;
  onToggle: (id: number) => void;
}

function Row({ task, onToggle }: RowProps) {
  return (
    <li className={'task' + (task.status === 'done' ? ' done' : '')} id={'task-' + task.id}>
      <button className="check" id={'check-' + task.id} aria-label="toggle" onClick={() => onToggle(task.id)} />
      <span className="title">{task.title}</span>
      <Tag tag={task.tag} />
      <Avatar who={task.who} />
      <span className="due">{task.due}</span>
    </li>
  );
}

function Board({ tasks }: { tasks: Task[] }) {
  return (
    <section className="board">
      {COLUMNS.map(([status, name]) => {
        const cards = tasks.filter((t) => t.status === status);
        return (
          <div className="column" key={status} id={'column-' + status}>
            <h3><span>{name}</span><span className="count">{cards.length}</span></h3>
            {cards.map((t) => (
              <div className="card" key={t.id}>
                <div>{t.title}</div>
                <div className="meta"><Tag tag={t.tag} /><Avatar who={t.who} /></div>
              </div>
            ))}
          </div>
        );
      })}
    </section>
  );
}

function App() {
  const [tasks, setTasks] = useState<Task[]>(INITIAL);
  const [view, setView] = useState<View>('list');
  const [filter, setFilter] = useState<Filter>('all');
  const [draft, setDraft] = useState('');
  const visible = useMemo(
    () => tasks.filter((t) => filter === 'all' || (filter === 'open' ? t.status !== 'done' : t.status === 'done')),
    [tasks, filter],
  );
  const counts: Record<Filter, number> = {
    all: tasks.length,
    open: tasks.filter((t) => t.status !== 'done').length,
    done: tasks.filter((t) => t.status === 'done').length,
  };
  const toggle = (id: number) =>
    setTasks((ts) => ts.map((t) => (t.id === id ? { ...t, status: t.status === 'done' ? 'todo' : 'done' } : t)));
  const add = (e: FormEvent) => {
    e.preventDefault();
    const title = draft.trim();
    if (!title) return;
    setTasks((ts) => [...ts, { id: ts.length + 1, title, tag: 'feature', who: 'cy', due: 'Oct 09', status: 'todo' }]);
    setDraft('');
  };
  const tab = (name: View, label: string) => (
    <button className={'tab' + (view === name ? ' active' : '')} id={'tab-' + name} onClick={() => setView(name)}>{label}</button>
  );
  const filterButton = (name: Filter, label: string) => (
    <button className={'filter' + (filter === name ? ' active' : '')} id={'filter-' + name} onClick={() => setFilter(name)}>
      <span>{label}</span><span className="count">{counts[name]}</span>
    </button>
  );
  return (
    <div className="shell">
      <header className="top">
        <span className="brand">Tracker</span>
        <nav className="tabs">{tab('list', 'List')}{tab('board', 'Board')}</nav>
        <span className="spacer" />
        <span className="who"><span>Cy Nakamura</span><Avatar who="cy" /></span>
      </header>
      <div className="body">
        <aside className="side">
          <h2>Filters</h2>
          {filterButton('all', 'All tasks')}{filterButton('open', 'Open')}{filterButton('done', 'Done')}
        </aside>
        <main className="main">
          <Stats tasks={tasks} />
          {view === 'list' ? (
            <>
              <form className="add" onSubmit={add}>
                <input id="new-task" placeholder="Add a task" value={draft} onChange={(e) => setDraft(e.target.value)} />
                <button type="submit" id="add-task">Add</button>
              </form>
              {visible.length ? (
                <ul className="list" id="list">{visible.map((t) => <Row key={t.id} task={t} onToggle={toggle} />)}</ul>
              ) : (
                <div className="list empty">Nothing here</div>
              )}
            </>
          ) : (
            <Board tasks={tasks} />
          )}
        </main>
      </div>
    </div>
  );
}

createRoot(document.getElementById('app')!).render(<App />);
