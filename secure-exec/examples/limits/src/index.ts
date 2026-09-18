// docs:start timeout
import { execute } from "secure-exec";

// `timeoutMs` stops runaway code. The result reports it; nothing is thrown.
const runaway = await execute("while (true) {}", { timeoutMs: 1_000 });
console.log(runaway.outcome); // timed_out
// docs:end timeout

// docs:start memory
// VM limits bound memory and other resources for the whole VM.
const bounded = await execute(
	"const chunks = []; while (true) chunks.push(new Array(1e6).fill(0));",
	{ limits: { jsRuntime: { v8HeapLimitMb: 64 } }, timeoutMs: 30_000 },
);
console.log(bounded.outcome); // failed
// docs:end memory
