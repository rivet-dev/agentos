import assert from "node:assert/strict";
import {
	mkdirSync,
	mkdtempSync,
	readdirSync,
	readFileSync,
	rmSync,
	writeFileSync,
} from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import test from "node:test";
import { assertPublishableV8Source, discoverRustCrates, RUST_CRATES } from "./rust-crates.js";

function withFixture(fn: (root: string) => void) {
	const root = mkdtempSync(join(tmpdir(), "publish-rust-crates-"));
	try {
		fn(root);
	} finally {
		rmSync(root, { recursive: true, force: true });
	}
}

function write(root: string, rel: string, contents: string) {
	const path = join(root, rel);
	mkdirSync(join(path, ".."), { recursive: true });
	writeFileSync(path, contents);
}

function assertBefore(crate: string, dependent: string) {
	const crateIndex = RUST_CRATES.indexOf(crate as (typeof RUST_CRATES)[number]);
	const dependentIndex = RUST_CRATES.indexOf(
		dependent as (typeof RUST_CRATES)[number],
	);

	assert.notEqual(crateIndex, -1, `${crate} is missing from publish order`);
	assert.notEqual(
		dependentIndex,
		-1,
		`${dependent} is missing from publish order`,
	);
	assert(
		crateIndex < dependentIndex,
		`${crate} must publish before ${dependent}`,
	);
}

test("Rust crate publish order satisfies internal dependencies", () => {
	assert.equal(new Set(RUST_CRATES).size, RUST_CRATES.length);
	assert(!RUST_CRATES.includes("agentos-sidecar-browser" as never));
	assert(!RUST_CRATES.includes("agentos-vm-browser" as never));
	assert(!RUST_CRATES.includes("agentos-sidecar-core" as never));
	assert(!RUST_CRATES.includes("agentos-build-support" as never));
	assert(!RUST_CRATES.includes("agentos-vm-core" as never));

	assertBefore("agentos-rivetkit-ars-client", "agentos-vm");
	assertBefore("agentos-v8", "agentos-executor-v8-runtime");
	assertBefore("agentos-vm-host-interface", "agentos-executor-v8-runtime");
	assertBefore("agentos-executor-contract", "agentos-executor-v8-runtime");
	assertBefore("agentos-resource-accounting", "agentos-driver-tokio");
	assertBefore("agentos-resource-accounting", "agentos-vm-kernel");
	assertBefore("agentos-resource-accounting", "agentos-executor-v8-runtime");
	assertBefore("agentos-resource-accounting", "agentos-executor-wasm-v8");
	assertBefore("agentos-executor-wasm-abi", "agentos-executor-wasm-v8");
	assertBefore("agentos-driver-tokio", "agentos-vm-kernel");
	assertBefore("agentos-driver-tokio", "agentos-executor-v8-runtime");
	assertBefore("agentos-driver-tokio", "agentos-vm");
	assertBefore("agentos-vfs-core", "agentos-vfs-storage");
	assertBefore("agentos-executor-v8-runtime", "agentos-executor-node-v8");
	assertBefore("agentos-executor-v8-runtime", "agentos-executor-python-v8-pyodide");
	assertBefore("agentos-executor-v8-runtime", "agentos-executor-wasm-v8");
	assertBefore("agentos-sidecar-protocol", "agentos-sidecar-client");
	assertBefore("agentos-executor-node-v8", "agentos-vm");
	assertBefore("agentos-executor-python-v8-pyodide", "agentos-vm");
	assertBefore("agentos-executor-wasm-v8", "agentos-vm");
	assertBefore("agentos-executor-wasm-wasmtime", "agentos-vm");
	assertBefore("agentos-sidecar-client", "agentos-vm");
	assertBefore("agentos-client", "agentos-vm");
	assertBefore("agentos-client", "agentos-sidecar");
});

test("archived browser crates stay excluded from real publish discovery", () => {
	const repoRoot = join(import.meta.dirname, "../../../..");
	const crates = discoverRustCrates(repoRoot);
	assert(!crates.includes("agentos-sidecar-browser" as never));
	assert(!crates.includes("agentos-vm-browser" as never));
	assert(crates.includes("agentos-executor-wasm-wasmtime"));
});

test("discovers the publishable Rust crate subset from a workspace", () => {
	withFixture((root) => {
		write(
			root,
			"Cargo.toml",
			[
				"[workspace]",
				"members = [",
				'  "crates/sidecar-protocol",',
				'  "crates/sidecar",',
				'  "crates/vm",',
				'  "crates/client",',
				"]",
				"",
			].join("\n"),
		);
		for (const [member, name] of [
			["crates/sidecar-protocol", "agentos-sidecar-protocol"],
			["crates/sidecar", "agentos-sidecar"],
			["crates/vm", "agentos-vm"],
			["crates/client", "agentos-client"],
		]) {
			write(root, join(member, "Cargo.toml"), `[package]\nname = "${name}"\n`);
		}

		assert.deepEqual(discoverRustCrates(root), [
			"agentos-sidecar-protocol",
			"agentos-client",
			"agentos-vm",
			"agentos-sidecar",
		]);
	});
});

test("hosted crates publish before their actual manifest dependents", () => {
	const repoRoot = join(import.meta.dirname, "../../../..");
	const crates = discoverRustCrates(repoRoot);
	for (const [member, name] of [
		["actor-contract", "agentos-actor-contract"],
		["preload", "agentos-preload"],
		["actor", "agentos-actor"],
		["sidecar", "agentos-sidecar"],
	]) {
		const manifest = readFileSync(
			join(repoRoot, "crates", member, "Cargo.toml"),
			"utf8",
		);
		assert.doesNotMatch(manifest, /^\s*publish\s*=\s*false/m);
		const index = crates.indexOf(name as (typeof crates)[number]);
		assert.notEqual(
			index,
			-1,
			`${name} is missing from real publish discovery`,
		);
		const dependencies =
			manifest.match(/\[dependencies\]([\s\S]*?)(?=\n\[|$)/)?.[1] ?? "";
		for (const match of dependencies.matchAll(/^(agentos-[a-z0-9-]+)\s*=/gm)) {
			const dependency = match[1];
			const dependencyIndex = crates.indexOf(
				dependency as (typeof crates)[number],
			);
			assert.notEqual(
				dependencyIndex,
				-1,
				`${dependency} required by ${name} is missing`,
			);
			assert(dependencyIndex < index, `${dependency} must publish before ${name}`);
		}
	}
});

test("real crate releases require every locked V8 dependency to resolve through crates.io", () => {
	withFixture((root) => {
		const registry = '[[package]]\nname = "v8"\nversion = "1.0.0"\nsource = "registry+https://github.com/rust-lang/crates.io-index"\n';
		write(root, "Cargo.lock", registry);
		const sha256 = "a".repeat(64);
		const pins = Object.fromEntries([
			"x86_64-unknown-linux-gnu", "aarch64-unknown-linux-gnu",
			"x86_64-apple-darwin", "aarch64-apple-darwin",
		].map((target) => [target, sha256]));
		write(root, "scripts/ci/v8/archives.json", JSON.stringify({ ...pins, "x86_64-apple-darwin": { sha256, artifactId: 123 } }));
		assert.throws(() => assertPublishableV8Source(root), /durable V8 archive SHA-256/);
		write(root, "scripts/ci/v8/archives.json", JSON.stringify(pins));
		assert.doesNotThrow(() => assertPublishableV8Source(root));
		for (const source of ['', 'source = "git+https://example.org/engine#abc"\n']) {
			write(root, "Cargo.lock", registry + '[[package]]\nname = "v8"\nversion = "2.0.0"\n' + source);
			assert.throws(() => assertPublishableV8Source(root), /outside the agentOS workspace/);
		}
	});
});

test("owned V8 bindings require a real publishable crate and durable pins", () => {
	withFixture((root) => {
		write(root, "Cargo.lock", '[[package]]\nname = "agentos-v8"\nversion = "0.0.1"\n');
		assert.throws(() => assertPublishableV8Source(root), /outside the agentOS workspace/);
		write(root, "crates/v8/Cargo.toml", '[package]\nname = "agentos-v8"\n');
		const pins = Object.fromEntries([
			"x86_64-unknown-linux-gnu", "aarch64-unknown-linux-gnu",
			"x86_64-apple-darwin", "aarch64-apple-darwin",
		].map((target) => [target, "a".repeat(64)]));
		write(root, "scripts/ci/v8/archives.json", JSON.stringify(pins));
		assert.doesNotThrow(() => assertPublishableV8Source(root));
		write(root, "Cargo.lock", '[[package]]\nname = "agentos-v8"\nversion = "0.0.1"\nsource = "git+https://example.org/binding"\n');
		assert.throws(() => assertPublishableV8Source(root), /outside the agentOS workspace/);
	});
});

// cargo publish resolves every non-dev dependency from crates.io, optional
// ones included, so each crate must come after all of them in the order.
test("every crate publishes after the agentOS crates its manifest depends on", () => {
	const repoRoot = join(import.meta.dirname, "../../../..");
	const cratesDir = join(repoRoot, "crates");
	const order: readonly string[] = RUST_CRATES;
	const seen = new Set<string>();
	for (const member of readdirSync(cratesDir, { withFileTypes: true })) {
		if (!member.isDirectory()) continue;
		let manifest: string;
		try {
			manifest = readFileSync(join(cratesDir, member.name, "Cargo.toml"), "utf8");
		} catch {
			continue;
		}
		const name = manifest.match(/^\s*name\s*=\s*"([^"]+)"/m)?.[1];
		if (!name || !order.includes(name)) continue;
		seen.add(name);
		const sections = manifest.matchAll(
			/^\[((?:target\.[^\]]+\.)?(?:build-)?dependencies)\]\n([\s\S]*?)(?=^\[|(?![\s\S]))/gm,
		);
		for (const [, section, body] of sections) {
			for (const [, dependency] of body.matchAll(
				/^(agentos-[a-z0-9-]+)(?:\.workspace)?\s*=/gm,
			)) {
				assert(
					order.includes(dependency),
					`${name} [${section}] depends on ${dependency}, which is not published`,
				);
				assert(
					order.indexOf(dependency) < order.indexOf(name),
					`${dependency} must publish before ${name}`,
				);
			}
		}
	}
	assert.deepEqual([...seen].sort(), [...order].sort());
});
