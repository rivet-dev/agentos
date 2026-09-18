import { evaluate, init, shutdown } from "secure-exec";

// docs:start init
// Every VM runs inside one shared sidecar process, which starts on first use.
// Start it when your server boots so the first request does not pay for it.
await init();

const started = performance.now();
await evaluate("1 + 2");
console.log(`first call: ${Math.round(performance.now() - started)}ms`);
// docs:end init

// docs:start shutdown
// Stop the sidecar and every VM still in it. The next call starts a new one.
// Test runners need this in a teardown hook, or the worker will not exit.
await shutdown();
// docs:end shutdown
