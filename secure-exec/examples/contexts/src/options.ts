import { createContext, evaluate } from "secure-exec";

// docs:start options
// VM options are fixed when the context is created, because the context owns the
// VM. Without `await using`, call `context.dispose()` yourself.
const context = await createContext({
	permissions: { network: "allow" },
	limits: { jsRuntime: { v8HeapLimitMb: 128 } },
});

try {
	const result = await evaluate<number>("21 * 2", {
		context,
		timeoutMs: 5_000,
	});
	console.log(result.outcome === "succeeded" ? result.value : result.error); // 42
} finally {
	await context.dispose();
}
// docs:end options
