import { expect, test } from "vitest";
import { AgentOs } from "../src/index.js";

// Run this file alone with AGENTOS_TEST_HANDLE_METADATA=1 after rebuilding the
// native sidecar. Host environment is process-global and precedes sidecar spawn.
const acceptanceTest = test.runIf(
	process.env.AGENTOS_TEST_HANDLE_METADATA === "1",
);
const limitNames = [
	"AGENTOS_JS_MAX_HANDLE_DESCRIPTIONS",
	"AGENTOS_JS_MAX_HANDLE_DESCRIPTION_BYTES",
] as const;

async function runWithHostLimits(
	count: string,
	bytes: string,
	code: string,
	expectedWarnings: readonly string[] = [],
) {
	// Keep warning capture bounded to the expected names, even if other limits
	// produce repeated events. Validate delivery while the VM is still alive.
	const limitWarnings = new Set<string>();
	const previous = limitNames.map((name) => process.env[name]);
	process.env[limitNames[0]] = count;
	process.env[limitNames[1]] = bytes;
	let sidecar: Awaited<ReturnType<typeof AgentOs.createSidecar>> | undefined;
	try {
		sidecar = await AgentOs.createSidecar();
		const vm = await AgentOs.create({
			sidecar: { kind: "explicit", handle: sidecar },
			defaultSoftware: false,
			onLimitWarning: (warning) => {
				if (expectedWarnings.includes(warning.limit))
					limitWarnings.add(warning.limit);
			},
			permissions: { network: "deny", env: "allow" },
			// Explicit guest bootstrap overrides must never set trusted host caps.
			environment: Object.fromEntries(
				limitNames.map((name) => [name, "99999999"]),
			),
		});
		try {
			const result = await vm.javascript.execute(
				`
				process.env.AGENTOS_JS_MAX_HANDLE_DESCRIPTIONS = "99999999";
				process.env.AGENTOS_JS_MAX_HANDLE_DESCRIPTION_BYTES = "99999999";
				const call = (method, args) => JSON.parse(globalThis._loadPolyfill.applySyncPromise(undefined, ["__bd:" + method + ":" + JSON.stringify(args)]));
				const check = (ok, message) => { if (!ok) throw Error(message); };
				${code}
				console.log("bounded");
			`,
				{ timeoutMs: 5_000, output: { capture: "all" } },
			);
			expect(result.outcome, JSON.stringify(result)).toBe("succeeded");
			expect(result.stdout).toBe("bounded\n");
			if (expectedWarnings.length > 0) {
				await expect
					.poll(() => [...limitWarnings], { timeout: 2_000 })
					.toEqual(expect.arrayContaining(expectedWarnings));
			}
		} finally {
			await vm.dispose();
		}
	} finally {
		try {
			await sidecar?.dispose();
		} finally {
			limitNames.forEach((name, index) => {
				const value = previous[index];
				if (value === undefined) delete process.env[name];
				else process.env[name] = value;
			});
		}
	}
}

acceptanceTest(
	"guest direct calls and wrappers obey host count and byte caps despite guest env overrides",
	async () => {
		await runWithHostLimits(
			"2",
			"32",
			`
		const activeBefore = globalThis._getActiveHandles().length;
		for (const id of ["a", "b"]) check(!call("kernelHandleRegister", [id, "ok"]).__bd_error, "early rejection");
		let rejected = call("kernelHandleRegister", ["c", "ok"]).__bd_error;
		check(rejected?.code === "ERR_AGENTOS_RESOURCE_LIMIT" && rejected.message.includes("AGENTOS_JS_MAX_HANDLE_DESCRIPTIONS=2"), "count bypass");
		check(!call("kernelHandleRegister", ["a", "x".repeat(28)]).__bd_error, "exact bytes rejected");
		rejected = call("kernelHandleRegister", ["a", "x".repeat(29)]).__bd_error;
		check(rejected?.code === "ERR_AGENTOS_RESOURCE_LIMIT" && rejected.message.includes("AGENTOS_JS_MAX_HANDLE_DESCRIPTION_BYTES=32"), "bytes bypass");
		const listed = call("kernelHandleList", []).__bd_result;
		check(listed.length === 2 && listed.find(entry => entry.id === "a").description.length === 28, "rejection mutated map");
		let wrapperError;
		try { globalThis._registerHandle("c", "ok"); } catch (error) { wrapperError = error; }
		check(wrapperError?.code === "ERR_AGENTOS_RESOURCE_LIMIT", "wrapper lost typed error");
		check(globalThis._getActiveHandles().length === activeBefore, "wrapper retained rejected handle");
		call("kernelHandleUnregister", ["a"]);
		call("kernelHandleUnregister", ["b"]);
		check(!call("kernelHandleRegister", ["c", "x".repeat(31)]).__bd_error, "unregister did not release bytes");
		call("kernelHandleUnregister", ["c"]);
	`,
			["javascript_handle_descriptions", "javascript_handle_description_bytes"],
		);
	},
	30_000,
);

acceptanceTest(
	"reachable escaped handle list fails before oversized response construction",
	async () => {
		await runWithHostLimits(
			"128",
			"1048576",
			`
		const description = String.fromCharCode(0).repeat(10000);
		for (let id = 0; id < 70; id++) check(!call("kernelHandleRegister", ["h" + id, description]).__bd_error, "early list fixture rejection");
		check(call("kernelHandleList", []).__bd_error?.code === "ERR_AGENTOS_BRIDGE_RESPONSE_LIMIT", "list preflight bypass");
		for (let id = 0; id < 70; id++) call("kernelHandleUnregister", ["h" + id]);
		check(call("kernelHandleList", []).__bd_result.length === 0, "list rejection prevented cleanup");
	`,
		);
	},
	30_000,
);
