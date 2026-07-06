/**
 * Builds the Pi SDK snapshot bundle (Step 2a).
 *
 * Bundles src/snapshot-entry.ts into a single IIFE at dist/sdk-snapshot.js that
 * evaluates the SDK graph and publishes it on globalThis.__PI_SDK_RUNTIME__. node:
 * builtins stay external (provided by the V8 runtime's bridge polyfills, already in
 * the snapshot heap); heavy provider SDKs reached only via dynamic import() stay
 * external so they remain lazy and load post-restore from the VFS.
 *
 * The build env intentionally clears DISPLAY/WAYLAND_DISPLAY (C0 mitigation): the
 * optional @mariozechner/clipboard NAPI addon is behind a DISPLAY guard; with it
 * unset the SDK bakes `clipboard = null` so no native pointer enters the snapshot.
 */
import { createHash } from "node:crypto";
import { createRequire } from "node:module";
import { readFileSync, writeFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";
import { pathToFileURL } from "node:url";

const here = dirname(fileURLToPath(import.meta.url));
const pkgRoot = join(here, "..");

// esbuild lives in the workspace pnpm store; resolve it from there.
const require = createRequire(import.meta.url);
const repoRoot = join(pkgRoot, "..", "..", "..");
let esbuildPath;
try {
	esbuildPath = require.resolve("esbuild", { paths: [pkgRoot, repoRoot] });
} catch {
	const { globSync } = await import("node:fs");
	const matches = globSync(
		join(repoRoot, "node_modules/.pnpm/esbuild@*/node_modules/esbuild/lib/main.js"),
	);
	if (matches.length === 0) throw new Error("esbuild not found in workspace");
	esbuildPath = matches.sort().reverse()[0];
}
const { build } = await import(pathToFileURL(esbuildPath).href);
// Entry/outfile are overridable (PI_SNAPSHOT_ENTRY / PI_SNAPSHOT_OUTFILE) for
// bisection/testing; default to the committed entry + artifact.
const entryPoint = process.env.PI_SNAPSHOT_ENTRY || join(pkgRoot, "src", "snapshot-entry.ts");
const outfile = process.env.PI_SNAPSHOT_OUTFILE || join(pkgRoot, "dist", "sdk-snapshot.js");

// Provider SDKs the pi-ai layer pulls only via dynamic import(); keep them lazy.
const lazyExternals = [
	"@anthropic-ai/sdk",
	"openai",
	"@google/genai",
	"@mistralai/mistralai",
	"@aws-sdk/client-bedrock-runtime",
	"proxy-agent",
	"@mariozechner/clipboard",
];

// The adapter deliberately imports deep `dist/core/*` paths to skip the SDK's TUI
// graph, but the package `exports` map only exposes `.`/`./hooks`. Mirror the
// adapter: resolve the deep specifiers to absolute file paths under the package's
// dist dir, bypassing the exports map (esbuild bundles absolute paths fine).
const { realpathSync, existsSync } = await import("node:fs");
const piCodingRoot = [
	join(pkgRoot, "node_modules/@mariozechner/pi-coding-agent"),
	join(repoRoot, "node_modules/@mariozechner/pi-coding-agent"),
].find((p) => existsSync(p));
if (!piCodingRoot) throw new Error("pi-coding-agent not found in node_modules");
const piCodingReal = realpathSync(piCodingRoot);
const piCodingDist = join(piCodingReal, "dist");
// pnpm sibling layout: pi-coding-agent's own deps (incl. pi-agent-core) live in the
// same `.pnpm/<hash>/node_modules/@mariozechner` dir. Resolve transitive
// @mariozechner/* deps from there so we bundle exactly what the SDK links against.
const piSiblingScope = dirname(piCodingReal); // .../node_modules/@mariozechner
const deepImportPlugin = {
	name: "pi-deep-imports",
	setup(b) {
		b.onResolve({ filter: /^@mariozechner\/pi-coding-agent\/dist\// }, (a) => {
			const rel = a.path.replace("@mariozechner/pi-coding-agent/dist/", "");
			return { path: join(piCodingDist, rel) };
		});
		b.onResolve({ filter: /^@mariozechner\/pi-agent-core/ }, (a) => {
			const sub = a.path.replace("@mariozechner/pi-agent-core", "") || "/dist/index.js";
			return { path: join(piSiblingScope, "pi-agent-core", sub) };
		});
	},
};

// Bisection helper (PI_SNAPSHOT_STUB_MODULES=a,b,c): alias the named bare specifiers
// to an empty module so they're elided from the snapshot graph — used to find which
// heavy dep creates an un-serializable native handle at module-init.
const stubModules = (process.env.PI_SNAPSHOT_STUB_MODULES || "")
	.split(",")
	.map((s) => s.trim())
	.filter(Boolean);
const stubPlugin = {
	name: "pi-stub-modules",
	setup(b) {
		if (stubModules.length === 0) return;
		// Substring match against the import specifier (catches bare specifiers AND
		// relative paths like ../modes/interactive/theme/theme.js).
		b.onResolve({ filter: /.*/ }, (a) => {
			if (stubModules.some((m) => a.path.includes(m))) {
				return { path: a.path, namespace: "pi-stub" };
			}
			return undefined;
		});
		b.onLoad({ filter: /.*/, namespace: "pi-stub" }, () => ({
			// Pure CJS so esbuild interop synthesizes ANY named import as a no-op.
			contents:
				"module.exports = new Proxy(function () {}, { get: function () { return function () {}; } });",
			loader: "js",
		}));
	},
};

// Make the bundle SNAPSHOT-SAFE by eliminating its two top-level I/O hazards at
// build time, so the IIFE evaluates as pure JS (no fs read, no pending promise)
// when run into the V8 snapshot where bridge fns are stubs. See the C0 scan.
const snapshotSafePlugin = {
	name: "pi-snapshot-safe",
	setup(b) {
		// config.js bakes VERSION/APP_NAME from a top-level readFileSync(package.json).
		// Inline the real package.json content so no fs read happens at eval.
		const piCodingPkgJson = readFileSync(
			join(piCodingReal, "package.json"),
			"utf8",
		);
		b.onLoad({ filter: /\/pi-coding-agent\/dist\/config\.js$/ }, (a) => {
			let src = readFileSync(a.path, "utf8");
			const needle = 'readFileSync(getPackageJsonPath(), "utf-8")';
			if (!src.includes(needle)) {
				throw new Error(
					"snapshot-safe: config.js readFileSync(package.json) shape changed; update the transform",
				);
			}
			src = src.replace(needle, JSON.stringify(piCodingPkgJson));
			return { contents: src, loader: "js" };
		});
		// env-api-keys.js eagerly fires 3 fire-and-forget dynamicImport().then()
		// at top level, leaving pending promises. Convert to synchronous require()
		// (grabs the polyfill module reference only — no I/O, no pending promise),
		// preserving the feature post-restore.
		const ENV_API_KEYS_EAGER_IMPORTS = 3;
		b.onLoad({ filter: /\/pi-ai\/dist\/env-api-keys\.js$/ }, (a) => {
			let src = readFileSync(a.path, "utf8");
			const re =
				/dynamicImport\((NODE_\w+_SPECIFIER)\)\.then\(\(m\)\s*=>\s*\{\s*(_\w+)\s*=\s*m\.(\w+);\s*\}\);/g;
			// Count matches BEFORE replacing: a `src === before` check only catches a
			// total shape change. If upstream adds a 4th eager import (or drops one),
			// the regex would silently transform a subset and leave a pending-promise
			// hazard at module-init — re-breaking snapshot-safety with no build error.
			// Assert the exact expected count so any drift fails the build loudly.
			const matchCount = (src.match(re) || []).length;
			if (matchCount !== ENV_API_KEYS_EAGER_IMPORTS) {
				throw new Error(
					`snapshot-safe: env-api-keys.js eager dynamicImport shape changed ` +
						`(found ${matchCount}, expected ${ENV_API_KEYS_EAGER_IMPORTS}); update the transform`,
				);
			}
			src = src.replace(
				re,
				(_m, spec, lhs, prop) => `try { ${lhs} = require(${spec}).${prop}; } catch {}`,
			);
			return { contents: src, loader: "js" };
		});
		// pi-tui/dist/utils.js creates a module-level `new Intl.Segmenter(...)` — an
		// ICU-backed native (External) object that V8's SnapshotCreator cannot
		// serialize ("global handle not serialized: <Foreign>"). Lazy-init it behind
		// a Proxy so the real segmenter is created on first use (post-restore, where
		// Intl is fully available) instead of at module-init. The headless ACP adapter
		// never renders the TUI, so the segmenter is only touched after restore anyway.
		b.onLoad({ filter: /\/pi-tui\/dist\/utils\.js$/ }, (a) => {
			let src = readFileSync(a.path, "utf8");
			const needle =
				'const segmenter = new Intl.Segmenter(undefined, { granularity: "grapheme" });';
			if (!src.includes(needle)) {
				throw new Error(
					"snapshot-safe: pi-tui utils.js Intl.Segmenter singleton shape changed; update the transform",
				);
			}
			src = src.replace(
				needle,
				"const segmenter = (() => { let s; const get = () => (s || (s = new Intl.Segmenter(undefined, { granularity: \"grapheme\" }))); return new Proxy({}, { get: (_, k) => { const v = get()[k]; return typeof v === \"function\" ? v.bind(get()) : v; } }); })();",
			);
			return { contents: src, loader: "js" };
		});
	},
};

// esbuild stubs `import.meta` to `{}` in IIFE output, so import.meta.url is
// undefined and the SDK's top-level install-detection (fileURLToPath(import.meta.url))
// crashes. Pin it to a single deterministic URL: the SDK's projected guest path, so
// any top-level dir/version computation resolves to where the SDK actually lives in
// the VM. Override with PI_SNAPSHOT_BASE_URL (e.g. the real host path for testing).
const baseUrl =
	process.env.PI_SNAPSHOT_BASE_URL ||
	"file:///root/node_modules/@mariozechner/pi-coding-agent/dist/index.js";

const result = await build({
	entryPoints: [entryPoint],
	outfile,
	bundle: true,
	format: "iife",
	platform: "node", // node: builtins become external require() calls
	target: "esnext",
	external: lazyExternals,
	plugins: [stubPlugin, deepImportPlugin, snapshotSafePlugin],
	define: {
		"import.meta.url": JSON.stringify(baseUrl),
		// C0 mitigation: force the headless path in the clipboard native-addon
		// guard so `require("@mariozechner/clipboard")` (a NAPI addon) is never
		// triggered at snapshot-eval time, regardless of the sidecar's env.
		"process.env.DISPLAY": '""',
		"process.env.WAYLAND_DISPLAY": '""',
		"process.env.TERMUX_VERSION": '""',
	},
	legalComments: "none",
	logLevel: "info",
	metafile: true,
});

const bytes = readFileSync(outfile);
const sha256 = createHash("sha256").update(bytes).digest("hex");
writeFileSync(`${outfile}.sha256`, `${sha256}\n`);

const inputs = Object.keys(result.metafile.inputs).length;
console.log(
	`\nsdk-snapshot.js: ${(bytes.length / 1024).toFixed(0)} KiB · ${inputs} modules inlined · sha256 ${sha256.slice(0, 12)}…`,
);
