// THE LIGHT/DARK SWITCH.
//
// The inline snippet in every page's <head> has already written `data-theme` on <html>,
// from the reader's stored choice or, failing that, from the device — before the first
// paint, so a light reader never sees a dark page flash past. This file only draws the
// control: it unhides the button, remembers a click, and keeps following the device for
// as long as nobody has clicked.
(() => {
  const root = document.documentElement;
  const device = window.matchMedia('(prefers-color-scheme: light)');
  const buttons = document.querySelectorAll('.theme');
  if (!buttons.length) return;

  const stored = () => {
    try {
      const v = localStorage.getItem('cw-theme');
      return v === 'light' || v === 'dark' ? v : null;
    } catch { return null; }
  };

  const apply = (theme) => {
    root.dataset.theme = theme;
    const to = theme === 'dark' ? 'light' : 'dark';
    for (const b of buttons) {
      b.hidden = false;
      b.setAttribute('aria-label', `Switch to the ${to} theme`);
      b.title = `Switch to the ${to} theme`;
    }
  };

  apply(stored() ?? (device.matches ? 'light' : 'dark'));

  for (const b of buttons) b.addEventListener('click', () => {
    const next = root.dataset.theme === 'dark' ? 'light' : 'dark';
    try { localStorage.setItem('cw-theme', next); } catch { /* private window: this visit only */ }
    apply(next);
  });

  // No stored choice means the page is the device's to decide, including mid-visit.
  device.addEventListener('change', (e) => { if (!stored()) apply(e.matches ? 'light' : 'dark'); });
})();
