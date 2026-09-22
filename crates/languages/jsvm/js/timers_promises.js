'use strict';

function setTimeoutP(ms, value, options) {
  return new Promise((resolve, reject) => {
    const t = setTimeout(() => resolve(value), ms);
    if (options && options.signal) {
      options.signal.addEventListener('abort', () => {
        clearTimeout(t);
        const e = new Error('The operation was aborted');
        e.name = 'AbortError';
        e.code = 'ABORT_ERR';
        reject(e);
      });
    }
  });
}

function setImmediateP(value) {
  return new Promise((resolve) => setImmediate(() => resolve(value)));
}

async function* setIntervalP(ms, value) {
  while (true) {
    await setTimeoutP(ms);
    yield value;
  }
}

const scheduler = {
  wait: (ms) => setTimeoutP(ms),
  yield: () => setImmediateP(),
};

module.exports = { setTimeout: setTimeoutP, setImmediate: setImmediateP, setInterval: setIntervalP, scheduler };
