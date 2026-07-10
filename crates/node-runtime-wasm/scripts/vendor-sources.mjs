#!/usr/bin/env node
import {
	copyFileSync,
	existsSync,
	lstatSync,
	mkdirSync,
	readFileSync,
	readdirSync,
	readlinkSync,
	rmSync,
	symlinkSync,
	writeFileSync,
} from "node:fs";
import { createHash } from "node:crypto";
import { execFileSync } from "node:child_process";
import { dirname, join, relative, resolve, sep } from "node:path";
import { fileURLToPath } from "node:url";

const pins = {
	edgejs: "b1feaa2c2b36f443ee5d527161dd93f3ac1544d6",
	napi: "b3709d2506b8bfeb1cd4ede3ab737f0679378a20",
	libuv: "cb7e09aed2fb784255d108d7c78c2063a61b3865",
	node: "848430679556aed0bd073f2bc263331ad84fa119",
	rustyV8CrateChecksum: "278d906d3513fce0be40e1b28eb4c482f44e9d3bf7c1be880441e706bebf5e43",
	rustyV8Vcs: "6a4f2ad9f4ee9677a421832679905b4bee1264f6",
};

const selections = {
	edgejs: [
		"src/**",
		"lib/**",
		"cmake/**",
		"scripts/**",
		"deps/xxhash/**",
		"wasix/**",
		"tests/**",
		"test/**",
		"AGENTS.md",
		"ARCHITECTURE.md",
		"CMakeLists.txt",
		"LICENSE",
		"Makefile",
		"README.md",
		"install.sh",
		"rust-toolchain.toml",
		"wasmer.toml",
	],
	napi: [
		"include/**",
		"lib/**",
		"src/**",
		"v8/**",
		"tests/**",
		"cmake/**",
		"CMakeLists.txt",
		"Cargo.lock",
		"Cargo.toml",
		"Makefile",
		"build.rs",
		"cargo-standalone.sh",
	],
	libuv: [],
	node: [
		"src/**",
		"deps/openssl/openssl/**",
		"deps/cares/**",
		"deps/uv/**",
		"deps/ncrypto/**",
		"deps/simdjson/**",
		"deps/icu-small/**",
		"deps/v8/include/**",
		"deps/icu-small/source/common/unicode/uvernum.h",
		"common.gypi",
		"configure.py",
		"node.gyp",
		"LICENSE",
	],
	rustyV8: [
		"src/**",
		"v8/include/**",
		"v8/include/v8-version.h",
		"third_party/icu/source/common/unicode/uvernum.h",
		"third_party/icu/version.json",
		".cargo_vcs_info.json",
		"Cargo.toml",
		"Cargo.toml.orig",
		"build.rs",
	],
};

// GitHub push protection classifies this public upstream OpenSSL application
// test fixture as a live SSH private key. It is not used by the Node runtime or
// OpenSSL library build, so exclude it deterministically instead of vendoring
// private-key-shaped material into the repository.
const exclusions = {
	node: new Set(["deps/openssl/openssl/apps/rsa8192.pem"]),
};

const crateDir = resolve(dirname(fileURLToPath(import.meta.url)), "..");
const repoRoot = resolve(crateDir, "../..");
const vendorDir = join(crateDir, "vendor");
const nodeVendorManifest = join(repoRoot, "crates/node-stdlib/vendor/manifest.json");

function parseArgs(argv) {
	const options = {
		check: false,
		edgejs: process.env.EDGEJS_SRC_DIR,
		napi: process.env.NAPI_SRC_DIR,
		libuv: process.env.LIBUV_WASIX_SRC_DIR,
		node: process.env.NODE_SRC_DIR,
		rustyV8: process.env.RUSTY_V8_SRC_DIR,
	};
	const names = new Map([
		["--edgejs-src", "edgejs"],
		["--napi-src", "napi"],
		["--libuv-src", "libuv"],
		["--node-src", "node"],
		["--rusty-v8-src", "rustyV8"],
	]);
	for (let index = 0; index < argv.length; index++) {
		const argument = argv[index];
		if (argument === "--check") {
			options.check = true;
			continue;
		}
		const equals = argument.indexOf("=");
		const flag = equals === -1 ? argument : argument.slice(0, equals);
		const name = names.get(flag);
		if (!name) throw new Error(`unknown argument: ${argument}`);
		const value = equals === -1 ? argv[++index] : argument.slice(equals + 1);
		if (!value) throw new Error(`${flag} requires a path`);
		options[name] = resolve(value);
	}
	for (const name of ["edgejs", "napi", "libuv", "node", "rustyV8"]) {
		if (!options[name]) {
			const flag = name === "rustyV8" ? "rusty-v8" : name;
			throw new Error(`--${flag}-src is required`);
		}
	}
	return options;
}

function git(path, ...args) {
	return execFileSync("git", ["-C", path, ...args], { encoding: "utf8" }).trim();
}

function sha256(bytes) {
	return createHash("sha256").update(bytes).digest("hex");
}

function slash(path) {
	return path.split(sep).join("/");
}

function trackedFiles(path, patterns) {
	const args = ["ls-files", "-z"];
	if (patterns.length > 0) args.push("--", ...patterns);
	const output = execFileSync("git", ["-C", path, ...args]);
	return output
		.toString("utf8")
		.split("\0")
		.filter(Boolean)
		.sort((a, b) => a.localeCompare(b));
}

function filesystemFiles(root, patterns) {
	const files = new Set();
	function visit(path) {
		const stat = lstatSync(path);
		if (!stat.isDirectory()) {
			files.add(slash(relative(root, path)));
			return;
		}
		for (const entry of readdirSync(path).sort((a, b) => a.localeCompare(b))) visit(join(path, entry));
	}
	for (const pattern of patterns) {
		const rel = pattern.endsWith("/**") ? pattern.slice(0, -3) : pattern;
		const path = join(root, rel);
		if (!existsSync(path)) throw new Error(`rusty_v8 input is missing: ${pattern}`);
		visit(path);
	}
	return [...files].sort((a, b) => a.localeCompare(b));
}

function fileIdentity(path) {
	const stat = lstatSync(path);
	return stat.isSymbolicLink()
		? `symlink:${readlinkSync(path)}`
		: sha256(readFileSync(path));
}

function expectedFiles(sources) {
	const files = {};
	for (const name of ["edgejs", "napi", "libuv", "node"]) {
		for (const path of trackedFiles(sources[name], selections[name])) {
			if (exclusions[name]?.has(path)) continue;
			files[`${name}/${slash(path)}`] = fileIdentity(join(sources[name], path));
		}
	}
	for (const path of filesystemFiles(sources.rustyV8, selections.rustyV8)) {
		files[`rustyV8/${path}`] = fileIdentity(join(sources.rustyV8, path));
	}
	return Object.fromEntries(Object.entries(files).sort(([a], [b]) => a.localeCompare(b)));
}

function collect(path, base, files) {
	for (const entry of readdirSync(path, { withFileTypes: true }).sort((a, b) => a.name.localeCompare(b.name))) {
		const child = join(path, entry.name);
		const rel = slash(relative(base, child));
		if (entry.isDirectory()) collect(child, base, files);
		else files[rel] = fileIdentity(child);
	}
}

function assertPins(sources) {
	for (const name of ["edgejs", "napi", "libuv", "node"]) {
		const head = git(sources[name], "rev-parse", "HEAD");
		if (head !== pins[name]) throw new Error(`${name} checkout is ${head}; expected ${pins[name]}`);
	}
	const rustyCargo = readFileSync(join(sources.rustyV8, "Cargo.toml"), "utf8");
	if (!rustyCargo.includes('version = "136.0.0"')) {
		throw new Error("rusty_v8 checkout is not crate version 136.0.0");
	}
	const vcs = JSON.parse(readFileSync(join(sources.rustyV8, ".cargo_vcs_info.json"), "utf8"));
	if (vcs.git?.sha1 !== pins.rustyV8Vcs) {
		throw new Error(`rusty_v8 VCS identity is ${vcs.git?.sha1}; expected ${pins.rustyV8Vcs}`);
	}
	if (!existsSync(nodeVendorManifest)) {
		throw new Error("crates/node-stdlib/vendor/manifest.json is missing");
	}
}

function manifest(sources, files) {
	const source = {};
	for (const name of ["edgejs", "napi", "libuv", "node"]) {
		source[name] = {
			commit: pins[name],
			tree: git(sources[name], "rev-parse", "HEAD^{tree}"),
			selection: selections[name].length === 0 ? ["all tracked files"] : selections[name],
			...(exclusions[name]?.size ? { exclusions: [...exclusions[name]] } : {}),
			role:
				name === "edgejs"
					? "source-porting-reference-only"
					: name === "napi"
						? "node-api-abi-reference"
						: name === "libuv"
							? "portable-libuv-source"
							: "target-node-native-runtime-and-dependencies",
		};
	}
	source.rustyV8 = {
		crateVersion: "136.0.0",
		crateChecksum: pins.rustyV8CrateChecksum,
		vcs: pins.rustyV8Vcs,
		v8Version: "13.6.233.2",
		icuVersion: "74.2",
		selection: selections.rustyV8,
		role: "existing-native-v8-isolate-rust-binding",
	};
	return {
		schema: 1,
		architecture: {
			edgejsAcceptedArchitecture: false,
			executionEngine: "existing-native-v8-isolate",
			nodeRuntimePlacement: "WebAssembly.Module/Instance-inside-the-same-v8-isolate",
		},
		sources: source,
		nodeJavaScriptVendor: {
			commit: pins.node,
			vendorManifestSha256: sha256(readFileSync(nodeVendorManifest)),
		},
		contentSha256: sha256(JSON.stringify(files)),
		files,
	};
}

function update(sources, expectedManifest) {
	rmSync(vendorDir, { recursive: true, force: true });
	for (const rel of Object.keys(expectedManifest.files)) {
		const [name, ...parts] = rel.split("/");
		const source = join(sources[name], ...parts);
		const destination = join(vendorDir, rel);
		mkdirSync(dirname(destination), { recursive: true });
		const stat = lstatSync(source);
		if (stat.isSymbolicLink()) symlinkSync(readlinkSync(source), destination);
		else copyFileSync(source, destination);
	}
	for (const name of ["edgejs", "napi", "libuv", "node", "rustyV8"]) {
		const patches = join(vendorDir, "patches", name);
		mkdirSync(patches, { recursive: true });
		writeFileSync(join(patches, ".gitkeep"), "");
	}
	writeFileSync(join(vendorDir, "manifest.json"), `${JSON.stringify(expectedManifest, null, 2)}\n`);
}

function verify(expectedManifest) {
	const manifestPath = join(vendorDir, "manifest.json");
	if (!existsSync(manifestPath)) throw new Error("vendor/manifest.json is missing; run without --check");
	const actualManifest = JSON.parse(readFileSync(manifestPath, "utf8"));
	if (JSON.stringify(actualManifest) !== JSON.stringify(expectedManifest)) {
		throw new Error("vendor manifest differs from pinned source checkouts");
	}
	const actualFiles = {};
	collect(vendorDir, vendorDir, actualFiles);
	delete actualFiles["manifest.json"];
	for (const name of ["edgejs", "napi", "libuv", "node", "rustyV8"]) delete actualFiles[`patches/${name}/.gitkeep`];
	const sorted = Object.fromEntries(Object.entries(actualFiles).sort(([a], [b]) => a.localeCompare(b)));
	if (JSON.stringify(sorted) !== JSON.stringify(expectedManifest.files)) {
		throw new Error("vendored sources differ from the file-level manifest");
	}
}

const options = parseArgs(process.argv.slice(2));
assertPins(options);
const files = expectedFiles(options);
const expectedManifest = manifest(options, files);
if (options.check) verify(expectedManifest);
else update(options, expectedManifest);
process.stdout.write(
	`vendor-sources: ${options.check ? "verified" : "updated"} ${Object.keys(files).length} files from pinned Node, EdgeJS, N-API, libuv, and rusty_v8 sources\n`,
);
