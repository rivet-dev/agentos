import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { dirname, join } from "node:path";
import test from "node:test";
import { fileURLToPath } from "node:url";
import codex from "../dist/index.js";

const __dirname = dirname(fileURLToPath(import.meta.url));

test("codex package does not advertise an ACP adapter until the real agent is wired", () => {
	const manifest = JSON.parse(
		readFileSync(join(__dirname, "..", "package.json"), "utf8"),
	);

	assert.equal(manifest.bin, undefined);
	// The package now re-exports the @agentos-software/codex-cli package
	// descriptor ({ packagePath }) instead of a bespoke shape.
	assert.equal(typeof codex.packagePath, "string");
	assert.equal(codex.agent, undefined);
});
