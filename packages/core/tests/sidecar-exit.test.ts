import type { ChildProcess } from "node:child_process";
import { join, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import { SidecarProcessExited } from "../src/sidecar-errors.js";
import { expect, test } from "vitest";
import { AgentOs, SidecarFrameTooLarge, SidecarProcess } from "../src/index.js";
import {
	serializeRootFilesystemForSidecar,
} from "../src/sidecar/rpc-client.js";

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

test("a running evaluation rejects when its sidecar exits", async () => {
	// A dedicated sidecar keeps the killed process out of the shared pool.
	const sidecar = await AgentOs.createSidecar();
	const vm = await AgentOs.create({
		sidecar: { kind: "explicit", handle: sidecar },
	});
	try {
		const evaluation = vm.javascript.evaluate("(() => { while (true) {} })()", {
			timeoutMs: 30_000,
		});
		await new Promise((resolve) => setTimeout(resolve, 300));
		sidecarChild(vm).kill("SIGKILL");

		await expect(evaluation).rejects.toBeInstanceOf(SidecarProcessExited);
	} finally {
		await vm.dispose().catch(() => {});
		await sidecar.dispose().catch(() => {});
	}
}, 60_000);

test("a shared pool replaces its sidecar after the process dies", async () => {
	const pool = `crash-${process.pid}`;
	const orphan = await AgentOs.create({ sidecar: { kind: "shared", pool } });
	const fresh = await (async () => {
		await orphan.filesystem.writeFile("/tmp/who", "orphan");
		sidecarChild(orphan).kill("SIGKILL");

		// A VM whose sidecar died fails fast with a typed error.
		await expect(orphan.filesystem.readFile("/tmp/who")).rejects.toBeInstanceOf(
			SidecarProcessExited,
		);

		// A new VM in the same pool gets a working sidecar.
		const vm = await AgentOs.create({ sidecar: { kind: "shared", pool } });
		await vm.filesystem.writeFile("/tmp/who", "fresh");
		expect(
			new TextDecoder().decode(await vm.filesystem.readFile("/tmp/who")),
		).toBe("fresh");
		return vm;
	})();
	// A VM whose sidecar died still shuts down cleanly.
	await expect(orphan.dispose()).resolves.toBeUndefined();
	await fresh.dispose();
}, 60_000);

test("a sidecar whose transport fails is stopped, not leaked", async () => {
	const pool = `transport-failure-${process.pid}`;
	const broken = await AgentOs.create({ sidecar: { kind: "shared", pool } });
	const child = sidecarChild(broken);
	const exited = new Promise<void>((resolve) => child.once("exit", () => resolve()));
	// A transport can fail while the process still runs, for example on an
	// event buffer overflow or an undecodable frame.
	protocolClient(broken).failPermanently(new Error("simulated frame decode failure"));

	// The sidecar process does not keep running with its VMs out of reach.
	await expect(
		Promise.race([
			exited.then(() => "exited"),
			new Promise((resolve) => setTimeout(() => resolve("still running"), 5_000)),
		]),
	).resolves.toBe("exited");
	// The pool replaces it.
	const fresh = await AgentOs.create({ sidecar: { kind: "shared", pool } });
	await fresh.filesystem.writeFile("/tmp/ok", "ok");
	await broken.dispose().catch(() => {});
	await fresh.dispose();
}, 60_000);

test("a sidecar outlives more VM create and dispose cycles than its event buffer holds", async () => {
	const eventBufferCapacity = 8;
	const client = SidecarProcess.spawn({
		cwd: REPO_ROOT,
		command: SIDECAR_BINARY,
		eventBufferCapacity,
	});
	try {
		const session = await client.authenticateAndOpenSession();
		// Each cycle leaves lifecycle events that no caller waits for.
		for (let cycle = 0; cycle < eventBufferCapacity * 2; cycle += 1) {
			const vm = await client.createVm(session, {
				runtime: "java_script",
				config: {
					env: {},
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
	} catch (error) {
		await client.dispose().catch(() => {});
		throw error;
	}
	await client.dispose();
}, 60_000);

test("an oversized request rejects without stopping the sidecar its VMs share", async () => {
	const uncaught: unknown[] = [];
	const recordUncaught = (error: unknown) => uncaught.push(error);
	process.on("uncaughtException", recordUncaught);
	const sidecar = await AgentOs.createSidecar();
	const placement = { kind: "explicit", handle: sidecar } as const;
	const writer = await AgentOs.create({
		defaultSoftware: false,
		sidecar: placement,
	});
	const bystander = await AgentOs.create({
		defaultSoftware: false,
		sidecar: placement,
	});
	try {
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
	} finally {
		process.off("uncaughtException", recordUncaught);
		await writer.dispose().catch(() => {});
		await bystander.dispose().catch(() => {});
		await sidecar.dispose().catch(() => {});
	}
}, 60_000);
