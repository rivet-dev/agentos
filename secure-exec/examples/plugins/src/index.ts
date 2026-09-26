import { evaluate } from "secure-exec";

// docs:start plugins
// Plugins are user-authored source you do not trust. Each one declares what it
// needs, and you decide what it gets.
interface Plugin {
	name: string;
	source: string;
	network: boolean;
}

const plugins: Plugin[] = [
	{
		name: "shout",
		source: `(() => inputs.text.toUpperCase())()`,
		network: false,
	},
	{
		name: "spin",
		source: `(() => { while (true) {} })()`,
		network: false,
	},
];
// docs:end plugins

// docs:start run
// Every plugin runs in its own VM, so plugins cannot see each other's state, and
// a plugin that hangs or crashes only fails its own call.
for (const plugin of plugins) {
	const result = await evaluate<string>(plugin.source, {
		inputs: { text: "hello" },
		permissions: { network: plugin.network ? "allow" : "deny" },
		timeoutMs: 1_000,
	});
	console.log(
		plugin.name,
		result.outcome === "succeeded" ? result.value : result.outcome,
	);
}
// shout HELLO
// spin timed_out
// docs:end run
