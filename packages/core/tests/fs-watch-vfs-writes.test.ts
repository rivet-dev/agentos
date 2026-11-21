import { afterAll, beforeAll, expect, test } from "vitest";
import { AgentOs } from "../src/index.js";

let sidecar: Awaited<ReturnType<typeof AgentOs.createSidecar>>;
let vm: AgentOs;

beforeAll(async () => {
	sidecar = await AgentOs.createSidecar();
	vm = await AgentOs.create({
		sidecar: { kind: "explicit", handle: sidecar },
		defaultSoftware: false,
		permissions: {
			fs: "allow",
			process: "allow",
			childProcess: "allow",
			network: "deny",
		},
	});
});

afterAll(async () => {
	try {
		await vm?.dispose();
	} finally {
		await sidecar?.dispose();
	}
});

// Watch a directory, as watchpack (`next dev`, webpack) does, and print each
// event as one JSON line.
const watcher = `
const fs = require("node:fs");
fs.watch("/project/src", (eventType, filename) => {
	process.stdout.write(JSON.stringify({ eventType, filename }) + "\\n");
});
process.stdout.write("ready\\n");
setTimeout(() => process.exit(0), 20_000);
`;

test("a guest directory watch reports writes from the host file API and from another guest process by entry name", async () => {
	await vm.filesystem.mkdir("/project/src", { recursive: true });
	await vm.filesystem.writeFiles([
		{ path: "/project/src/page.tsx", content: "export default 1;\n" },
		{ path: "/project/watcher.cjs", content: watcher },
	]);

	const { pid } = await vm.process.spawn("node", ["/project/watcher.cjs"], {});
	const decoder = new TextDecoder();
	const events: Array<{ eventType: string; filename: string }> = [];
	let output = "";
	const waiters: Array<() => void> = [];
	vm.onProcessOutput(pid, (event) => {
		if (event.stream !== "stdout") return;
		output += decoder.decode(event.data);
		const lines = output.split("\n");
		output = lines.pop() ?? "";
		for (const line of lines) {
			if (line.startsWith("{")) events.push(JSON.parse(line));
			else if (line === "ready") events.push({ eventType: "ready", filename: "" });
		}
		for (const wake of waiters.splice(0)) wake();
	});
	const waitFor = async (match: (e: { eventType: string; filename: string }) => boolean) => {
		const deadline = Date.now() + 5_000;
		while (!events.some(match)) {
			if (Date.now() > deadline) {
				throw new Error(`no matching watch event; saw ${JSON.stringify(events)}`);
			}
			await new Promise<void>((resolve) => {
				waiters.push(resolve);
				setTimeout(resolve, 100);
			});
		}
	};

	await waitFor((e) => e.eventType === "ready");

	await vm.filesystem.writeFiles([
		{ path: "/project/src/page.tsx", content: "export default 22;\n" },
	]);
	await waitFor((e) => e.eventType === "change" && e.filename === "page.tsx");

	await vm.filesystem.writeFiles([{ path: "/project/src/layout.tsx", content: "x" }]);
	await waitFor((e) => e.eventType === "rename" && e.filename === "layout.tsx");

	const other = await vm.process.execFile(
		"node",
		["-e", 'require("node:fs").unlinkSync("/project/src/layout.tsx")'],
		{ timeoutMs: 10_000 },
	);
	expect(other.exitCode).toBe(0);
	await waitFor(
		(e) => e.eventType === "rename" && e.filename === "layout.tsx" && events.filter((x) => x.filename === "layout.tsx").length >= 2,
	);

	await vm.process.kill(pid);
}, 60_000);
