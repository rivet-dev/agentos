import { allowAll, createBrowserDriver, createBrowserRuntimeDriverFactory, } from "@rivet-dev/agentos-browser";
const runtimes = new Map();
const statusElement = document.querySelector("#harness-status");
const workerUrl = new URL("/agentos-worker.js", window.location.origin);
const runtimeFactory = createBrowserRuntimeDriverFactory({ workerUrl });
function setStatus(state, message) {
    if (!statusElement) {
        return;
    }
    statusElement.dataset.state = state;
    statusElement.textContent = message;
}
function requireRuntime(runtimeId) {
    const entry = runtimes.get(runtimeId);
    if (!entry) {
        throw new Error(`Unknown browser harness runtime: ${runtimeId}`);
    }
    return entry;
}
function takeStdio(entry) {
    const stdio = [...entry.stdio];
    entry.stdio.length = 0;
    return stdio;
}
function getRuntimeDebugState(runtime) {
    const internal = runtime;
    return {
        disposed: internal.disposed === true,
        pendingCount: internal.pending?.size ?? 0,
        signalState: internal.syncBridge
            ? Array.from(new Int32Array(internal.syncBridge.signalBuffer))
            : [],
        workerOnmessage: internal.worker?.onmessage === null ? "null" : "set",
        workerOnerror: internal.worker?.onerror === null ? "null" : "set",
    };
}
const harness = {
    async createRuntime(options) {
        const system = await createBrowserDriver({
            filesystem: options?.filesystem ?? "memory",
            permissions: allowAll,
            useDefaultNetwork: options?.useDefaultNetwork,
        });
        const stdio = [];
        const runtime = runtimeFactory.createRuntimeDriver({
            system,
            runtime: system.runtime,
            onStdio: (event) => {
                stdio.push({
                    channel: event.channel,
                    message: event.message,
                });
            },
            timingMitigation: options?.timingMitigation,
            payloadLimits: options?.payloadLimits,
        });
        const runtimeId = globalThis.crypto.randomUUID();
        runtimes.set(runtimeId, {
            runtime,
            stdio,
        });
        return {
            crossOriginIsolated: window.crossOriginIsolated,
            runtimeId,
            workerUrl: workerUrl.href,
        };
    },
    async exec(runtimeId, code, options) {
        const entry = requireRuntime(runtimeId);
        entry.stdio.length = 0;
        const result = await entry.runtime.exec(code, options);
        return {
            crossOriginIsolated: window.crossOriginIsolated,
            result,
            stdio: takeStdio(entry),
        };
    },
    async disposeRuntime(runtimeId) {
        const entry = requireRuntime(runtimeId);
        runtimes.delete(runtimeId);
        if (typeof entry.runtime.terminate === "function") {
            await entry.runtime.terminate();
            return;
        }
        entry.runtime.dispose();
    },
    async disposeAllRuntimes() {
        const runtimeEntries = Array.from(runtimes.entries());
        runtimes.clear();
        for (const [, entry] of runtimeEntries) {
            try {
                if (typeof entry.runtime.terminate === "function") {
                    await entry.runtime.terminate();
                }
                else {
                    entry.runtime.dispose();
                }
            }
            catch {
                entry.runtime.dispose();
            }
        }
    },
    async terminatePendingExec(runtimeId, code, delayMs = 20) {
        const entry = requireRuntime(runtimeId);
        entry.stdio.length = 0;
        const execution = entry.runtime.exec(code);
        await new Promise((resolve) => setTimeout(resolve, delayMs));
        if (typeof entry.runtime.terminate === "function") {
            await entry.runtime.terminate();
        }
        else {
            entry.runtime.dispose();
        }
        let outcome = "resolved";
        let resultCode = null;
        let errorMessage = null;
        try {
            const result = await execution;
            resultCode = result.code;
        }
        catch (error) {
            outcome = "rejected";
            errorMessage = error instanceof Error ? error.message : String(error);
        }
        runtimes.delete(runtimeId);
        return {
            outcome,
            resultCode,
            errorMessage,
            debug: getRuntimeDebugState(entry.runtime),
        };
    },
    async dispatchExtensionRequest(runtimeId, namespace, payload) {
        const entry = requireRuntime(runtimeId);
        const browserRuntime = entry.runtime;
        try {
            const response = await browserRuntime.dispatchExtensionRequest(namespace, new Uint8Array(payload));
            return {
                ok: true,
                namespace,
                payload: Array.from(response),
            };
        }
        catch (error) {
            const typedError = error;
            return {
                ok: false,
                errorMessage: typedError.message ?? String(error),
                errorCode: typedError.code,
            };
        }
    },
    async smoke() {
        const { runtimeId } = await harness.createRuntime();
        try {
            const response = await harness.exec(runtimeId, 'console.log("harness-ready");');
            return {
                ...response,
                workerUrl: workerUrl.href,
            };
        }
        finally {
            await harness.disposeRuntime(runtimeId);
        }
    },
};
window.__agentOsBrowserHarness = harness;
setStatus("ready", "ready");
