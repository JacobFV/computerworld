// Compiled by cw-tsx from Notes.tsx: types stripped, JSX as React.createElement.
'use strict';
const __cw_m = [{}, {}, {}];
// ../sdk/cw.ts
(() => {
Object.defineProperties(__cw_m[1], { declaredStore: { enumerable: true, get: () => declaredStore }, useStore: { enumerable: true, get: () => useStore }, useEnv: { enumerable: true, get: () => useEnv } });
const { useEffect, useState, useSyncExternalStore } = React;
function declaredStore(init) {
	const saved = cw.state.get();
	const first = saved === null;
	const holder = { current: saved === null ? init() : saved };
	if (first) cw.state.set(holder.current);
	const listeners = new Set();
	const set = (next) => {
		if (Object.is(next, holder.current)) return;
		holder.current = next;
		cw.state.set(next);
		for (const listener of Array.from(listeners)) listener();
	};
	return {
		restored: !first,
		get: () => holder.current,
		set,
		update: (change) => set(change(holder.current)),
		subscribe: (listener) => {
			listeners.add(listener);
			return () => {
				listeners.delete(listener);
			};
		}
	};
}
function useStore(store) {
	return useSyncExternalStore(store.subscribe, store.get);
}
function useEnv() {
	const [env, setEnv] = useState(cw.env);
	useEffect(() => cw.onEnv(setEnv), []);
	return env;
}
})();
// Notes.tsx
(() => {
const __cw_i1 = __cw_m[1];
const { useEffect, useLayoutEffect, useRef } = React;
const { createRoot } = ReactDOM;
/** A note is bounded like every field the desktop keeps. */
const TEXT_LIMIT = 64 * 1024;
const store = __cw_i1.declaredStore(() => ({
	folder: cw.argument ? cw.argument.replace(/\/+$/, '') : 'Notes',
	entries: [],
	open: null,
	text: '',
	dirty: false,
	problem: null,
	editing: false
}));
/** Code point order, which is the order the machine sorts names in. */
function byCodePoint(a, b) {
	const x = Array.from(a);
	const y = Array.from(b);
	for (let i = 0; i < Math.min(x.length, y.length); i++) {
		const d = (x[i].codePointAt(0) ?? 0) - (y[i].codePointAt(0) ?? 0);
		if (d !== 0) return d;
	}
	return x.length - y.length;
}
function describe(error) {
	return error instanceof Error ? error.message : String(error);
}
function list() {
	const { folder } = store.get();
	cw.fs.list(folder).then((names) => store.update((s) => ({
		...s,
		entries: names.filter((n) => !n.endsWith('/')),
		problem: null
	})), (error) => store.update((s) => ({
		...s,
		problem: describe(error)
	})));
}
function save() {
	const s = store.get();
	if (s.open === null) {
		cw.refuse('no note is open');
		return false;
	}
	const path = `${s.folder}/${s.open}`;
	store.set({
		...s,
		dirty: false
	});
	// The folder may not exist yet on a machine that has never taken a note.
	cw.fs.mkdir(s.folder);
	cw.fs.writeFile(path, s.text).then(list);
	return true;
}
function newNote() {
	// A new note is named from the world clock, so two machines agree.
	const name = `note-${Math.floor(cw.now() / 1e6)}.txt`;
	store.update((s) => ({
		...s,
		open: name,
		editing: true,
		text: '',
		dirty: true,
		entries: s.entries.includes(name) ? s.entries : [...s.entries, name].sort(byCodePoint)
	}));
}
function open(name) {
	const s = store.get();
	if (!s.entries.includes(name)) {
		cw.refuse('note not found');
		return;
	}
	store.set({
		...s,
		open: name,
		editing: false,
		text: '',
		dirty: false
	});
	cw.fs.readFile(`${s.folder}/${name}`).then(
		(content) => store.update((now) => now.open === name ? {
			...now,
			text: content,
			dirty: false
		} : now),
		// A note that cannot be read fails the click that opened it.
		(error) => cw.refuse(describe(error))
	);
}
/** A phone's back button: the list again, and the keyboard goes down. Unsaved text is written first. */
function close() {
	if (store.get().dirty) save();
	store.update((s) => ({
		...s,
		open: null,
		editing: false,
		text: '',
		dirty: false
	}));
}
function edit(text) {
	store.update((s) => ({
		...s,
		text: text.slice(0, TEXT_LIMIT),
		dirty: true
	}));
}
/** Keys a note's text takes; any other key is refused, as the native Notes refused it. */
const EDITING_KEYS = new Set([
	'Backspace',
	'Delete',
	'Enter',
	'ArrowLeft',
	'ArrowRight',
	'ArrowUp',
	'ArrowDown',
	'Home',
	'End',
	'PageUp',
	'PageDown'
]);
function onKey(event) {
	const s = store.get();
	const command = event.ctrlKey || event.metaKey;
	if (command && event.key.toLowerCase() === 's') {
		event.preventDefault();
		save();
		return;
	}
	// One character, which may be outside the BMP (an emoji is two UTF-16 units).
	const printable = Array.from(event.key).length === 1 && !command && !event.altKey;
	if (s.open === null) {
		event.preventDefault();
		cw.refuse(event.key === 'Backspace' || event.key === 'Enter' ? 'no note is open' : `unsupported notes key ${event.key}`);
		return;
	}
	const inBody = event.target === document.getElementById('notes:body');
	if (!inBody) {
		// The note is open but its body does not have the focus (a phone before the
		// body is tapped): the keys that edit still edit it.
		if (event.key === 'Backspace') {
			event.preventDefault();
			edit(Array.from(s.text).slice(0, -1).join(''));
		} else if (event.key === 'Enter') {
			event.preventDefault();
			edit(s.text + '\n');
		} else if (printable) {
			event.preventDefault();
			edit(s.text + event.key);
		} else {
			event.preventDefault();
			cw.refuse(`unsupported notes key ${event.key}`);
		}
		return;
	}
	if (!printable && !EDITING_KEYS.has(event.key)) {
		event.preventDefault();
		cw.refuse(`unsupported notes key ${event.key}`);
	} else if (event.key === 'Backspace' && s.text === '') {
		// An erase in an empty note still counts as an edit, as it always has.
		store.set({
			...s,
			dirty: true
		});
	}
}
function title(platform) {
	return platform === 'windows' ? 'Sticky Notes' : platform === 'android' ? 'Keep' : 'Notes';
}
function Action(props) {
	return React.createElement('button', {
		id: props.id,
		className: `action ${props.place}${props.primary ? ' primary' : ''}`,
		onClick: props.onClick
	}, props.label);
}
function List(props) {
	const { s } = props;
	return React.createElement(React.Fragment, null, s.entries.map((name) => React.createElement('button', {
		key: name,
		id: `notes:open:${name}`,
		'aria-label': name,
		className: s.open === name ? 'row on' : 'row',
		onClick: () => open(name)
	}, name.replace(/\.txt$/, ''))));
}
/** The problem, or why the list is empty; it sits under the buttons. */
function Notice(props) {
	const { s } = props;
	if (s.problem !== null) {
		return React.createElement('p', {
			id: 'notes-problem',
			role: 'alert',
			className: 'notice listed'
		}, s.problem);
	}
	return s.entries.length === 0 ? React.createElement('p', { className: 'notice listed' }, 'No notes yet') : null;
}
function Buttons() {
	return React.createElement('div', { className: 'buttons' }, React.createElement(Action, {
		id: 'notes:new',
		place: 'new',
		label: 'New note',
		primary: true,
		onClick: newNote
	}), React.createElement(Action, {
		id: 'notes:reload',
		place: 'reload',
		label: 'Reload',
		primary: false,
		onClick: list
	}));
}
/** The open note: its name, Save, and the body, which scrolls when it is longer than the window. */
function Note(props) {
	const { s, phone, heading } = props;
	const name = (s.open ?? '').replace(/\.txt$/, '');
	return React.createElement('section', { className: phone ? 'note phone' : 'note' }, phone && React.createElement('button', {
		id: 'notes:close',
		'data-cw-back': '',
		className: 'back',
		'aria-label': 'Back to notes',
		onClick: close
	}, React.createElement('svg', {
		className: 'chevron',
		width: '20',
		height: '20',
		viewBox: '0 0 20 20',
		'aria-hidden': 'true'
	}, React.createElement('path', {
		d: 'M12.5 4.5 L7 10 L12.5 15.5',
		fill: 'none',
		stroke: 'currentColor',
		strokeWidth: '2.2',
		strokeLinecap: 'round',
		strokeLinejoin: 'round'
	})), React.createElement('span', { className: 'back-title' }, heading)), React.createElement('strong', { className: 'name' }, name), React.createElement('textarea', {
		id: 'notes:body',
		'data-page-id': 'notes-body',
		'data-cw-pane': 'note',
		'aria-label': 'Note',
		className: 'body',
		value: s.text,
		maxLength: TEXT_LIMIT,
		spellCheck: false,
		onChange: (e) => edit(e.currentTarget.value),
		onClick: () => store.update((now) => now.editing ? now : {
			...now,
			editing: true
		})
	}), s.text === '' && React.createElement('span', {
		className: 'empty',
		'aria-hidden': 'true'
	}, 'Empty note'), React.createElement('button', {
		id: 'notes:save',
		className: s.dirty ? 'action primary save' : 'action save',
		onClick: save
	}, s.dirty ? 'Save •' : 'Save'));
}
function Notes() {
	const s = __cw_i1.useStore(store);
	const env = __cw_i1.useEnv();
	const narrow = env.mobile || env.width < 480;
	const heading = title(env.platform);
	const body = useRef(null);
	// The body has the keyboard whenever a note is open on a desktop, and on a phone
	// once it was tapped (and only then): after every render, and whenever something
	// else took it.
	const wantsFocus = s.open !== null && (s.editing || !env.mobile);
	useLayoutEffect(() => {
		const field = document.getElementById('notes:body');
		body.current = field;
		if (wantsFocus && field && document.activeElement !== field) {
			field.focus();
			const end = field.value.length;
			field.setSelectionRange(end, end);
		} else if (!wantsFocus && field && document.activeElement === field) {
			// Focus is a function of the state and the platform, whatever came before.
			field.blur();
		}
	});
	useEffect(() => {
		const refocus = () => {
			const field = body.current;
			if (wantsFocus && field && document.activeElement !== field) field.focus();
		};
		document.addEventListener('focusin', refocus);
		return () => document.removeEventListener('focusin', refocus);
	}, [wantsFocus]);
	// A first launch lists the folder; a restored window already holds its listing.
	useEffect(() => {
		if (!store.restored) list();
	}, []);
	useEffect(() => {
		document.addEventListener('keydown', onKey);
		return () => document.removeEventListener('keydown', onKey);
	}, []);
	useEffect(() => {
		cw.window.set({
			document: s.open === null ? '' : `${s.folder}/${s.open}`,
			caption: s.open ?? '',
			modified: s.dirty
		});
	}, [
		s.folder,
		s.open,
		s.dirty
	]);
	const folder = React.createElement('h2', {
		id: 'notes-folder',
		hidden: true
	}, s.folder);
	// A phone shows one thing at a time: the note that is open, or the list.
	if (narrow && s.open !== null) {
		return React.createElement('div', { className: 'app narrow' }, folder, React.createElement(Note, {
			s,
			phone: true,
			heading
		}));
	}
	if (env.mobile) {
		return React.createElement('div', { className: `app narrow ${env.platform}` }, folder, env.platform === 'android' && React.createElement('header', { className: 'appbar' }, React.createElement('strong', null, heading)), React.createElement('div', {
			id: 'main',
			className: 'main',
			'data-cw-large-title': env.platform === 'ios' ? heading : undefined,
			'data-cw-large-title-height': env.platform === 'ios' ? '52' : undefined
		}, env.platform === 'ios' && React.createElement('strong', { className: 'large-title' }, heading), React.createElement('div', { className: 'sheet' }, React.createElement(Notice, { s }), React.createElement(Buttons, null), React.createElement('div', { className: 'rows' }, React.createElement(List, { s })))));
	}
	return React.createElement('div', { className: narrow ? 'app desktop narrow' : 'app desktop' }, folder, React.createElement('header', { className: 'toolbar' }, React.createElement('strong', null, heading)), React.createElement('aside', { className: 'sidebar' }, React.createElement(Notice, { s }), React.createElement(Buttons, null), React.createElement('div', {
		id: 'list',
		className: 'list'
	}, React.createElement(List, { s }))), !narrow && React.createElement('main', { className: 'pane' }, s.open !== null ? React.createElement(Note, {
		s,
		phone: false,
		heading
	}) : React.createElement('p', { className: 'notice' }, 'Select a note')));
}
createRoot(document.getElementById('root')).render(React.createElement(Notes, null));
})();
