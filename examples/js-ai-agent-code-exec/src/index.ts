import { JavaScriptRuntime } from "@rivet-dev/agentos-javascript";

// Imagine this expression came from an AI agent. It executes inside an
// isolated AgentOS VM and can only use capabilities granted to that VM.
const untrustedExpression = `
(async () => {
  const fib = [0, 1];
  while (fib.length < 20) {
    fib.push(fib[fib.length - 1] + fib[fib.length - 2]);
  }
  console.log("computed", fib.length, "fibonacci numbers");
  return { fibonacci: fib, sum: fib.reduce((a, b) => a + b, 0) };
})()
`;

const runtime = await JavaScriptRuntime.create();
try {
	const result = await runtime.evaluate<{
		fibonacci: number[];
		sum: number;
	}>(untrustedExpression, { timeoutMs: 5_000 });

	console.log("exitCode:", result.exitCode);
	console.log("stdout:", result.stdout.trim());
	if (result.outcome === "succeeded") console.log("returned value:", result.value);
} finally {
	await runtime.dispose();
}
