// No framework, no third-party requests: a few listeners and an IntersectionObserver.

// Reveal sections as they come into view.
const reveal = new IntersectionObserver(
  entries => entries.forEach(e => e.isIntersecting && (e.target.classList.add('in'), reveal.unobserve(e.target))),
  { rootMargin: '0px 0px 12% 0px', threshold: 0.02 },
);
document.querySelectorAll('.reveal').forEach(el => reveal.observe(el));

// The machines boot themselves: the simulator downloads as soon as the page loads, and
// each panel becomes a running computer as its machine comes up. No button to press.
const notes = () => [...document.querySelectorAll('[data-boot-note]')];
const say = text => notes().forEach(n => { n.textContent = text; n.hidden = !text; });

(async () => {
  // A browser asking for Save-Data gets the stills and a way in, not a 10 MB download
  // it did not ask for. Everyone else gets running machines.
  if (navigator.connection?.saveData) {
    say('Data saver is on. Tap to download the simulator (about 10 MB) and start the machines.');
    await new Promise(resolve => notes().forEach(n => n.addEventListener('click', resolve, { once: true })));
  }
  try {
    const live = await import('./live.js');
    await live.boot((text, machine) => {
      if (!machine) return say(text);
      const note = document.querySelector(`.tile[data-machine="${machine}"] [data-boot-note]`);
      if (note) { note.textContent = text; note.hidden = !text; }
    });
  } catch (error) {
    say('This browser could not start the simulator. The screenshots below are real renders of it.');
    console.error(error);
  }
})();

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
