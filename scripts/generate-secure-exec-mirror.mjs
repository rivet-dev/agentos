#!/usr/bin/env node
import {
	mkdirSync,
	readFileSync,
	rmSync,
	writeFileSync,
} from "node:fs";
import { dirname, join, relative, resolve } from "node:path";
import { fileURLToPath } from "node:url";

const AGENTOS_ROOT = resolve(dirname(fileURLToPath(import.meta.url)), "..");
const DEFAULT_MIRROR_ROOT = resolve(AGENTOS_ROOT, "../secure-exec");
const map = JSON.parse(
	readFileSync(join(AGENTOS_ROOT, "scripts/secure-exec-agentos-map.json"), "utf8"),
);

function parseArgs(argv) {
	let mirrorRoot = DEFAULT_MIRROR_ROOT;
	for (let i = 0; i < argv.length; i++) {
		const arg = argv[i];
		if (arg === "--mirror-root") {
			mirrorRoot = resolve(argv[++i]);
			continue;
		}
		if (arg.startsWith("--mirror-root=")) {
			mirrorRoot = resolve(arg.slice("--mirror-root=".length));
			continue;
		}
		throw new Error(`unknown argument: ${arg}`);
	}
	return { mirrorRoot };
}

function write(path, contents) {
	mkdirSync(dirname(path), { recursive: true });
	writeFileSync(path, contents);
}

function packageDirFor(pkg) {
	if (pkg === "secure-exec") return "packages/secure-exec";
	const unscoped = pkg.replace(/^@secure-exec\//, "");
	return `packages/${unscoped}`;
}

function crateDirFor(crateName) {
	return `crates/${crateName.replace(/^secure-exec-/, "")}`;
}

function json(value) {
	return `${JSON.stringify(value, null, "\t")}\n`;
}

function npmShimExports(targetPackage) {
	return {
		".": {
			types: "./dist/index.d.ts",
			import: "./dist/index.js",
			default: "./dist/index.js",
		},
		"./package.json": "./package.json",
	};
}

function writeNpmShim(root, spec) {
	const packageName = spec.shimPackage;
	const dir = join(root, packageDirFor(packageName));
	const target = spec.targetPackage;
	const targetPath = join(AGENTOS_ROOT, spec.targetPath.replace(/^agentos\//, ""));
	const targetLink = `link:${relative(dir, targetPath)}`;
	write(
		join(dir, "package.json"),
		json({
			name: packageName,
			version: "0.0.1",
			type: "module",
			license: "Apache-2.0",
			description: `${packageName} compatibility shim for ${target}.`,
			main: "./dist/index.js",
			types: "./dist/index.d.ts",
			files: ["dist", "README.md"],
			exports: npmShimExports(target),
			scripts: {
				build: "tsc",
				"check-types": "tsc --noEmit",
			},
			dependencies: {
				[target]: targetLink,
			},
			devDependencies: {
				typescript: "^5.9.2",
			},
		}),
	);
	write(
		join(dir, "tsconfig.json"),
		json({
			extends: "../../tsconfig.base.json",
			compilerOptions: {
				outDir: "dist",
				rootDir: "src",
			},
			include: ["src/**/*.ts"],
		}),
	);
	write(
		join(dir, "src/index.ts"),
		`export * from "${target}";\n`,
	);
	write(
		join(dir, "README.md"),
		`# ${packageName}\n\nCompatibility shim for \`${target}\`.\n`,
	);
}

function rustDependencyName(spec) {
	return spec.targetPackage === "agentos-vfs-core"
		? "vfs"
		: spec.targetRustIdentifier;
}

function writeRustShim(root, spec) {
	const dir = join(root, crateDirFor(spec.shimPackage));
	const targetRel = relative(dir, join(AGENTOS_ROOT, spec.targetPath.replace(/^agentos\//, "")));
	write(
		join(dir, "Cargo.toml"),
		`[package]
name = "${spec.shimPackage}"
version.workspace = true
edition.workspace = true
license.workspace = true
repository.workspace = true
description = "${spec.shimPackage} compatibility shim for ${spec.targetPackage}"

[lib]
name = "${spec.sourceRustIdentifier}"

[dependencies]
${rustDependencyName(spec)} = { package = "${spec.targetPackage}", path = "${targetRel}", version = "0.0.1" }
`,
	);
	write(
		join(dir, "src/lib.rs"),
		`//! Compatibility shim for \`${spec.targetPackage}\`.\n\npub use ${rustDependencyName(spec)}::*;\n`,
	);
}

function writeRoot(root, npmShims, rustShims) {
	write(
		join(root, "package.json"),
		json({
			name: "secure-exec-workspace",
			private: true,
			license: "Apache-2.0",
			type: "module",
			packageManager: "pnpm@10.13.1",
			scripts: {
				build: "turbo run build",
				"check-types": "turbo run check-types",
				test: "pnpm check-types",
			},
			devDependencies: {
				turbo: "^2.5.6",
				typescript: "^5.9.2",
			},
		}),
	);
	write(
		join(root, "pnpm-workspace.yaml"),
		"packages:\n  - packages/*\n\nonlyBuiltDependencies: []\n",
	);
	write(
		join(root, "tsconfig.base.json"),
		json({
			compilerOptions: {
				target: "ES2022",
				module: "NodeNext",
				moduleResolution: "NodeNext",
				declaration: true,
				strict: true,
				skipLibCheck: true,
			},
		}),
	);
	write(
		join(root, "Cargo.toml"),
		`[workspace]
resolver = "2"
members = [
${rustShims.map((spec) => `    "${crateDirFor(spec.shimPackage)}",`).join("\n")}
]

[workspace.package]
version = "0.0.1"
edition = "2021"
license = "Apache-2.0"
repository = "https://github.com/rivet-dev/secure-exec"
`,
	);
	write(
		join(root, "README.md"),
		`# secure-exec\n\nCompatibility mirror. Active runtime development moved to the AgentOS runtime packages and crates.\n`,
	);
	write(
		join(root, "CLAUDE.md"),
		"# secure-exec\n\nThis repository is a generated compatibility mirror. Make runtime changes in the AgentOS repository and regenerate the shims.\n",
	);
	write(
		join(root, "AGENTS.md"),
		"# secure-exec\n\nThis repository is a generated compatibility mirror. Make runtime changes in the AgentOS repository and regenerate the shims.\n",
	);
	write(
		join(root, ".github/workflows/ci.yml"),
		`name: CI

on:
  pull_request:
    branches: [main]
  push:
    branches: [main]

jobs:
  static:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v4
      - uses: pnpm/action-setup@v4
      - uses: actions/setup-node@v4
        with:
          node-version: 22
          cache: pnpm
      - uses: dtolnay/rust-toolchain@stable
      - run: pnpm install --frozen-lockfile
      - run: pnpm check-types
      - run: cargo check --workspace
`,
	);
	write(
		join(root, ".github/workflows/publish.yaml"),
		`name: publish

on:
  workflow_dispatch:
    inputs:
      version:
        required: true
        type: string

jobs:
  static:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v4
      - run: echo "secure-exec publishes generated compatibility shims only."
`,
	);
	write(
		join(root, ".github/workflows/sync-from-agentos.yml"),
		`name: sync-from-agentos

on:
  workflow_dispatch:

jobs:
  static:
    runs-on: ubuntu-latest
    steps:
      - run: echo "Regenerate this mirror from AgentOS with scripts/generate-secure-exec-mirror.mjs."
`,
	);
}

function main() {
	const { mirrorRoot } = parseArgs(process.argv.slice(2));
	const npmShims = map.npmPackages.filter((item) => item.shimPackage);
	const rustShims = map.rustCrates.filter((item) => item.shimPackage);

	for (const rel of [
		"crates",
		"docker",
		"examples",
		"packages",
		"registry",
		"scripts",
		".claude",
		".github/workflows",
	]) {
		rmSync(join(mirrorRoot, rel), { recursive: true, force: true });
	}
	writeRoot(mirrorRoot, npmShims, rustShims);
	for (const spec of npmShims) writeNpmShim(mirrorRoot, spec);
	for (const spec of rustShims) writeRustShim(mirrorRoot, spec);
	console.log(
		`generated ${npmShims.length} npm shims and ${rustShims.length} Rust shims in ${mirrorRoot}`,
	);
}

main();
