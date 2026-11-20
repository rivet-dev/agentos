import { finished } from "node:stream/promises";
import { afterEach, beforeEach, describe, expect, test } from "vitest";
import { AgentOs, type AgentOsSidecar } from "../src/index.js";
import type { StdioSidecarProcess } from "../src/process.js";
import type {
	AuthenticatedSession,
	CreatedVm,
	SidecarProcess,
} from "../src/sidecar-process.js";

const LISTENER_SCRIPT = `
const net = require("net");
const server = net.createServer(() => {});
server.listen(0, "127.0.0.1", () => {
  console.log("LISTENING:" + server.address().port);
});
setTimeout(() => {}, 30000);
`;

// 6 MiB exceeds the default 4 MiB VM socket buffer budget, so the writer must
// wait for the in-VM reader to drain instead of failing.
const LARGE_WRITE_SCRIPT = `
const net = require("net");
const payload = Buffer.alloc(6 * 1024 * 1024);
for (let index = 0; index < payload.length; index += 1) payload[index] = index % 251;
const finish = (result) => {
  console.log(JSON.stringify(result));
  process.exit(0);
};
const server = net.createServer((socket) => {
  const chunks = [];
  socket.on("data", (chunk) => chunks.push(chunk));
  socket.on("end", () => {
    const received = Buffer.concat(chunks);
    finish({ receivedBytes: received.length, intact: received.equals(payload) });
  });
});
server.listen(0, "127.0.0.1", () => {
  const client = net.connect(server.address().port, "127.0.0.1", () => client.end(payload));
  client.on("error", (error) => finish({ clientError: error.code ?? error.message }));
});
`;

describe("networking", () => {
	let vm: AgentOs;
	let sidecar: AgentOsSidecar | undefined;
	let hostProcess: StdioSidecarProcess | undefined;

	beforeEach(async () => {
		sidecar = undefined;
		hostProcess = undefined;
		sidecar = await AgentOs.createSidecar();
		vm = await AgentOs.create({
			sidecar: { kind: "explicit", handle: sidecar },
		});
		hostProcess = (
			vm as unknown as {
				_sidecarClient: {
					protocolClient: { sidecarProcess: StdioSidecarProcess };
				};
			}
		)._sidecarClient.protocolClient.sidecarProcess;
	});

	afterEach(async () => {
		const errors: unknown[] = [];
		try {
			if (hostProcess) await vm.dispose();
		} catch (error) {
			errors.push(error);
		}
		try {
			await sidecar?.dispose();
		} catch (error) {
			errors.push(error);
		}
		try {
			if (hostProcess) {
				// Process exit may precede pipe EOF; observe all bounded host diagnostics.
				await finished(hostProcess.child.stderr, {
					cleanup: true,
					signal: AbortSignal.timeout(5_000),
				});
				const stderr = hostProcess.stderrText();
				expect(stderr).not.toContain("bytes of sidecar stderr dropped");
				expect(stderr).not.toContain("ERR_AGENTOS_");
			}
		} catch (error) {
			errors.push(error);
		}
		if (errors.length === 1) throw errors[0];
		if (errors.length > 1)
			throw new AggregateError(errors, "network sidecar retirement failed");
	});

	test("starts a guest TCP listener", async () => {
		await vm.writeFile("/tmp/network.js", LISTENER_SCRIPT);

		let resolvePort!: (value: number) => void;
		const portPromise = new Promise<number>((resolve) => {
			resolvePort = resolve;
		});

		const { pid } = await vm.process.spawn("node", ["/tmp/network.js"], {
			onStdout: (data: Uint8Array) => {
				const text = new TextDecoder().decode(data);
				const match = text.match(/LISTENING:(\d+)/);
				if (match) {
					resolvePort(Number(match[1]));
				}
			},
		});

		const port = await Promise.race([
			portPromise,
			new Promise<number>((_, reject) =>
				setTimeout(
					() => reject(new Error("timed out waiting for guest listener")),
					5_000,
				),
			),
		]);
		expect(port).toBeGreaterThan(0);

		await vm.process.kill(pid);
	});

	test("delivers a socket write larger than the VM socket buffer to an in-VM reader", async () => {
		// The owning sidecar ledger observes retirement before VM disposal.
		const internal = vm as unknown as {
			_sidecarClient: SidecarProcess;
			_sidecarSession: AuthenticatedSession;
			_sidecarVm: CreatedVm;
		};
		const snapshot = () =>
			internal._sidecarClient.getResourceSnapshot(
				internal._sidecarSession,
				internal._sidecarVm,
			);
		const before = await snapshot();
		const result = await vm.javascript.execute(LARGE_WRITE_SCRIPT, {
			cwd: "/",
			output: { capture: "all" },
		});

		expect(result.stderr).toBe("");
		expect(JSON.parse(result.stdout.trim())).toEqual({
			receivedBytes: 6 * 1024 * 1024,
			intact: true,
		});
		const after = await snapshot();
		for (const key of [
			"runningProcesses",
			"stoppedProcesses",
			"fdTables",
			"openFds",
			"pipes",
			"pipeBufferedBytes",
			"sockets",
			"socketListeners",
			"socketConnections",
			"socketBufferedBytes",
			"socketDatagramQueueLen",
		] as const) {
			expect(
				after[key],
				`process exit must retire ${key} before VM disposal`,
			).toBe(before[key]);
		}
	}, 30_000);
});
