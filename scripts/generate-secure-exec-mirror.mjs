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

function lockedRootVersion(lockfile, packageName) {
	const importerStart = lockfile.indexOf("\n  .:\n");
	const importerEnd = lockfile.indexOf("\n  examples/", importerStart);
	if (importerStart === -1 || importerEnd === -1) {
		throw new Error("could not locate the AgentOS root importer in pnpm-lock.yaml");
	}
	const importer = lockfile.slice(importerStart, importerEnd);
	const match = importer.match(
		new RegExp(
			`\\n      ${packageName}:\\n(?:        [^\\n]*\\n)*?        version: ([^\\n]+)`,
		),
	);
	if (!match) {
		throw new Error(`could not locate locked root dependency ${packageName}`);
	}
	return match[1];
}

function writePnpmLock(root, npmShims) {
	const source = readFileSync(join(AGENTOS_ROOT, "pnpm-lock.yaml"), "utf8");
	const packagesStart = source.indexOf("\npackages:\n");
	if (packagesStart === -1) {
		throw new Error("could not locate package snapshots in pnpm-lock.yaml");
	}
	const turboVersion = lockedRootVersion(source, "turbo");
	const typescriptVersion = lockedRootVersion(source, "typescript");
	const importers = npmShims
		.map((spec) => {
			const packageDir = packageDirFor(spec.shimPackage);
			const targetPath = join(
				AGENTOS_ROOT,
				spec.targetPath.replace(/^agentos\//, ""),
			);
			const targetLink = `link:${relative(join(root, packageDir), targetPath)}`;
			return `  ${packageDir}:
    dependencies:
      ${JSON.stringify(spec.targetPackage)}:
        specifier: ${JSON.stringify(targetLink)}
        version: ${JSON.stringify(targetLink)}
    devDependencies:
      typescript:
        specifier: ^5.9.2
        version: ${typescriptVersion}`;
		})
		.join("\n\n");
	write(
		join(root, "pnpm-lock.yaml"),
		`lockfileVersion: '9.0'

settings:
  autoInstallPeers: true
  excludeLinksFromLockfile: false

importers:

  .:
    devDependencies:
      turbo:
        specifier: ^2.5.6
        version: ${turboVersion}
      typescript:
        specifier: ^5.9.2
        version: ${typescriptVersion}

${importers}
${source.slice(packagesStart)}`,
	);
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
	const browserShim = packageName === "@secure-exec/browser";
	const targetPath = join(AGENTOS_ROOT, spec.targetPath.replace(/^agentos\//, ""));
	const targetLink = `link:${relative(dir, targetPath)}`;
	write(
		join(dir, "package.json"),
		json({
			name: packageName,
			...(browserShim ? { private: true } : {}),
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
		browserShim
			? `# ${packageName}\n\nCompatibility shim source for \`${target}\`. Browser runtime support is retained but disabled from default CI and publication pending a dedicated security design.\n`
			: `# ${packageName}\n\nCompatibility shim for \`${target}\`.\n`,
	);
}

function rustDependencyName(spec) {
	return spec.targetPackage === "agentos-vfs-core"
		? "vfs"
		: spec.targetRustIdentifier;
}

function writeRustShim(root, spec) {
	const dir = join(root, crateDirFor(spec.shimPackage));
	const browserShim = spec.shimPackage === "secure-exec-sidecar-browser";
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
${browserShim ? "publish = false" : ""}

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
				build: "turbo run build --filter='!@secure-exec/browser'",
				"check-types": "turbo run check-types --filter='!@secure-exec/browser'",
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
		join(root, "turbo.json"),
		json({
			$schema: "https://turbo.build/schema.json",
			tasks: {
				build: {
					dependsOn: ["^build"],
					outputs: ["dist/**"],
				},
				"check-types": {
					dependsOn: ["^check-types"],
					outputs: [],
				},
			},
		}),
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
	// Compatibility crates and AgentOS ship in lockstep. Seed the mirror from
	// the authoritative runtime lock so Cargo cannot independently select a
	// newer transitive API for path-linked AgentOS crates.
	write(join(root, "Cargo.lock"), readFileSync(join(AGENTOS_ROOT, "Cargo.lock")));
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
      # Browser compatibility source is retained but disabled until its
      # independent reactor/security design is complete.
      - run: cargo check --workspace --exclude secure-exec-sidecar-browser
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
	writePnpmLock(mirrorRoot, npmShims);
	console.log(
		`generated ${npmShims.length} npm shims and ${rustShims.length} Rust shims in ${mirrorRoot}`,
	);
}

main();
