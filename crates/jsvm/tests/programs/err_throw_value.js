// Throwing values that are not Error objects.
console.log('throwing an object');
process.on('exit', (code) => console.log('exit code', code));
throw { code: 'E_CUSTOM', detail: [1, 2, 3], nested: { ok: false } };
