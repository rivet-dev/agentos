import { execFileSync } from "node:child_process";
import { mkdirSync, mkdtempSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { dirname, join } from "node:path";
import test from "node:test";
import assert from "node:assert/strict";
import { fileURLToPath } from "node:url";

import { auditThinClientDocs } from "./verify-thin-client-docs.mjs";

const scriptPath = join(
	dirname(fileURLToPath(import.meta.url)),
	"verify-thin-client-docs.mjs",
);

const requiredContent = new Map([
	["README.md", "Omitted permissions allow all VM capabilities."],
	[
		"permissions.mdx",
		"Omitting permissions selects the sidecar-owned allow-all product default. Omitted top-level scopes inherit allow. Inside an explicit rule set, an omitted default means deny.",
	],
	[
		"permissions.md",
		"Omitting permissions selects the sidecar-owned allow-all product default. Omitted top-level scopes inherit allow. Inside an explicit rule set, an omitted default means deny.",
	],
	[
		"security-model.mdx",
		"The sidecar resolves omitted permissions to allow-all.",
	],
	[
		"security-model.md",
		"The sidecar resolves omitted permissions to allow-all.",
	],
	[
		"networking.mdx",
		"Network operations are allowed when permissions or its network scope is omitted.",
	],
	[
		"networking.md",
		"Network operations are allowed when permissions or its network scope is omitted.",
	],
	[
		"python-runtime.mdx",
		"Network is allowed when permissions or its network scope is omitted.",
	],
	[
		"python-runtime.md",
		"Network is allowed when permissions or its network scope is omitted.",
	],
	[
		"architecture.mdx",
		"The sidecar resolves omitted top-level scopes to allow.",
	],
	[
		"architecture.md",
		"The sidecar resolves omitted top-level scopes to allow.",
	],
	[
		"architecture/filesystem.mdx",
		"The sidecar installs allow when the top-level fs scope is omitted.",
	],
	[
		"architecture/filesystem.md",
		"The sidecar installs allow when the top-level fs scope is omitted.",
	],
	[
		"architecture/processes.mdx",
		"The sidecar installs allow when childProcess is omitted.",
	],
	[
		"architecture/processes.md",
		"The sidecar installs allow when childProcess is omitted.",
	],
	[
		"versus-sandbox.mdx",
		"Permissions are allow-all when omitted.",
	],
	["versus-sandbox.md", "Permissions are allow-all when omitted."],
]);

function writeFile(root, path, content) {
	const absolutePath = join(root, path);
	mkdirSync(dirname(absolutePath), { recursive: true });
	writeFileSync(absolutePath, `${content}\n`);
}

function writeValidFixture(root) {
	for (const [path, content] of requiredContent) {
		if (path === "README.md") {
			writeFile(root, path, content);
			continue;
		}
		const sourcePath = path.endsWith(".mdx")
			? join("website/src/content/docs/docs", path)
			: join("website/public/docs/docs", path);
		writeFile(root, sourcePath, content);
	}
}

function withFixture(run) {
	const root = mkdtempSync(join(tmpdir(), "thin-client-docs-"));
	try {
		writeValidFixture(root);
		run(root);
	} finally {
		rmSync(root, { recursive: true, force: true });
	}
}

test("passes on the current tree", () => {
	assert.equal(auditThinClientDocs().ok, true);
});

test("rejects a deny-by-default product claim with its path and line", () => {
	withFixture((root) => {
		writeFile(root, "README.md", "Omitted permissions allow all.\nDeny-by-default permissions.");
		const result = auditThinClientDocs({ root });
		assert.deepEqual(
			result.failures.find(
				(failure) => failure.ruleId === "permission-product-deny-default",
			),
			{
				path: "README.md",
				line: 2,
				ruleId: "permission-product-deny-default",
				text: "Deny-by-default permissions.",
			},
		);
	});
});

test("rejects reworded default network denial", () => {
	withFixture((root) => {
		writeFile(
			root,
			"website/src/content/docs/docs/networking.mdx",
			"Network operations are allowed when permissions or its network scope is omitted.\nBy default the guest cannot reach the network.",
		);
		const result = auditThinClientDocs({ root });
		assert.ok(
			result.failures.some(
				(failure) => failure.ruleId === "permission-network-default-deny",
			),
		);
	});
});

test("rejects a direct claim that omitted permissions deny scopes", () => {
	withFixture((root) => {
		writeFile(
			root,
			"README.md",
			"Omitted permissions allow all. Omitted permissions deny every scope.",
		);
		const result = auditThinClientDocs({ root });
		assert.ok(
			result.failures.some(
				(failure) => failure.ruleId === "permission-omission-denies",
			),
		);
	});
});

test("requires positive omission guidance", () => {
	withFixture((root) => {
		writeFile(root, "README.md", "Granular sidecar permissions.");
		const result = auditThinClientDocs({ root });
		assert.ok(
			result.failures.some(
				(failure) =>
					failure.path === "README.md" &&
					failure.ruleId === "required-allow-all-claim",
			),
		);
	});
});

test("allows explicit deny rules and fenced configuration", () => {
	withFixture((root) => {
		const path = "website/src/content/docs/docs/permissions.mdx";
		writeFile(
			root,
			path,
			`${requiredContent.get("permissions.mdx")}\nAn explicit rule set may deny by default to create an allowlist.\nUse a deny-by-default policy for untrusted workloads.\n\`\`\`ts\nconst policy = { default: "deny" };\n\`\`\``,
		);
		assert.equal(auditThinClientDocs({ root }).ok, true);
	});
});

test("audits the checked public Markdown copy", () => {
	withFixture((root) => {
		const path = "website/public/docs/docs/security-model.md";
		writeFile(
			root,
			path,
			"The sidecar resolves omitted permissions to allow-all. Nothing is allowed until you opt in.",
		);
		const result = auditThinClientDocs({ root });
		assert.ok(
			result.failures.some(
				(failure) =>
					failure.path === path &&
					failure.ruleId === "permission-nothing-allowed",
			),
		);
	});
});

test("rejects unknown CLI arguments", () => {
	assert.throws(
		() =>
			execFileSync(process.execPath, [scriptPath, "--wat"], {
				stdio: "pipe",
			}),
		(error) =>
			error.status === 1 &&
			String(error.stderr).includes("unknown argument: --wat"),
	);
});
