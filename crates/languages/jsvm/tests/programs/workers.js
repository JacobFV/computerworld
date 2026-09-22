// worker_threads: workers from a file and from source, workerData, messages,
// channels, transferred ports and how a worker ends.
//
// Every worker is dealt with on its own: two threads that finish at the same
// time reach the main thread in whatever order the machine happens to give,
// which is not something a program can rely on.
const fs = require('fs');
const path = require('path');
const {
  Worker,
  MessageChannel,
  MessagePort,
  isMainThread,
  threadId,
  parentPort,
  workerData,
  receiveMessageOnPort,
} = require('worker_threads');

console.log('main thread', isMainThread, threadId, parentPort, workerData);

const child = path.join(__dirname, 'child.js');
fs.writeFileSync(
  child,
  `const { parentPort, workerData, isMainThread, threadId } = require('worker_threads');
   console.log('in worker', isMainThread, threadId, JSON.stringify(workerData));
   parentPort.on('message', (m) => {
     if (m === 'stop') { parentPort.close(); return; }
     parentPort.postMessage({ seen: m, doubled: workerData.factor * m.n });
   });
  `
);

function fileWorker() {
  return new Promise((resolve) => {
    const w = new Worker(child, { workerData: { factor: 3 } });
    const seen = [];
    w.on('online', () => seen.push('online'));
    w.on('message', (m) => {
      seen.push(m);
      if (seen.length === 3) w.postMessage('stop');
    });
    w.on('exit', (code) => {
      console.log('file worker', code, JSON.stringify(seen));
      resolve();
    });
    w.postMessage({ n: 1 });
    w.postMessage({ n: 2 });
  });
}

function evalWorker() {
  return new Promise((resolve) => {
    const w = new Worker(
      `const { parentPort, workerData } = require('worker_threads');
       setTimeout(() => {
         parentPort.postMessage('late ' + workerData);
         process.exit(7);
       }, 5);`,
      { eval: true, workerData: 'hello' }
    );
    w.on('message', (m) => console.log('eval worker said', m));
    w.on('exit', (code) => {
      console.log('eval worker exit', code);
      resolve();
    });
  });
}

function errorWorker() {
  return new Promise((resolve) => {
    const w = new Worker('throw new TypeError("no good")', { eval: true });
    w.on('error', (e) => console.log('error event', e.name, e.message));
    w.on('exit', (code) => {
      console.log('error worker exit', code);
      resolve();
    });
  });
}

function terminated() {
  return new Promise((resolve) => {
    const w = new Worker(
      `const { parentPort } = require('worker_threads');
       parentPort.on('message', () => parentPort.postMessage('still here'));`,
      { eval: true }
    );
    w.postMessage('ping');
    w.on('message', async (m) => {
      console.log('before terminate', m);
      const code = await w.terminate();
      console.log('terminate resolved', code);
    });
    w.on('exit', (code) => {
      console.log('terminated exit', code);
      resolve();
    });
  });
}

function channels() {
  return new Promise((resolve) => {
    const local = new MessageChannel();
    console.log('channel ports', local.port1 instanceof MessagePort);
    local.port1.on('message', (m) => {
      console.log('port1 heard', m);
      local.port1.close();
      local.port2.close();

      const poll = new MessageChannel();
      poll.port2.postMessage('one');
      poll.port2.postMessage('two');
      console.log('polled', JSON.stringify(receiveMessageOnPort(poll.port1)));
      console.log('polled', JSON.stringify(receiveMessageOnPort(poll.port1)));
      console.log('polled', receiveMessageOnPort(poll.port1));
      poll.port1.close();
      poll.port2.close();

      const { port1, port2 } = new MessageChannel();
      const w = new Worker(
        `const { workerData } = require('worker_threads');
         const port = workerData.port;
         port.on('message', (m) => { port.postMessage('pong:' + m); port.close(); });`,
        { eval: true, workerData: { port: port2 }, transferList: [port2] }
      );
      port1.postMessage('ping');
      port1.on('message', (m) => {
        console.log('over the channel', m);
        port1.close();
      });
      w.on('exit', (code) => {
        console.log('channel worker exit', code);
        resolve();
      });

      try {
        const stray = new MessageChannel();
        w.postMessage({ port: stray.port1 });
      } catch (e) {
        console.log('untransferred port', e.name);
      }
    });
    local.port2.postMessage({ hello: 'channel' });
  });
}

async function main() {
  await fileWorker();
  await evalWorker();
  await errorWorker();
  await terminated();
  await channels();
  fs.unlinkSync(child);
  console.log('done');
}

main();
