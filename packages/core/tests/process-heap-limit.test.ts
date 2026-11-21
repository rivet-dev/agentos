import { afterAll, beforeAll, expect, test } from "vitest";
import { AgentOs } from "../src/index.js";

// A guest that outgrows limits.jsRuntime.v8HeapLimitMb must end with the typed
// heap-limit error whenever in its life that happens. These guests start, stay
// alive, and only later allocate, inside the continuation of an awaited host
// call. No other event reaches the guest afterwards, so nothing else ends it.

const HEAP_LIMIT_ERROR =
	"Script execution exceeded the V8 heap limit (limits.jsRuntime.v8HeapLimitMb)";
const EXIT_DEADLINE_MS = 5_000;

const allocateForever =
	"const hold = []; for (;;) hold.push(new Array(100000).fill('x' + hold.length));";

let sidecar: Awaited<ReturnType<typeof AgentOs.createSidecar>>;
let vm: AgentOs;
let neighbor: AgentOs;

beforeAll(async () => {
	sidecar = await AgentOs.createSidecar();
	const options = {
		sidecar: { kind: "explicit", handle: sidecar },
		defaultSoftware: false,
		permissions: {
			fs: "allow",
			process: "allow",
			childProcess: "allow",
			network: "allow",
		},
		limits: { jsRuntime: { v8HeapLimitMb: 64 } },
	} as const;
	vm = await AgentOs.create(options);
	neighbor = await AgentOs.create(options);
});

afterAll(async () => {
	try {
		await neighbor?.dispose();
		await vm?.dispose();
	} finally {
		await sidecar?.dispose();
	}
});

/** Start a long-lived guest and resolve once its stdout matches `ready`. */
async function startGuest(source: string, ready: RegExp) {
	const output = { stdout: "", stderr: "" };
	const started = Promise.withResolvers<RegExpMatchArray>();
	const waiters: Array<() => void> = [];
	const { pid } = await vm.process.spawn("node", ["-e", source], {
		onStdout: (bytes) => {
			output.stdout += Buffer.from(bytes).toString();
			const match = output.stdout.match(ready);
			if (match) started.resolve(match);
			for (const wake of waiters.splice(0)) wake();
		},
		onStderr: (bytes) => {
			output.stderr += Buffer.from(bytes).toString();
		},
	});
	const stdoutIncludes = async (text: string) => {
		while (!output.stdout.includes(text)) {
			await new Promise<void>((resolve) => waiters.push(resolve));
		}
	};
	return { pid, output, match: await started.promise, stdoutIncludes };
}

/** Settle with `settled`, or with "pending" once the exit deadline passes. */
function withinExitDeadline<T>(settled: Promise<T>): Promise<T | "pending"> {
	return Promise.race([
		settled,
		new Promise<"pending">((resolve) =>
			setTimeout(() => resolve("pending"), EXIT_DEADLINE_MS),
		),
	]);
}

const cases = [
	{
		name: "a stdin data callback",
		source: `process.stdin.on('data', () => { ${allocateForever} }); console.log('ready 0');`,
		trigger: async (pid: number) => {
			await vm.process.writeStdin(pid, "go\n");
			return undefined;
		},
	},
	{
		name: "an HTTP handler after an awaited file write",
		source: `
const http = require('node:http');
const fs = require('node:fs');
const server = http.createServer(async () => {
	await fs.promises.writeFile('/tmp/request', 'seen');
	${allocateForever}
});
server.listen(0, '127.0.0.1', () => console.log('ready ' + server.address().port));`,
		// The host call stays pending: the handler never responds.
		trigger: async (_pid: number, port: number) =>
			vm.httpRequest({ port, path: "/" }).then(
				() => "responded",
				() => "rejected",
			),
	},
];

test.each(cases)("a long-lived guest that outgrows the heap limit in $name exits with the heap-limit error", async ({
	source,
	trigger,
}) => {
	const bystander = await startGuest(
		"process.stdin.on('data', (chunk) => process.stdout.write('echo:' + chunk)); console.log('ready 0');",
		/ready (\d+)/,
	);
	const guest = await startGuest(source, /ready (\d+)/);

	const hostCall = trigger(guest.pid, Number(guest.match[1]));
	const exit = await withinExitDeadline(vm.process.wait(guest.pid));

	expect(exit).toMatchObject({ exitCode: 1 });
	expect(guest.output.stderr).toContain(HEAP_LIMIT_ERROR);
	// A host call that was waiting on the guest fails; it does not hang.
	expect(await withinExitDeadline(hostCall)).not.toBe("pending");
	expect(await withinExitDeadline(hostCall)).not.toBe("responded");

	// The process that was already running in the VM still answers.
	await vm.process.writeStdin(bystander.pid, "ping\n");
	expect(await withinExitDeadline(bystander.stdoutIncludes("echo:ping"))).not.toBe(
		"pending",
	);
	await vm.process.closeStdin(bystander.pid);
	expect(await vm.process.wait(bystander.pid)).toMatchObject({ exitCode: 0 });

	// New processes start in the same VM and in another VM on the sidecar.
	for (const target of [vm, neighbor]) {
		const after = await target.process.execFile(
			"node",
			["-e", "console.log(1 + 1)"],
			{ timeoutMs: 10_000, output: { capture: "all" } },
		);
		expect({ exitCode: after.exitCode, stdout: after.stdout }).toEqual({
			exitCode: 0,
			stdout: "2\n",
		});
	}
}, 30_000);
