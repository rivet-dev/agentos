import { BrowserbaseShellSession, type BrowserVmState } from "./session.js";

const output =
	process.env.AGENTOS_E2E_QUIET === "1" ? undefined : process.stdout;
let session: BrowserbaseShellSession | undefined;

function assert(condition: unknown, message: string): asserts condition {
	if (!condition) throw new Error(message);
}

try {
	session = await BrowserbaseShellSession.open({
		onStatus: (message) => output?.write(`[host] ${message}\n`),
		onEvent: (event) => {
			if (event.kind === "pty") output?.write(event.bytes);
			if (event.kind === "error") process.stderr.write(event.bytes);
		},
	});
	await session.waitFor("sh-0.4$", 60_000);
	let checkpoint = session.checkpoint();
	await session.write("echo BROWSERBASE_SHELL_E2E_OK | tr A-Z a-z\r");
	await session.waitFor("browserbase_shell_e2e_ok", 30_000, checkpoint);

	// Ctrl-C is sent as a byte through CDP to the AgentOS PTY, rather than
	// being handled as a host signal.
	checkpoint = session.checkpoint();
	await session.write("partial-browserbase-command\u0003");
	await session.waitFor("partial-browserbase-command^C", 30_000, checkpoint);

	// Write a no-shebang script with a trailing newline, mark it executable, and
	// run it with the real Bash WASM command on the same browser VM filesystem.
	checkpoint = session.checkpoint();
	await session.write(
		"echo 'echo BROWSERBASE_SCRIPT_E2E_OK' > /tmp/browserbase-script.sh && chmod +x /tmp/browserbase-script.sh && bash /tmp/browserbase-script.sh | tr A-Z a-z\r",
	);
	await session.waitFor("browserbase_script_e2e_ok", 30_000, checkpoint);
	await session.waitFor("sh-0.4$", 30_000, checkpoint);

	// Exercise Linux-style executable dispatch. The browser command host reads
	// the script from the guest VFS, honors its shebang, and starts the real
	// Brush WASM interpreter selected by /bin/sh.
	checkpoint = session.checkpoint();
	await session.write(
		"printf '#!/bin/sh\\necho BROWSERBASE_SHEBANG_E2E_OK\\n' > /tmp/browserbase-direct.sh && chmod +x /tmp/browserbase-direct.sh && /tmp/browserbase-direct.sh\r",
	);
	await session.waitFor("BROWSERBASE_SHEBANG_E2E_OK", 30_000, checkpoint);
	await session.waitFor("sh-0.4$", 30_000, checkpoint);

	// Drive real Vim through the PTY, write a shell script, save, return to the
	// same Brush process, and execute the saved file directly.
	checkpoint = session.checkpoint();
	await session.write(
		"vim -N -u NONE -i NONE -n --cmd 'set t_u7= t_RV= t_RF= t_RB=' /tmp/browserbase-vim-script.sh\r",
	);
	await session.waitFor("All", 60_000, checkpoint);
	checkpoint = session.checkpoint();
	await session.write(
		"i#!/bin/sh\recho BROWSERBASE_VIM_SCRIPT_E2E_OK\u001b:wq\r",
	);
	await session.waitFor("written", 30_000, checkpoint);
	await session.waitFor("sh-0.4$", 30_000, checkpoint);
	checkpoint = session.checkpoint();
	await session.write(
		"chmod +x /tmp/browserbase-vim-script.sh && /tmp/browserbase-vim-script.sh\r",
	);
	await session.waitFor("BROWSERBASE_VIM_SCRIPT_E2E_OK", 30_000, checkpoint);
	await session.waitFor("sh-0.4$", 30_000, checkpoint);

	// Prove the real AgentOS Git WASM command can create and commit a local
	// repository. rev-parse is used because the intentionally small Git command
	// currently does not implement status/log/show porcelain.
	checkpoint = session.checkpoint();
	await session.write(
		"mkdir /tmp/browserbase-git && git init /tmp/browserbase-git && echo BROWSERBASE_GIT_E2E_OK > /tmp/browserbase-git/proof.txt && git -C /tmp/browserbase-git add proof.txt && git -C /tmp/browserbase-git -c user.name=AgentOS -c user.email=agentos@example.com commit -m proof && git -C /tmp/browserbase-git rev-parse HEAD && cat /tmp/browserbase-git/proof.txt\r",
	);
	await session.waitFor(/[0-9a-f]{40}/, 60_000, checkpoint);
	await session.waitFor("BROWSERBASE_GIT_E2E_OK", 30_000, checkpoint);
	await session.waitFor("sh-0.4$", 30_000, checkpoint);

	const transcript = session.plainTranscript();
	assert(
		!transcript.includes("WARN could not retrieve pid for child process"),
		"Brush emitted the missing child PID warning",
	);
	assert(!transcript.includes("fatal:"), "Git emitted a fatal error");

	const state: BrowserVmState = await session.state();
	assert(state.crossOriginIsolated, "browser VM is not cross-origin isolated");
	assert(state.mode === "shell", `expected shell mode, received ${state.mode}`);
	assert(state.shell?.masterFd != null, "shell PTY master fd is missing");
	assert(state.shell.slaveFd != null, "shell PTY slave fd is missing");
	assert(state.shell.running, "shell PTY is not interactive/running");
	process.stdout.write(
		`\nBROWSERBASE_SHELL_E2E_RESULT=${JSON.stringify({
			transport: "CDP",
			runtime: "AgentOS browser VM",
			sessionId: session.sessionId,
			sessionUrl: session.sessionUrl,
			crossOriginIsolated: state.crossOriginIsolated,
			shellPty: {
				masterFd: state.shell.masterFd,
				slaveFd: state.shell.slaveFd,
				interactive: state.shell.running,
			},
			commands: {
				sh: "Brush",
				bash: "AgentOS command",
				chmod: "AgentOS command",
				vim: "Vim WASM over PTY",
				git: "AgentOS Git WASM",
			},
			commandResolution: "executable bytes from guest VFS",
		})}\n`,
	);
} finally {
	await session?.close();
}
