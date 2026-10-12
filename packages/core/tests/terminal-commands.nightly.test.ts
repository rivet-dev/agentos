import { describe, expect, test } from "vitest";
import { AgentOs } from "../src/index.js";

describe("terminal commands", () => {
	test("GNU tty and stty follow the kernel PTY and report non-terminal stdin", async () => {
		const vm = await AgentOs.create();
		let shellId: string | undefined;
		let unsubscribe = () => {};
		try {
			const nonTerminalTty = await vm.process.execFile("tty", [], {
				timeoutMs: 5_000,
				output: { capture: "all" },
			});
			expect(nonTerminalTty.exitCode).toBe(1);
			expect(nonTerminalTty.stdout).toBe("not a tty\n");
			const nonTerminalStty = await vm.process.execFile("stty", ["size"], {
				timeoutMs: 5_000,
				output: { capture: "all" },
			});
			expect(nonTerminalStty.exitCode).toBe(1);
			expect(nonTerminalStty.stderr).toContain("Not a tty");

			({ shellId } = vm.terminal.open({ cols: 80, rows: 24 }));
			let output = "";
			const decoder = new TextDecoder();
			unsubscribe = vm.onShellData(shellId, ({ data }) => {
				output += decoder.decode(data, { stream: true });
			});
			await expect.poll(() => output, { timeout: 5_000 }).toContain("sh-0.4$ ");
			await vm.terminal.write(
				shellId,
				"printf 'BEGIN%s\\n' 713; tty; stty size; stty rows 31 cols 92; stty -F /dev/tty size; stty -F \"$(tty)\" size; printf 'END%s\\n' 713\n",
			);
			await expect.poll(() => output, { timeout: 5_000 }).toContain("END713");
			expect(output).toMatch(
				/BEGIN713\r?\n\/dev\/pts\/\d+\r?\n24 80\r?\n31 92\r?\n31 92\r?\nEND713/,
			);
			await vm.terminal.write(shellId, "exit\n");
			expect(await vm.terminal.wait(shellId)).toBe(0);
		} finally {
			unsubscribe();
			if (shellId) vm.terminal.close(shellId);
			await vm.dispose();
		}
	}, 20_000);
});
