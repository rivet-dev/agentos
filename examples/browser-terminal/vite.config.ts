import {
	cpSync,
	createReadStream,
	existsSync,
	mkdirSync,
	statSync,
} from "node:fs";
import { extname, join, resolve } from "node:path";
import react from "@vitejs/plugin-react";
import { defineConfig, type Plugin } from "vite";

const browserPackage = resolve(import.meta.dirname, "../../packages/browser");
const browserAssets = join(browserPackage, "tests/browser-wasm");
const wasmAssets = join(browserPackage, ".cache/agentos-sidecar-wasm-web");
const isolationHeaders = {
	"Cross-Origin-Opener-Policy": "same-origin",
	"Cross-Origin-Embedder-Policy": "require-corp",
};

const MIME: Record<string, string> = {
	".js": "text/javascript; charset=utf-8",
	".cjs": "text/javascript; charset=utf-8",
	".wasm": "application/wasm",
	".json": "application/json; charset=utf-8",
};

function resolveLocalAsset(pathname: string): string | undefined {
	if (pathname.startsWith("/wasm/")) {
		return join(wasmAssets, pathname.slice("/wasm/".length));
	}
	if (pathname.startsWith("/commands/")) {
		return join(browserAssets, pathname.slice(1));
	}
	if (
		[
			"/agentos-worker.js",
			"/real-terminal.bundle.js",
			"/pi-tui.bundle.js",
			"/pi-cli.bundle.cjs",
			"/pi-package.json",
			"/pi-theme-dark.json",
			"/pi-theme-light.json",
		].includes(pathname)
	) {
		return join(browserAssets, pathname.slice(1));
	}
	return undefined;
}

function browserRuntimeAssets(): Plugin {
	return {
		name: "agentos-browser-runtime-assets",
		configureServer(server) {
			server.middlewares.use((request, response, next) => {
				const pathname = decodeURIComponent((request.url ?? "/").split("?")[0]);
				const file = resolveLocalAsset(pathname);
				if (!file) return next();
				const allowed =
					file.startsWith(browserAssets) || file.startsWith(wasmAssets);
				if (!allowed || !existsSync(file) || !statSync(file).isFile()) {
					response.statusCode = 404;
					response.end(`missing browser runtime asset: ${pathname}`);
					return;
				}
				response.setHeader(
					"Content-Type",
					MIME[extname(file)] ?? "application/octet-stream",
				);
				response.setHeader("Cross-Origin-Opener-Policy", "same-origin");
				response.setHeader("Cross-Origin-Embedder-Policy", "require-corp");
				createReadStream(file).pipe(response);
			});
		},
		closeBundle() {
			const dist = resolve(import.meta.dirname, "dist");
			mkdirSync(join(dist, "wasm"), { recursive: true });
			cpSync(wasmAssets, join(dist, "wasm"), { recursive: true });
			cpSync(join(browserAssets, "commands"), join(dist, "commands"), {
				recursive: true,
			});
			for (const asset of [
				"agentos-worker.js",
				"real-terminal.bundle.js",
				"pi-tui.bundle.js",
				"pi-cli.bundle.cjs",
				"pi-package.json",
				"pi-theme-dark.json",
				"pi-theme-light.json",
			]) {
				cpSync(join(browserAssets, asset), join(dist, asset));
			}
		},
	};
}

export default defineConfig({
	plugins: [react(), browserRuntimeAssets()],
	server: {
		port: 5173,
		headers: isolationHeaders,
	},
	preview: {
		headers: isolationHeaders,
	},
	build: {
		rollupOptions: {
			input: {
				index: resolve(import.meta.dirname, "index.html"),
				actor: resolve(import.meta.dirname, "actor.html"),
				browser: resolve(import.meta.dirname, "browser.html"),
				localShell: resolve(import.meta.dirname, "local-shell.html"),
				localPi: resolve(import.meta.dirname, "local-pi.html"),
			},
		},
	},
});
