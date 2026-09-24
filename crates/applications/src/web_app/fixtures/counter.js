// Compiled by cw-tsx from counter.tsx: types stripped, JSX as React.createElement.
'use strict';
const { useState } = React;
const { createRoot } = ReactDOM;
function Counter() {
	const [count, setCount] = useState(0);
	const [name, setName] = useState('');
	return React.createElement('main', null, React.createElement('h1', { id: 'counter-title' }, 'Counter'), React.createElement('p', {
		id: 'counter-value',
		role: 'status'
	}, count), React.createElement('button', {
		id: 'counter:add',
		onClick: () => setCount((c) => c + 1)
	}, 'Add'), React.createElement('input', {
		id: 'counter:name',
		'aria-label': 'Name',
		value: name,
		onChange: (e) => setName(e.target.value)
	}), React.createElement('p', {
		id: 'counter-greeting',
		role: 'status'
	}, name === '' ? 'Nobody' : `Hello ${name}`));
}
createRoot(document.getElementById('root')).render(React.createElement(Counter, null));
