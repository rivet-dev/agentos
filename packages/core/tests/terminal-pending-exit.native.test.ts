import { expect, it } from "vitest";
import { AgentOs } from "../src/agent-os.js";

it.each([
	"caught",
	"ignored",
] as const)("terminal close preserves actual %s-signal completion", async (kind) => {
	const vm = await AgentOs.create({
		permissions: { fs: "allow", childProcess: "allow" },
	});
	const waitForFile = async (path: string) => {
		const deadline = Date.now() + 5_000;
		while (!(await vm.filesystem.exists(path))) {
			if (Date.now() >= deadline)
				throw Error(`terminal marker deadline exceeded: ${path}`);
			await new Promise((resolve) => setTimeout(resolve, 20));
		}
	};
	try {
		const script =
			kind === "caught"
				? "const fs=require('fs');process.on('SIGTERM',()=>process.exit(0));fs.writeFileSync('/tmp/terminal-ready','1');setInterval(()=>{},1000)"
				: "const fs=require('fs');process.on('SIGTERM',()=>fs.writeFileSync('/tmp/terminal-ignored','1'));fs.writeFileSync('/tmp/terminal-ready','1');setInterval(()=>{if(fs.existsSync('/tmp/terminal-release'))process.exit(0)},10)";
		const shell = vm.terminal.open({ command: "node", args: ["-e", script] });
		await waitForFile("/tmp/terminal-ready");
		vm.terminal.close(shell.shellId);
		expect(() => vm.terminal.write(shell.shellId, "closed")).toThrow(
			"Shell not found",
		);
		const waiting = vm.terminal.waitExit(shell.shellId);
		if (kind === "ignored") {
			await waitForFile("/tmp/terminal-ignored");
			const early = await Promise.race([
				waiting.then(() => "exited"),
				new Promise((resolve) => setTimeout(() => resolve("pending"), 20)),
			]);
			expect(early).toBe("pending");
			await vm.filesystem.writeFile("/tmp/terminal-release", "1");
		}
		expect(await waiting).toEqual({ shellId: shell.shellId, exitCode: 0 });
		expect(await vm.terminal.waitExit(shell.shellId)).toEqual({
			shellId: shell.shellId,
			exitCode: 0,
		});
		expect(await vm.terminal.wait(shell.shellId)).toBe(0);
	} finally {
		await vm.dispose();
	}
}, 30_000);
