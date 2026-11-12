// Nightly: requires the non-core Git command package.

import { spawnSync } from "node:child_process";
import {
	chmodSync,
	existsSync,
	mkdtempSync,
	readFileSync,
	rmSync,
	statSync,
	writeFileSync,
} from "node:fs";
import { tmpdir } from "node:os";
import { dirname, join, resolve } from "node:path";
import common from "@agentos-software/common";
import git from "@agentos-software/git";
import { afterEach, beforeEach, describe, expect, test } from "vitest";
import { AgentOs } from "../src/index.js";
import { moduleAccessMounts } from "./helpers/node-modules-mount.js";

type ExecResult = {
	stdout: string;
	stderr: string;
	exitCode: number;
};

const GIT_QUICKSTART_PERMISSIONS = {
	fs: "allow",
	childProcess: "allow",
	env: "allow",
} as const;

const COMMON_SOFTWARE = common;
const GIT_PACKAGE = requireBuiltPackage(git, "git");
const MODULE_ACCESS_CWD = resolve(import.meta.dirname, "..");
const GIT_CONFIG = [
	"-c safe.directory=*",
	"-c init.defaultBranch=main",
	"-c user.name=agentos",
	"-c user.email=agentos@example.invalid",
].join(" ");

function requireBuiltPackage<T extends { packagePath: string }>(
	pkg: T,
	name: string,
): T {
	const packageDir = pkg.packagePath.endsWith(".aospkg")
		? join(dirname(pkg.packagePath), "package")
		: pkg.packagePath;
	const built =
		existsSync(pkg.packagePath) &&
		statSync(pkg.packagePath).size > 16 &&
		existsSync(join(packageDir, "agentos-package.json")) &&
		existsSync(join(packageDir, "bin", name));
	if (!built) {
		throw new Error(
			`software package ${name} is NOT BUILT (no valid ${pkg.packagePath}).`,
		);
	}
	return pkg;
}

function parseCurrentBranch(output: string): string {
	const branch = output
		.split("\n")
		.map((line) => line.trim())
		.find((line) => line.startsWith("* "))
		?.slice(2)
		.trim();

	if (!branch) {
		throw new Error(`could not determine current branch from:\n${output}`);
	}

	return branch;
}

function parseHeadRef(content: string): string {
	const headRef = content.trim().match(/^ref: refs\/heads\/(.+)$/)?.[1];
	if (!headRef) {
		throw new Error(`could not determine HEAD ref from:\n${content}`);
	}
	return headRef;
}

function gitCommand(args: string): string {
	return `git ${GIT_CONFIG} ${args}`;
}

describe("git quickstart integration", () => {
	let vm: AgentOs;

	beforeEach(async () => {
		vm = await AgentOs.create({
			mounts: moduleAccessMounts(MODULE_ACCESS_CWD),
			permissions: GIT_QUICKSTART_PERMISSIONS,
			software: [COMMON_SOFTWARE, GIT_PACKAGE],
		});
	});

	afterEach(async () => {
		await vm?.dispose();
	});

	async function run(command: string): Promise<ExecResult> {
		const result = await vm.exec(command);
		if (result.exitCode !== 0) {
			throw new Error(
				`command failed: ${command}\n${result.stderr || result.stdout}`,
			);
		}
		return result;
	}

	test("covers the quickstart local origin -> clone -> checkout flow", async () => {
		await run(gitCommand("init /tmp/origin"));
		await vm.writeFile("/tmp/origin/README.md", "# demo repo\n");
		await run(gitCommand("-C /tmp/origin add README.md"));
		await run(gitCommand("-C /tmp/origin commit -m 'initial commit'"));

		const defaultBranch = parseCurrentBranch(
			(await run(gitCommand("-C /tmp/origin branch"))).stdout,
		);

		await run(gitCommand("-C /tmp/origin checkout -b feature"));
		await vm.writeFile("/tmp/origin/feature.txt", "checked out from feature\n");
		await run(gitCommand("-C /tmp/origin add feature.txt"));
		await run(gitCommand("-C /tmp/origin commit -m 'add feature file'"));

		await run(gitCommand("clone /tmp/origin /tmp/clone"));

		const cloneHead = new TextDecoder().decode(
			await vm.readFile("/tmp/clone/.git/HEAD"),
		);
		expect(parseHeadRef(cloneHead)).toBe("feature");
		expect(defaultBranch).not.toBe("feature");

		const featureFile = await vm.readFile("/tmp/clone/feature.txt");
		expect(new TextDecoder().decode(featureFile)).toBe(
			"checked out from feature\n",
		);

		const readme = await vm.readFile("/tmp/clone/README.md");
		expect(new TextDecoder().decode(readme)).toBe("# demo repo\n");

		const currentBranch = (
			await run(gitCommand("-C /tmp/clone branch --show-current"))
		).stdout.trim();
		expect(currentBranch).toBe("feature");

		const log = await run(gitCommand("-C /tmp/clone log --oneline --all"));
		expect(log.stdout).toContain("add feature file");
		expect(log.stdout).toContain("initial commit");

		await vm.writeFile("/tmp/origin/pulled.txt", "new origin commit\n");
		await run(gitCommand("-C /tmp/origin add pulled.txt"));
		await run(gitCommand("-C /tmp/origin commit -m 'new origin commit'"));
		await run(gitCommand("-C /tmp/clone pull --ff-only"));
		expect(
			new TextDecoder().decode(await vm.readFile("/tmp/clone/pulled.txt")),
		).toBe("new origin commit\n");

		await vm.writeFile("/tmp/clone/README.md", "# changed demo repo\n");
		const diff = await run(gitCommand("-C /tmp/clone diff -- README.md"));
		expect(diff.stdout).toContain("-# demo repo");
		expect(diff.stdout).toContain("+# changed demo repo");
	}, 120_000);

	test("submodule add, init, update, and recursive clone match native Git", async () => {
		const nativeRoot = mkdtempSync(join(tmpdir(), "agentos-git-submodule-"));
		const nativeGit = (...args: string[]) => {
			const result = spawnSync(
				"git",
				[
					"-c",
					"init.defaultBranch=main",
					"-c",
					"user.name=agentos",
					"-c",
					"user.email=agentos@example.invalid",
					"-c",
					"protocol.file.allow=always",
					...args,
				],
				{ encoding: "utf8" },
			);
			if (result.status !== 0)
				throw new Error(`native git ${args.join(" ")}: ${result.stderr}`);
			return result.stdout;
		};
		try {
			const child = join(nativeRoot, "child");
			const parent = join(nativeRoot, "parent");
			const clone = join(nativeRoot, "clone");
			const recursiveClone = join(nativeRoot, "recursive-clone");
			nativeGit("init", child);
			writeFileSync(join(child, "child.txt"), "nested payload\n");
			nativeGit("-C", child, "add", "child.txt");
			nativeGit("-C", child, "commit", "-m", "child");
			nativeGit("init", parent);
			nativeGit("-C", parent, "submodule", "add", child, "deps/child");
			nativeGit("-C", parent, "commit", "-am", "parent");
			nativeGit("clone", parent, clone);
			const nativeBefore = nativeGit("-C", clone, "submodule", "status");
			nativeGit("-C", clone, "submodule", "init");
			nativeGit("-C", clone, "submodule", "update");
			const nativeAfter = nativeGit("-C", clone, "submodule", "status");
			nativeGit("clone", "--recurse-submodules", parent, recursiveClone);

			await run(gitCommand("init /tmp/child"));
			await vm.writeFile("/tmp/child/child.txt", "nested payload\n");
			await run(gitCommand("-C /tmp/child add child.txt"));
			await run(gitCommand("-C /tmp/child commit -m child"));
			await run(gitCommand("init /tmp/parent"));
			await run(
				gitCommand(
					"-c protocol.file.allow=always -C /tmp/parent submodule add /tmp/child deps/child",
				),
			);
			await run(gitCommand("-C /tmp/parent commit -am parent"));
			await run(gitCommand("clone /tmp/parent /tmp/clone"));
			const guestBefore = (
				await run(gitCommand("-C /tmp/clone submodule status"))
			).stdout;
			await run(gitCommand("-C /tmp/clone submodule init"));
			await run(
				gitCommand(
					"-c protocol.file.allow=always -C /tmp/clone submodule update",
				),
			);
			const guestAfter = (
				await run(gitCommand("-C /tmp/clone submodule status"))
			).stdout;
			await run(
				gitCommand(
					"-c protocol.file.allow=always clone --recurse-submodules /tmp/parent /tmp/recursive-clone",
				),
			);

			expect(guestBefore[0]).toBe(nativeBefore[0]);
			expect(guestAfter[0]).toBe(nativeAfter[0]);
			expect(guestAfter).toContain(" deps/child ");
			expect(
				new TextDecoder().decode(
					await vm.readFile("/tmp/clone/deps/child/child.txt"),
				),
			).toBe(readFileSync(join(clone, "deps/child/child.txt"), "utf8"));
			expect(
				new TextDecoder().decode(
					await vm.readFile("/tmp/recursive-clone/deps/child/child.txt"),
				),
			).toBe(
				readFileSync(join(recursiveClone, "deps/child/child.txt"), "utf8"),
			);
			const nativeMode = nativeGit(
				"-C",
				clone,
				"ls-files",
				"--stage",
				"deps/child",
			);
			const guestMode = (
				await run(gitCommand("-C /tmp/clone ls-files --stage deps/child"))
			).stdout;
			expect(guestMode.split(/\s+/)[0]).toBe(nativeMode.split(/\s+/)[0]);
		} finally {
			rmSync(nativeRoot, { recursive: true, force: true });
		}
	}, 120_000);

	test("pre-commit hook execution and rejection match native Git", async () => {
		const nativeRoot = mkdtempSync(join(tmpdir(), "agentos-git-hook-"));
		const nativeGit = (...args: string[]) =>
			spawnSync(
				"git",
				[
					"-c",
					"init.defaultBranch=main",
					"-c",
					"user.name=agentos",
					"-c",
					"user.email=agentos@example.invalid",
					...args,
				],
				{ encoding: "utf8", timeout: 10_000 },
			);
		try {
			const nativeRepo = join(nativeRoot, "repo");
			expect(nativeGit("init", nativeRepo).status).toBe(0);
			await run(gitCommand("init /tmp/hook-repo"));
			const hook = "#!/bin/sh\nprintf 'hook-ran\\n' > .hook-marker\n";
			const nativeHook = join(nativeRepo, ".git/hooks/pre-commit");
			writeFileSync(nativeHook, hook);
			chmodSync(nativeHook, 0o755);
			await vm.writeFile("/tmp/hook-repo/.git/hooks/pre-commit", hook);
			await run("chmod +x /tmp/hook-repo/.git/hooks/pre-commit");
			writeFileSync(join(nativeRepo, "first.txt"), "first\n");
			await vm.writeFile("/tmp/hook-repo/first.txt", "first\n");
			expect(nativeGit("-C", nativeRepo, "add", "first.txt").status).toBe(0);
			await run(gitCommand("-C /tmp/hook-repo add first.txt"));
			expect(nativeGit("-C", nativeRepo, "commit", "-m", "first").status).toBe(
				0,
			);
			await run(gitCommand("-C /tmp/hook-repo commit -m first"));
			expect(
				new TextDecoder().decode(
					await vm.readFile("/tmp/hook-repo/.hook-marker"),
				),
			).toBe(readFileSync(join(nativeRepo, ".hook-marker"), "utf8"));

			const rejectingHook = "#!/bin/sh\nexit 7\n";
			writeFileSync(nativeHook, rejectingHook);
			await vm.writeFile("/tmp/hook-repo/.git/hooks/pre-commit", rejectingHook);
			writeFileSync(join(nativeRepo, "second.txt"), "second\n");
			await vm.writeFile("/tmp/hook-repo/second.txt", "second\n");
			expect(nativeGit("-C", nativeRepo, "add", "second.txt").status).toBe(0);
			await run(gitCommand("-C /tmp/hook-repo add second.txt"));
			const nativeRejected = nativeGit(
				"-C",
				nativeRepo,
				"commit",
				"-m",
				"rejected",
			);
			const guestRejected = await vm.exec(
				gitCommand("-C /tmp/hook-repo commit -m rejected"),
			);
			expect(nativeRejected.status).toBe(1);
			expect(guestRejected.exitCode).toBe(nativeRejected.status);
		} finally {
			rmSync(nativeRoot, { recursive: true, force: true });
		}
	}, 45_000);

	test("synthetic credential helper output matches native Git", async () => {
		const nativeRoot = mkdtempSync(join(tmpdir(), "agentos-git-credential-"));
		try {
			const helper =
				"#!/bin/sh\nif [ \"$1\" = get ]; then printf 'username=fixture-user\\npassword=fixture-pass\\n'; fi\n";
			const nativeHelper = join(nativeRoot, "helper");
			writeFileSync(nativeHelper, helper);
			chmodSync(nativeHelper, 0o755);
			await vm.writeFile("/tmp/credential-helper", helper);
			await run("chmod +x /tmp/credential-helper");
			const input = "protocol=https\nhost=example.invalid\n\n";
			const native = spawnSync(
				"git",
				[
					"-c",
					"credential.helper=",
					"-c",
					`credential.helper=${nativeHelper}`,
					"credential",
					"fill",
				],
				{ input, encoding: "utf8", timeout: 10_000 },
			);
			const guest = await vm.exec(
				"git -c credential.helper= -c credential.helper=/tmp/credential-helper credential fill",
				{ stdin: input },
			);
			expect(native.status, native.stderr).toBe(0);
			expect(guest.exitCode, guest.stderr).toBe(native.status);
			expect(guest.stdout).toBe(native.stdout);
		} finally {
			rmSync(nativeRoot, { recursive: true, force: true });
		}
	}, 30_000);
});
