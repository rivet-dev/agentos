import assert from "node:assert/strict";
import { mkdirSync, mkdtempSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import test from "node:test";
import { checkRegistrySoftwareSplit } from "./check-registry-software-split.mjs";

function withFixture(fn) {
	const root = mkdtempSync(join(tmpdir(), "registry-software-split-"));
	try {
		return fn(root);
	} finally {
		rmSync(root, { recursive: true, force: true });
	}
}

function writeJson(root, rel, value) {
	const path = join(root, rel);
	mkdirSync(join(path, ".."), { recursive: true });
	writeFileSync(path, `${JSON.stringify(value, null, "\t")}\n`);
}

test("accepts agent-os-pkgs registry software package metadata", () => {
	withFixture((root) => {
		writeJson(root, "registry/software/coreutils/package.json", {
			name: "@agent-os-pkgs/coreutils",
			dependencies: {
				"@secure-exec/registry-types": "workspace:*",
			},
		});
		writeJson(root, "registry/software/coreutils/secure-exec-package.json", {
			name: "@agent-os-pkgs/coreutils",
		});

		assert.deepEqual(checkRegistrySoftwareSplit({ root }), []);
	});
});

test("rejects stale Agent OS package names and metadata files", () => {
	withFixture((root) => {
		writeJson(root, "registry/software/grep/package.json", {
			name: "@rivet-dev/agentos-grep",
		});
		writeJson(root, "registry/software/grep/agent-os-package.json", {
			name: "@rivet-dev/agentos-grep",
		});

		assert.deepEqual(checkRegistrySoftwareSplit({ root }), [
			"registry/software/grep/package.json must be named @agent-os-pkgs/grep, found @rivet-dev/agentos-grep",
			"registry/software/grep/agent-os-package.json must be renamed to secure-exec-package.json",
			"registry/software/grep/secure-exec-package.json is required",
		]);
	});
});

test("rejects metadata name drift", () => {
	withFixture((root) => {
		writeJson(root, "registry/software/sed/package.json", {
			name: "@agent-os-pkgs/sed",
		});
		writeJson(root, "registry/software/sed/secure-exec-package.json", {
			name: "@agent-os-pkgs/grep",
		});

		assert.deepEqual(checkRegistrySoftwareSplit({ root }), [
			"registry/software/sed/secure-exec-package.json name must match package.json (@agent-os-pkgs/sed), found @agent-os-pkgs/grep",
		]);
	});
});

test("rejects Agent OS dependencies inside software manifests", () => {
	withFixture((root) => {
		writeJson(root, "registry/software/common/package.json", {
			name: "@agent-os-pkgs/common",
			dependencies: {
				"@rivet-dev/agentos-coreutils": "workspace:*",
			},
		});
		writeJson(root, "registry/software/common/secure-exec-package.json", {
			name: "@agent-os-pkgs/common",
		});

		assert.deepEqual(checkRegistrySoftwareSplit({ root }), [
			"@agent-os-pkgs/common must not depend on Agent OS package @rivet-dev/agentos-coreutils in registry software dependencies",
		]);
	});
});
