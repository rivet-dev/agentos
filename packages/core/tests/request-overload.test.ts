import { afterEach, expect, test } from "vitest";
import { AgentOs, SidecarRejectedError } from "../src/index.js";

const vms: AgentOs[] = [];

async function createVm(
	options?: Parameters<typeof AgentOs.create>[0],
): Promise<AgentOs> {
	const vm = await AgentOs.create(options);
	vms.push(vm);
	return vm;
}

afterEach(async () => {
	await Promise.all(vms.splice(0).map((vm) => vm.dispose()));
});

test("a request spike beyond the sidecar's limits fails only with typed retryable errors", async () => {
	const vm = await createVm({ defaultSoftware: false });

	// Issued in one tick, so the in-flight slots and the sidecar request queue
	// fill before any response arrives; the rest of the spike is rejected.
	const results = await Promise.allSettled(
		Array.from({ length: 5_000 }, (_, index) =>
			vm.filesystem.exists(`/tmp/spike-${index}`),
		),
	);

	const failures = results.flatMap((result) =>
		result.status === "rejected" ? [result.reason] : [],
	);
	expect(failures.length).toBeGreaterThan(0);
	for (const failure of failures) {
		expect(failure).toBeInstanceOf(SidecarRejectedError);
		expect((failure as SidecarRejectedError).detail).toMatchObject({
			retryable: true,
			errno: "EAGAIN",
			configuration_path: expect.stringMatching(/^runtime\.protocol\./),
		});
	}
	const queueFull = failures.find(
		(failure) =>
			(failure as SidecarRejectedError).detail.code ===
			"ERR_AGENTOS_REQUEST_QUEUE_LIMIT",
	) as SidecarRejectedError | undefined;
	expect(queueFull?.detail.configuration_path).toBe(
		"runtime.protocol.requestQueueLimit",
	);
	// Everything the queue held was served, not rejected.
	expect(results.length - failures.length).toBeGreaterThan(
		queueFull?.detail.configured_limit as number,
	);
	expect(
		results.every(
			(result) => result.status === "rejected" || result.value === false,
		),
	).toBe(true);
}, 60_000);

// More creates than the ordinary in-flight lane admits, so creates wait in the
// sidecar request queue while the stdin probes run.
const CREATE_FLOOD = 192;
const STDIN_PROBES = 8;

test("a live process's stdin round trip stays fast while VM creates flood the sidecar", async () => {
	const vm = await createVm({ defaultSoftware: false });
	let stdout = "";
	const stdoutWaiters = new Set<() => void>();
	const echo = await vm.process.spawn(
		"node",
		["-e", "process.stdin.on('data', (chunk) => process.stdout.write(chunk))"],
		{
			onStdout: (chunk) => {
				stdout += new TextDecoder().decode(chunk);
				for (const waiter of stdoutWaiters) waiter();
			},
		},
	);
	const roundTrip = async (token: string): Promise<number> => {
		const startedAt = performance.now();
		const echoed = new Promise<void>((resolve) => {
			const check = () => {
				if (!stdout.includes(token)) return;
				stdoutWaiters.delete(check);
				resolve();
			};
			stdoutWaiters.add(check);
		});
		await vm.process.writeStdin(echo.pid, `${token}\n`);
		await echoed;
		return performance.now() - startedAt;
	};
	await roundTrip("warm-up");

	let lastCreateSettledAt = 0;
	const flood = Promise.allSettled(
		Array.from({ length: CREATE_FLOOD }, () =>
			createVm({ defaultSoftware: false }).finally(() => {
				lastCreateSettledAt = performance.now();
			}),
		),
	);
	// Let the flood reach the sidecar and fill its ordinary lane first.
	await new Promise((resolve) => setTimeout(resolve, 100));
	const latencies: number[] = [];
	for (let probe = 0; probe < STDIN_PROBES; probe++) {
		latencies.push(await roundTrip(`probe-${probe}`));
	}
	const probesDoneAt = performance.now();
	await flood;

	// The probes ran while creates were still waiting, not after the flood.
	expect(lastCreateSettledAt).toBeGreaterThan(probesDoneAt);
	expect(Math.max(...latencies)).toBeLessThan(1_000);
}, 120_000);

// The guest server holds every response until HOLD_MS after the first request,
// so the held requests fill every ordinary in-flight slot for that window.
const HOLD_MS = 3_000;
// One VM holds at most this many connections inside its socket limits.
const HELD_PER_VM = 100;
const HOLDING_SERVER = `
const http = require("node:http");
const held = [];
let released = false;
const server = http.createServer((req, res) => {
	if (released) return res.end("late");
	held.push(res);
	if (held.length === 1) {
		setTimeout(() => {
			released = true;
			for (const response of held) response.end("held");
		}, ${HOLD_MS});
	}
});
server.listen(0, "0.0.0.0", () => console.log("LISTENING:" + server.address().port));
`;

async function startHoldingServer(vm: AgentOs): Promise<number> {
	return await new Promise<number>((resolve, reject) => {
		let stdout = "";
		void vm.process
			.spawn("node", ["-e", HOLDING_SERVER], {
				onStdout: (chunk) => {
					stdout += new TextDecoder().decode(chunk);
					const match = stdout.match(/LISTENING:(\d+)/);
					if (match) resolve(Number(match[1]));
				},
			})
			.catch(reject);
	});
}

test("dispose with a running process completes while held requests fill every in-flight slot", async () => {
	const permissions = {
		fs: "allow",
		network: "allow",
		childProcess: "allow",
	} as const;
	const busy = await Promise.all([
		createVm({ permissions }),
		createVm({ permissions }),
	]);
	const target = await createVm({ defaultSoftware: false });
	const ports = await Promise.all(busy.map(startHoldingServer));
	const worker = await target.process.spawn("node", [
		"-e",
		"setInterval(() => {}, 1e6)",
	]);
	// Settles during dispose, so capture the outcome before anything awaits it.
	const workerExit = target.process.wait(worker.pid).then(
		() => null,
		(error: unknown) => error,
	);

	let firstHeldResponseAt = Number.POSITIVE_INFINITY;
	const held = Promise.allSettled(
		busy.flatMap((vm, index) =>
			Array.from({ length: HELD_PER_VM }, () =>
				vm
					.httpRequest({ port: ports[index], path: "/hold" })
					.then((response) => {
						firstHeldResponseAt = Math.min(
							firstHeldResponseAt,
							performance.now(),
						);
						return response;
					}),
			),
		),
	);
	// Let the held requests reach the guests before the dispose is issued.
	await new Promise((resolve) => setTimeout(resolve, 500));

	await target.dispose();
	const disposedAt = performance.now();
	vms.splice(vms.indexOf(target), 1);

	expect(disposedAt).toBeLessThan(firstHeldResponseAt);
	// The worker's wait settles with the disposal instead of hanging.
	expect(String(await workerExit)).toMatch(/VM disposed/);
	const results = await held;
	expect(results.map((result) => result.status)).toEqual(
		results.map(() => "fulfilled"),
	);
}, 60_000);
