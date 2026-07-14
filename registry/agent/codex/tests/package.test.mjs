import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { dirname, join } from "node:path";
import test from "node:test";
import { fileURLToPath } from "node:url";
import codex from "../dist/index.js";

const __dirname = dirname(fileURLToPath(import.meta.url));

test("codex package projects the ACP adapter and codex-cli command package", () => {
	const packageManifest = JSON.parse(
		readFileSync(join(__dirname, "..", "package.json"), "utf8"),
	);
	const agentosManifest = JSON.parse(
		readFileSync(join(__dirname, "..", "agentos-package.json"), "utf8"),
	);

	assert.equal(packageManifest.bin["codex-acp"], "./dist/adapter.js");
	assert.equal(agentosManifest.name, "codex");
	assert.equal(agentosManifest.agent.acpEntrypoint, "codex-acp");
	assert.equal(codex.length, 2);
	assert.equal(typeof codex[0].packagePath, "string");
	assert.equal(typeof codex[1].packagePath, "string");
});
