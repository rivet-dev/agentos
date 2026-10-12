import { ChildProcess } from "node:child_process";
import { readFileSync } from "node:fs";
import { readFile } from "node:fs/promises";
import { join, resolve } from "node:path";
import { pathToFileURL } from "node:url";

// Observe real Node child exit without replacing any native lifecycle result.
// Delegation uses the original receiver and arguments, and only the exact
// explicitly pinned sidecar executable receives an observational listener.
const originalSpawn = ChildProcess.prototype.spawn;
ChildProcess.prototype.spawn = function (...args) {
	const result = Reflect.apply(originalSpawn, this, args);
	const file = args[0]?.file;
	const pinned = process.env.AGENTOS_SIDECAR_BIN;
	if (typeof file === "string" && pinned && resolve(file) === resolve(pinned)) {
		const pid = this.pid;
		let startTicks;
		try {
			const raw = readFileSync(`/proc/${pid}/stat`, "utf8");
			startTicks = Number(raw.slice(raw.lastIndexOf(")") + 2).split(" ")[19]);
		} catch (error) {
			console.log(JSON.stringify({phase:"child-observation-error", pid, message:String(error)}));
		}
		this.once("exit", (code, signal) => {
			console.log(JSON.stringify({phase:"child-exit", pid, startTicks, code, signal}));
		});
	}
	return result;
};

const [entry, directory, mode] = process.argv.slice(2);
let AgentOs;
let revived = 0;
const send = (phase) => console.log(JSON.stringify({ phase, revived }));
async function gate(name) {
	const deadline = Date.now() + 10_000;
	while (Date.now() < deadline) {
		try {
			await readFile(join(directory, name));
			return;
		} catch (error) {
			if (error.code !== "ENOENT") throw error;
		}
		await new Promise((resolve) => setTimeout(resolve, 10));
	}
	throw new Error(`parent barrier ${name} exceeded 10000ms`);
}
async function create(pool, sandbox) {
	const vm = await AgentOs.create({
		defaultSoftware: false,
		...(sandbox ? { sandbox } : {}),
		database: { type: "sqlite_file", path: join(directory, `${pool}.sqlite`) },
		sidecar: { kind: "shared", pool },
	});
	try {
		const result = await vm.javascript.evaluate("1 + 1");
		if (result.outcome !== "succeeded" || result.value !== 2)
			throw new Error("public evaluation failed");
		return vm;
	} catch (error) {
		try {
			await vm.dispose();
		} catch (cleanup) {
			throw new AggregateError([error, cleanup], "evaluation and cleanup failed");
		}
		throw error;
	}
}
// Real public provider hook: an unresolved setup Promise and unreferenced gate
// must rely on the selected SDK owner, not on this fixture, to keep Node alive.
function providerGate() {
	return new Promise((resolve, reject) => {
		let checks = 0;
		const deadline = Date.now() + 10_000;
		async function check() {
			try {
				await readFile(join(directory, "release-provider"));
				resolve();
				return;
			} catch (error) {
				if (error.code !== "ENOENT") { reject(error); return; }
			}
			if (Date.now() >= deadline) { reject(new Error("provider gate exceeded10000ms")); return; }
			checks += 1;
			if (checks === 3) send("provider-pending");
			setTimeout(() => { void check().catch(reject); }, 10).unref();
		}
		void check().catch(reject);
	});
}
async function providerCase() {
	const primary = new Error("injected public provider setup failure");
	let started = 0;
	let disposed = 0;
	const unexpected = async () => { throw new Error("fixture provider process API must not be invoked"); };
	const provider = {
		async start() {
			started += 1;
			send("provider-entered");
			await providerGate();
			if (mode === "provider-failure") throw primary;
			// No provider filesystem/process operation is requested. Native
			// mounted-plugin construction still follows the real public path.
			return { baseUrl: "http://127.0.0.1:1", dispose() { disposed += 1; },
				runProcess: unexpected, createProcess: unexpected, listProcesses: unexpected,
				stopProcess: unexpected, killProcess: unexpected, getProcessLogs: unexpected,
				sendProcessInput: unexpected };
		},
	};
	if (mode === "provider-failure") {
		let caught = false;
		const unexpected = await create("first", { provider }).then(
			(vm) => ({ vm }),
			(error) => {
				if (error !== primary) throw error;
				caught = true;
				return null;
			},
		);
		if (unexpected) await unexpected.vm.dispose();
		if (!caught || started !== 1 || disposed !== 0)
			throw new Error("provider failure identity/lifecycle mismatch");
		send("provider-failed");
		return;
	}
	const vm = await create("first", { provider });
	const errors = [];
	try { send("provider-ready"); await gate("release-provider-vm"); }
	catch (error) { errors.push(error); }
	try { await vm.dispose(); } catch (error) { errors.push(error); }
	if (errors.length === 1) throw errors[0];
	if (errors.length > 1) throw new AggregateError(errors, "provider VM body and cleanup failed");
	if (started !== 1 || disposed !== 1) throw new Error("provider successful setup must dispose exactly once");
	send("provider-disposed");
}

function revive() {
	if (revived !== 0) return;
	revived += 1;
	void (async () => {
		const vm = await create("first");
		const errors = [];
		try {
			send("revived-ready");
			await gate("release-revived");
		} catch (error) {
			errors.push(error);
		}
		try {
			await vm.dispose();
		} catch (error) {
			errors.push(error);
		}
		if (errors.length === 1) throw errors[0];
		if (errors.length > 1)
			throw new AggregateError(errors, "revived VM body and cleanup failed");
		send("revived-done");
	})().catch((error) => {
		console.error(error);
		process.exitCode = 1;
	});
}
if (mode === "before") process.on("beforeExit", revive);
({ AgentOs } = await import(pathToFileURL(entry).href));
let first;
let second;
let bodyFailed = false;
let bodyError;
try {
	first = await create("first");
	if (mode === "after") process.on("beforeExit", revive);
	if (mode === "during") process.on("beforeExit", () => {
		if (revived === 0) setImmediate(revive);
	});
	if (mode === "two-pools") second = await create("second");
	send("ready");
	await gate("release-first");
	await first.dispose();
	first = undefined;
	if (second) {
		await second.dispose();
		second = undefined;
	}
	send("idle");
	await gate("release-idle");
	if (mode === "reuse") {
		first = await create("first");
		send("reused-ready");
		await gate("release-reused");
		await first.dispose();
		first = undefined;
	}
	if (mode === "provider-success" || mode === "provider-failure") await providerCase();
	send("done");
} catch (error) {
	bodyFailed = true;
	bodyError = error;
} finally {
	const errors = bodyFailed ? [bodyError] : [];
	for (const owner of [first, second]) {
		if (!owner) continue;
		try {
			await owner.dispose();
		} catch (error) {
			errors.push(error);
		}
	}
	if (errors.length === 1) throw errors[0];
	if (errors.length > 1) throw new AggregateError(errors, "fixture body and cleanup failed");
}
// No process.exit, explicit shared-pool disposal, or signal listener.
