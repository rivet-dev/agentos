import { expect, test } from "@playwright/test";
import {
	createRuntime,
	dispatchExtensionRequest,
	disposeAllRuntimes,
	execRuntime,
	getLastStdioMessage,
	openHarnessPage,
	terminatePendingExec,
} from "./harness.js";

test.beforeEach(async ({ page }) => {
	await openHarnessPage(page);
});

test.afterEach(async ({ page }) => {
	await disposeAllRuntimes(page);
});

test("routes extension control messages through browser worker postMessage", async ({
	page,
}) => {
	const { runtimeId } = await createRuntime(page);

	const response = await dispatchExtensionRequest(
		page,
		runtimeId,
		"dev.secure-exec.browser-extension-smoke",
		[112, 105, 110, 103],
	);

	if (response.ok) {
		throw new Error("extension dispatch unexpectedly succeeded");
	}
	expect(response.errorCode).toBe(
		"ERR_SECURE_EXEC_BROWSER_EXTENSION_UNSUPPORTED",
	);
	expect(response.errorMessage).toContain(
		"Browser worker extension dispatch is not implemented for namespace dev.secure-exec.browser-extension-smoke",
	);
});

test("preserves sync filesystem and module loading parity in a real Chromium worker", async ({
	page,
}) => {
	const { runtimeId, workerUrl, crossOriginIsolated } =
		await createRuntime(page);

	expect(crossOriginIsolated).toBe(true);
	expect(workerUrl).toContain("/agent-os-worker.js");

	const filesystemRoundTrip = await execRuntime(
		page,
		runtimeId,
		`
			const fs = require("fs");
			fs.mkdirSync("/workspace");
			fs.writeFileSync("/workspace/hello.txt", "hello");
			fs.writeFileSync("/workspace/helper.js", "module.exports = { value: 42 };");
			const text = fs.readFileSync("/workspace/hello.txt", "utf8");
			const stat = fs.statSync("/workspace/hello.txt");
			console.log(text + ":" + stat.size);
		`,
	);

	expect(filesystemRoundTrip.result.code).toBe(0);
	expect(filesystemRoundTrip.stdio).toContainEqual({
		channel: "stdout",
		message: "hello:5",
	});

	const moduleRoundTrip = await execRuntime(
		page,
		runtimeId,
		`
			const fs = require("fs");
			const helper = require("./helper.js");
			console.log(JSON.stringify({
				moduleValue: helper.value,
				fileText: fs.readFileSync("/workspace/hello.txt", "utf8"),
			}));
		`,
		{
			cwd: "/workspace",
			filePath: "/workspace/index.js",
		},
	);

	expect(moduleRoundTrip.result.code).toBe(0);
	expect(JSON.parse(getLastStdioMessage(moduleRoundTrip, "stdout"))).toEqual({
		moduleValue: 42,
		fileText: "hello",
	});
});

test("captures stdio, stdin, exit codes, and runtime errors through the browser harness", async ({
	page,
}) => {
	const { runtimeId } = await createRuntime(page);

	const stdinResult = await execRuntime(
		page,
		runtimeId,
		`
			process.stdin.setEncoding("utf8");
			let stdinText = "";
			process.stdin.on("data", (chunk) => {
				stdinText += chunk;
			});
			process.stdin.on("end", () => {
				console.log("stdin:" + stdinText.trim());
				console.error("stderr:captured");
			});
			process.stdin.resume();
		`,
		{
			stdin: "playwright-input\n",
		},
	);

	expect(stdinResult.crossOriginIsolated).toBe(true);
	expect(stdinResult.result.code).toBe(0);
	expect(stdinResult.stdio).toContainEqual({
		channel: "stdout",
		message: "stdin:playwright-input",
	});
	expect(stdinResult.stdio).toContainEqual({
		channel: "stderr",
		message: "stderr:captured",
	});

	const exitResult = await execRuntime(page, runtimeId, `process.exit(7);`);
	expect(exitResult.result.code).toBe(7);

	const errorResult = await execRuntime(
		page,
		runtimeId,
		`throw new Error("browser-runtime-boom");`,
	);
	expect(errorResult.result.code).toBe(1);
	expect(errorResult.result.errorMessage).toContain("browser-runtime-boom");
});

test("applies frozen time by default and restores live timing when disabled", async ({
	page,
}) => {
	const { runtimeId } = await createRuntime(page);

	const frozen = await execRuntime(
		page,
		runtimeId,
		`
			console.log(JSON.stringify({
				firstDate: Date.now(),
				secondDate: Date.now(),
				firstPerformance: performance.now(),
				secondPerformance: performance.now(),
				frozenDate: new Date().getTime(),
				sharedType: typeof SharedArrayBuffer,
			}));
		`,
	);

	const frozenValues = JSON.parse(getLastStdioMessage(frozen, "stdout")) as {
		firstDate: number;
		secondDate: number;
		firstPerformance: number;
		secondPerformance: number;
		frozenDate: number;
		sharedType: string;
	};
	expect(frozen.result.code).toBe(0);
	expect(frozenValues.firstDate).toBe(frozenValues.secondDate);
	expect(frozenValues.frozenDate).toBe(frozenValues.firstDate);
	expect(frozenValues.firstPerformance).toBe(0);
	expect(frozenValues.secondPerformance).toBe(0);
	expect(frozenValues.sharedType).toBe("undefined");

	const restored = await execRuntime(
		page,
		runtimeId,
		`
			(async () => {
				const startDate = Date.now();
				const startPerformance = performance.now();
				await new Promise((resolve) => setTimeout(resolve, 25));
				const endDate = Date.now();
				const endPerformance = performance.now();
				console.log(JSON.stringify({
					startDate,
					endDate,
					startPerformance,
					endPerformance,
					sharedType: typeof SharedArrayBuffer,
				}));
			})();
		`,
		{
			timingMitigation: "off",
		},
	);

	const restoredValues = JSON.parse(
		getLastStdioMessage(restored, "stdout"),
	) as {
		startDate: number;
		endDate: number;
		startPerformance: number;
		endPerformance: number;
		sharedType: string;
	};
	expect(restored.result.code).toBe(0);
	expect(restoredValues.endDate).toBeGreaterThan(restoredValues.startDate);
	expect(restoredValues.endPerformance).toBeGreaterThan(
		restoredValues.startPerformance,
	);
	expect(restoredValues.sharedType).not.toBe("undefined");
});

test("rejects forged guest control traffic and keeps the runtime usable", async ({
	page,
}) => {
	const { runtimeId } = await createRuntime(page);

	const forgedMessageAttempt = await execRuntime(
		page,
		runtimeId,
		`
			(async () => {
				const rawPostMessageType = typeof _realPostMessage;
				await self.onmessage({
					data: {
						id: 999,
						type: "dispose",
					},
				});
				console.log(JSON.stringify({
					rawPostMessageType,
					onmessageType: typeof self.onmessage,
					stillRunning: true,
				}));
			})();
		`,
	);

	expect(forgedMessageAttempt.result.code).toBe(0);
	expect(
		JSON.parse(getLastStdioMessage(forgedMessageAttempt, "stdout")),
	).toEqual({
		rawPostMessageType: "undefined",
		onmessageType: "function",
		stillRunning: true,
	});

	const followUp = await execRuntime(
		page,
		runtimeId,
		`console.log("second-pass");`,
	);
	expect(followUp.result.code).toBe(0);
	expect(getLastStdioMessage(followUp, "stdout")).toBe("second-pass");
});

test("hard termination rejects pending work and clears sync bridge state", async ({
	page,
}) => {
	const { runtimeId } = await createRuntime(page);

	const warmup = await execRuntime(page, runtimeId, `console.log("warmup");`);
	expect(warmup.result.code).toBe(0);
	expect(getLastStdioMessage(warmup, "stdout")).toBe("warmup");

	const terminated = await terminatePendingExec(
		page,
		runtimeId,
		`
			(async () => {
				await new Promise(() => undefined);
			})();
		`,
	);

	expect(terminated.outcome).toBe("rejected");
	expect(terminated.errorMessage).toContain("disposed");
	expect(terminated.debug.disposed).toBe(true);
	expect(terminated.debug.pendingCount).toBe(0);
	expect(terminated.debug.signalState).toEqual([0, 0, 0, 0]);
	expect(terminated.debug.workerOnmessage).toBe("null");
	expect(terminated.debug.workerOnerror).toBe("null");
});

// ---------------------------------------------------------------------------
// Security: adversarial tests (security review shard aos-browser).
// Untrusted guest code / peer runtimes try to escape the network gate or read
// another tenant's storage. Each asserts the system DENIES the attack.
// ---------------------------------------------------------------------------

// F-012 (HIGH) — in-VM guest code: ambient `fetch` egress bypass.
// Runtime created with network disabled (useDefaultNetwork omitted => no
// network adapter, deny-all). The worker's deny-list (worker.ts dangerousApis)
// must shadow the ambient WorkerGlobalScope.fetch. The guest MUST NOT be able
// to touch an unmediated `fetch` that bypasses the kernel egress gate. We
// assert that none of `fetch`/`globalThis.fetch`/`self.fetch` is a reachable
// callable. A reachable callable `fetch` is the egress bypass and FAILS.
test("guest cannot reach ambient fetch global to bypass the network gate", async ({
	page,
}) => {
	const { runtimeId } = await createRuntime(page);

	const probe = await execRuntime(
		page,
		runtimeId,
		`
			(() => {
				const report = {};
				// Direct global identifier reference.
				try {
					report.fetchType = typeof fetch;
				} catch (e) {
					report.fetchType = "throw:" + (e && e.message);
				}
				// Via globalThis / self (worker global).
				try {
					report.globalThisFetchType = typeof globalThis.fetch;
				} catch (e) {
					report.globalThisFetchType = "throw:" + (e && e.message);
				}
				try {
					report.selfFetchType = typeof self.fetch;
				} catch (e) {
					report.selfFetchType = "throw:" + (e && e.message);
				}
				console.log(JSON.stringify(report));
			})();
		`,
	);

	expect(probe.result.code).toBe(0);
	const report = JSON.parse(getLastStdioMessage(probe, "stdout")) as {
		fetchType: string;
		globalThisFetchType: string;
		selfFetchType: string;
	};

	// The ambient fetch must be unreachable to guest code when the runtime has
	// no permission-wrapped network adapter. A reachable callable `fetch` is an
	// unmediated egress channel that bypasses the network permission gate.
	const reachable = [
		report.fetchType,
		report.globalThisFetchType,
		report.selfFetchType,
	].filter((value) => value === "function");
	expect(
		reachable,
		`ambient fetch reachable from guest (egress bypass): ${JSON.stringify(report)}`,
	).toEqual([]);
});

// F-015 (HIGH) — peer runtimes, same origin: OPFS cross-tenant storage bleed.
// driver.ts OpfsFileSystem must namespace each runtime under its own per-tenant
// OPFS subdirectory. Runtime A writes /secret.txt; runtime B reads it. B MUST
// get ENOENT. If B reads A's secret, that is a cross-tenant storage leak.
test("two OPFS runtimes do not share storage across tenants", async ({
	page,
}) => {
	const secret = `cross-tenant-${Date.now()}`;
	const secretPath = "/secret.txt";

	const runtimeA = await createRuntime(page, { filesystem: "opfs" });
	const runtimeB = await createRuntime(page, { filesystem: "opfs" });

	const writeResult = await execRuntime(
		page,
		runtimeA.runtimeId,
		`
			const fs = require("fs");
			fs.writeFileSync(${JSON.stringify(secretPath)}, ${JSON.stringify(secret)});
			console.log("wrote");
		`,
	);
	expect(writeResult.result.code).toBe(0);

	const readResult = await execRuntime(
		page,
		runtimeB.runtimeId,
		`
			const fs = require("fs");
			let outcome;
			try {
				outcome = { read: fs.readFileSync(${JSON.stringify(secretPath)}, "utf8") };
			} catch (e) {
				outcome = { error: (e && e.message) || String(e) };
			}
			console.log(JSON.stringify(outcome));
		`,
	);
	expect(readResult.result.code).toBe(0);
	const outcome = JSON.parse(getLastStdioMessage(readResult, "stdout")) as {
		read?: string;
		error?: string;
	};

	expect(
		outcome.read,
		`runtime B read runtime A's secret (cross-tenant OPFS bleed): ${JSON.stringify(outcome)}`,
	).toBeUndefined();
	expect(outcome.error, "expected ENOENT for isolated tenant").toBeTruthy();
	expect(outcome.error).toContain("ENOENT");
});
