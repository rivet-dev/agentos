import "@xterm/xterm/css/xterm.css";

const bundleUrl = "/real-terminal.bundle.js";
window.__agentOSTerminalConfig = { software: "full" };
void import(/* @vite-ignore */ bundleUrl)
	.then(() => window.__realTerminal?.start())
	.catch((error: unknown) => {
		const status = document.getElementById("status");
		if (status) status.textContent = "error";
		console.error("browser-local shell failed to boot", error);
	});
