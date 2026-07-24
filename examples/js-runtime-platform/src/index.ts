import { JavaScriptRuntime } from "@rivet-dev/agentos-javascript";

// docs:start nodejs
const runtime = await JavaScriptRuntime.create();

try {
	const result = await runtime.evaluate<{
		filename: string;
		digest: string;
	}>(`(async () => {
		const { createHash } = await import("node:crypto");
		const { join } = await import("node:path");
		return {
			filename: join("/workspace", "report.json"),
			digest: createHash("sha256").update("agentos").digest("hex"),
		};
	})()`);

	if (result.outcome !== "succeeded") throw new Error(result.error.message);
	console.log(result.value);
} finally {
	await runtime.dispose();
}
// docs:end nodejs
