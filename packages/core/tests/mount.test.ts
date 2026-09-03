import { afterEach, describe, expect, test } from "vitest";
import { AgentOs, type VirtualFileSystem } from "../src/index.js";
import { createInMemoryFileSystem } from "../src/test/runtime.js";

const VFS_METHODS = [
	"readFile",
	"readTextFile",
	"readDir",
	"readDirWithTypes",
	"writeFile",
	"createDir",
	"mkdir",
	"exists",
	"stat",
	"removeFile",
	"removeDir",
	"rename",
	"realpath",
	"symlink",
	"readlink",
	"lstat",
	"link",
	"chmod",
	"chown",
	"utimes",
	"truncate",
	"pread",
	"pwrite",
] as const;

function createRecordingFilesystem(): {
	fs: VirtualFileSystem;
	calls: string[];
} {
	const base = createInMemoryFileSystem();
	const calls: string[] = [];
	const delegates = base as unknown as Record<
		(typeof VFS_METHODS)[number],
		(...args: unknown[]) => unknown
	>;
	const fs = Object.fromEntries(
		VFS_METHODS.map((method) => [
			method,
			(...args: unknown[]) => {
				calls.push(`${method}:${String(args[0])}`);
				return delegates[method].apply(base, args);
			},
		]),
	) as unknown as VirtualFileSystem;

	return { fs, calls };
}

function createMountVm(
	options: NonNullable<Parameters<typeof AgentOs.create>[0]> = {},
): Promise<AgentOs> {
	return AgentOs.create({ defaultSoftware: false, ...options });
}

describe("mount integration", () => {
	let vm: AgentOs | undefined;

	afterEach(async () => {
		await vm?.dispose();
		vm = undefined;
	});

	test("create with memory mount", async () => {
		vm = await createMountVm({
			mounts: [{ path: "/data", driver: createInMemoryFileSystem() }],
		});
		expect(await vm.exists("/data")).toBe(true);
	});

	test("writeFile and readFile round-trip through mounted backend", async () => {
		vm = await createMountVm({
			mounts: [{ path: "/data", driver: createInMemoryFileSystem() }],
		});
		await vm.writeFile("/data/foo.txt", "hello mount");
		const data = await vm.readFile("/data/foo.txt");
		expect(new TextDecoder().decode(data)).toBe("hello mount");
	});

	test("create with declarative native memory mount config", async () => {
		vm = await createMountVm({
			mounts: [
				{
					path: "/native",
					plugin: {
						id: "memory",
						config: {},
					},
				},
			],
		});
		await vm.writeFile("/native/plugin.txt", "native mount");
		const data = await vm.readFile("/native/plugin.txt");
		expect(new TextDecoder().decode(data)).toBe("native mount");
	});

	test("root FS and mount are separate", async () => {
		vm = await createMountVm({
			mounts: [{ path: "/data", driver: createInMemoryFileSystem() }],
		});
		await vm.writeFile("/home/agentos/foo.txt", "root content");
		await vm.writeFile("/data/foo.txt", "mount content");

		const rootData = await vm.readFile("/home/agentos/foo.txt");
		const mountData = await vm.readFile("/data/foo.txt");
		expect(new TextDecoder().decode(rootData)).toBe("root content");
		expect(new TextDecoder().decode(mountData)).toBe("mount content");
	});

	test("runtime mountFs and unmountFs work", async () => {
		vm = await createMountVm();

		await vm.mountFs({
			path: "/mnt/dynamic",
			plugin: { id: "memory", config: {} },
		});
		await vm.writeFile("/mnt/dynamic/test.txt", "dynamic");
		const data = await vm.readFile("/mnt/dynamic/test.txt");
		expect(new TextDecoder().decode(data)).toBe("dynamic");

		await vm.unmountFs("/mnt/dynamic");
		await expect(vm.readFile("/mnt/dynamic/test.txt")).rejects.toThrow();
		// Runtime mount + unmount each trigger a full sidecar reconfigure, so this
		// integration test needs more than the 30s default (see PR #1521 CI).
	}, 120_000);

	test("runtime mountFs accepts a portable sidecar descriptor", async () => {
		vm = await createMountVm();

		await vm.mountFs({
			path: "/mnt/custom",
			plugin: { id: "memory", config: {} },
		});
		await vm.writeFile("/mnt/custom/note.txt", "from custom vfs");

		expect(
			new TextDecoder().decode(await vm.readFile("/mnt/custom/note.txt")),
		).toBe("from custom vfs");
		await vm.unmountFs("/mnt/custom");
		await expect(vm.readFile("/mnt/custom/note.txt")).rejects.toThrow();
		// Runtime mount + unmount each trigger a full sidecar reconfigure, so this
		// integration test needs more than the 30s default (see PR #1521 CI).
	}, 120_000);

	test("guest processes can read and write a create-time plain JS VFS mount", async () => {
		const mounted = createRecordingFilesystem();
		// Mount flags do not override POSIX DAC: this fixture starts root-owned.
		await mounted.fs.chmod("/", 0o777);
		vm = await createMountVm({
			mounts: [{ path: "/mnt/custom", driver: mounted.fs }],
		});

		await vm.writeFile("/mnt/custom/host.txt", "from host api");
		const result = await vm.execArgv("node", [
			"-e",
			[
				'const { readFileSync, writeFileSync } = require("node:fs");',
				'console.log(readFileSync("/mnt/custom/host.txt", "utf8"));',
				'writeFileSync("/mnt/custom/guest.txt", "from guest process");',
			].join("\n"),
		]);
		expect(result.exitCode, result.stderr).toBe(0);
		expect(result.stdout.trim()).toBe("from host api");
		// Guest reads use bounded pread callbacks, not an unbounded readFile.
		expect(mounted.calls).toContain("pread:/host.txt");
		expect(mounted.calls).toContain("writeFile:/guest.txt");
		expect(
			new TextDecoder().decode(await vm.readFile("/mnt/custom/guest.txt")),
		).toBe("from guest process");
	});

	test("guest processes can read and write a runtime-mounted portable VFS", async () => {
		vm = await createMountVm({ user: { uid: 0, gid: 0 } });

		await vm.mountFs({
			path: "/mnt/custom",
			plugin: { id: "memory", config: {} },
		});
		await vm.writeFile("/mnt/custom/host.txt", "from host api");
		const result = await vm.execArgv("node", [
			"-e",
			[
				// Provision the root-owned memory mount, then test as the usual
				// unprivileged guest. Writable mount flags alone do not grant DAC.
				'require("node:fs").chmodSync("/mnt/custom", 0o777);',
				"process.setgid(1000); process.setuid(1000);",
				'const { readFileSync, writeFileSync } = require("node:fs");',
				'console.log(readFileSync("/mnt/custom/host.txt", "utf8"));',
				'writeFileSync("/mnt/custom/guest.txt", "from guest process");',
			].join("\n"),
		]);
		expect(result.exitCode, result.stderr).toBe(0);
		expect(result.stdout.trim()).toBe("from host api");
		expect(
			new TextDecoder().decode(await vm.readFile("/mnt/custom/guest.txt")),
		).toBe("from guest process");
		await vm.unmountFs("/mnt/custom");
		await expect(vm.readFile("/mnt/custom/guest.txt")).rejects.toThrow(
			"ENOENT",
		);
	});

	test("mount contents never overwrite or expose the hidden root filesystem", async () => {
		vm = await createMountVm();
		await vm.mkdir("/mnt/overlay", { recursive: true });
		await vm.writeFile("/mnt/overlay/keep.txt", "root original");
		await vm.writeFile("/mnt/overlay/remove.txt", "root retained");
		await vm.mountFs({
			path: "/mnt/overlay",
			plugin: { id: "memory", config: {} },
		});
		await expect(vm.readFile("/mnt/overlay/keep.txt")).rejects.toThrow(
			"ENOENT",
		);
		await vm.writeFile("/mnt/overlay/keep.txt", "mount replacement");
		await vm.writeFile("/mnt/overlay/remove.txt", "mount removed");
		await vm.remove("/mnt/overlay/remove.txt");
		await vm.unmountFs("/mnt/overlay");
		expect(
			new TextDecoder().decode(await vm.readFile("/mnt/overlay/keep.txt")),
		).toBe("root original");
		expect(
			new TextDecoder().decode(await vm.readFile("/mnt/overlay/remove.txt")),
		).toBe("root retained");
	});

	test("guest mount mutations leave hidden root files and metadata intact", async () => {
		vm = await createMountVm({ user: { uid: 0, gid: 0 } });
		await vm.mkdir("/mnt/overlay", { recursive: true });
		for (const name of ["remove", "rename", "mode"]) {
			await vm.writeFile(`/mnt/overlay/${name}`, "root original");
		}
		const originalMode = (await vm.stat("/mnt/overlay/mode")).mode;
		await vm.mountFs({
			path: "/mnt/overlay",
			plugin: { id: "memory", config: {} },
		});
		for (const name of ["remove", "rename", "mode"]) {
			await vm.writeFile(`/mnt/overlay/${name}`, "mount content");
		}
		const result = await vm.execArgv("node", [
			"-e",
			[
				'const fs = require("node:fs");',
				'fs.unlinkSync("/mnt/overlay/remove");',
				'fs.renameSync("/mnt/overlay/rename", "/mnt/overlay/moved");',
				'fs.chmodSync("/mnt/overlay/mode", 0o777);',
			].join("\n"),
		]);
		expect(result.exitCode, result.stderr).toBe(0);
		await vm.unmountFs("/mnt/overlay");
		for (const name of ["remove", "rename", "mode"]) {
			expect(
				new TextDecoder().decode(await vm.readFile(`/mnt/overlay/${name}`)),
			).toBe("root original");
		}
		expect((await vm.stat("/mnt/overlay/mode")).mode).toBe(originalMode);
		expect(await vm.exists("/mnt/overlay/moved")).toBe(false);
	});

	test.each([
		"JS bridge",
		"native memory",
	])("%s mounts preserve POSIX write denial", async (kind) => {
		vm = await createMountVm({
			mounts: [
				kind === "JS bridge"
					? { path: "/restricted", driver: createInMemoryFileSystem() }
					: { path: "/restricted", plugin: { id: "memory", config: {} } },
			],
		});
		expect((await vm.stat("/restricted")).uid).toBe(0);
		const result = await vm.execArgv("node", [
			"-e",
			[
				"console.log(process.getuid());",
				'try { require("node:fs").writeFileSync("/restricted/blocked.txt", "no"); }',
				"catch (error) { console.log(error.code); }",
			].join("\n"),
		]);
		expect(result.exitCode, result.stderr).toBe(0);
		expect(result.stdout.trim()).toBe("1000\nEACCES");
		expect(await vm.exists("/restricted/blocked.txt")).toBe(false);
	});

	test("readdir('/') includes 'data' alongside standard POSIX dirs", async () => {
		vm = await createMountVm({
			mounts: [{ path: "/data", driver: createInMemoryFileSystem() }],
		});
		const entries = await vm.readdir("/");
		expect(entries).toContain("data");
		// Standard POSIX dirs should also be present
		expect(entries).toContain("tmp");
		expect(entries).toContain("home");
	});

	test("rename across mounts throws EXDEV", async () => {
		vm = await createMountVm({
			mounts: [{ path: "/data", driver: createInMemoryFileSystem() }],
		});
		await vm.writeFile("/data/cross.txt", "cross-mount");
		await expect(
			vm.move("/data/cross.txt", "/home/agentos/cross.txt"),
		).rejects.toThrow("EXDEV");
	});

	test("readOnly mount blocks writeFile with EROFS", async () => {
		const mounted = createInMemoryFileSystem();
		await mounted.chmod("/", 0o777);
		vm = await createMountVm({
			mounts: [{ path: "/ro", driver: mounted, readOnly: true }],
		});
		await expect(
			vm.writeFile("/ro/blocked.txt", "should fail"),
		).rejects.toThrow("EROFS");
		const result = await vm.execArgv("node", [
			"-e",
			[
				'try { require("node:fs").writeFileSync("/ro/blocked.txt", "no"); }',
				"catch (error) { console.log(error.code); }",
			].join("\n"),
		]);
		expect(result.exitCode, result.stderr).toBe(0);
		expect(result.stdout.trim()).toBe("EROFS");
	});
});
