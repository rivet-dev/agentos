#!/usr/bin/env node
import {
	cpSync,
	existsSync,
	lstatSync,
	mkdirSync,
	readFileSync,
	readdirSync,
	readlinkSync,
	rmSync,
	writeFileSync,
} from "node:fs";
import { createHash } from "node:crypto";
import { dirname, join, relative, resolve, sep } from "node:path";
import { execFileSync } from "node:child_process";
import { fileURLToPath } from "node:url";

const NODE_TAG = "v24.15.0";
const NODE_COMMIT = "848430679556aed0bd073f2bc263331ad84fa119";
const OPENSSL_VERSION = "3.5.5";
const root = resolve(dirname(fileURLToPath(import.meta.url)), "..");
const vendor = join(root, "vendor");

const mappings = [
	["LICENSE", "LICENSE"],
	["lib", "lib"],
	["deps/undici/undici.js", "deps/undici/undici.js"],
	["deps/undici/LICENSE", "deps/undici/LICENSE"],
	["deps/acorn", "deps/acorn"],
	["deps/llhttp", "native/llhttp"],
	["deps/ada", "native/ada"],
	["deps/nbytes", "native/nbytes"],
	["deps/merve", "native/merve"],
	["deps/v8/third_party/simdutf", "native/simdutf"],
	["deps/zlib", "native/zlib"],
	["deps/brotli", "native/brotli"],
	["deps/zstd", "native/zstd"],
	["deps/nghttp2", "native/nghttp2"],
	["test/common", "test/common"],
	["test/fixtures", "test/fixtures"],
	["test/parallel", "test/parallel"],
	["test/sequential", "test/sequential"],
];

function parseArgs(argv) {
	let nodeSrc = process.env.NODE_SRC_DIR;
	let check = false;
	for (let i = 0; i < argv.length; i++) {
		if (argv[i] === "--check") check = true;
		else if (argv[i] === "--node-src") nodeSrc = argv[++i];
		else if (argv[i].startsWith("--node-src=")) nodeSrc = argv[i].slice(11);
		else throw new Error(`unknown argument: ${argv[i]}`);
	}
	if (!nodeSrc) throw new Error("--node-src (or NODE_SRC_DIR) is required");
	return { check, nodeSrc: resolve(nodeSrc) };
}

function git(nodeSrc, ...args) {
	return execFileSync("git", ["-C", nodeSrc, ...args], { encoding: "utf8" }).trim();
}

function sha256(bytes) {
	return createHash("sha256").update(bytes).digest("hex");
}

function slash(path) {
	return path.split(sep).join("/");
}

function collect(dir, base, files) {
	for (const entry of readdirSync(dir, { withFileTypes: true }).sort((a, b) => a.name.localeCompare(b.name))) {
		const path = join(dir, entry.name);
		const rel = slash(relative(base, path));
		if (entry.isDirectory()) collect(path, base, files);
		else if (entry.isSymbolicLink()) files[rel] = `symlink:${readlinkSync(path)}`;
		else if (entry.isFile()) files[rel] = sha256(readFileSync(path));
	}
}

function expectedFiles(nodeSrc) {
	const files = {};
	for (const [sourceRel, destRel] of mappings) {
		const source = join(nodeSrc, sourceRel);
		if (!existsSync(source)) throw new Error(`pinned Node input is missing: ${sourceRel}`);
		const stat = lstatSync(source);
		if (stat.isDirectory()) {
			const nested = {};
			collect(source, source, nested);
			for (const [rel, hash] of Object.entries(nested)) files[slash(join(destRel, rel))] = hash;
		} else {
			files[destRel] = stat.isSymbolicLink()
				? `symlink:${readlinkSync(source)}`
				: sha256(readFileSync(source));
		}
	}
	return Object.fromEntries(Object.entries(files).sort(([a], [b]) => a.localeCompare(b)));
}

function sourceIdentity(nodeSrc, files) {
	const opensslTree = git(nodeSrc, "rev-parse", "HEAD:deps/openssl/openssl");
	const inputs = Object.fromEntries(
		mappings.map(([source, destination]) => [
			destination,
			git(nodeSrc, "rev-parse", `HEAD:${source}`),
		]),
	);
	const contentHash = sha256(JSON.stringify(files));
	return {
		schema: 1,
		node: { tag: NODE_TAG, commit: NODE_COMMIT },
		openssl: { version: OPENSSL_VERSION, node_tree: opensslTree },
		inputs,
		content_sha256: contentHash,
		files,
	};
}

function assertPinnedCheckout(nodeSrc) {
	const head = git(nodeSrc, "rev-parse", "HEAD");
	if (head !== NODE_COMMIT) {
		throw new Error(`Node checkout is ${head}; expected ${NODE_COMMIT} (${NODE_TAG})`);
	}
	const version = readFileSync(join(nodeSrc, "deps/openssl/openssl/VERSION.dat"), "utf8");
	for (const line of ["MAJOR=3", "MINOR=5", "PATCH=5"]) {
		if (!version.split(/\r?\n/).includes(line)) throw new Error(`Node-bundled OpenSSL is not ${OPENSSL_VERSION}`);
	}
}

function update(nodeSrc, manifest) {
	rmSync(vendor, { recursive: true, force: true });
	mkdirSync(vendor, { recursive: true });
	for (const [sourceRel, destRel] of mappings) {
		const destination = join(vendor, destRel);
		mkdirSync(dirname(destination), { recursive: true });
		cpSync(join(nodeSrc, sourceRel), destination, { recursive: true, verbatimSymlinks: true });
	}
	mkdirSync(join(vendor, "patches"), { recursive: true });
	writeFileSync(join(vendor, "patches/.gitkeep"), "");
	writeFileSync(join(vendor, "manifest.json"), `${JSON.stringify(manifest, null, 2)}\n`);
}

function verify(manifest) {
	const path = join(vendor, "manifest.json");
	if (!existsSync(path)) throw new Error("vendor/manifest.json is missing; run without --check");
	const actual = JSON.parse(readFileSync(path, "utf8"));
	const vendorFiles = {};
	collect(vendor, vendor, vendorFiles);
	delete vendorFiles["manifest.json"];
	delete vendorFiles["patches/.gitkeep"];
	const sortedVendorFiles = Object.fromEntries(
		Object.entries(vendorFiles).sort(([a], [b]) => a.localeCompare(b)),
	);
	if (JSON.stringify(actual) !== JSON.stringify(manifest)) throw new Error("vendor manifest differs from pinned Node checkout");
	if (JSON.stringify(sortedVendorFiles) !== JSON.stringify(manifest.files)) throw new Error("vendored Node files differ from manifest");
}

const options = parseArgs(process.argv.slice(2));
assertPinnedCheckout(options.nodeSrc);
const files = expectedFiles(options.nodeSrc);
const manifest = sourceIdentity(options.nodeSrc, files);
if (options.check) verify(manifest);
else update(options.nodeSrc, manifest);
process.stdout.write(
	`vendor-node: ${options.check ? "verified" : "updated"} ${Object.keys(files).length} files from ${NODE_TAG} @ ${NODE_COMMIT}\n`,
);
