const { spawn } = require('node:child_process');
const { AsyncLocalStorage } = require('node:async_hooks');
const config = globalThis.__onceProbe || { mode: process.argv[2], channel: process.argv[3], command: process.argv[4] || process.execPath };
const { mode, channel, command } = config;
if (!['throw-once','add-once','reentrant-once','registration-order','manual'].includes(mode) || !['stdout','stderr'].includes(channel)) throw new Error('invalid probe arguments');
const storage = new AsyncLocalStorage();
let child, handled = 0, first = 0, later = 0, closed = 0, ended = 0, caught = 0;
const trace = [], chunks = [], contexts = [];
const record = tag => { trace.push(tag); contexts.push(storage.getStore() || null); };
const onUncaught = error => {
  if (error.message !== 'owned once sentinel') throw error;
  handled++; record('handled');
  if (handled === 1 && mode === 'throw-once') child.stdin.end('next');
};
process.on('uncaughtException', onUncaught);
(async () => {
  let code;
  try {
    await storage.run('once-child-owner', async () => {
      const childSource = mode === 'manual' ? '' : `process.stdin.once('data', () => { process.${channel}.write('TWO'); process.stdin.pause(); });process.stdin.resume();process.${channel}.write('ONE');`;
      child = spawn(command, ['-e', childSource], { stdio: ['pipe','pipe','pipe'] });
      const output = child[channel];
      child[channel === 'stdout' ? 'stderr' : 'stdout'].on('data', () => {});
      output.on('data', chunk => { chunks.push(chunk.toString()); });
      output.once('end', () => { ended++; });
      const closing = new Promise((resolve, reject) => {
        child.once('error', reject);
        child.once('close', value => { closed++; resolve(value); });
      });
      if (mode === 'throw-once' || mode === 'manual') {
        output.once('data', chunk => { first++; record('first:' + chunk); throw new Error('owned once sentinel'); });
        // A callback not reached after the preceding throw remains pending.
        output.once('data', chunk => { later++; record('later:' + chunk); });
      } else if (mode === 'add-once') {
        output.once('data', chunk => {
          first++; record('first:' + chunk);
          output.once('data', next => { later++; record('later:' + next); });
          child.stdin.end('next');
        });
      } else if (mode === 'reentrant-once') {
        output.once('data', chunk => {
          first++; record('first:' + chunk);
          if (first === 1) {
            output.once('data', next => { later++; record('later:' + next); });
            // Bounded even on a broken remove-after-call once implementation.
            output.emit('data', Buffer.from('NESTED'));
            child.stdin.end('next');
          }
        });
      } else {
        output.once('data', chunk => { first++; record('A:' + chunk); });
        output.on('data', chunk => { record('B:' + chunk); });
        output.once('data', chunk => { later++; record('C:' + chunk); });
        output.on('data', chunk => { record('D:' + chunk); if (chunk.toString() === 'ONE') child.stdin.end('next'); });
      }
      if (mode === 'manual') {
        for (const value of ['MANUAL_ONE','MANUAL_TWO']) {
          try { output.emit('data', Buffer.from(value)); }
          catch (error) { if (error.message !== 'owned once sentinel') throw error; caught++; }
        }
        child.stdin.end();
      }
      code = await closing;
      await new Promise(resolve => setImmediate(resolve));
    });
  } finally { process.off('uncaughtException', onUncaught); }
  process.stdout.write('ONCE_RESULT=' + JSON.stringify({mode,channel,handled,first,later,closed,ended,caught,code,trace,chunks,contexts}) + '\n');
})().catch(error => { console.error(error); process.exitCode = 1; });
