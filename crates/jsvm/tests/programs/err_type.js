// An uncaught TypeError deep in a call chain.
function loadConfig(source) {
  return source.settings.theme;
}
function start(options) {
  console.log('starting with', options);
  return loadConfig(options);
}
console.log('before');
start({ name: 'app' });
console.log('never printed');
