import "@xterm/xterm/css/xterm.css";

const bundleUrl = "/pi-tui.bundle.js";
void import(/* @vite-ignore */ bundleUrl)
	.then(() => window.__piTui?.start())
	.catch((error: unknown) => {
		const status = document.getElementById("status");
		if (status) status.textContent = "error";
		console.error("browser-local Pi failed to boot", error);
	});
