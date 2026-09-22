// A rejected promise nobody handles terminates the process.
async function fetchUser(id) {
  await null;
  if (id < 0) throw new RangeError(`invalid user id ${id}`);
  return { id };
}
fetchUser(1).then((u) => console.log('got', u));
fetchUser(-5).then((u) => console.log('never', u));
setTimeout(() => console.log('timer never runs'), 100);
console.log('main done');
