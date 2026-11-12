import type { ChildProcess } from "node:child_process";
import { isAbsolute } from "node:path";
import { expect, test, vi } from "vitest";
import { AgentOs } from "../src/agent-os.js";
import {
	type AuthenticatedSession,
	type CreatedVm,
	SidecarProcess,
} from "../src/sidecar/process-client.js";
import { SidecarRejectedError } from "../src/sidecar-errors.js";
import {
	bounded,
	observeOwnedClose,
	ownedProcessState,
} from "./helpers/native-owner-fixture.js";
import { boundedNativeFixture } from "./helpers/native-owner-scope.js";

const runNative =
	process.platform === "linux" &&
	process.env.AGENTOS_STAB_HOSTED_LIFECYCLE_NATIVE === "1";
function settled<T>(promise: Promise<T>): Promise<PromiseSettledResult<T>> {
	return promise.then(
		(value) => ({ status: "fulfilled", value }),
		(reason) => ({ status: "rejected", reason }),
	);
}

test.skipIf(!runNative)(
	"public disposal fences delivery of an actually created native VM and preserves its peer",
	async () => {
		const binary = process.env.AGENTOS_SIDECAR_BIN;
		expect(
			binary && isAbsolute(binary),
			"pin an absolute AGENTOS_SIDECAR_BIN",
		).toBe(true);
		const sidecar = await AgentOs.createSidecar(); // Lazy handle: no child/handshake awaits here.
		let peer: AgentOs | undefined;
		let initial: Promise<PromiseSettledResult<AgentOs>> | undefined;
		let late: Promise<PromiseSettledResult<AgentOs>> | undefined;
		let child: ChildProcess | undefined;
		let closed: ReturnType<typeof observeOwnedClose> | undefined;
		let admissionOpen = true;
		let cleanupOpen = true;
		let releaseGate = () => {};
		let restoreVmSpies = () => {};
		const originalSpawn = SidecarProcess.spawn.bind(SidecarProcess);
		// Installed BEFORE first create. Capture the real child synchronously before
		// authentication, not after the public VM promise happens to complete.
		const spawnSpy = vi
			.spyOn(SidecarProcess, "spawn")
			.mockImplementation((options) => {
				if (!admissionOpen)
					throw new Error("native fixture child admission retired");
				if (child)
					throw new Error("native fixture expected exactly one owned sidecar");
				const client = originalSpawn(options);
				child = (
					client as unknown as { protocolClient: { child: ChildProcess } }
				).protocolClient.child;
				closed = observeOwnedClose(child);
				return client;
			});
		await boundedNativeFixture({
			bodyMs: 35_000,
			cleanupMs: 12_000,
			finishMs: 3_000,
			async body(phase) {
				initial = settled(
					AgentOs.create({ sidecar: { kind: "explicit", handle: sidecar } }),
				);
				const first = await phase.step(
					() => initial!,
					10_000,
					"first real native VM creation",
				);
				if (first.status === "rejected") throw first.reason;
				peer = first.value;
				const livePeer = peer;
				const client = (peer as unknown as { _sidecarClient: SidecarProcess })
					._sidecarClient;
				expect(child?.pid).toBeTypeOf("number");
				const originalCreate = client.createVm.bind(client);
				let announce!: (value: {
					session: AuthenticatedSession;
					vm: CreatedVm;
				}) => void;
				const arrived = new Promise<{
					session: AuthenticatedSession;
					vm: CreatedVm;
				}>((resolve) => {
					announce = resolve;
				});
				let release!: () => void;
				const gate = new Promise<void>((resolve) => {
					release = resolve;
				});
				releaseGate = release;
				const createSpy = vi
					.spyOn(client, "createVm")
					.mockImplementation(async (session, options) => {
						const vm = await originalCreate(session, options);
						announce({ session, vm }); // Actual native vm_created, before client publication.
						await bounded(
							gate,
							10_000,
							"test-only native create delivery gate",
						);
						return vm;
					});
				const deleteSpy = vi.spyOn(client, "disposeVm"); // Call-through actual native deletion.
				restoreVmSpies = () => {
					createSpy.mockRestore();
					deleteSpy.mockRestore();
				};
				late = settled(
					AgentOs.create({ sidecar: { kind: "explicit", handle: sidecar } }),
				);
				const parked = await phase.step(
					() => arrived,
					10_000,
					"actual native VM creation response",
				);
				expect(
					Array.isArray(
						await phase.step(
							() => client.getProcessSnapshot(parked.session, parked.vm),
							2_000,
							"live native ownership query",
						),
					),
				).toBe(true);
				expect(sidecar.describe().activeVmCount).toBe(2);
				await expect(
					phase.step(
						() => sidecar.dispose(),
						2_000,
						"public pending-creation disposal",
					),
				).rejects.toMatchObject({
					code: "ERR_AGENTOS_SIDECAR_CREATION_PENDING",
				});
				expect(sidecar.describe()).toMatchObject({
					state: "disposing",
					activeVmCount: 2,
				});
				await phase.step(
					() =>
						livePeer.filesystem.writeFile(
							"/tmp/creation-peer",
							"alive while delivery parked",
						),
					2_000,
					"live peer write",
				);
				expect(
					new TextDecoder().decode(
						await phase.step(
							() => livePeer.filesystem.readFile("/tmp/creation-peer"),
							2_000,
							"live peer read",
						),
					),
				).toBe("alive while delivery parked");
				releaseGate();
				const result = await phase.step(
					() => late!,
					15_000,
					"late publication rejection and native cleanup",
				);
				expect(result.status).toBe("rejected");
				if (result.status === "rejected")
					expect(String(result.reason)).toContain(
						"disposal began during VM creation",
					);
				expect(
					deleteSpy.mock.calls.filter(([, vm]) => vm.vmId === parked.vm.vmId),
				).toHaveLength(1);
				// This is an actual native request while the peer/connection remain live.
				// Distinguish absent VM membership from missing session/permission errors.
				const gone = await phase.step(
					() => settled(client.getProcessSnapshot(parked.session, parked.vm)),
					2_000,
					"deleted native VM ownership query",
				);
				expect(gone.status).toBe("rejected");
				if (gone.status === "rejected") {
					expect(gone.reason).toBeInstanceOf(SidecarRejectedError);
					expect((gone.reason as SidecarRejectedError).detail).toMatchObject({
						code: "ERR_AGENTOS_COORDINATOR_NOT_FOUND",
						errno: "ENOENT",
						vm_id: parked.vm.vmId,
						operation: "stdio.requestAdmission",
						message: `ERR_AGENTOS_COORDINATOR_NOT_FOUND: unknown VM ${parked.session.connectionId}:${parked.session.sessionId}:${parked.vm.vmId}`,
					});
				}
				expect(sidecar.describe()).toMatchObject({
					state: "disposing",
					activeVmCount: 1,
				});
				expect(
					new TextDecoder().decode(
						await phase.step(
							() => livePeer.filesystem.readFile("/tmp/creation-peer"),
							2_000,
							"peer remains live after victim cleanup",
						),
					),
				).toBe("alive while delivery parked");
				await phase.step(
					() => livePeer.dispose(),
					2_000,
					"peer disposal with existing1s inner cap",
				);
				peer = undefined;
				await phase.step(
					() => sidecar.dispose(),
					8_000,
					"native owner disposal with default exit waits",
				);
				expect(sidecar.describe()).toMatchObject({
					state: "disposed",
					activeVmCount: 0,
				});
				await phase.step(() => closed!, 2_000, "owned child close");
				expect(
					await phase.step(
						() => ownedProcessState(child!),
						500,
						"owned child reap query",
					),
				).toBeNull();
			},
			async cleanup() {
				async function cleanupStep<T>(
					work: () => Promise<T>,
					timeoutMs: number,
					label: string,
				): Promise<T> {
					if (!cleanupOpen)
						throw new Error("native fixture cleanup phase retired");
					const value = await bounded(
						Promise.resolve().then(work),
						timeoutMs,
						label,
					);
					if (!cleanupOpen)
						throw new Error("native fixture cleanup phase retired");
					return value;
				}
				admissionOpen = false;
				// Synchronously fence this public owner before joining any parked work.
				const fence = settled(sidecar.dispose());
				releaseGate();
				const errors: unknown[] = [];
				const vms = new Set<AgentOs>(peer ? [peer] : []);
				for (const pending of [initial, late]) {
					if (!pending) continue;
					try {
						const result = await cleanupStep(
							() => pending,
							2_000,
							"cleanup join of owned creation",
						);
						if (result.status === "fulfilled") vms.add(result.value);
					} catch (error) {
						errors.push(error);
					}
				}
				for (const vm of vms) {
					try {
						await cleanupStep(
							() => vm.dispose(),
							2_000,
							"cleanup real VM disposal",
						);
					} catch (error) {
						errors.push(error);
					}
				}
				try {
					const closedOwner = await cleanupStep(
						() => fence,
						2_000,
						"cleanup public admission fence",
					);
					if (
						closedOwner.status === "rejected" &&
						(closedOwner.reason as { code?: string })?.code !==
							"ERR_AGENTOS_SIDECAR_CREATION_PENDING"
					)
						throw closedOwner.reason;
					await cleanupStep(
						() => sidecar.dispose(),
						8_000,
						"cleanup final native owner disposal",
					);
				} catch (error) {
					errors.push(error);
				}
				if (errors.length)
					throw new AggregateError(
						errors,
						"native creation fixture cleanup failed",
					);
			},
			async finish() {
				// Finally is independent of ALL prior pending creates/disposals.
				cleanupOpen = false;
				admissionOpen = false;
				releaseGate();
				let emergency = false;
				const terminationErrors: unknown[] = [];
				try {
					if (child && closed) {
						if (child.exitCode === null && child.signalCode === null) {
							emergency = true;
							try {
								if (!child.kill("SIGKILL"))
									terminationErrors.push(
										new Error("owned native cleanup SIGKILL was not sent"),
									);
							} catch (error) {
								terminationErrors.push(error);
							}
						}
						try {
							await bounded(closed, 2_000, "final owned native kill/reap");
						} catch (error) {
							terminationErrors.push(error);
						}
						try {
							expect(
								await bounded(
									ownedProcessState(child),
									500,
									"final owned reap query",
								),
							).toBeNull();
						} catch (error) {
							terminationErrors.push(error);
						}
					}
				} finally {
					restoreVmSpies();
					spawnSpy.mockRestore();
				}
				if (emergency)
					terminationErrors.push(
						new Error("native creation fixture required emergency child kill"),
					);
				if (terminationErrors.length)
					throw new AggregateError(
						terminationErrors,
						"owned native termination failed",
					);
			},
		});
	},
	60_000,
);
