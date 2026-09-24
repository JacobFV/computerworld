// Compiled by cw-tsx from tsx-tasks.tsx: types stripped, JSX as React.createElement.
'use strict';
const { useMemo, useState } = React;
const { createRoot } = ReactDOM;
const PEOPLE = {
	ada: ['AL', 'rgb(200, 90, 60)'],
	bo: ['BK', 'rgb(60, 120, 190)'],
	cy: ['CN', 'rgb(120, 90, 170)']
};
const INITIAL = [
	{
		id: 1,
		title: 'Checkout fails when the basket holds a gift card',
		tag: 'bug',
		who: 'ada',
		due: 'Sep 24',
		status: 'doing'
	},
	{
		id: 2,
		title: 'Export invoices as CSV',
		tag: 'feature',
		who: 'bo',
		due: 'Sep 26',
		status: 'todo'
	},
	{
		id: 3,
		title: 'Rotate the staging certificates',
		tag: 'chore',
		who: 'cy',
		due: 'Sep 22',
		status: 'done'
	},
	{
		id: 4,
		title: 'Search results lose their filters after paging back',
		tag: 'bug',
		who: 'bo',
		due: 'Sep 25',
		status: 'todo'
	},
	{
		id: 5,
		title: 'Dark mode for the settings pages',
		tag: 'feature',
		who: 'ada',
		due: 'Oct 02',
		status: 'todo'
	},
	{
		id: 6,
		title: 'Upgrade the build image',
		tag: 'chore',
		who: 'cy',
		due: 'Sep 20',
		status: 'done'
	}
];
const COLUMNS = [
	['todo', 'To do'],
	['doing', 'In progress'],
	['done', 'Done']
];
function Avatar({ who }) {
	const [initials, colour] = PEOPLE[who];
	return React.createElement('span', {
		className: 'avatar',
		style: { backgroundColor: colour }
	}, initials);
}
function Tag({ tag }) {
	return React.createElement('span', { className: 'tag ' + tag }, tag);
}
function Stats({ tasks }) {
	const done = tasks.filter((t) => t.status === 'done').length;
	const pct = tasks.length ? Math.round(done * 100 / tasks.length) : 0;
	return React.createElement('section', { className: 'stats' }, React.createElement('div', {
		className: 'stat',
		id: 'stat-total'
	}, React.createElement('div', { className: 'label' }, 'Total'), React.createElement('div', { className: 'value' }, tasks.length)), React.createElement('div', {
		className: 'stat',
		id: 'stat-open'
	}, React.createElement('div', { className: 'label' }, 'Open'), React.createElement('div', { className: 'value' }, tasks.length - done)), React.createElement('div', {
		className: 'stat',
		id: 'stat-done'
	}, React.createElement('div', { className: 'label' }, 'Done'), React.createElement('div', { className: 'value' }, pct + '%'), React.createElement('div', { className: 'bar' }, React.createElement('div', { style: { width: pct + '%' } }))));
}
function Row({ task, onToggle }) {
	return React.createElement('li', {
		className: 'task' + (task.status === 'done' ? ' done' : ''),
		id: 'task-' + task.id
	}, React.createElement('button', {
		className: 'check',
		id: 'check-' + task.id,
		'aria-label': 'toggle',
		onClick: () => onToggle(task.id)
	}), React.createElement('span', { className: 'title' }, task.title), React.createElement(Tag, { tag: task.tag }), React.createElement(Avatar, { who: task.who }), React.createElement('span', { className: 'due' }, task.due));
}
function Board({ tasks }) {
	return React.createElement('section', { className: 'board' }, COLUMNS.map(([status, name]) => {
		const cards = tasks.filter((t) => t.status === status);
		return React.createElement('div', {
			className: 'column',
			key: status,
			id: 'column-' + status
		}, React.createElement('h3', null, React.createElement('span', null, name), React.createElement('span', { className: 'count' }, cards.length)), cards.map((t) => React.createElement('div', {
			className: 'card',
			key: t.id
		}, React.createElement('div', null, t.title), React.createElement('div', { className: 'meta' }, React.createElement(Tag, { tag: t.tag }), React.createElement(Avatar, { who: t.who })))));
	}));
}
function App() {
	const [tasks, setTasks] = useState(INITIAL);
	const [view, setView] = useState('list');
	const [filter, setFilter] = useState('all');
	const [draft, setDraft] = useState('');
	const visible = useMemo(() => tasks.filter((t) => filter === 'all' || (filter === 'open' ? t.status !== 'done' : t.status === 'done')), [tasks, filter]);
	const counts = {
		all: tasks.length,
		open: tasks.filter((t) => t.status !== 'done').length,
		done: tasks.filter((t) => t.status === 'done').length
	};
	const toggle = (id) => setTasks((ts) => ts.map((t) => t.id === id ? {
		...t,
		status: t.status === 'done' ? 'todo' : 'done'
	} : t));
	const add = (e) => {
		e.preventDefault();
		const title = draft.trim();
		if (!title) return;
		setTasks((ts) => [...ts, {
			id: ts.length + 1,
			title,
			tag: 'feature',
			who: 'cy',
			due: 'Oct 09',
			status: 'todo'
		}]);
		setDraft('');
	};
	const tab = (name, label) => React.createElement('button', {
		className: 'tab' + (view === name ? ' active' : ''),
		id: 'tab-' + name,
		onClick: () => setView(name)
	}, label);
	const filterButton = (name, label) => React.createElement('button', {
		className: 'filter' + (filter === name ? ' active' : ''),
		id: 'filter-' + name,
		onClick: () => setFilter(name)
	}, React.createElement('span', null, label), React.createElement('span', { className: 'count' }, counts[name]));
	return React.createElement('div', { className: 'shell' }, React.createElement('header', { className: 'top' }, React.createElement('span', { className: 'brand' }, 'Tracker'), React.createElement('nav', { className: 'tabs' }, tab('list', 'List'), tab('board', 'Board')), React.createElement('span', { className: 'spacer' }), React.createElement('span', { className: 'who' }, React.createElement('span', null, 'Cy Nakamura'), React.createElement(Avatar, { who: 'cy' }))), React.createElement('div', { className: 'body' }, React.createElement('aside', { className: 'side' }, React.createElement('h2', null, 'Filters'), filterButton('all', 'All tasks'), filterButton('open', 'Open'), filterButton('done', 'Done')), React.createElement('main', { className: 'main' }, React.createElement(Stats, { tasks }), view === 'list' ? React.createElement(React.Fragment, null, React.createElement('form', {
		className: 'add',
		onSubmit: add
	}, React.createElement('input', {
		id: 'new-task',
		placeholder: 'Add a task',
		value: draft,
		onChange: (e) => setDraft(e.target.value)
	}), React.createElement('button', {
		type: 'submit',
		id: 'add-task'
	}, 'Add')), visible.length ? React.createElement('ul', {
		className: 'list',
		id: 'list'
	}, visible.map((t) => React.createElement(Row, {
		key: t.id,
		task: t,
		onToggle: toggle
	}))) : React.createElement('div', { className: 'list empty' }, 'Nothing here')) : React.createElement(Board, { tasks }))));
}
createRoot(document.getElementById('app')).render(React.createElement(App, null));
