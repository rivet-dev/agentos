import type { ChildProcess } from "node:child_process";
import { join, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import { expect, test } from "vitest";
import { AgentOs, SidecarFrameTooLarge, SidecarProcess } from "../src/index.js";
import { serializeRootFilesystemForSidecar } from "../src/sidecar/rpc-client.js";
import { SidecarProcessExited } from "../src/sidecar-errors.js";
import { bodyWithCleanup } from "./helpers/native-owner-fixture.js";

const REPO_ROOT = fileURLToPath(new URL("../../..", import.meta.url));
const SIDECAR_BINARY = process.env.AGENTOS_SIDECAR_BIN
	? resolve(process.env.AGENTOS_SIDECAR_BIN)
	: join(REPO_ROOT, "target/debug/agentos-sidecar");

// Kills the VM's real sidecar process. This reaches into internals only to find
// the process; the behavior under test is the public evaluate() result.
function protocolClient(vm: AgentOs): {
	child: ChildProcess;
	failPermanently(error: Error): void;
} {
	return (
		vm as unknown as {
			_sidecarClient: {
				protocolClient: {
					child: ChildProcess;
					failPermanently(error: Error): void;
				};
			};
		}
	)._sidecarClient.protocolClient;
}

function sidecarChild(vm: AgentOs): ChildProcess {
	return protocolClient(vm).child;
}

// Accept only the intentional terminal failure, including aggregates made solely
// from that failure. Unexpected cleanup errors still fail the test.
function matchesTerminalFailure(
	error: unknown,
	expected: (error: unknown) => boolean,
): boolean {
	return (
		expected(error) ||
		(error instanceof AggregateError &&
			error.errors.length > 0 &&
			error.errors.every((entry: unknown) =>
				matchesTerminalFailure(entry, expected),
			))
	);
}

async function disposeOwners(
	owners: Array<{
		dispose: () => Promise<void>;
		expected?: (error: unknown) => boolean;
	}>,
): Promise<void> {
	const errors: unknown[] = [];
	for (const owner of owners) {
		try {
			await owner.dispose();
		} catch (error) {
			if (!owner.expected || !matchesTerminalFailure(error, owner.expected))
				errors.push(error);
			else
				console.warn(
					"[sidecar-exit fixture] observed expected terminal disposal failure",
					error,
				);
		}
	}
	if (errors.length === 1) throw errors[0];
	if (errors.length > 1)
		throw new AggregateError(errors, "sidecar exit fixture cleanup failed");
}

test("a running evaluation rejects when its sidecar exits", async () => {
	// A dedicated sidecar keeps the killed process out of the shared pool.
	const sidecar = await AgentOs.createSidecar();
	let vm: AgentOs | undefined;
	await bodyWithCleanup(
		async () => {
			vm = await AgentOs.create({
				sidecar: { kind: "explicit", handle: sidecar },
			});
			const evaluation = vm.javascript.evaluate(
				"(() => { while (true) {} })()",
				{
					timeoutMs: 30_000,
				},
			);
			await new Promise((resolve) => setTimeout(resolve, 300));
			sidecarChild(vm).kill("SIGKILL");

			await expect(evaluation).rejects.toBeInstanceOf(SidecarProcessExited);
		},
		() =>
			disposeOwners([
				...(vm
					? [
							{
								dispose: () => vm!.dispose(),
								expected: (error: unknown) =>
									error instanceof SidecarProcessExited,
							},
						]
					: []),
				{
					dispose: () => sidecar.dispose(),
					expected: (error: unknown) => error instanceof SidecarProcessExited,
				},
			]),
	);
}, 60_000);

test("a shared pool replaces its sidecar after the process dies", async () => {
	const pool = `crash-${process.pid}`;
	const orphan = await AgentOs.create({ sidecar: { kind: "shared", pool } });
	let fresh: AgentOs | undefined;
	await bodyWithCleanup(
		async () => {
			await orphan.filesystem.writeFile("/tmp/who", "orphan");
			sidecarChild(orphan).kill("SIGKILL");

			// A VM whose sidecar died fails fast with a typed error.
			await expect(
				orphan.filesystem.readFile("/tmp/who"),
			).rejects.toBeInstanceOf(SidecarProcessExited);

			// A new VM in the same pool gets a working sidecar.
			fresh = await AgentOs.create({ sidecar: { kind: "shared", pool } });
			await fresh.filesystem.writeFile("/tmp/who", "fresh");
			expect(
				new TextDecoder().decode(await fresh.filesystem.readFile("/tmp/who")),
			).toBe("fresh");
			// Recovery releases the dead owner, but preserves the failed VM's
			// terminal teardown result instead of reporting false success.
			const disposal = orphan.dispose();
			await expect(disposal).rejects.toThrow("sidecar process exited");
			const failure = await disposal.catch((error: unknown) => error);
			await expect(orphan.dispose()).rejects.toBe(failure);
			expect(
				new TextDecoder().decode(await fresh.filesystem.readFile("/tmp/who")),
			).toBe("fresh");
		},
		() =>
			disposeOwners([
				{
					dispose: () => orphan.dispose(),
					expected: (error: unknown) => error instanceof SidecarProcessExited,
				},
				...(fresh ? [{ dispose: () => fresh!.dispose() }] : []),
			]),
	);
}, 60_000);

test("a sidecar whose transport fails is stopped, not leaked", async () => {
	const pool = `transport-failure-${process.pid}`;
	const broken = await AgentOs.create({ sidecar: { kind: "shared", pool } });
	let fresh: AgentOs | undefined;
	const transportFailure = new Error("simulated frame decode failure");
	await bodyWithCleanup(
		async () => {
			const child = sidecarChild(broken);
			const exited = new Promise<void>((resolve) =>
				child.once("exit", () => resolve()),
			);
			// A transport can fail while the process still runs, for example on an
			// event buffer overflow or an undecodable frame.
			protocolClient(broken).failPermanently(transportFailure);

			// The sidecar process does not keep running with its VMs out of reach.
			await expect(
				Promise.race([
					exited.then(() => "exited"),
					new Promise((resolve) =>
						setTimeout(() => resolve("still running"), 5_000),
					),
				]),
			).resolves.toBe("exited");
			// The pool replaces it.
			fresh = await AgentOs.create({ sidecar: { kind: "shared", pool } });
			await fresh.filesystem.writeFile("/tmp/ok", "ok");
		},
		() =>
			disposeOwners([
				{
					dispose: () => broken.dispose(),
					expected: (error: unknown) => error === transportFailure,
				},
				...(fresh ? [{ dispose: () => fresh!.dispose() }] : []),
			]),
	);
}, 60_000);

test("a sidecar outlives more VM create and dispose cycles than its event buffer holds", async () => {
	const eventBufferCapacity = 8;
	const client = SidecarProcess.spawn({
		cwd: REPO_ROOT,
		command: SIDECAR_BINARY,
		eventBufferCapacity,
	});
	await bodyWithCleanup(
		async () => {
			const session = await client.authenticateAndOpenSession();
			// Each cycle leaves lifecycle events that no caller waits for.
			for (let cycle = 0; cycle < eventBufferCapacity * 2; cycle += 1) {
				const vm = await client.createVm(session, {
					runtime: "java_script",
					config: {
						env: {},
						loopbackExemptPorts: [],
						rootFilesystem: serializeRootFilesystemForSidecar(),
					},
				});
				await client.waitForEvent(
					{
						type: "vm_lifecycle",
						ownership: {
							scope: "vm",
							connection_id: session.connectionId,
							session_id: session.sessionId,
							vm_id: vm.vmId,
						},
						state: "ready",
					},
					10_000,
				);
				await client.disposeVm(session, vm);
			}
		},
		() => client.dispose(),
	);
}, 60_000);

test("an oversized request rejects without stopping the sidecar its VMs share", async () => {
	const uncaught: unknown[] = [];
	const recordUncaught = (error: unknown) => uncaught.push(error);
	const sidecar = await AgentOs.createSidecar();
	process.on("uncaughtException", recordUncaught);
	const placement = { kind: "explicit", handle: sidecar } as const;
	let writer: AgentOs | undefined;
	let bystander: AgentOs | undefined;
	await bodyWithCleanup(
		async () => {
			writer = await AgentOs.create({
				defaultSoftware: false,
				sidecar: placement,
			});
			bystander = await AgentOs.create({
				defaultSoftware: false,
				sidecar: placement,
			});
			// Base64 content encoding puts this request over the sidecar frame limit.
			const error = await writer.filesystem
				.writeFile("/tmp/big.bin", new Uint8Array(13 * 1024 * 1024))
				.catch((caught: unknown) => caught);
			expect(error).toMatchObject({ name: "SidecarFrameTooLarge" });
			expect(error).toBeInstanceOf(SidecarFrameTooLarge);
			const { frameBytes, maxFrameBytes } = error as SidecarFrameTooLarge;
			expect(frameBytes).toBeGreaterThan(maxFrameBytes);
			expect((error as Error).message).toContain(`limit is ${maxFrameBytes}`);

			// Another VM on the same sidecar keeps working, and so does the writer.
			expect(await bystander.javascript.evaluate("1 + 1")).toMatchObject({
				outcome: "succeeded",
				value: 2,
			});
			await writer.filesystem.writeFile("/tmp/small.txt", "ok");
			expect(uncaught).toEqual([]);
		},
		async () => {
			process.off("uncaughtException", recordUncaught);
			await disposeOwners([
				...(writer ? [{ dispose: () => writer!.dispose() }] : []),
				...(bystander ? [{ dispose: () => bystander!.dispose() }] : []),
				{ dispose: () => sidecar.dispose() },
			]);
		},
	);
}, 60_000);

// Fast injected control: no native process or resource saturation.
test("unexpected cleanup fails after every retained owner is attempted", async () => {
	const failure = new Error("injected unexpected cleanup failure");
	const calls: string[] = [];
	await expect(
		bodyWithCleanup(
			async () => {},
			() =>
				disposeOwners([
					{
						dispose: async () => {
							calls.push("vm");
							throw failure;
						},
					},
					{
						dispose: async () => {
							calls.push("sidecar");
						},
					},
				]),
		),
	).rejects.toBe(failure);
	expect(calls).toEqual(["vm", "sidecar"]);
});
