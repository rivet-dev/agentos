// Nightly: requires a non-core registry command.
import { existsSync } from "node:fs";
import { mkdir, mkdtemp, rm, writeFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";
import {
	NodeFileSystem,
	createKernel,
	createWasmVmRuntime,
	describeIf,
} from "@rivet-dev/agentos-test-harness";
import type { Kernel } from "@rivet-dev/agentos-test-harness";
import { afterEach, expect, it } from "vitest";
import { AgentOs, createSnapshotExport } from "../../../packages/core/src/index.js";

const VIM_COMMAND_DIR = fileURLToPath(new URL("../bin", import.meta.url));
const VIM_RUNTIME_DIR = fileURLToPath(
	new URL("../dist/package/share/vim/vim92", import.meta.url),
);
const hasVimPackage = existsSync(join(VIM_COMMAND_DIR, "vim")) &&
	existsSync(join(VIM_RUNTIME_DIR, "defaults.vim"));

let tempRoot: string | undefined;

async function writeFixture(path: string, contents: string): Promise<void> {
	if (!tempRoot) throw new Error("fixture root not initialized");
	const hostPath = join(tempRoot, path.replace(/^\/+/, ""));
	await mkdir(dirname(hostPath), { recursive: true });
	await writeFile(hostPath, contents);
}

async function createTestVFS(): Promise<NodeFileSystem> {
	tempRoot = await mkdtemp(join(tmpdir(), "agentos-vim-"));
	await writeFixture("/project/input.txt", "alpha\nbeta\ngamma\n");
	await writeFixture(
		"/project/edit.vim",
		"set nomore\nedit /project/input.txt\n%s/beta/delta/\nwrite\nquitall!\n",
	);
	await mkdir(join(tempRoot, "usr/local/share/vim"), { recursive: true });
	return new NodeFileSystem({ root: tempRoot });
}

describeIf(hasVimPackage, "vim command", { timeout: 60_000 }, () => {
	let kernel: Kernel | undefined;

	afterEach(async () => {
		await kernel?.dispose();
		kernel = undefined;
		if (tempRoot) {
			await rm(tempRoot, { recursive: true, force: true });
			tempRoot = undefined;
		}
	});

	async function mountFixture(): Promise<void> {
		const vfs = await createTestVFS();
		kernel = createKernel({ filesystem: vfs });
		kernel.mountFs(
			"/usr/local/share/vim/vim92",
			new NodeFileSystem({ root: VIM_RUNTIME_DIR }),
			{ readOnly: true },
		);
		await kernel.mount(createWasmVmRuntime({ commandDirs: [VIM_COMMAND_DIR] }));
	}

	async function runVim(args: string[]) {
		if (!kernel) throw new Error("kernel not mounted");
		let stdout = "";
		let stderr = "";
		const proc = kernel.spawn("vim", args, {
			streamStdin: true,
			env: {
				TERM: "xterm",
				VIM: "/usr/local/share/vim",
				VIMRUNTIME: "/usr/local/share/vim/vim92",
			},
			onStdout: (chunk) => {
				stdout += Buffer.from(chunk).toString("utf8");
			},
			onStderr: (chunk) => {
				stderr += Buffer.from(chunk).toString("utf8");
			},
		});
		proc.closeStdin();
		const exitCode = await proc.wait();
		await new Promise<void>((resolve) => setTimeout(resolve, 0));
		return { stdout, stderr, exitCode };
	}

	it("starts the packaged binary and reports Vim features", async () => {
		await mountFixture();

		const result = await runVim(["--version"]);

		expect(result.exitCode, result.stderr || result.stdout).toBe(0);
		expect(result.stdout).toContain("VIM - Vi IMproved");
		expect(result.stdout).toContain("-libcall");
	});

	it("edits and writes a file in Ex mode", async () => {
		await mountFixture();

		const result = await runVim([
			"-Nu",
			"NONE",
			"-n",
			"-es",
			"-S",
			"/project/edit.vim",
		]);

		expect(result.exitCode, result.stderr || result.stdout).toBe(0);
		if (!kernel) throw new Error("kernel not mounted");
		const edited = Buffer.from(await kernel.readFile("/project/input.txt")).toString(
			"utf8",
		);
		expect(edited).toBe("alpha\ndelta\ngamma\n");
	});
});

const VIM_AOSPKG = fileURLToPath(new URL("../dist/package.aospkg", import.meta.url));

describeIf(existsSync(VIM_AOSPKG), "vim packed runtime", { timeout: 60_000 }, () => {
	it("loads packaged autoload and import scripts through the public SDK", async () => {
		const sidecar = await AgentOs.createSidecar();
		let vm: AgentOs | undefined;
		try {
			vm = await AgentOs.create({
				defaultSoftware: false,
				sidecar: { kind: "explicit", handle: sidecar },
				software: [{ packagePath: VIM_AOSPKG }],
				permissions: { fs: "allow", process: "allow", childProcess: "allow" },
				rootFilesystem: {
					lowers: [createSnapshotExport([
						{ path: "/project", type: "directory", mode: "0755", uid: 1000, gid: 1000 },
						{
							path: "/project/runtime-contract.vim", type: "file", mode: "0644", uid: 1000, gid: 1000,
							content: [
								"vim9script",
								"import autoload 'dist/json.vim'",
								"import 'dist/vimhighlight.vim'",
								"var formatted = json.Format({'answer': 42})",
								"writefile(split(formatted, \"\\n\"), '/project/formatted.json')",
								"writefile(['runtime-ready'], '/project/runtime-ready.txt')",
							].join("\n") + "\n",
						},
					])],
				},
			});
			const result = await vm.process.execFile(
				"vim", ["-Nu", "NONE", "-n", "-es", "-V1", "-S", "/project/runtime-contract.vim", "-c", "qa!"],
				{ env: { TERM: "xterm" }, timeoutMs: 45_000,
					output: { capture: "all", maxBufferedBytes: 64 * 1024 } },
			);
			expect(result.exitCode, result.stderr || result.stdout).toBe(0);
			expect(JSON.parse(Buffer.from(await vm.filesystem.readFile("/project/formatted.json")).toString("utf8")))
				.toEqual({ answer: 42 });
			expect(Buffer.from(await vm.filesystem.readFile("/project/runtime-ready.txt")).toString("utf8"))
				.toBe("runtime-ready\n");
		} finally {
			try { await vm?.dispose(); }
			finally { await sidecar.dispose(); }
		}
	});
});
