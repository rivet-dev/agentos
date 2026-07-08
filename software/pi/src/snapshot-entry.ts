/**
 * Pi SDK snapshot entry (Step 2a/2c — eval/entry split).
 *
 * This module is bundled by esbuild into a single IIFE (`dist/pi-sdk-snapshot.js`)
 * whose ONLY job is to evaluate the Pi SDK module graph and publish its exports on
 * a well-known global, `globalThis.__PI_SDK_RUNTIME__`. The bundle is run once at V8
 * snapshot-creation time; the evaluated SDK heap is captured into the startup blob
 * and reused to seed every fresh per-session isolate. After restore the adapter
 * reads the global instead of dynamically importing the SDK from the guest VFS,
 * collapsing the ~830ms per-session module-load/eval tax.
 *
 * INVARIANTS (enforced by the C0 snapshot-safety scan — see
 * ~/.agents/research/pi-snapshot-safety-scan.md):
 *  - This entry performs NO per-session work: it must not read cwd, model,
 *    ANTHROPIC env vars, HOME, argv, open fds/sockets, or start timers. It only
 *    binds the SDK's static exports. All per-session configuration is injected
 *    post-restore by the adapter (newSession), exactly as before.
 *  - Heavy provider SDKs (@anthropic-ai/sdk, openai, …) are loaded by the SDK via
 *    dynamic import() and are intentionally NOT part of this static graph; they
 *    stay lazy and load post-restore from the VFS on first prompt.
 *  - The bundle is keyed by the sha256 of its own (fully inlined) bytes, so any SDK
 *    dependency change invalidates the snapshot.
 */

import { Agent } from "@mariozechner/pi-agent-core";
import { AuthStorage } from "@mariozechner/pi-coding-agent/dist/core/auth-storage.js";
import {
	getAgentDir,
	getDocsPath,
} from "@mariozechner/pi-coding-agent/dist/config.js";
import { DEFAULT_THINKING_LEVEL } from "@mariozechner/pi-coding-agent/dist/core/defaults.js";
import { convertToLlm } from "@mariozechner/pi-coding-agent/dist/core/messages.js";
import { ModelRegistry } from "@mariozechner/pi-coding-agent/dist/core/model-registry.js";
import { DefaultResourceLoader } from "@mariozechner/pi-coding-agent/dist/core/resource-loader.js";
import {
	createAgentSession,
	createCodingTools,
} from "@mariozechner/pi-coding-agent/dist/core/sdk.js";
import { SessionManager } from "@mariozechner/pi-coding-agent/dist/core/session-manager.js";
import { SettingsManager } from "@mariozechner/pi-coding-agent/dist/core/settings-manager.js";
import { createAllTools } from "@mariozechner/pi-coding-agent/dist/core/tools/index.js";

// The shape mirrors `PiSdkRuntime` in adapter.ts so the adapter can read the
// global with no behavioral difference from the dynamic-import path.
const runtime = {
	Agent,
	AuthStorage,
	DefaultResourceLoader,
	DEFAULT_THINKING_LEVEL,
	ModelRegistry,
	SettingsManager,
	SessionManager,
	convertToLlm,
	getAgentDir,
	getDocsPath,
	createAgentSession,
	createCodingTools,
	createAllTools,
};

// Publish on the global so it survives into every fresh context cloned from the
// snapshotted default context.
(globalThis as Record<string, unknown>).__PI_SDK_RUNTIME__ = runtime;
