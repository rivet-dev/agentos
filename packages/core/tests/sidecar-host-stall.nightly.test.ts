import type { ChildProcess } from "node:child_process";
import { join, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import { expect, test } from "vitest";
import { SidecarProcess, SidecarSilenceTimeout } from "../src/index.js";
import { serializePermissionsForSidecar } from "../src/sidecar/permissions.js";
import { serializeRootFilesystemForSidecar } from "../src/sidecar/rpc-client.js";
import {
	bodyWithCleanup,
	bounded,
	observeOwnedClose,
} from "./helpers/native-owner-fixture.js";
import { ALLOW_ALL_VM_PERMISSIONS } from "./helpers/permissions.js";

const REPO_ROOT = fileURLToPath(new URL("../../..", import.meta.url));
const SIDECAR_BINARY = process.env.AGENTOS_SIDECAR_BIN
	? resolve(process.env.AGENTOS_SIDECAR_BIN)
	: join(REPO_ROOT, "target/debug/agentos-sidecar");

function blockHostEventLoop(ms: number): void {
	Atomics.wait(new Int32Array(new SharedArrayBuffer(4)), 0, 0, ms);
}

// Reaches into internals only to find the real process to freeze.
function sidecarChild(client: SidecarProcess): ChildProcess {
	return (client as unknown as { protocolClient: { child: ChildProcess } })
		.protocolClient.child;
}

function spawnSidecar(silenceTimeoutMs: number): SidecarProcess {
	return SidecarProcess.spawn({
		cwd: REPO_ROOT,
		command: SIDECAR_BINARY,
		silenceTimeoutMs,
	});
}

function isLive(child: ChildProcess): boolean {
	return child.exitCode === null && child.signalCode === null;
}

function signalOwned(child: ChildProcess, signal: NodeJS.Signals): void {
	if (!child.kill(signal))
		throw new Error(`owned sidecar did not accept ${signal}`);
}

function isExpectedSilence(error: unknown): boolean {
	return (
		error instanceof SidecarSilenceTimeout ||
		(error instanceof AggregateError &&
			error.errors.length > 0 &&
			error.errors.every(isExpectedSilence))
	);
}

async function retireOwned(
	client: SidecarProcess,
	child: ChildProcess,
	closed: ReturnType<typeof observeOwnedClose>,
	expectSilence = false,
	disposeVm?: () => Promise<void>,
): Promise<void> {
	const errors: unknown[] = [];
	// Resume the exact owned child even if a failed assertion interrupted a stop.
	try {
		if (isLive(child)) signalOwned(child, "SIGCONT");
	} catch (error) {
		errors.push(error);
	}
	try {
		if (disposeVm) await bounded(disposeVm(), 5_000, "host-stall VM disposal");
	} catch (error) {
		errors.push(error);
	}
	try {
		await bounded(client.dispose(), 8_000, "host-stall sidecar disposal");
	} catch (error) {
		if (expectSilence && isExpectedSilence(error)) {
			console.warn(
				"host-stall observed expected silence disposal failure",
				error,
			);
		} else {
			errors.push(error);
		}
	}
	try {
		if (isLive(child)) signalOwned(child, "SIGKILL");
	} catch (error) {
		errors.push(error);
	}
	try {
		await bounded(closed, 2_000, "host-stall owned child close");
	} catch (error) {
		errors.push(error);
	}
	if (errors.length === 1) throw errors[0];
	if (errors.length > 1)
		throw new AggregateError(errors, "host-stall cleanup failed");
}

const vmConfig = () => ({
	runtime: "java_script" as const,
	config: {
		env: {},
		loopbackExemptPorts: [],
		rootFilesystem: serializeRootFilesystemForSidecar(),
		permissions: serializePermissionsForSidecar(ALLOW_ALL_VM_PERMISSIONS),
	},
});

test.each([
	{
		name: "a host event loop blocked past the silence window",
		freezeSidecar: false,
		// The guest kept running through the stall.
		minTicksAfterStall: 30,
	},
	{
		// As with a paused container: no heartbeats wait in the pipe on wake.
		name: "a host and sidecar frozen together past the silence window",
		freezeSidecar: true,
		minTicksAfterStall: 1,
	},
])("$name leaves the sidecar and its guest running", async ({
	freezeSidecar,
	minTicksAfterStall,
}) => {
	// The sidecar only heartbeats every 10s, so a window this low relies on the
	// guest's output to stay alive before and after the stall.
	const silenceTimeoutMs = 5_000;
	const client = spawnSidecar(silenceTimeoutMs);
	const child = sidecarChild(client);
	const closed = observeOwnedClose(child);
	let disposeVm: (() => Promise<void>) | undefined;
	await bodyWithCleanup(
		async () => {
			const session = await client.authenticateAndOpenSession();
			const vm = await client.createVm(session, vmConfig());
			disposeVm = () => client.disposeVm(session, vm);
			await client.waitForEvent(
				(event) =>
					event.payload.type === "vm_lifecycle" &&
					event.payload.state === "ready",
				silenceTimeoutMs,
			);
			await client.writeFile(
				session,
				vm,
				"/tmp/ticker.mjs",
				"let tick = 0; setInterval(() => console.log(++tick), 100);",
			);

			let lastTick = 0;
			let tickBeforeStall = 0;
			let guestExited = false;
			let stallFinished = false;
			let callbackFailure: { error: unknown } | undefined;
			let eventWaitAcrossStall: Promise<unknown> | undefined;
			client.onEvent((event) => {
				try {
					if (event.payload.type === "process_exited") {
						guestExited = true;
					}
					if (event.payload.type !== "process_output") {
						return;
					}
					const ticks = Buffer.from(event.payload.chunk)
						.toString("utf8")
						.match(/\d+/g);
					lastTick = Number(ticks?.at(-1) ?? lastTick);
					if (tickBeforeStall !== 0 || lastTick === 0) {
						return;
					}
					tickBeforeStall = lastTick;
					if (!freezeSidecar) {
						// The stall outlasts this timer, and the next tick waits in the pipe.
						eventWaitAcrossStall = client.waitForEvent(
							(next) =>
								next !== event && next.payload.type === "process_output",
							1_000,
						);
					}
					if (freezeSidecar) {
						signalOwned(child, "SIGSTOP");
					}
					// Blocking inside an I/O callback, as a slow mount driver does, is the
					// worst case: Node runs timers before it polls for I/O again, so timers
					// run before the frames that wait in the pipe are read.
					blockHostEventLoop(silenceTimeoutMs + 1_000);
					if (freezeSidecar) {
						signalOwned(child, "SIGCONT");
					}
					stallFinished = true;
				} catch (error) {
					callbackFailure = { error };
					stallFinished = true;
				}
			});
			await client.execute(session, vm, {
				processId: "ticker",
				runtime: "java_script",
				entrypoint: "/tmp/ticker.mjs",
			});
			await expect.poll(() => stallFinished, { timeout: 10_000 }).toBe(true);
			if (callbackFailure) throw callbackFailure.error;

			// A timed wait that spans the stall gets its event, not a timeout.
			await eventWaitAcrossStall;
			// The guest's output still arrives.
			await expect
				.poll(() => lastTick, { timeout: 4_000 })
				.toBeGreaterThanOrEqual(tickBeforeStall + minTicksAfterStall);
			expect(guestExited).toBe(false);

			// The same VM still serves calls.
			await client.writeFile(session, vm, "/tmp/after-stall.txt", "still here");
			expect(
				Buffer.from(
					await client.readFile(session, vm, "/tmp/after-stall.txt"),
				).toString("utf8"),
			).toBe("still here");
		},
		() => retireOwned(client, child, closed, false, disposeVm),
	);
}, 30_000);

test("a silent sidecar is killed and its in-flight call rejects while the host keeps stalling", async () => {
	const silenceTimeoutMs = 2_000;
	const client = spawnSidecar(silenceTimeoutMs);
	const child = sidecarChild(client);
	const closed = observeOwnedClose(child);
	const killedBy = new Promise((resolve) =>
		child.once("exit", (_code, signal) => resolve(signal)),
	);
	await bodyWithCleanup(
		async () => {
			const session = await client.authenticateAndOpenSession();
			signalOwned(child, "SIGSTOP");

			let settled = false;
			const inFlight = client.createVm(session, vmConfig()).finally(() => {
				settled = true;
			});
			const rejection = expect(inFlight).rejects.toBeInstanceOf(
				SidecarSilenceTimeout,
			);

			// Each burst outlasts the watchdog's check interval, and the loop only
			// gets a moment between bursts.
			const deadline = performance.now() + 15_000;
			while (!settled && performance.now() < deadline) {
				blockHostEventLoop(800);
				await new Promise((resolve) => setTimeout(resolve, 0));
			}

			expect(settled).toBe(true);
			await rejection;
			await expect(
				bounded(killedBy, 2_000, "host-stall watchdog exit signal"),
			).resolves.toBe("SIGKILL");
			await expect(client.createVm(session, vmConfig())).rejects.toBeInstanceOf(
				SidecarSilenceTimeout,
			);
		},
		() => retireOwned(client, child, closed, true),
	);
}, 30_000);
