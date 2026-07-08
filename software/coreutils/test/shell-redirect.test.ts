import { describe, it, expect, afterEach } from "vitest";
import {
	createInMemoryFileSystem,
	createKernel,
	createWasmVmRuntime,
	COMMANDS_DIR,
	describeIf,
	hasWasmBinaries,
	type Kernel,
} from '@rivet-dev/agentos-test-harness';

describeIf(hasWasmBinaries, "wasmvm shell redirects", () => {
	let kernel: Kernel | undefined;

	afterEach(async () => {
		await kernel?.dispose();
		kernel = undefined;
	});

	it("creates a redirected file relative to the changed cwd", async () => {
		const vfs = createInMemoryFileSystem();
		await (vfs as any).chmod("/", 0o1777);
		await vfs.mkdir("/tmp", { recursive: true });
		await (vfs as any).chmod("/tmp", 0o1777);
		kernel = createKernel({ filesystem: vfs, syncFilesystemOnDispose: false });
		await kernel.mount(createWasmVmRuntime({ commandDirs: [COMMANDS_DIR] }));

		const result = await kernel.exec(
			'sh -c "mkdir -p /tmp/r && cd /tmp/r && echo hi > a.txt && cat a.txt"',
		);

		expect(result.exitCode).toBe(0);
		expect(result.stdout).toBe("hi\n");
		expect(await vfs.exists("/tmp/r/a.txt")).toBe(true);
	}, 15_000);
});
