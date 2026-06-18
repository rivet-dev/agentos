/**
 * Regression guard for issue #6:
 * "No hook to inject custom VFS mounts on VM creation".
 *
 * Original bug: there was no supported hook to inject a custom VFS mount at VM
 * creation, forcing callers to monkeypatch `ensureVm`/`createVm` to add a
 * host-synced session VFS (e.g. `/home/user/.pi/agent/sessions`).
 *
 * Fix: `AgentOs.create({ mounts: [...] })` is a documented public option that
 * accepts `MountConfig[]`. A host-synced directory is injected via the public
 * `createHostDirBackend(...)` descriptor, mounted at an arbitrary path -- no
 * patching of any internal VM-creation method required.
 *
 * This test reproduces the exact original use case end-to-end:
 *   1. A host directory with a pre-existing marker file is mounted at the
 *      Pi session path through the public `mounts` option.
 *   2. The host content is visible from inside the VM (proves injection worked).
 *   3. Writes round-trip back to the host directory when `readOnly: false`.
 *   4. Writes are rejected (EROFS) when the mount is read-only.
 *   5. The public API surface (`mounts` option + `createHostDirBackend` export)
 *      exists, guarding against regression back to the patch-only state.
 */

import * as fs from "node:fs";
import * as os from "node:os";
import * as path from "node:path";
import { afterEach, beforeEach, describe, expect, test } from "vitest";
import { AgentOs, createHostDirBackend } from "../src/index.js";

const SESSION_MOUNT_PATH = "/home/user/.pi/agent/sessions";

describe("custom VFS mount injection hook on VM creation", () => {
	let vm: AgentOs | undefined;
	let hostDir: string;

	beforeEach(() => {
		hostDir = fs.mkdtempSync(path.join(os.tmpdir(), "custom-vfs-mount-"));
		// Pre-seed a marker file on the host, mimicking an existing
		// host-synced Pi sessions directory.
		fs.writeFileSync(
			path.join(hostDir, "marker.json"),
			JSON.stringify({ origin: "host", id: "custom-vfs-mount" }),
		);
	});

	afterEach(async () => {
		if (vm) {
			await vm.dispose();
			vm = undefined;
		}
		fs.rmSync(hostDir, { recursive: true, force: true });
	});

	test("public API exposes the mount-injection hook", () => {
		// The supported hook is the `mounts` create-option plus the exported
		// host-dir descriptor helper. If either disappears, callers are forced
		// back to monkeypatching ensureVm/createVm (the original bug).
		expect(typeof createHostDirBackend).toBe("function");
		const descriptor = createHostDirBackend({ hostPath: hostDir });
		expect(descriptor.id).toBe("host_dir");
		expect(descriptor.config.hostPath).toBe(hostDir);
	});

	test("host-synced mount injected at VM creation exposes existing host files", async () => {
		vm = await AgentOs.create({
			mounts: [
				{
					path: SESSION_MOUNT_PATH,
					plugin: createHostDirBackend({ hostPath: hostDir }),
				},
			],
		});

		const raw = await vm.readFile(`${SESSION_MOUNT_PATH}/marker.json`);
		const decoded = JSON.parse(new TextDecoder().decode(raw));
		expect(decoded).toEqual({ origin: "host", id: "custom-vfs-mount" });
	});

	test("writes through a writable injected mount round-trip back to the host", async () => {
		vm = await AgentOs.create({
			mounts: [
				{
					path: SESSION_MOUNT_PATH,
					plugin: createHostDirBackend({
						hostPath: hostDir,
						readOnly: false,
					}),
				},
			],
		});

		await vm.writeFile(
			`${SESSION_MOUNT_PATH}/session-123.json`,
			JSON.stringify({ from: "vm" }),
		);

		// Host-sync write-through: the file must appear on the real host path.
		const onHost = fs.readFileSync(
			path.join(hostDir, "session-123.json"),
			"utf-8",
		);
		expect(JSON.parse(onHost)).toEqual({ from: "vm" });
	});

	test("read-only injected mount rejects writes with EROFS", async () => {
		vm = await AgentOs.create({
			mounts: [
				{
					path: SESSION_MOUNT_PATH,
					plugin: createHostDirBackend({
						hostPath: hostDir,
						readOnly: true,
					}),
				},
			],
		});

		await expect(
			vm.writeFile(`${SESSION_MOUNT_PATH}/should-fail.json`, "{}"),
		).rejects.toThrow("EROFS");
	});
});
