// Regression for https://github.com/rivet-dev/agentos/issues/1872
//
// Guest WASM processes (a shell redirect, `cat`, `ls`, `tar`) must be able to
// read and write a writable native `host_dir` mount, exactly like the host-side
// `vm.readFile` / `vm.writeFile` API can. agentos-apps' `deployApp` pack step
// depends on this: it runs guest `tar -cf` into a writable host_dir mount.
//
// Listing the mount root from the guest updates its atime, which the host_dir
// plugin rejected with EINVAL for "/", so `ls /mnt` and `tar -C /mnt` failed.
import * as fs from "node:fs";
import * as os from "node:os";
import * as path from "node:path";
import { afterEach, beforeEach, describe, expect, test } from "vitest";
import { AgentOs, createHostDirBackend } from "../src/index.js";

describe("issue-1872: guest processes can use writable host_dir mounts", () => {
	let vm: AgentOs | undefined;
	let hostDir: string;

	beforeEach(() => {
		hostDir = fs.mkdtempSync(path.join(os.tmpdir(), "issue-1872-host-dir-"));
		fs.writeFileSync(path.join(hostDir, "existing.txt"), "seed from host\n");
	});

	afterEach(async () => {
		await vm?.dispose();
		vm = undefined;
		fs.rmSync(hostDir, { recursive: true, force: true });
	});

	test("shell redirects, cat, and tar work into and out of the mount", async () => {
		const guest = await AgentOs.create({
			mounts: [
				{
					path: "/mnt",
					plugin: createHostDirBackend({ hostPath: hostDir, readOnly: false }),
				},
			],
		});
		vm = guest;

		const run = async (command: string) => {
			const result = await guest.exec(command, {
				cwd: "/workspace",
				timeoutMs: 60_000,
			});
			expect(
				result.exitCode,
				`${command}\nstdout=${result.stdout}\nstderr=${result.stderr}`,
			).toBe(0);
			return result;
		};

		// Read an existing host file from the guest.
		expect((await run("cat /mnt/existing.txt")).stdout).toBe(
			"seed from host\n",
		);

		// Create a new file and overwrite an existing one through the mount.
		await run("sh -c 'echo from-guest > /mnt/new.txt'");
		await run("sh -c 'echo overwrite > /mnt/existing.txt'");
		expect(fs.readFileSync(path.join(hostDir, "new.txt"), "utf8")).toBe(
			"from-guest\n",
		);
		expect(fs.readFileSync(path.join(hostDir, "existing.txt"), "utf8")).toBe(
			"overwrite\n",
		);

		// List the mount root, then tar reading FROM the mount into the guest
		// filesystem.
		expect((await run("ls /mnt")).stdout).toContain("new.txt");
		await run("tar -cf /workspace/probe.tar -C /mnt .");
		expect((await run("tar -tf /workspace/probe.tar")).stdout).toContain(
			"new.txt",
		);

		// tar writing INTO the mount (the agentos-apps pack step).
		await run(
			"sh -c 'mkdir -p /workspace/app && echo app > /workspace/app/index.js'",
		);
		await run("tar -cf /mnt/out.tar -C /workspace/app .");
		expect(fs.statSync(path.join(hostDir, "out.tar")).size).toBeGreaterThan(0);
	}, 120_000);
});
