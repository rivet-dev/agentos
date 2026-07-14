import { describe, expect, it } from "vitest";
import { POLYFILL_CODE_MAP } from "../../src/runtime.js";
import { createWasiCommandBootstrapScript } from "../../src/wasi-command-bootstrap.js";

describe("wasi command bootstrap", () => {
	it("generates a production browser guest launcher for the WASI command host", () => {
		const source = createWasiCommandBootstrapScript({
			commandSource: "/commands/sh",
			command: "sh",
			commandFiles: {
				"/opt/agentos/pkgs/demo/0.0.1/bin/echo": "/commands/echo",
			},
			args: ["-i"],
			commands: {
				echo: "/commands/echo",
				ls: "/commands/ls",
			},
			externalCommands: ["claude"],
			env: {
				PATH: "/bin:/usr/bin",
				TERM: "xterm-256color",
			},
			cwd: "/",
			bootMessage: "BOOT",
			errorMessagePrefix: "ERR:",
		});

		expect(source).toContain("globalThis.__agentOSWasiModule.WASI");
		expect(source).not.toContain('require("node:wasi")');
		expect(source).toContain(
			"globalThis.__agentOSWasiCommandHost.createWasiCommandHost",
		);
		expect(source).toContain('const commandSource = "/commands/sh";');
		expect(source).toContain("/commands/sh");
		expect(source).toContain("/commands/echo");
		expect(source).toContain("/commands/ls");
		expect(source).toContain("/opt/agentos/pkgs/demo/0.0.1/bin/echo");
		expect(source).toContain("fs.writeFileSync(guestPath, commandBytes)");
		expect(source).toContain('externalCommands: ["claude"]');
		expect(source).toContain('args: ["sh","-i"]');
		expect(source).toContain("commandHost.installBlockingStdin(process)");
		expect(source).toContain("commandHost.setParentWasi(wasi)");
		expect(source).toContain("commandHost.setMemory(instance.exports.memory)");
		expect(source).toContain("ERR:");
	});

	it("exposes unavailable raw-socket imports as explicit ENOSYS calls", () => {
		const commandHost = POLYFILL_CODE_MAP["secure-exec:wasi-command-host"];
		expect(commandHost).toContain("net_socket() { return errnoNosys; }");
		expect(commandHost).toContain("net_connect() { return errnoNosys; }");
		expect(commandHost).toContain("net_send() { return errnoNosys; }");
		expect(commandHost).toContain("net_recv() { return errnoNosys; }");
	});
});
