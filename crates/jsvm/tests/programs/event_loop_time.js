// Event-loop orderings that follow from how long the program takes to run,
// rather than from when its callbacks were queued.
const out = [];

// The 1 ms timer becomes due while the main module is still busy, so the timers
// phase of the first turn runs it before the check phase runs the immediate.
setTimeout(() => out.push('timer'), 1);
setImmediate(() => out.push('immediate'));
const until = Date.now() + 20;
while (Date.now() < until) {}

// A timer started after a long stretch of work counts from now, not from when
// the turn began: 'late' is due 1 ms after the 30 ms of work below, which is
// after 'mid' (due 10 ms in).
setTimeout(() => {
  const end = Date.now() + 30;
  while (Date.now() < end) {}
  setTimeout(() => out.push('late'), 1);
}, 1);
setTimeout(() => out.push('mid'), 10);

// Work inside a callback delays the timers queued behind it.
setTimeout(() => {
  const t0 = Date.now();
  const end = t0 + 15;
  while (Date.now() < end) {}
  out.push('slow >= 15: ' + (Date.now() - t0 >= 15));
}, 40);
setTimeout(() => out.push('after slow'), 45);

setTimeout(() => {
  console.log(out.join('\n'));
}, 200);
