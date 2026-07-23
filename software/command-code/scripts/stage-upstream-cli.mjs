#!/usr/bin/env node

import { build } from "esbuild";
import {
	chmodSync,
	mkdirSync,
	readFileSync,
	writeFileSync,
} from "node:fs";
import { createRequire } from "node:module";
import { dirname, resolve } from "node:path";

const require = createRequire(import.meta.url);
const cliPath = require.resolve("command-code");
const upstreamDir = dirname(cliPath);
const upstreamPackage = JSON.parse(
	readFileSync(resolve(upstreamDir, "..", "package.json"), "utf8"),
);
const outputDir = resolve(import.meta.dirname, "..", "dist", "command-code");
const outputCli = resolve(outputDir, "cli.mjs");

const defaultImport = 'import De from"@opentelemetry/semantic-conventions";';
const namespaceImport = 'import*as De from"@opentelemetry/semantic-conventions";';
const source = readFileSync(cliPath, "utf8");
if (source.split(defaultImport).length !== 2) {
	throw new Error(
		"Command Code's semantic-conventions import changed; review the bundling compatibility patch",
	);
}

mkdirSync(outputDir, { recursive: true });
await build({
	entryPoints: [cliPath],
	outfile: outputCli,
	bundle: true,
	platform: "node",
	target: "node22",
	format: "esm",
	banner: {
		js: 'import { createRequire as __createRequire } from "node:module"; const require = __createRequire(import.meta.url);',
	},
	external: ["@crosscopy/clipboard"],
	plugins: [
		{
			name: "command-code-runtime-compat",
			setup(buildApi) {
				buildApi.onLoad({ filter: /.*/, namespace: "file" }, (args) => {
					if (args.path !== cliPath) return undefined;
					return {
						contents: source.replace(defaultImport, namespaceImport),
						loader: "js",
						resolveDir: upstreamDir,
					};
				});
				buildApi.onResolve({ filter: /^react-devtools-core$/ }, () => ({
					path: "react-devtools-core",
					namespace: "command-code-empty",
				}));
				buildApi.onLoad(
					{ filter: /.*/, namespace: "command-code-empty" },
					() => ({
						contents: "export default { connectToDevTools() {} };",
						loader: "js",
					}),
				);
			},
		},
	],
});

writeFileSync(
	resolve(outputDir, "index.mjs"),
	readFileSync(resolve(upstreamDir, "index.mjs")),
);
writeFileSync(
	resolve(outputDir, "package.json"),
	`${JSON.stringify(
		{
			name: upstreamPackage.name,
			version: upstreamPackage.version,
			type: "module",
		},
		null,
		2,
	)}\n`,
);
writeFileSync(
	resolve(outputDir, "agentos-build.json"),
	`${JSON.stringify(
		{
			sourcePackage: `${upstreamPackage.name}@${upstreamPackage.version}`,
			patches: [
				"bundle runtime dependencies for a bounded registry artifact",
				"normalize the semantic-conventions namespace import for bundling",
				"stub Ink's undeclared react-devtools-core development-only export",
			],
		},
		null,
		2,
	)}\n`,
);
chmodSync(resolve(outputDir, "index.mjs"), 0o755);

process.stdout.write(
	`Staged Command Code ${upstreamPackage.version} as a bundled Node.js CLI\n`,
);
