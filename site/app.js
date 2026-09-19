// No framework, no third-party requests: a few listeners and an IntersectionObserver.

// Reveal sections as they come into view.
const reveal = new IntersectionObserver(
  entries => entries.forEach(e => e.isIntersecting && (e.target.classList.add('in'), reveal.unobserve(e.target))),
  { rootMargin: '0px 0px 12% 0px', threshold: 0.02 },
);
document.querySelectorAll('.reveal').forEach(el => reveal.observe(el));

// The live demo is a ~10 MB download, so it loads only when asked for.
const slot = document.getElementById('demo-slot');
document.getElementById('demo-start')?.addEventListener('click', () => {
  const frame = document.createElement('iframe');
  frame.src = './demo/examples/browser/index.html';
  frame.title = 'Computerworld world console';
  frame.allow = 'clipboard-write';
  slot.replaceWith(frame);
});

// The live machines: one module, five running computers, booted on request.
const boot = document.getElementById('boot');
const bootNote = document.getElementById('boot-note');
boot?.addEventListener('click', async () => {
  boot.disabled = true;
  bootNote.hidden = false;
  const note = text => { bootNote.textContent = text; bootNote.hidden = !text; };
  try {
    const live = await import('./live.js');
    await live.boot(note);
    boot.textContent = '✓ Running in this tab';
    note('Click, type and scroll in any of them. Click a name to open another app.');
  } catch (error) {
    boot.disabled = false;
    boot.textContent = '⚡ Boot the machines';
    note(`Could not start: ${error}`);
    console.error(error);
  }
});

// Language tabs, and copying the visible snippet.
const panes = [...document.querySelectorAll('.codebox pre')];
document.querySelectorAll('.code .tabs button').forEach(tab => {
  tab.addEventListener('click', () => {
    document.querySelectorAll('.code .tabs button').forEach(b => b.setAttribute('aria-selected', String(b === tab)));
    panes.forEach(p => (p.hidden = p.dataset.lang !== tab.dataset.lang));
  });
});
document.getElementById('copy')?.addEventListener('click', async event => {
  const shown = panes.find(p => !p.hidden);
  try {
    await navigator.clipboard.writeText(shown.innerText);
    event.target.textContent = 'Copied';
  } catch {
    event.target.textContent = 'Press ⌘/Ctrl+C';
  }
  setTimeout(() => (event.target.textContent = 'Copy'), 1600);
});
