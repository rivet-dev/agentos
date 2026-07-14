import assert from "node:assert/strict";
import test from "node:test";
import {
	defaultRustfmtPackages,
	rustfmtCheckArgs,
} from "./check-rustfmt.mjs";

function metadata(defaultMembers = ["native-id"]) {
	return {
		workspace_default_members: defaultMembers,
		packages: [
			{ id: "native-id", name: "agentos-native-sidecar" },
			{ id: "browser-id", name: "agentos-native-sidecar-browser" },
			{ id: "wrapper-browser-id", name: "agentos-sidecar-browser" },
		],
	};
}

test("formats only Cargo default workspace members", () => {
	assert.deepEqual(defaultRustfmtPackages(metadata()), [
		"agentos-native-sidecar",
	]);
	assert.deepEqual(rustfmtCheckArgs(metadata()), [
		"fmt",
		"--check",
		"--package",
		"agentos-native-sidecar",
	]);
});

test("rejects disabled browser crates in the formatting selection", () => {
	assert.throws(
		() => defaultRustfmtPackages(metadata(["native-id", "browser-id"])),
		/disabled browser packages must not be Cargo default members: agentos-native-sidecar-browser/,
	);
	assert.throws(
		() =>
			defaultRustfmtPackages(metadata(["native-id", "wrapper-browser-id"])),
		/disabled browser packages must not be Cargo default members: agentos-sidecar-browser/,
	);
});

test("rejects stale or empty Cargo default-member metadata", () => {
	assert.throws(
		() => defaultRustfmtPackages(metadata(["missing-id"])),
		/Cargo default workspace member is unknown: missing-id/,
	);
	assert.throws(
		() => defaultRustfmtPackages(metadata([])),
		/Cargo default workspace member list is empty/,
	);
});
