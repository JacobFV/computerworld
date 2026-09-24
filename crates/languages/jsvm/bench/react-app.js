'use strict';
const PEOPLE = { ada: ['AL', 'rgb(200, 90, 60)'], bo: ['BK', 'rgb(60, 120, 190)'], cy: ['CN', 'rgb(120, 90, 170)'] };
const INITIAL = [
  { id: 1, title: 'Checkout fails when the basket holds a gift card', tag: 'bug', who: 'ada', due: 'Sep 24', status: 'doing' },
  { id: 2, title: 'Export invoices as CSV', tag: 'feature', who: 'bo', due: 'Sep 26', status: 'todo' },
  { id: 3, title: 'Rotate the staging certificates', tag: 'chore', who: 'cy', due: 'Sep 22', status: 'done' },
  { id: 4, title: 'Search results lose their filters after paging back', tag: 'bug', who: 'bo', due: 'Sep 25', status: 'todo' },
  { id: 5, title: 'Dark mode for the settings pages', tag: 'feature', who: 'ada', due: 'Oct 02', status: 'todo' },
  { id: 6, title: 'Upgrade the build image', tag: 'chore', who: 'cy', due: 'Sep 20', status: 'done' },
];
const COLUMNS = [['todo', 'To do'], ['doing', 'In progress'], ['done', 'Done']];
const h = React.createElement;
const { useState, useMemo } = React;

function Avatar({ who }) {
  const [initials, colour] = PEOPLE[who];
  return h('span', { className: 'avatar', style: { backgroundColor: colour } }, initials);
}

function Tag({ tag }) {
  return h('span', { className: 'tag ' + tag }, tag);
}

function Stats({ tasks }) {
  const done = tasks.filter((t) => t.status === 'done').length;
  const pct = tasks.length ? Math.round((done * 100) / tasks.length) : 0;
  return h('section', { className: 'stats' },
    h('div', { className: 'stat', id: 'stat-total' }, h('div', { className: 'label' }, 'Total'), h('div', { className: 'value' }, tasks.length)),
    h('div', { className: 'stat', id: 'stat-open' }, h('div', { className: 'label' }, 'Open'), h('div', { className: 'value' }, tasks.length - done)),
    h('div', { className: 'stat', id: 'stat-done' }, h('div', { className: 'label' }, 'Done'), h('div', { className: 'value' }, pct + '%'),
      h('div', { className: 'bar' }, h('div', { style: { width: pct + '%' } }))));
}

function Row({ task, onToggle }) {
  return h('li', { className: 'task' + (task.status === 'done' ? ' done' : ''), id: 'task-' + task.id },
    h('button', { className: 'check', id: 'check-' + task.id, 'aria-label': 'toggle', onClick: () => onToggle(task.id) }),
    h('span', { className: 'title' }, task.title),
    h(Tag, { tag: task.tag }),
    h(Avatar, { who: task.who }),
    h('span', { className: 'due' }, task.due));
}

function Board({ tasks }) {
  return h('section', { className: 'board' }, COLUMNS.map(([status, name]) => {
    const cards = tasks.filter((t) => t.status === status);
    return h('div', { className: 'column', key: status, id: 'column-' + status },
      h('h3', null, h('span', null, name), h('span', { className: 'count' }, cards.length)),
      cards.map((t) => h('div', { className: 'card', key: t.id },
        h('div', null, t.title),
        h('div', { className: 'meta' }, h(Tag, { tag: t.tag }), h(Avatar, { who: t.who })))));
  }));
}

function App() {
  const [tasks, setTasks] = useState(INITIAL);
  const [view, setView] = useState('list');
  const [filter, setFilter] = useState('all');
  const [draft, setDraft] = useState('');
  const visible = useMemo(() => tasks.filter((t) =>
    filter === 'all' || (filter === 'open' ? t.status !== 'done' : t.status === 'done')), [tasks, filter]);
  const counts = {
    all: tasks.length,
    open: tasks.filter((t) => t.status !== 'done').length,
    done: tasks.filter((t) => t.status === 'done').length,
  };
  const toggle = (id) => setTasks((ts) => ts.map((t) => (t.id === id ? { ...t, status: t.status === 'done' ? 'todo' : 'done' } : t)));
  const add = (e) => {
    e.preventDefault();
    const title = draft.trim();
    if (!title) return;
    setTasks((ts) => [...ts, { id: ts.length + 1, title, tag: 'feature', who: 'cy', due: 'Oct 09', status: 'todo' }]);
    setDraft('');
  };
  const tab = (name, label) => h('button', { className: 'tab' + (view === name ? ' active' : ''), id: 'tab-' + name, onClick: () => setView(name) }, label);
  const filterButton = (name, label) => h('button', { className: 'filter' + (filter === name ? ' active' : ''), id: 'filter-' + name, onClick: () => setFilter(name) },
    h('span', null, label), h('span', { className: 'count' }, counts[name]));
  return h('div', { className: 'shell' },
    h('header', { className: 'top' },
      h('span', { className: 'brand' }, 'Tracker'),
      h('nav', { className: 'tabs' }, tab('list', 'List'), tab('board', 'Board')),
      h('span', { className: 'spacer' }),
      h('span', { className: 'who' }, h('span', null, 'Cy Nakamura'), h(Avatar, { who: 'cy' }))),
    h('div', { className: 'body' },
      h('aside', { className: 'side' },
        h('h2', null, 'Filters'),
        filterButton('all', 'All tasks'), filterButton('open', 'Open'), filterButton('done', 'Done')),
      h('main', { className: 'main' },
        h(Stats, { tasks }),
        view === 'list'
          ? h(React.Fragment, null,
            h('form', { className: 'add', onSubmit: add },
              h('input', { id: 'new-task', placeholder: 'Add a task', value: draft, onChange: (e) => setDraft(e.target.value) }),
              h('button', { type: 'submit', id: 'add-task' }, 'Add')),
            visible.length
              ? h('ul', { className: 'list', id: 'list' }, visible.map((t) => h(Row, { key: t.id, task: t, onToggle: toggle })))
              : h('div', { className: 'list empty' }, 'Nothing here'))
          : h(Board, { tasks }))));
}

ReactDOM.createRoot(document.getElementById('app')).render(h(App));
