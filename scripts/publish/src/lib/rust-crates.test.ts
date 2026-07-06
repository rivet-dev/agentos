import assert from "node:assert/strict";
import { mkdtempSync, mkdirSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import test from "node:test";
import { discoverRustCrates, RUST_CRATES } from "./rust-crates.js";

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

	assertBefore("agentos-build-support", "agentos-v8-runtime");
	assertBefore("agentos-bridge", "agentos-execution");
	assertBefore("agentos-vfs-core", "agentos-vfs");
	assertBefore("agentos-kernel", "agentos-execution");
	assertBefore("agentos-sidecar-protocol", "agentos-sidecar-client");
	assertBefore("agentos-execution", "agentos-native-sidecar");
	assertBefore("agentos-native-sidecar-core", "agentos-native-sidecar");
	assertBefore("agentos-sidecar-client", "agentos-native-sidecar");
	assertBefore("agentos-native-sidecar", "agentos-native-sidecar-browser");
	assertBefore("agentos-sidecar-core", "agentos-sidecar");
	assertBefore("agentos-protocol", "agentos-client");
	assertBefore("agentos-client", "agentos-sidecar");
});

test("discovers the publishable Rust crate subset from a workspace", () => {
	withFixture((root) => {
		write(
			root,
			"Cargo.toml",
			[
				"[workspace]",
				"members = [",
				'  "crates/agentos-protocol",',
				'  "crates/agentos-sidecar",',
				'  "crates/native-sidecar",',
				'  "crates/client",',
				"]",
				"",
			].join("\n"),
		);
		for (const [member, name] of [
			["crates/agentos-protocol", "agentos-protocol"],
			["crates/agentos-sidecar", "agentos-sidecar"],
			["crates/native-sidecar", "agentos-native-sidecar"],
			["crates/client", "agentos-client"],
		]) {
			write(root, join(member, "Cargo.toml"), `[package]\nname = "${name}"\n`);
		}

		assert.deepEqual(discoverRustCrates(root), [
			"agentos-native-sidecar",
			"agentos-protocol",
			"agentos-client",
			"agentos-sidecar",
		]);
	});
});
