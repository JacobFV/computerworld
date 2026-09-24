// A web application inside cw-tsx's compiled subset, for the host's cw-ui backend:
// `cw-tsx build counter.tsx -o . --name counter` writes counter.ui.json and counter.js.
import { useState } from 'react';
import { createRoot } from 'react-dom/client';

function Counter() {
  const [count, setCount] = useState(0);
  const [name, setName] = useState('');
  return (
    <main>
      <h1 id="counter-title">Counter</h1>
      <p id="counter-value" role="status">{count}</p>
      <button id="counter:add" onClick={() => setCount((c) => c + 1)}>
        Add
      </button>
      <input id="counter:name" aria-label="Name" value={name} onChange={(e) => setName(e.target.value)} />
      <p id="counter-greeting" role="status">{name === '' ? 'Nobody' : `Hello ${name}`}</p>
    </main>
  );
}

createRoot(document.getElementById('root')!).render(<Counter />);
