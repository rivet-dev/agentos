import { afterEach, beforeEach, describe, expect, test } from "vitest";
import { AgentOs } from "../src/index.js";

describe("filesystem symlink deletion", () => {
	let vm: AgentOs;

	beforeEach(async () => {
		vm = await AgentOs.create({ defaultSoftware: false });
	});

	afterEach(async () => {
		await vm.dispose();
	});

	test("delete symlink to directory removes the link, not the target", async () => {
		const vfs = (
			vm as unknown as {
				_vfs(): {
					symlink(target: string, linkPath: string): Promise<void>;
					lstat(path: string): Promise<{ isSymbolicLink: boolean }>;
				};
			}
		)._vfs();

		await vm.mkdir("/tmp/real-dir");
		await vm.writeFile("/tmp/real-dir/child.txt", "keep me");
		await vfs.symlink("/tmp/real-dir", "/tmp/dir-link");
		expect((await vfs.lstat("/tmp/dir-link")).isSymbolicLink).toBe(true);

		await vm.remove("/tmp/dir-link", { recursive: true });

		await expect(vfs.lstat("/tmp/dir-link")).rejects.toThrow("ENOENT");
		expect(new TextDecoder().decode(await vm.readFile("/tmp/real-dir/child.txt"))).toBe(
			"keep me",
		);
	});
});
