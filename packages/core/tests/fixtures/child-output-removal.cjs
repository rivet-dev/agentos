const assert = require('node:assert/strict');
const { spawn } = require('node:child_process');
const { AsyncLocalStorage } = require('node:async_hooks');
const { channel, command } = globalThis.__removalProbe;
const storage = new AsyncLocalStorage();
(async () => storage.run('output-removal-owner', async () => {
  const child = spawn(command, ['-e', ''], { stdio: ['ignore', 'pipe', 'pipe'] });
  const output = child[channel];
  child[channel === 'stdout' ? 'stderr' : 'stdout'].on('data', () => {});
  let ended = 0, closed = 0;
  output.once('end', () => ended++);
  const closing = new Promise((resolve, reject) => {
    child.once('error', reject);
    child.once('close', code => { closed++; resolve(code); });
  });
  const rows = [], receivers = [], contexts = [];
  const listener = (trace, tag) => function(value) {
    receivers.push(this === output);
    contexts.push(storage.getStore());
    trace.push(tag + ':' + value);
  };
  {
    const trace = [], fn = listener(trace, 'F');
    assert.equal(output.once('remove-once', fn), output);
    assert.equal(output.off('remove-once', fn), output);
    output.emit('remove-once', 'X'); rows.push({ mode:'off-original', trace });
  }
  {
    const trace = [], fn = listener(trace, 'F');
    output.once('remove-alias', fn); assert.equal(output.removeListener('remove-alias', fn), output);
    output.emit('remove-alias', 'X'); rows.push({ mode:'removeListener-original', trace });
  }
  {
    const trace = [], fn = listener(trace, 'F');
    output.once('duplicate-once', fn); output.on('duplicate-once', listener(trace,'M'));
    output.once('duplicate-once', fn); output.off('duplicate-once',fn);
    output.emit('duplicate-once','X'); output.emit('duplicate-once','Y');
    rows.push({ mode:'remove-last-once',trace });
  }
  {
    const trace = [], fn = listener(trace, 'F');
    output.on('duplicate-on',fn); output.on('duplicate-on',listener(trace,'M'));
    output.on('duplicate-on',fn); output.removeListener('duplicate-on',fn);
    output.emit('duplicate-on','X'); rows.push({ mode:'remove-last-on',trace });
  }
  {
    const trace = [], fn = listener(trace,'F');
    output.on('mixed-on-first',fn); output.once('mixed-on-first',fn); output.off('mixed-on-first',fn);
    output.emit('mixed-on-first','X'); output.emit('mixed-on-first','Y');
    rows.push({ mode:'remove-later-once-keeps-on',trace });
  }
  {
    const trace = [], fn = listener(trace,'F');
    output.once('mixed-once-first',fn); output.on('mixed-once-first',fn); output.off('mixed-once-first',fn);
    output.emit('mixed-once-first','X'); output.emit('mixed-once-first','Y');
    rows.push({ mode:'remove-later-on-keeps-once',trace });
  }
  {
    const trace = [], later = listener(trace,'later');
    output.on('snapshot', function(value) { receivers.push(this === output); contexts.push(storage.getStore()); trace.push('first:'+value); output.off('snapshot',later); });
    output.once('snapshot',later);
    output.emit('snapshot','X'); output.emit('snapshot','Y');
    rows.push({ mode:'removal-during-dispatch-keeps-snapshot',trace });
  }
  {
    const trace = [], fn = listener(trace,'F');
    output.once('independent',fn); output.once('independent',fn);
    output.emit('independent','X'); output.emit('independent','Y');
    rows.push({ mode:'duplicate-once-independent',trace });
  }
  const code = await closing;
  await new Promise(resolve => setImmediate(resolve));
  assert.equal(code,0); assert.equal(ended,1); assert.equal(closed,1);
  assert(receivers.every(Boolean),'on and once listener this must be the output stream');
  assert(contexts.every(value => value === 'output-removal-owner'),'manual emit keeps caller async context');
  process.stdout.write('REMOVAL_RESULT='+JSON.stringify({channel,rows,receiverCount:receivers.length,contextPreserved:true,ended,closed,code})+'\n');
}))().catch(error => { console.error(error); process.exitCode=1; });
