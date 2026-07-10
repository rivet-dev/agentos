// One matrix cell, run as a STANDALONE node process inside a freshly-installed
// temp project (so it exercises the published packages exactly as a user would).
//
// Resolves @rivet-dev/agentos-core + the agent's @agentos-software/* package from
// the temp project's own node_modules, opens a session, sends a prompt, and asserts
// that tokens stream LIVE mid-turn (the ACP streaming contract) — then prints a
// single `E2E_RESULT_JSON:{...}` line and exits 0 on PASS.
//
// Driven by env: AGENT (pi|pi-cli|claude|opencode), ANTHROPIC_API_KEY,
// AGENTOS_MATRIX_MODEL (opencode model id; must be a CURRENT id).

import { join } from "node:path";
import { pathToFileURL } from "node:url";

const AGENT = process.env.AGENT || "pi";
const ANTHROPIC_API_KEY = process.env.ANTHROPIC_API_KEY;
const ANTHROPIC_BASE_URL =
	process.env.ANTHROPIC_BASE_URL || "https://api.anthropic.com";
// OpenCode pins an explicit model; a retired id 404s and the turn ends empty, so
// this is intentionally configurable and defaults to a current model.
const OPENCODE_MODEL =
	process.env.AGENTOS_MATRIX_MODEL || "anthropic/claude-haiku-4-5-20251001";

const PKG = {
	pi: "@agentos-software/pi",
	"pi-cli": "@agentos-software/pi-cli",
	claude: "@agentos-software/claude-code",
	opencode: "@agentos-software/opencode",
}[AGENT];
if (!PKG) throw new Error(`unknown AGENT ${AGENT}`);

const REPO_ROOT = process.env.AGENTOS_MATRIX_REPO_ROOT;
const localAgentDirs = {
	pi: "pi",
	"pi-cli": "pi-cli",
	claude: "claude",
	opencode: "opencode",
};
const importTarget = (path) => pathToFileURL(path).href;
const coreModule = REPO_ROOT
	? await import(importTarget(join(REPO_ROOT, "packages/core/dist/index.js")))
	: await import("@rivet-dev/agentos-core");
const agentModule = REPO_ROOT
	? await import(
			importTarget(
				join(REPO_ROOT, "registry/agent", localAgentDirs[AGENT], "dist/index.js"),
			),
		)
	: await import(PKG);
const { AgentOs } = coreModule;
const software = agentModule.default;

const result = {
	agent: AGENT,
	pkg: PKG,
	ok: false,
	streaming: false,
	nativeResume: false,
	transcriptRestore: false,
	error: null,
};

let vm;
let sessionId;
try {
	vm = await AgentOs.create({
		software: [software],
		// Real LLM egress needs network; the secure baseline denies it by default.
		// Keys are fs/network/childProcess/process/env (NOT filesystem/environment).
		permissions: {
			fs: "allow",
			network: "allow",
			childProcess: "allow",
			process: "allow",
			env: "allow",
		},
	});

	const homeDir = "/home/agentos";
	const env = { HOME: homeDir };
	if (ANTHROPIC_API_KEY) {
		env.ANTHROPIC_API_KEY = ANTHROPIC_API_KEY;
		env.ANTHROPIC_BASE_URL = ANTHROPIC_BASE_URL;
	}

	// OpenCode has no built-in default model/provider: write its config FIRST or the
	// prompt resolves empty. The Anthropic baseURL MUST end in /v1 (else 404).
	if (AGENT === "opencode") {
		await vm.mkdir(`${homeDir}/.config/opencode`, { recursive: true });
		await vm.writeFile(
			`${homeDir}/.config/opencode/opencode.json`,
			JSON.stringify({
				$schema: "https://opencode.ai/config.json",
				autoupdate: false,
				share: "disabled",
				snapshot: false,
				model: OPENCODE_MODEL,
				provider: {
					anthropic: { options: { baseURL: `${ANTHROPIC_BASE_URL}/v1` } },
				},
			}),
		);
	}

	// pi/pi-cli read provider config from ~/.pi/agent/models.json
	if (AGENT === "pi" || AGENT === "pi-cli") {
		await vm.mkdir(`${homeDir}/.pi/agent`, { recursive: true });
		await vm.writeFile(
			`${homeDir}/.pi/agent/models.json`,
			JSON.stringify({
				providers: {
					anthropic: { baseUrl: ANTHROPIC_BASE_URL, apiKey: ANTHROPIC_API_KEY },
				},
			}),
		);
	}

	// OpenCode (and others) need an existing cwd; the default /workspace may not exist.
	const workspaceDir = `${homeDir}/workspace`;
	await vm.mkdir(workspaceDir, { recursive: true });

	// ACP bootstrap can flake; retry a couple times before declaring a failure.
	let created;
	for (let attempt = 1; attempt <= 3; attempt++) {
		try {
			created = await vm.createSession(AGENT, { cwd: workspaceDir, env });
			break;
		} catch (err) {
			if (attempt === 3) throw err;
		}
	}
	sessionId = created.sessionId;

	const events = [];
	let promptStart = 0;
	vm.onSessionEvent(sessionId, (event) => {
		events.push({
			method: event.method,
			kind: event.params?.update?.sessionUpdate,
			t: performance.now() - promptStart,
		});
	});

	promptStart = performance.now();
	const nativeToken = `native-${AGENT}-2718`;
	const { text, response } = await vm.prompt(
		sessionId,
		`Remember the token ${nativeToken}. Then write a haiku about the ocean. Output only the haiku.`,
	);
	const resolvedAt = performance.now() - promptStart;

	const updates = events.filter((e) => e.method === "session/update");
	const chunks = updates.filter(
		(e) => e.kind === "agent_message_chunk" || e.kind === "agent_thought_chunk",
	);
	const firstChunk = chunks.length ? chunks[0].t : NaN;
	const lastChunk = chunks.length ? chunks[chunks.length - 1].t : NaN;
	const chunksBeforeResolve = chunks.filter((e) => e.t < resolvedAt - 50).length;
	const span = lastChunk - firstChunk;
	const gap = resolvedAt - firstChunk; // live-delivery signal (the ACP fix)

	// Streaming contract: >=2 text chunks delivered LIVE mid-turn, not batched at
	// prompt resolution. The batching bug clusters EVERY chunk at the resolve
	// instant (firstChunk == resolve, so gap ~= 0). Live delivery puts the first
	// chunk meaningfully before resolve. `gap > 100` cleanly separates the two
	// without false-failing agents (e.g. opencode) that emit a tight, short burst
	// on a fast turn — those still arrive hundreds of ms before resolve. `span`
	// is kept only as an informational metric, not a pass condition.
	const streaming =
		chunks.length >= 2 && chunksBeforeResolve >= 2 && gap > 100;

	result.ok = !response?.error && (text || "").length > 0;
	result.streaming = streaming;

	vm.closeSession(sessionId);
	await vm._sessionClosePromises?.get(sessionId);
	const native = await vm.resumeSession(sessionId, AGENT, {
		cwd: workspaceDir,
		env,
		transcriptPath: `/root/.agentos/threads/${sessionId}.md`,
	});
	const nativeReply = await vm.prompt(
		native.sessionId,
		"What token did I ask you to remember? Output only the token.",
	);
	result.nativeResume =
		native.mode === "native" &&
		native.sessionId === sessionId &&
		nativeReply.text.includes(nativeToken);

	vm.closeSession(native.sessionId);
	await vm._sessionClosePromises?.get(native.sessionId);
	const missingSessionId = `00000000-0000-4000-8000-${
		{
			pi: "000000000041",
			"pi-cli": "000000000042",
			claude: "000000000043",
			opencode: "000000000044",
		}[AGENT]
	}`;
	const restoreToken = `restore-${AGENT}-3141`;
	const transcriptDir = `${workspaceDir}/.agentos/threads`;
	const transcriptPath = `${transcriptDir}/${missingSessionId}.md`;
	await vm.mkdir(transcriptDir, { recursive: true });
	await vm.writeFile(
		transcriptPath,
		`# Earlier conversation\n\nUser asked the agent to remember ${restoreToken}.`,
	);
	const restored = await vm.resumeSession(missingSessionId, AGENT, {
		cwd: workspaceDir,
		env,
		transcriptPath,
	});
	sessionId = restored.sessionId;
	const restoredReply = await vm.prompt(
		restored.sessionId,
		`Use your file-reading tool to read ${transcriptPath}, then output only its restore token. Do not describe what you will do.`,
	);
	result.transcriptRestore =
		restored.mode === "fallback" && restoredReply.text.includes(restoreToken);
	result.metrics = {
		resolvedAt: Math.round(resolvedAt),
		totalUpdates: updates.length,
		chunks: chunks.length,
		chunksBeforeResolve,
		firstChunkAt: Math.round(firstChunk),
		lastChunkAt: Math.round(lastChunk),
		spanMs: Math.round(span),
		gapMs: Math.round(gap),
		textLen: (text || "").length,
		textSample: (text || "").slice(0, 80),
		nativeResumeMode: native.mode,
		nativeReply: nativeReply.text.slice(0, 80),
		restoreMode: restored.mode,
		restoreReply: restoredReply.text.slice(0, 80),
	};
} catch (err) {
	result.error = String(err?.stack || err);
} finally {
	try {
		if (sessionId) vm?.closeSession(sessionId);
	} catch {}
	try {
		await vm?.dispose();
	} catch {}
}

console.log("E2E_RESULT_JSON:" + JSON.stringify(result));
process.exit(
	result.ok && result.streaming && result.nativeResume && result.transcriptRestore
		? 0
		: 1,
);
