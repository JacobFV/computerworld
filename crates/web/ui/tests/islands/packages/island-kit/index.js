// Written for cw-ui's island tests (tests/islands). Plain ESM with
// React.createElement, as packages ship.
import * as React from 'react';

const h = React.createElement;

export const ThemeContext = React.createContext('light');

export function useTheme() {
  return React.useContext(ThemeContext);
}

export function ThemeProvider({ theme, children }) {
  return h(ThemeContext.Provider, { value: theme }, children);
}

export function Card({ title, onToggle, children }) {
  const [open, setOpen] = React.useState(true);
  const theme = useTheme();
  React.useEffect(() => {
    console.log('card effect', title, open);
    return () => console.log('card cleanup', title, open);
  }, [title, open]);
  return h(
    'section',
    { className: 'card ' + theme + (open ? ' open' : ''), 'data-title': title },
    h(
      'button',
      {
        className: 'toggle',
        onClick: (e) => {
          setOpen(!open);
          if (onToggle) onToggle(!open, e.type);
        },
      },
      title,
    ),
    open ? h('div', { className: 'body' }, children) : null,
  );
}

export const FancyInput = React.forwardRef(function FancyInput({ label, ...rest }, ref) {
  return h('label', null, label, h('input', Object.assign({ ref, className: 'fancy' }, rest)));
});

export function Emphasize({ children }) {
  return h(
    'ul',
    null,
    React.Children.map(children, (child, i) =>
      React.isValidElement(child)
        ? React.cloneElement(child, { className: 'em-' + i, key: 'k' + i })
        : h('li', { key: 'x' + i }, String(child)),
    ),
  );
}

export function Counter({ start, render }) {
  const [n, setN] = React.useReducer((s, a) => (a === 'inc' ? s + 1 : s), start);
  const ref = React.useRef(0);
  ref.current += 1;
  return render(n, () => setN('inc'), ref.current);
}
