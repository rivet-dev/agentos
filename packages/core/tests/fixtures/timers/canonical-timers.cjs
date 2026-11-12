const assert = require("node:assert/strict");
const timers = require("node:timers");
(async () => {
const { AsyncLocalStorage } = require("node:async_hooks");
const storage = new AsyncLocalStorage();
return storage.run("timer-owner-context", async () => {
const promiseTimers = require("node:timers/promises");
const trace = [];
let overflow = false;
const record = name => { if (trace.length >= 16) { overflow = true; return; } assert.equal(storage.getStore(), "timer-owner-context", "timer callback async context"); trace.push(name); process.stderr.write("TIMER_ROUTE " + name + "\n"); };
const same = { clearImmediate: timers.clearImmediate === globalThis.clearImmediate, clearInterval: timers.clearInterval === globalThis.clearInterval, timeout: timers.setTimeout === globalThis.setTimeout, clearTimeout: timers.clearTimeout === globalThis.clearTimeout, immediate: timers.setImmediate === globalThis.setImmediate, interval: timers.setInterval === globalThis.setInterval };
const canonicalTimeout = timers.setTimeout;
const originalGlobalTimeout = globalThis.setTimeout;
let stableAfterGlobalReplacement;
try {
  globalThis.setTimeout = () => { throw new Error("replacement global timer must not be used"); };
  stableAfterGlobalReplacement = timers.setTimeout === canonicalTimeout;
} finally { globalThis.setTimeout = originalGlobalTimeout; }
record("registered");
const promiseErrors = [];
promiseTimers.setTimeout(0, "value").then(value => { assert.equal(value, "value"); record("module:promise-timeout"); }, error => { promiseErrors.push(String(error)); });
const canceled = timers.setTimeout(() => record("module:canceled-fired"), 0);
timers.clearTimeout(canceled);
const canceledImmediate = timers.setImmediate(() => record("module:canceled-immediate-fired"));
timers.clearImmediate(canceledImmediate);
for (const handle of [canceled, canceledImmediate]) {
  for (const method of ["ref", "unref", "hasRef"]) assert.equal(typeof handle?.[method], "function", "real timer handle " + method);
}
const crossCleanup = [];
for (const [kind, makeModule, clearModule, makeGlobal, clearGlobal] of [
  ["timeout", timers.setTimeout, timers.clearTimeout, globalThis.setTimeout, globalThis.clearTimeout],
  ["interval", timers.setInterval, timers.clearInterval, globalThis.setInterval, globalThis.clearInterval],
  ["immediate", timers.setImmediate, timers.clearImmediate, globalThis.setImmediate, globalThis.clearImmediate],
]) {
  const fromModule = makeModule(() => record("cross-cancel-fired:" + kind), 1);
  clearGlobal(fromModule);
  const fromGlobal = makeGlobal(() => record("cross-cancel-fired:" + kind), 1);
  clearModule(fromGlobal);
  crossCleanup.push(() => { clearGlobal(fromModule); clearGlobal(fromGlobal); });
}

let interval;
interval = timers.setInterval((value) => { assert.equal(value, "interval-arg"); record("module:interval"); if (trace.filter(x => x === "module:interval").length === 2) timers.clearInterval(interval); }, 1, "interval-arg");
timers.setTimeout((a, b) => { assert.equal(a, "timeout-arg"); assert.equal(b, 7); record("module:timeout"); }, 0, "timeout-arg", 7);
timers.setImmediate(value => { assert.equal(value, "immediate-arg"); record("module:immediate"); }, "immediate-arg");
globalThis.setTimeout(() => record("global:timeout"), 0);
globalThis.setImmediate(() => record("global:immediate"));
await new Promise(resolve => globalThis.setTimeout(resolve, 100));
timers.clearInterval(interval);
globalThis.clearInterval(interval);
for (const cleanup of crossCleanup) cleanup();
assert(!overflow, "bounded timer route trace overflow");
assert(!trace.some(name => name.startsWith("cross-cancel-fired:")), "cross-owner timer cancellation failed");
const report = { same, stableAfterGlobalReplacement, promiseErrors, trace };
process.stdout.write(JSON.stringify(report) + "\n");
for (const [name, identical] of Object.entries(same)) assert(identical, "initial canonical identity: " + name);
assert(stableAfterGlobalReplacement, "module timer followed mutable global");
assert.deepEqual(promiseErrors, []);
for (const marker of ["module:promise-timeout", "module:timeout", "module:immediate", "module:interval", "global:timeout", "global:immediate"]) assert(trace.includes(marker), "missing timer route: " + marker);
assert(!trace.includes("module:canceled-fired"), "canceled module timer fired");
assert(!trace.includes("module:canceled-immediate-fired"), "canceled module immediate fired");
assert.equal(trace.filter(x => x === "module:interval").length, 2);

});
})().catch(error => { console.error(error.stack); process.exitCode = 1; });
