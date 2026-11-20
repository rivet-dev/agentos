import { afterEach, describe, expect, test } from "vitest";
import { AgentOs } from "../src/index.js";

// The root of a fresh VM, as `readdir("/")` lists it to the host and the guest.
const ROOT_ENTRIES = [
	"bin",
	"boot",
	"dev",
	"etc",
	"home",
	"lib",
	"media",
	"mnt",
	"opt",
	"proc",
	"root",
	"run",
	"sbin",
	"srv",
	"sys",
	"tmp",
	"usr",
	"var",
	"workspace",
];

// `path:mode:uid:gid` for directories whose mode or owner is not 0755 root.
const STANDARD_DIRECTORY_MODES = [
	"/tmp:1777:0:0",
	// Compatibility node_modules projection permits traversal, not listing.
	"/root:711:0:0",
	"/home/agentos:2755:1000:1000",
	"/workspace:755:1000:1000",
	"/sys:555:0:0",
	"/var/tmp:1777:0:0",
];

const GUEST_ROOT_PROBE = `
const fs = require('fs');
const modes = ${JSON.stringify(STANDARD_DIRECTORY_MODES.map((entry) => entry.split(":")[0]))}
	.map((path) => { const s = fs.statSync(path); return [path, (s.mode & 0o7777).toString(8), s.uid, s.gid].join(':'); });
process.stdout.write(JSON.stringify({
	root: fs.readdirSync('/').sort(),
	modes,
	devIsDir: fs.statSync('/dev').isDirectory(),
	procIsDir: fs.statSync('/proc').isDirectory(),
	devNull: fs.readFileSync('/dev/null').length,
}));
`;

async function runGuest(vm: AgentOs, code: string): Promise<string> {
	let stdout = "";
	let stderr = "";
	const { pid } = await vm.process.spawn("node", ["-e", code], {
		onStdout: (chunk) => {
			stdout += Buffer.from(chunk).toString();
		},
		onStderr: (chunk) => {
			stderr += Buffer.from(chunk).toString();
		},
	});
	const exit = await vm.process.wait(pid);
	expect({ exitCode: exit.exitCode, stderr }).toEqual({
		exitCode: 0,
		stderr: "",
	});
	return stdout;
}

async function hostModes(vm: AgentOs): Promise<string[]> {
	return Promise.all(
		STANDARD_DIRECTORY_MODES.map(async (entry) => {
			const path = entry.split(":")[0];
			const stat = await vm.stat(path);
			return [path, (stat.mode & 0o7777).toString(8), stat.uid, stat.gid].join(
				":",
			);
		}),
	);
}

describe("root filesystem layout", () => {
	const vms: AgentOs[] = [];
	const create = async (options?: Parameters<typeof AgentOs.create>[0]) => {
		const vm = await AgentOs.create({
			permissions: { network: "deny" },
			...options,
		});
		vms.push(vm);
		return vm;
	};

	afterEach(async () => {
		await Promise.all(vms.splice(0).map((vm) => vm.dispose()));
	});

	test("a fresh VM lists the standard root directories, kernel mountpoints included, to host and guest", async () => {
		const vm = await create();

		expect([...(await vm.readdir("/"))].sort()).toEqual(ROOT_ENTRIES);
		expect(await hostModes(vm)).toEqual(STANDARD_DIRECTORY_MODES);
		expect(JSON.parse(await runGuest(vm, GUEST_ROOT_PROBE))).toEqual({
			root: ROOT_ENTRIES,
			modes: STANDARD_DIRECTORY_MODES,
			devIsDir: true,
			procIsDir: true,
			devNull: 0,
		});
	}, 60_000);

	test("a VM without the default base layer still gets every standard directory", async () => {
		const vm = await create({
			rootFilesystem: { disableDefaultBaseLayer: true },
		});

		const root = [...(await vm.readdir("/"))].sort();
		expect(root).toEqual(expect.arrayContaining(ROOT_ENTRIES));
		expect(await hostModes(vm)).toEqual(STANDARD_DIRECTORY_MODES);
		expect((await vm.stat("/var/lock")).mode & 0o7777).toBe(0o777);
	}, 60_000);

	test("deleting, recreating, and renaming base-layer entries behaves like one filesystem", async () => {
		const vm = await create();
		const lowerFile = "/etc/passwd";

		// Listing a base-layer directory updates its access time. That
		// metadata write must not hide the directory's base entries.
		const homeEntries = await vm.readdir("/home");
		expect(homeEntries).toContain("agentos");
		expect(await vm.readdir("/home")).toEqual(homeEntries);
		const varEntries = [...(await vm.readdir("/var"))].sort();
		expect(varEntries).toEqual(
			expect.arrayContaining(["cache", "lib", "log", "tmp"]),
		);
		expect([...(await vm.readdir("/var"))].sort()).toEqual(varEntries);

		await vm.remove(lowerFile);
		expect(await vm.exists(lowerFile)).toBe(false);
		expect(await vm.readdir("/etc")).not.toContain(
			lowerFile.slice("/etc/".length),
		);
		await expect(vm.readFile(lowerFile)).rejects.toThrow("ENOENT");

		await vm.writeFile(lowerFile, "recreated");
		expect(new TextDecoder().decode(await vm.readFile(lowerFile))).toBe(
			"recreated",
		);
		await vm.remove(lowerFile);
		expect(await vm.exists(lowerFile)).toBe(false);

		await vm.remove("/usr/share", { recursive: true });
		expect(await vm.exists("/usr/share/man")).toBe(false);
		await vm.mkdir("/usr/share");
		expect(await vm.readdir("/usr/share")).toEqual([]);

		const usrEntries = [...(await vm.readdir("/usr"))].sort();
		expect(usrEntries).toEqual(expect.arrayContaining(["bin", "lib", "share"]));
		await vm.move("/usr", "/usr2");
		expect(await vm.exists("/usr")).toBe(false);
		expect([...(await vm.readdir("/usr2"))].sort()).toEqual(usrEntries);

		// The guest sees the same merged tree, and no overlay bookkeeping.
		const guest = JSON.parse(
			await runGuest(
				vm,
				`const fs = require('fs'); process.stdout.write(JSON.stringify({
					root: fs.readdirSync('/').sort(),
					lowerFile: fs.existsSync(${JSON.stringify(lowerFile)}),
					usrShare: fs.readdirSync('/usr2/share'),
					usr2: fs.readdirSync('/usr2').sort(),
				}));`,
			),
		);
		expect(guest).toEqual({
			root: ROOT_ENTRIES.map((name) => (name === "usr" ? "usr2" : name)).sort(),
			lowerFile: false,
			usrShare: [],
			usr2: usrEntries,
		});
	}, 60_000);
});
