import assert from "node:assert/strict";
import { mkdirSync, mkdtempSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import test from "node:test";
import { checkStaleSplitNames } from "./check-stale-split-names.mjs";

function withFixture(fn) {
	const root = mkdtempSync(join(tmpdir(), "stale-split-names-"));
	try {
		return fn(root);
	} finally {
		rmSync(root, { recursive: true, force: true });
	}
}

function write(root, rel, contents) {
	const path = join(root, rel);
	mkdirSync(join(path, ".."), { recursive: true });
	writeFileSync(path, contents);
}

test("accepts current split names", () => {
	withFixture((root) => {
		write(
			root,
			"packages/core/src/example.ts",
			'process.env.SECURE_EXEC_KEEP_STDIN_OPEN = "1";\nprocess.env.AGENTOS_SIDECAR_BIN = "/tmp/agentos-sidecar";\n',
		);
		write(root, "Cargo.toml", '# secure exec lives at "../secure-exec"\n');

		assert.deepEqual(checkStaleSplitNames({ root }), []);
	});
});

test("rejects stale env vars and legacy repo paths", () => {
	withFixture((root) => {
		write(
			root,
			"packages/core/src/example.ts",
			'process.env.AGENT_OS_KEEP_STDIN_OPEN = "1";\nprocess.env.AGENTOS_SIDECAR_BINARY = "/tmp/agentos-sidecar";\n',
		);
		write(root, "Cargo.toml", '# legacy path: "../se1"\n');

		assert.deepEqual(checkStaleSplitNames({ root }), [
			"Cargo.toml:1:17 uses legacy secure-exec repo path ../se1; use ../secure-exec or ~/secure-exec",
			"packages/core/src/example.ts:1:13 uses legacy stdin env var AGENT_OS_KEEP_STDIN_OPEN; use SECURE_EXEC_KEEP_STDIN_OPEN",
			"packages/core/src/example.ts:2:13 uses legacy sidecar binary env var AGENTOS_SIDECAR_BINARY; use AGENTOS_SIDECAR_BIN",
		]);
	});
});

test("rejects compat protocol schema constants in Rust callers", () => {
	withFixture((root) => {
		write(
			root,
			"crates/client/src/sidecar.rs",
			[
				"fn authenticate() {",
				"\tlet version = secure_exec_client::protocol::PROTOCOL_VERSION;",
				"\tlet name = secure_exec_client::protocol::PROTOCOL_NAME;",
				"}",
				"",
			].join("\n"),
		);

		assert.deepEqual(checkStaleSplitNames({ root }), [
			"crates/client/src/sidecar.rs:2:16 uses compat protocol version constant secure_exec_client::protocol::PROTOCOL_VERSION; use secure_exec_client::wire::PROTOCOL_VERSION",
			"crates/client/src/sidecar.rs:3:13 uses compat protocol name constant secure_exec_client::protocol::PROTOCOL_NAME; use secure_exec_client::wire::PROTOCOL_NAME",
		]);
	});
});

test("rejects stale agentos-client wire-surface docs", () => {
	withFixture((root) => {
		write(
			root,
			"crates/client/src/lib.rs",
			"//! define a new wire protocol; all wire types are reused from `secure_exec_client::protocol`.\n",
		);

		assert.deepEqual(checkStaleSplitNames({ root }), [
			"crates/client/src/lib.rs:1:33 uses stale agentos-client wire-surface documentation all wire types are reused from `secure_exec_client::protocol`; use document secure_exec_client::wire as the generated schema surface",
		]);
	});
});

test("rejects stale session replay surface docs", () => {
	withFixture((root) => {
		write(
			root,
			"packages/core/README.md",
			[
				"| `resumeSession` | Confirm and return an active session ID |",
				"| `getSessionEvents` | Get event history with sequence numbers |",
				"- `SequencedEvent`",
				"- `GetEventsOptions`",
				"",
			].join("\n"),
		);
		write(
			root,
			"packages/core/CLAUDE.md",
			"- ACP session event retention is ack-based now.\n",
		);

		assert.deepEqual(checkStaleSplitNames({ root }), [
			"packages/core/CLAUDE.md:1:34 uses legacy acknowledged session event replay docs ack-based; use live-only session events",
			"packages/core/README.md:1:4 uses legacy session resume API resumeSession; use live sessions created through createSession",
			"packages/core/README.md:2:4 uses legacy session event history API getSessionEvents; use live onSessionEvent subscriptions",
			"packages/core/README.md:3:4 uses legacy sequenced session event type SequencedEvent; use live session events",
			"packages/core/README.md:4:4 uses legacy session event options type GetEventsOptions; use live onSessionEvent subscriptions",
		]);
	});
});

test("rejects stale host callback registration docs", () => {
	withFixture((root) => {
		write(
			root,
			"packages/core/CLAUDE.md",
			"- Host tool registration still uses register_toolkit on the wire.\n",
		);

		assert.deepEqual(checkStaleSplitNames({ root }), [
			"packages/core/CLAUDE.md:1:37 uses legacy host callback registration wire name register_toolkit; use registerHostCallbacks or RegisterHostCallbacks",
		]);
	});
});

test("rejects stale registered tool schema docs", () => {
	withFixture((root) => {
		write(
			root,
			"crates/sidecar/CLAUDE.md",
			"- Validate the registered tool `input_schema` before invoking callbacks.\n",
		);

		assert.deepEqual(checkStaleSplitNames({ root }), [
			"crates/sidecar/CLAUDE.md:1:16 uses legacy registered tool input schema wording registered tool `input_schema`; use registered host callback `input_schema`",
		]);
	});
});

test("rejects stale core ACP relocation docs", () => {
	withFixture((root) => {
		write(
			root,
			"crates/CLAUDE.md",
			[
				"- ACP client compatibility behavior in `crates/sidecar/src/acp/` is required.",
				"- ACP transport write failures in `crates/sidecar/src/acp/client.rs` store AcpClientError.",
				"- Synthetic updates belong in `crates/sidecar/src/acp/session.rs`.",
				"- In `crates/sidecar/src/service.rs`, `CreateSession` owns adapter setup.",
				"- The ACP orchestration embedded in `service.rs` owns the handshake.",
				"- Unknown callbacks use SidecarRequestPayload::AcpRequest and SidecarResponsePayload::AcpRequestResult.",
				"- AcpSessionState cleanup calls close_agent_session.",
				"",
			].join("\n"),
		);

		assert.deepEqual(checkStaleSplitNames({ root }), [
			"crates/CLAUDE.md:1:41 uses legacy core ACP implementation path crates/sidecar/src/acp/; use crates/agentos-sidecar/src/acp_extension.rs",
			"crates/CLAUDE.md:2:36 uses legacy core ACP implementation path crates/sidecar/src/acp/client.rs; use crates/agentos-sidecar/src/acp_extension.rs",
			"crates/CLAUDE.md:3:32 uses legacy core ACP implementation path crates/sidecar/src/acp/session.rs; use crates/agentos-sidecar/src/acp_extension.rs",
			"crates/CLAUDE.md:4:7 uses legacy core ACP create-session guidance crates/sidecar/src/service.rs`, `CreateSession; use crates/agentos-sidecar/src/acp_extension.rs create-session handling",
			"crates/CLAUDE.md:5:7 uses legacy core ACP orchestration guidance ACP orchestration embedded in `service.rs`; use ACP orchestration embedded in `acp_extension.rs`",
			"crates/CLAUDE.md:6:25 uses legacy core ACP callback payload SidecarRequestPayload::AcpRequest; use ACP Ext callbacks",
			"crates/CLAUDE.md:6:63 uses legacy core ACP callback payload SidecarResponsePayload::AcpRequestResult; use ACP Ext callbacks",
			"crates/CLAUDE.md:7:3 uses legacy core ACP session state AcpSessionState; use Agent OS ACP extension session records",
			"crates/CLAUDE.md:7:33 uses legacy core ACP session state close_agent_session; use Agent OS ACP extension session records",
			"crates/CLAUDE.md:2:76 uses legacy ACP client error surface AcpClientError; use SidecarError propagation from the Agent OS ACP extension",
		]);
	});
});

test("rejects stale manual BARE discriminant docs", () => {
	withFixture((root) => {
		write(
			root,
			"crates/CLAUDE.md",
			[
				"- The Rust BARE codec must use explicit schema discriminants.",
				"- Keep manual Rust tag mappings in sync.",
				"- Also preserve the existing human-readable JSON encoding for the migration window.",
				"",
			].join("\n"),
		);

		assert.deepEqual(checkStaleSplitNames({ root }), [
			"crates/CLAUDE.md:1:23 uses legacy manual BARE discriminant guidance must use explicit schema discriminants; use generated positional tag layout",
			"crates/CLAUDE.md:2:8 uses legacy manual BARE discriminant guidance manual Rust tag mappings; use generated positional tag layout",
			"crates/CLAUDE.md:3:8 uses legacy manual BARE discriminant guidance preserve the existing human-readable JSON encoding for the migration window; use generated positional tag layout",
		]);
	});
});

test("rejects stale secure-exec protocol schema names", () => {
	withFixture((root) => {
		write(
			root,
			"crates/sidecar/src/wire.rs",
			'pub const PROTOCOL_NAME: &str = "agentos-sidecar";\n',
		);
		write(
			root,
			"crates/sidecar/protocol/README.md",
			"- `ProtocolSchema.name` remains `agentos-sidecar`\n- `ProtocolSchema.version` remains `1`\n",
		);

		assert.deepEqual(checkStaleSplitNames({ root }), [
			'crates/sidecar/src/wire.rs has stale Rust secure-exec protocol schema name; expected "pub const PROTOCOL_NAME: &str = \\"secure-exec-sidecar\\";"',
			'crates/sidecar/protocol/README.md has stale secure-exec protocol schema documentation; expected "`ProtocolSchema.name` is `secure-exec-sidecar`"',
			'crates/sidecar/protocol/README.md has stale secure-exec protocol schema version documentation; expected "`ProtocolSchema.version` is `7`"',
		]);
	});
});

test("rejects legacy Agent OS internal command paths", () => {
	withFixture((root) => {
		write(
			root,
			"crates/sidecar/src/bootstrap.rs",
			'const COMMAND_ROOT: &str = "/__agentos/commands";\nconst NODE_ROOT: &str = "/__agentos/node-runtime";\n',
		);

		assert.deepEqual(checkStaleSplitNames({ root }), [
			"crates/sidecar/src/bootstrap.rs:1:29 uses legacy Agent OS command projection path /__agentos/commands; use /__secure_exec/{commands,node-runtime}",
			"crates/sidecar/src/bootstrap.rs:2:26 uses legacy Agent OS command projection path /__agentos/node-runtime; use /__secure_exec/{commands,node-runtime}",
		]);
	});
});

test("does not reject se1 inside unrelated words", () => {
	withFixture((root) => {
		write(root, "Cargo.lock", 'name = "base16ct"\n');

		assert.deepEqual(checkStaleSplitNames({ root }), []);
	});
});

test("ignores generated and Ralph transcript paths", () => {
	withFixture((root) => {
		write(
			root,
			"crates/execution/assets/pyodide/pyodide.asm.js",
			"AGENT_OS_KEEP_STDIN_OPEN ~/se1\n",
		);
		write(
			root,
			"scripts/ralph/codex-streams/step-1.log",
			"AGENT_OS_KEEP_STDIN_OPEN ~/se1\n",
		);

		assert.deepEqual(checkStaleSplitNames({ root }), []);
	});
});
