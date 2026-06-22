import assert from "node:assert/strict";
import { mkdirSync, mkdtempSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import test from "node:test";
import { checkAgentOsClientProtocolCompat } from "./check-agentos-client-protocol-compat.mjs";

function withFixture(fn) {
	const root = mkdtempSync(join(tmpdir(), "agentos-client-protocol-compat-"));
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

function writeSidecar(root, extra = "") {
	write(
		root,
		"crates/client/src/sidecar.rs",
		[
			"use secure_exec_client::wire;",
			"",
			"fn authenticate() {",
			"\tlet _ = wire::RequestPayload::AuthenticateRequest(wire::AuthenticateRequest {",
			"\t\tprotocol_version: wire::PROTOCOL_VERSION,",
			"\t});",
			"}",
			extra,
			"",
		].join("\n"),
	);
}

test("allows generated wire imports and wire auth version", () => {
	withFixture((root) => {
		writeSidecar(root);
		write(
			root,
			"crates/client/src/agent_os.rs",
			"use secure_exec_client::wire;\n",
		);

		assert.deepEqual(checkAgentOsClientProtocolCompat({ root }), []);
	});
});

test("allows agentos-sidecar generated wire imports", () => {
	withFixture((root) => {
		writeSidecar(root);
		write(
			root,
			"crates/agentos-sidecar/src/acp_extension.rs",
			[
				"use secure_exec_sidecar::wire::{",
				"\tCloseStdinRequest, EventPayload, ExecuteRequest, GuestFilesystemCallRequest,",
				"\tGuestFilesystemOperation, GuestRuntimeKind, KillProcessRequest, StreamChannel,",
				"\tWriteStdinRequest,",
				"};",
				"",
				"fn accepts(events: &[secure_exec_sidecar::wire::EventFrame]) {",
				"\tlet _ = events;",
				"}",
				"",
			].join("\n"),
		);

		assert.deepEqual(checkAgentOsClientProtocolCompat({ root }), []);
	});
});

test("rejects agentos-sidecar primitive protocol imports", () => {
	withFixture((root) => {
		writeSidecar(root);
		write(
			root,
			"crates/agentos-sidecar/src/acp_extension.rs",
			[
				"use secure_exec_sidecar::protocol::{",
				"\tCloseStdinRequest, EventPayload, ExecuteRequest, GuestFilesystemCallRequest,",
				"\tGuestFilesystemOperation, GuestRuntimeKind, KillProcessRequest, StreamChannel,",
				"\tWriteStdinRequest,",
				"};",
				"",
				"fn accepts(events: &[secure_exec_sidecar::protocol::EventFrame]) {",
				"\tlet _ = events;",
				"}",
				"",
			].join("\n"),
		);

		assert.deepEqual(checkAgentOsClientProtocolCompat({ root }), [
			"crates/agentos-sidecar/src/acp_extension.rs:1:5 imports the secure-exec sidecar compatibility protocol surface; use secure_exec_sidecar::wire for generated wire types",
			"crates/agentos-sidecar/src/acp_extension.rs:7:22 imports the secure-exec sidecar compatibility protocol surface; use secure_exec_sidecar::wire for generated wire types",
		]);
	});
});

test("rejects new agentos-client live protocol imports outside the inventory", () => {
	withFixture((root) => {
		writeSidecar(root);
		write(
			root,
			"crates/client/src/new_feature.rs",
			"use secure_exec_client::protocol::RequestPayload;\n",
		);

		assert.deepEqual(checkAgentOsClientProtocolCompat({ root }), [
			"crates/client/src/new_feature.rs:1:5 imports the live protocol compatibility surface; use secure_exec_client::wire for generated wire types or add this file to the migration inventory with justification",
		]);
	});
});

test("rejects agentos-client test protocol imports", () => {
	withFixture((root) => {
		writeSidecar(root);
		write(
			root,
			"crates/client/tests/session_e2e.rs",
			"use secure_exec_client::protocol::RequestPayload;\n",
		);

		assert.deepEqual(checkAgentOsClientProtocolCompat({ root }), [
			"crates/client/tests/session_e2e.rs:1:5 imports the live protocol compatibility surface; use secure_exec_client::wire for generated wire types or add this file to the migration inventory with justification",
		]);
	});
});

test("rejects production agentos-sidecar dispatch protocol imports", () => {
	withFixture((root) => {
		writeSidecar(root);
		write(
			root,
			"crates/agentos-sidecar/src/acp_extension.rs",
			"use secure_exec_sidecar::protocol::{EventPayload, RequestFrame, SidecarRequestPayload};\n",
		);

		assert.deepEqual(checkAgentOsClientProtocolCompat({ root }), [
			"crates/agentos-sidecar/src/acp_extension.rs:1:5 imports the secure-exec sidecar compatibility protocol surface; use secure_exec_sidecar::wire for generated wire types",
		]);
	});
});

test("rejects agentos-sidecar test protocol imports", () => {
	withFixture((root) => {
		writeSidecar(root);
		write(
			root,
			"crates/agentos-sidecar/tests/acp_extension.rs",
			"use secure_exec_sidecar::protocol::EventPayload;\n",
		);

		assert.deepEqual(checkAgentOsClientProtocolCompat({ root }), [
			"crates/agentos-sidecar/tests/acp_extension.rs:1:5 imports the secure-exec sidecar compatibility protocol surface; use secure_exec_sidecar::wire for generated wire types",
		]);
	});
});

test("rejects production agentos-sidecar qualified dispatch protocol paths", () => {
	withFixture((root) => {
		writeSidecar(root);
		write(
			root,
			"crates/agentos-sidecar/src/acp_extension.rs",
			"fn dispatch() { let _ = secure_exec_sidecar::protocol::RequestFrame::new; }\n",
		);

		assert.deepEqual(checkAgentOsClientProtocolCompat({ root }), [
			"crates/agentos-sidecar/src/acp_extension.rs:1:25 imports the secure-exec sidecar compatibility protocol surface; use secure_exec_sidecar::wire for generated wire types",
		]);
	});
});

test("rejects error taxonomy regressions to the compatibility protocol surface", () => {
	withFixture((root) => {
		writeSidecar(root);
		write(
			root,
			"crates/client/src/error.rs",
			"use secure_exec_client::protocol::ProtocolCodecError;\n",
		);

		assert.deepEqual(checkAgentOsClientProtocolCompat({ root }), [
			"crates/client/src/error.rs:1:5 imports the live protocol compatibility surface; use secure_exec_client::wire for generated wire types or add this file to the migration inventory with justification",
		]);
	});
});

test("rejects docs regressions to naming the compatibility protocol surface", () => {
	withFixture((root) => {
		writeSidecar(root);
		write(
			root,
			"crates/client/src/lib.rs",
			"//! The generated schema surface is secure_exec_client::wire, not secure_exec_client::protocol.\n",
		);

		assert.deepEqual(checkAgentOsClientProtocolCompat({ root }), [
			"crates/client/src/lib.rs:1:67 imports the live protocol compatibility surface; use secure_exec_client::wire for generated wire types or add this file to the migration inventory with justification",
		]);
	});
});

test("rejects docs regressions to stale generated-wire migration wording", () => {
	withFixture((root) => {
		writeSidecar(root);
		write(
			root,
			"crates/client/src/lib.rs",
			"//! secure_exec_client::wire; the live transport still uses the compatibility protocol surface while migration continues.\n",
		);

		assert.deepEqual(checkAgentOsClientProtocolCompat({ root }), [
			"crates/client/src/lib.rs:1:35 documents stale generated-wire migration state; describe secure_exec_client::wire as the active transport surface",
		]);
	});
});

test("rejects auth version regressions to the compatibility protocol surface", () => {
	withFixture((root) => {
		write(
			root,
			"crates/client/src/sidecar.rs",
			[
				"use secure_exec_client::protocol::{AuthenticateRequest, RequestPayload};",
				"",
				"fn authenticate() {",
				"\tlet _ = RequestPayload::Authenticate(AuthenticateRequest {",
				"\t\tprotocol_version: secure_exec_client::protocol::PROTOCOL_VERSION,",
				"\t});",
				"}",
				"",
			].join("\n"),
		);

		assert.deepEqual(checkAgentOsClientProtocolCompat({ root }), [
			"crates/client/src/sidecar.rs:1:5 imports the live protocol compatibility surface; use secure_exec_client::wire for generated wire types or add this file to the migration inventory with justification",
			"crates/client/src/sidecar.rs:5:21 imports the live protocol compatibility surface; use secure_exec_client::wire for generated wire types or add this file to the migration inventory with justification",
			"crates/client/src/sidecar.rs must import secure_exec_client::wire",
			"crates/client/src/sidecar.rs authenticate request must use wire::PROTOCOL_VERSION",
		]);
	});
});

test("rejects default frame limit regressions to the compatibility protocol surface", () => {
	withFixture((root) => {
		writeSidecar(root);
		write(
			root,
			"crates/client/src/net.rs",
			[
				"const LIMIT: usize = secure_exec_client::protocol::DEFAULT_MAX_FRAME_BYTES;",
				"",
			].join("\n"),
		);

		assert.deepEqual(checkAgentOsClientProtocolCompat({ root }), [
			"crates/client/src/net.rs:1:22 reads the default frame limit through the compatibility protocol surface; use secure_exec_client::wire::DEFAULT_MAX_FRAME_BYTES",
			"crates/client/src/net.rs:1:22 imports the live protocol compatibility surface; use secure_exec_client::wire for generated wire types or add this file to the migration inventory with justification",
		]);
	});
});
