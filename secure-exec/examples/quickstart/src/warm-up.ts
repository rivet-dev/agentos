import { evaluate, init } from "secure-exec";

// Start the shared sidecar process at boot so the first request does not pay
// for it.
await init();

const started = performance.now();
await evaluate("1 + 2");
console.log(`first call: ${Math.round(performance.now() - started)}ms`);
