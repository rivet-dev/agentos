import { spawnSync } from "node:child_process";
import { expect, test } from "vitest";
import { AgentOs } from "../src/index.js";

async function guest(source: string, preload = false) {
	const sidecar = await AgentOs.createSidecar();
	let vm: AgentOs | undefined;
	try {
		vm = await AgentOs.create({
			sidecar: { kind: "explicit", handle: sidecar },
			defaultSoftware: false,
			permissions: { network: "deny" },
			...(preload
				? { environment: { NODE_OPTIONS: '--require "/tmp/preload one.cjs"' } }
				: {}),
		});
		if (preload)
			await vm.filesystem.writeFile(
				"/tmp/preload one.cjs",
				"globalThis.preloadValue=42;",
			);
		let stdout = "",
			stderr = "";
		const p = await vm.process.spawn("node", ["-e", source], {
			timeoutMs: 10000,
			onStdout: (b) => (stdout += Buffer.from(b).toString()),
			onStderr: (b) => (stderr += Buffer.from(b).toString()),
		});
		expect((await vm.process.wait(p.pid)).exitCode, stderr).toBe(0);
		return stdout;
	} finally {
		await vm?.dispose();
		await sidecar.dispose();
	}
}
test("require and require.resolve consult an installed Module resolution hook", async () => {
	const source = `const M=require('node:module');const original=M._resolveFilename;M._resolveFilename=function(request,...args){if(request==='synthetic-hook-target')throw Object.assign(Error('hook rejection'),{code:'HOOK_REJECT'});return Reflect.apply(original,this,[request,...args]);};const out=[];for(const fn of [()=>require('synthetic-hook-target'),()=>require.resolve('synthetic-hook-target')]){try{fn();}catch(e){out.push(e.code);}}console.log(JSON.stringify(out));`;
	const host = spawnSync(process.execPath, ["-e", source], {
		encoding: "utf8",
		timeout: 10000,
	});
	expect(host.status, host.stderr).toBe(0);
	expect(await guest(source)).toBe(host.stdout);
}, 15000);
test("NODE_OPTIONS runs a quoted CommonJS preload before the entry module", async () => {
	expect(await guest("console.log(globalThis.preloadValue)", true)).toBe(
		"42\n",
	);
}, 15000);

test("CommonJS file and preload preserve the builtin gate", async () => {
	const vm = await AgentOs.create({
		defaultSoftware: false,
		allowedNodeBuiltins: ["path"],
		environment: { NODE_OPTIONS: "--require /tmp/gated-preload.cjs" },
	});
	try {
		await vm.filesystem.writeFile(
			"/tmp/gated-preload.cjs",
			`const requireFrom = globalThis.__agentOsGuestCreateRequire(__filename);
			let denied;
			try { requireFrom("node:module"); } catch (error) { denied = error.code; }
			globalThis.preloadResult = [
				denied,
				Reflect.set(globalThis, "__agentOsGuestCreateRequire", () => () => null),
				typeof globalThis._moduleModule
			];`,
		);
		await vm.filesystem.writeFile(
			"/tmp/gated-main.cjs",
			`console.log(JSON.stringify([
				globalThis.preloadResult,
				require("node:path").basename(__filename),
				require.main === module,
				process.mainModule === module
			]));`,
		);
		const result = await vm.process.execFile("node", ["/tmp/gated-main.cjs"], {
			timeoutMs: 3_000,
			output: { capture: "all" },
		});
		expect(result.exitCode, result.stderr).toBe(0);
		expect(JSON.parse(result.stdout)).toEqual([
			["ERR_ACCESS_DENIED", false, "undefined"],
			"gated-main.cjs",
			true,
			true,
		]);
	} finally {
		await vm.dispose();
	}
}, 10_000);

test("CommonJS retained calls preserve the builtin gate and context", async () => {
	const vm = await AgentOs.create({
		defaultSoftware: false,
		allowedNodeBuiltins: ["path"],
	});
	try {
		await vm.createContext("gated-commonjs");
		for (const count of [1, 2]) {
			const result = await vm.javascript.execute(
				`{
					let denied;
					try { require("node:module"); } catch (error) { denied = error.code; }
					globalThis.gatedCount = (globalThis.gatedCount ?? 0) + 1;
					console.log(JSON.stringify([
						denied,
						typeof globalThis._moduleModule,
						require("node:path").basename("/tmp/gated-main.cjs"),
						globalThis.gatedCount
					]));
				}`,
				{
					contextId: "gated-commonjs",
					format: "commonjs",
					timeoutMs: 2_000,
					output: { capture: "all" },
				},
			);
			expect(result.outcome, JSON.stringify(result)).toBe("succeeded");
			expect(JSON.parse(result.stdout ?? "")).toEqual([
				"ERR_ACCESS_DENIED",
				"undefined",
				"gated-main.cjs",
				count,
			]);
		}
	} finally {
		await vm.dispose();
	}
}, 10_000);
