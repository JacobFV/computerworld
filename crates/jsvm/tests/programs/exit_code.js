// process.exitCode, exit handlers and process.exit from a timer.
process.on('exit', (code) => {
  console.log('exit handler sees', code, process.exitCode);
});
process.exitCode = 3;
console.log('argv length', process.argv.length, 'script', process.argv[1].endsWith('main.js'));
setTimeout(() => {
  console.log('timer fired, exiting with 7');
  process.exit(7);
  console.log('unreachable');
}, 10);
setTimeout(() => console.log('second timer never fires'), 50);
