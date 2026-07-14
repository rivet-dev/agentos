import {
	cpSync,
	createReadStream,
	existsSync,
	mkdirSync,
	readFileSync,
	statSync,
} from "node:fs";
import { extname, join, resolve } from "node:path";
import { defineConfig, type Plugin } from "vite";

const repoRoot = resolve(import.meta.dirname, "../../..");
const browserAssets = join(repoRoot, "packages/browser/tests/browser-wasm");
const wasmAssets = join(
	repoRoot,
	"packages/browser/.cache/agentos-sidecar-wasm-web",
);
const claudePackage = join(repoRoot, "registry/agent/claude/dist");
const claudeWrapper = readFileSync(
	join(claudePackage, "claude-cli.mjs"),
	"utf8",
);
const claudeModuleName = claudeWrapper.match(
	/import\(["']\.\/([^"']+)["']\)/,
)?.[1];
if (!claudeModuleName) {
	throw new Error(
		"AgentOS Claude package wrapper does not import its CLI payload",
	);
}
const claudeModule = join(claudePackage, claudeModuleName);
const vimCommandCandidate = [
	process.env.AGENTOS_VIM_COMMAND,
	join(repoRoot, "registry/software/vim/dist/package/bin/vim"),
	join(repoRoot, "registry/native/target/wasm32-wasip1/release/commands/vim"),
	join(browserAssets, "commands/vim"),
	join(repoRoot, ".local-cmds/vim"),
]
	.filter((value): value is string => Boolean(value))
	.find((value) => existsSync(value));
if (!vimCommandCandidate) {
	throw new Error(
		"Vim command is missing; run pnpm start to build it or set AGENTOS_VIM_COMMAND to an existing AgentOS Vim artifact",
	);
}
const vimCommand = vimCommandCandidate;
const isolationHeaders = {
	"Cross-Origin-Opener-Policy": "same-origin",
	"Cross-Origin-Embedder-Policy": "require-corp",
};
const rootAssets = new Map([
	["agentos-worker.js", join(browserAssets, "agentos-worker.js")],
	["claude-cli.mjs", claudeModule],
]);
const mime: Record<string, string> = {
	".cjs": "text/javascript; charset=utf-8",
	".js": "text/javascript; charset=utf-8",
	".mjs": "text/javascript; charset=utf-8",
	".json": "application/json; charset=utf-8",
	".wasm": "application/wasm",
};

function resolveAsset(pathname: string): string | undefined {
	if (pathname.startsWith("/wasm/")) {
		return join(wasmAssets, pathname.slice("/wasm/".length));
	}
	if (pathname.startsWith("/commands/")) {
		if (pathname === "/commands/vim" && vimCommand) return vimCommand;
		return join(browserAssets, pathname.slice(1));
	}
	const name = pathname.slice(1);
	return rootAssets.get(name);
}

function agentOsAssets(): Plugin {
	return {
		name: "agentos-browser-base-shell-assets",
		configureServer(server) {
			server.middlewares.use((request, response, next) => {
				const pathname = decodeURIComponent((request.url ?? "/").split("?")[0]);
				const file = resolveAsset(pathname);
				if (!file) return next();
				if (!existsSync(file) || !statSync(file).isFile()) {
					response.statusCode = 404;
					response.end(`missing AgentOS browser asset: ${pathname}`);
					return;
				}
				response.setHeader(
					"Content-Type",
					mime[extname(file)] ?? "application/octet-stream",
				);
				for (const [name, value] of Object.entries(isolationHeaders)) {
					response.setHeader(name, value);
				}
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
			cpSync(vimCommand, join(dist, "commands", "vim"));
			for (const [asset, source] of rootAssets) {
				cpSync(source, join(dist, asset));
			}
		},
	};
}

export default defineConfig({
	plugins: [agentOsAssets()],
	// The hostname is generated per Cloudflare Quick Tunnel invocation.
	server: { allowedHosts: true, headers: isolationHeaders },
	preview: { headers: isolationHeaders },
});
