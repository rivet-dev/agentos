// Browser-bundle entry for the pi ACP adapter. The adapter normally loads its SDK via
// computed dynamic import() from a VM-mounted node_modules; in the browser converged
// executor there is no such mount, so we statically import the SDK submodules (which
// esbuild bundles into a single self-contained file) and publish the same
// `__PI_SDK_RUNTIME__` object used by the native V8 startup snapshot. The adapter is
// otherwise unchanged — same ACP behavior, just bundled instead of VFS-resolved.
//
// Imports use relative node_modules file paths (not bare package subpaths) so esbuild
// resolves the dist files directly, bypassing the packages' restrictive `exports`.
import * as agentCore from "./node_modules/@mariozechner/pi-agent-core/dist/index.js";
import * as authStorage from "./node_modules/@mariozechner/pi-coding-agent/dist/core/auth-storage.js";
import * as config from "./node_modules/@mariozechner/pi-coding-agent/dist/config.js";
import * as defaults from "./node_modules/@mariozechner/pi-coding-agent/dist/core/defaults.js";
import * as messages from "./node_modules/@mariozechner/pi-coding-agent/dist/core/messages.js";
import * as modelRegistry from "./node_modules/@mariozechner/pi-coding-agent/dist/core/model-registry.js";
import * as resourceLoader from "./node_modules/@mariozechner/pi-coding-agent/dist/core/resource-loader.js";
import * as sdk from "./node_modules/@mariozechner/pi-coding-agent/dist/core/sdk.js";
import * as sessionManager from "./node_modules/@mariozechner/pi-coding-agent/dist/core/session-manager.js";
import * as settingsManager from "./node_modules/@mariozechner/pi-coding-agent/dist/core/settings-manager.js";
import * as tools from "./node_modules/@mariozechner/pi-coding-agent/dist/core/tools/index.js";

// Keep this shape identical to `runtime` in src/snapshot-entry.ts. The adapter reads
// it lazily at session/new, so setting it after the adapter import (ESM-hoisted) is
// safe.
globalThis.__PI_SDK_RUNTIME__ = {
	Agent: agentCore.Agent,
	AuthStorage: authStorage.AuthStorage,
	DefaultResourceLoader: resourceLoader.DefaultResourceLoader,
	DEFAULT_THINKING_LEVEL: defaults.DEFAULT_THINKING_LEVEL,
	ModelRegistry: modelRegistry.ModelRegistry,
	SettingsManager: settingsManager.SettingsManager,
	SessionManager: sessionManager.SessionManager,
	convertToLlm: messages.convertToLlm,
	getAgentDir: config.getAgentDir,
	getDocsPath: config.getDocsPath,
	createAgentSession: sdk.createAgentSession,
	createCodingTools: sdk.createCodingTools,
	createAllTools: tools.createAllTools,
};

import "./dist/adapter.js";
