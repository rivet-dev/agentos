import { createContext, evaluate } from "secure-exec";
import { install } from "secure-exec/npm";

// Packages install into a context's VM, so they need a context. Installing
// needs the network, which is denied unless you allow it.
await using context = await createContext({
	permissions: { network: "allow" },
});

const installed = await install(["zod"], {
	context,
	output: { capture: "all" },
});
if (installed.outcome !== "succeeded") {
	throw new Error(`npm install failed: ${installed.stderr}`);
}

const parsed = await evaluate<{ name: string }>(
	// `evaluate` takes one expression, so more than one statement goes in a function.
	`(async () => {
		const { z } = await import("zod");
		return z.object({ name: z.string() }).parse(inputs.input);
	})()`,
	{
		context,
		inputs: { input: { name: "secure-exec" } },
		// Packages install into the working directory, /workspace. Inline code
		// resolves imports from `filePath`, so place it next to node_modules.
		filePath: "/workspace/main.mjs",
	},
);
console.log(parsed.outcome === "succeeded" ? parsed.value : parsed.error);
