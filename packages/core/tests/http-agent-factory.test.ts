import { spawnSync } from "node:child_process";
import { readFileSync } from "node:fs";
import { runInNewContext } from "node:vm";
import * as ts from "typescript";
import { expect, test } from "vitest";
import { AgentOs } from "../src/index.js";

const program = `
const http = require('node:http');
const modes = ['disabled-success', 'disabled-error', 'factory-error', 'ordinary-success'];
const closeServer = server => server.listening ? new Promise((resolve, reject) => server.close(error => error ? reject(error) : resolve())) : Promise.resolve();
async function capture(mode) {
  const server = http.createServer((request, response) => response.end('ok'));
  await new Promise((resolve, reject) => { server.once('error', reject); server.listen(0, '127.0.0.1', resolve); });
  const port = server.address().port;
  if (mode === 'disabled-error') await closeServer(server);
  const row = { mode, factoryCalls: 0, errors: 0, error: null, status: null, body: '' };
  try {
    return await new Promise((resolve, reject) => {
      const options = { host: '127.0.0.1', port };
      if (mode !== 'factory-error') options.agent = false;
      if (mode !== 'ordinary-success') options.createConnection = () => { row.factoryCalls++; throw new Error('synthetic connection factory failure'); };
      const request = http.get(options, response => {
        row.status = response.statusCode;
        response.setEncoding('utf8');
        response.on('data', chunk => { row.body += chunk; if (row.body.length > 32) { reject(new Error('response bound exceeded')); request.destroy(); } });
        response.once('end', settle);
      });
      const deadline = setTimeout(() => { reject(new Error('request deadline exceeded: ' + mode)); request.destroy(); }, 1500);
      let settling = false;
      function settle() {
        if (settling) return;
        settling = true;
        setImmediate(() => closeServer(server).then(() => { clearTimeout(deadline); resolve(row); }, reject));
      }
      request.on('error', error => { row.errors++; row.error = error.code ?? error.message; settle(); });
    });
  } finally { await closeServer(server); }
}
(async () => { const rows = []; for (const mode of modes) rows.push(await capture(mode)); console.log(JSON.stringify(rows)); })().catch(error => { console.error(error.stack); process.exitCode = 1; });
`;

test("HTTP agent:false ignores request connection factories and preserves error delivery like Node", async () => {
	const native = spawnSync(process.execPath, ["-e", program], {
		encoding: "utf8",
		timeout: 10_000,
		maxBuffer: 4096,
	});
	expect(native.status, native.error?.message ?? native.stderr).toBe(0);
	const expected = JSON.parse(native.stdout);
	expect(expected).toEqual([
		{
			mode: "disabled-success",
			factoryCalls: 0,
			errors: 0,
			error: null,
			status: 200,
			body: "ok",
		},
		{
			mode: "disabled-error",
			factoryCalls: 0,
			errors: 1,
			error: "ECONNREFUSED",
			status: null,
			body: "",
		},
		{
			mode: "factory-error",
			factoryCalls: 1,
			errors: 1,
			error: "synthetic connection factory failure",
			status: null,
			body: "",
		},
		{
			mode: "ordinary-success",
			factoryCalls: 0,
			errors: 0,
			error: null,
			status: 200,
			body: "ok",
		},
	]);
	const vm = await AgentOs.create({
		defaultSoftware: false,
		permissions: {
			fs: "allow",
			process: "allow",
			childProcess: "allow",
			network: "allow",
		},
	});
	try {
		const actual = await vm.process.execFile("node", ["-e", program], {
			timeoutMs: 8_000,
			output: { capture: "all" },
		});
		expect(actual.exitCode, actual.stderr).toBe(0);
		expect(actual.stderr).toBe("");
		expect(typeof actual.stdout).toBe("string");
		expect(Buffer.byteLength(actual.stdout!)).toBeLessThanOrEqual(16_384);
		expect(JSON.parse(actual.stdout!)).toEqual(expected);
	} finally {
		await vm.dispose();
	}
}, 20_000);

function extractedFactoryMethod(
	className = "ClientRequest",
	methodName = "_execute",
) {
	const path =
		process.env.AGENTOS_HTTP_FACTORY_SOURCE ??
		new URL("../../build-tools/bridge-src/builtins/http.ts", import.meta.url);
	const source = ts.createSourceFile(
		"http.ts",
		readFileSync(path, "utf8"),
		ts.ScriptTarget.Latest,
		true,
		ts.ScriptKind.TS,
	);
	let method: ts.MethodDeclaration | undefined;
	function visit(node: ts.Node) {
		if (
			ts.isVariableDeclaration(node) &&
			node.name.getText(source) === className &&
			node.initializer &&
			ts.isClassExpression(node.initializer)
		) {
			method = node.initializer.members.find(
				(member): member is ts.MethodDeclaration =>
					ts.isMethodDeclaration(member) &&
					member.name.getText(source) === methodName,
			);
		}
		ts.forEachChild(node, visit);
	}
	visit(source);
	if (!method) throw new Error(`${className}.${methodName} source missing`);
	const ticks: Array<() => void> = [];
	const execute = runInNewContext(
		`({${method.getText(source)}})[${JSON.stringify(methodName)}]`,
		{
			process: { nextTick: (callback: () => void) => ticks.push(callback) },
			isProcessExitError: (error: { _isProcessExit?: boolean }) =>
				error?._isProcessExit === true,
		},
	);
	return { execute, ticks };
}

test("HTTP Agent actual method preserves listener failures and process exit", () => {
	for (const mode of [
		"returned",
		"sync-callback",
		"async-callback",
		"factory-exit",
	]) {
		const { execute } = extractedFactoryMethod(
			"Agent",
			"_createSocketForRequest",
		);
		const marker = Object.assign(new Error(`agent-${mode}`), {
			_isProcessExit: mode === "factory-exit",
		});
		const socket = { once() {} };
		let callback!: (error: unknown, socket?: unknown) => void;
		let assigned = 0;
		const agent = {
			totalSocketCount: 0,
			_activateSocket() {},
			_processPendingRequests() {},
			createSocket(
				_request: unknown,
				_options: unknown,
				done: typeof callback,
			) {
				callback = done;
				if (mode === "factory-exit") throw marker;
				if (mode === "sync-callback") done(null, socket);
				if (mode === "returned") return socket;
			},
		};
		const request = {
			_assignSocket() {
				assigned++;
				throw marker;
			},
			_handleSocketError() {
				throw new Error(
					"listener failures and process exit are not socket errors",
				);
			},
		};
		const run = () => execute.call(agent, "synthetic", request, {});
		if (mode === "async-callback") {
			run();
			expect(() => callback(null, socket)).toThrow(marker);
		} else {
			expect(run).toThrow(marker);
		}
		expect(assigned).toBe(mode === "factory-exit" ? 0 : 1);
		expect(agent.totalSocketCount).toBe(assigned);
	}
});

test("HTTP Agent socket listener failures match Node error ownership", async () => {
	const vm = await AgentOs.create({
		defaultSoftware: false,
		permissions: { fs: "allow", childProcess: "allow", network: "deny" },
	});
	try {
		for (const mode of [
			"handled",
			"unhandled",
			"handler-throws",
			"handler-exits",
		] as const) {
			const source = `
const http = require('node:http'); const net = require('node:net');
const mode = ${JSON.stringify(mode)};
const marker = new Error('socket-listener-failure');
const deadline = setTimeout(() => { console.log('MISSING'); process.exit(4); }, 100);
process.on('unhandledRejection', () => { console.log('WRONG_OWNER'); process.exit(5); });
if (mode !== 'unhandled') process.on('uncaughtException', (error, origin) => {
  console.log('CAUGHT:' + (error === marker) + ':' + origin);
  if (mode === 'handler-throws') throw Error('fatal-listener-handler');
  clearTimeout(deadline); process.exit(mode === 'handler-exits' ? 9 : 0);
});
const agent = new http.Agent(); agent.createConnection = () => new net.Socket();
const request = http.request({ agent, hostname: '127.0.0.1', port: 1 });
request.on('socket', () => { throw marker; });
`;
			const native = spawnSync(process.execPath, ["-e", source], {
				encoding: "utf8",
				timeout: 2_000,
				maxBuffer: 16_384,
			});
			expect(native.status, native.error?.message ?? native.stderr).toBe(
				{
					handled: 0,
					unhandled: 1,
					"handler-throws": 7,
					"handler-exits": 9,
				}[mode],
			);
			const actual = await vm.process.execFile("node", ["-e", source], {
				timeoutMs: 2_000,
				output: { capture: "all" },
			});
			expect(actual.exitCode, `${mode}: ${actual.stderr}`).toBe(native.status);
			expect(actual.stdout, mode).toBe(native.stdout);
			if (mode === "unhandled")
				expect(actual.stderr).toContain("socket-listener-failure");
			if (mode === "handler-throws")
				expect(actual.stderr).toContain("fatal-listener-handler");
		}
	} finally {
		await vm.dispose();
	}
}, 15_000);

test("HTTP factory actual method preserves deferred errors, once completion and unwind", () => {
	for (const mode of ["async", "sync", "throw"]) {
		const { execute, ticks } = extractedFactoryMethod();
		const marker = new Error(`factory-${mode}`);
		const errors: unknown[] = [];
		let complete!: (error: unknown, socket?: unknown) => void;
		const request = {
			_options: {
				path: "/request-target",
				createConnection(options: { path: string }, callback: typeof complete) {
					options.path = "/transport-target";
					complete = callback;
					if (mode === "sync") callback(marker);
					if (mode === "throw") throw marker;
				},
			},
			_handleSocketError: (error: unknown) => errors.push(error),
			_emitClose: () => errors.push("unexpected premature close"),
			_assignSocket: () => errors.push("unexpected socket"),
		};
		execute.call(request);
		expect(errors).toEqual([]);
		if (mode === "async") complete(marker);
		if (mode !== "throw") complete(new Error("duplicate"));
		expect(ticks).toHaveLength(1);
		ticks.shift()!();
		expect(errors).toEqual([marker]);
		expect(request._options.path).toBe("/request-target");
	}
	const { execute, ticks } = extractedFactoryMethod();
	const exit = Object.assign(new Error("injected process exit unwind"), {
		_isProcessExit: true,
	});
	expect(() =>
		execute.call({
			_options: {
				createConnection() {
					throw exit;
				},
			},
		}),
	).toThrow(exit);
	expect(ticks).toEqual([]);
});

test("HTTP factory actual method assigns one socket and releases late cancelled sockets", () => {
	for (const mode of [
		"returned",
		"callback",
		"callback-and-return",
		"cancelled",
	]) {
		const { execute, ticks } = extractedFactoryMethod();
		let destroyed = 0;
		const socket = {
			destroy() {
				destroyed++;
			},
		};
		const assigned: unknown[] = [];
		let complete!: (error: unknown, socket?: unknown) => void;
		const request = {
			destroyed: false,
			_options: {
				createConnection(_options: unknown, callback: typeof complete) {
					complete = callback;
					if (mode === "callback-and-return") callback(null, socket);
					if (mode === "returned" || mode === "callback-and-return")
						return socket;
				},
			},
			_handleSocketError(error: unknown) {
				throw error;
			},
			_emitClose() {
				throw new Error("premature close");
			},
			_assignSocket: (value: unknown) => assigned.push(value),
		};
		execute.call(request);
		if (mode === "cancelled") request.destroyed = true;
		complete(null, socket);
		complete(null, socket);
		expect(ticks).toHaveLength(1);
		ticks.shift()!();
		expect(assigned).toEqual(mode === "cancelled" ? [] : [socket]);
		expect(destroyed).toBe(mode === "cancelled" ? 1 : 0);
	}
	const { execute, ticks } = extractedFactoryMethod();
	const listenerFailure = new Error("injected socket listener failure");
	execute.call({
		_options: { createConnection: () => ({}) },
		_assignSocket() {
			throw listenerFailure;
		},
	});
	expect(() => ticks.shift()!()).toThrow(listenerFailure);
});

const constructorFactoryProgram = `
const http = require('node:http');
(async () => {
  const rows = [];
  for (const entry of ['constructor', 'request']) {
    for (const mode of ['async', 'sync', 'throw', 'duplicate', 'callback-and-return']) {
      const events = [];
      const marker = Object.assign(new Error('factory-specific'), { code: 'EFACTORY' });
      let complete;
      const options = { hostname: '127.0.0.1', port: 1, path: '/request-target',
        createConnection(options, callback) {
          options.path = '/transport-target';
          complete = callback;
          if (mode === 'throw') throw marker;
          if (mode === 'sync' || mode === 'callback-and-return') callback(marker);
          if (mode === 'callback-and-return') return {};
        } };
      const request = entry === 'constructor' ? new http.ClientRequest(options) : http.request(options);
      request.on('error', error => events.push({ sameError: error === marker, code: error.code, message: error.message }));
      await new Promise(setImmediate);
      const before = events.length;
      if (mode === 'async' || mode === 'duplicate') complete(marker);
      if (mode === 'duplicate') complete(new Error('duplicate error'));
      await new Promise(setImmediate);
      rows.push({ entry, mode, before, events: [...events], path: request.path });
      request.destroy();
      await new Promise(setImmediate);
    }
  }
  console.log(JSON.stringify(rows));
})().catch(error => { console.error(error.stack); process.exitCode = 1; });
`;

function nativeConstructorFactoryRows() {
	const native = spawnSync(
		process.execPath,
		["-e", constructorFactoryProgram],
		{ encoding: "utf8", timeout: 5_000, maxBuffer: 16_384 },
	);
	expect(native.status, native.error?.message ?? native.stderr).toBe(0);
	return JSON.parse(native.stdout);
}

test("HTTP factory native reference defers and deduplicates callback errors", () => {
	const rows = nativeConstructorFactoryRows();
	expect(rows).toHaveLength(10);
	for (const row of rows) {
		expect(row.before).toBe(
			row.mode === "async" || row.mode === "duplicate" ? 0 : 1,
		);
		expect(row.events).toEqual([
			{ sameError: true, code: "EFACTORY", message: "factory-specific" },
		]);
		expect(row.path).toBe("/request-target");
	}
});

test("HTTP constructor factory errors match ordinary request and native Node", async () => {
	const expected = nativeConstructorFactoryRows();
	const vm = await AgentOs.create({
		defaultSoftware: false,
		permissions: {
			fs: "allow",
			process: "allow",
			childProcess: "allow",
			network: "deny",
		},
	});
	try {
		const actual = await vm.process.execFile(
			"node",
			["-e", constructorFactoryProgram],
			{
				timeoutMs: 8_000,
				output: { capture: "all" },
			},
		);
		expect(actual.exitCode, actual.stderr).toBe(0);
		expect(actual.stderr).toBe("");
		expect(typeof actual.stdout).toBe("string");
		expect(Buffer.byteLength(actual.stdout!)).toBeLessThanOrEqual(16_384);
		expect(JSON.parse(actual.stdout!)).toEqual(expected);
	} finally {
		await vm.dispose();
	}
});

test("HTTP factory native reference socket completion and cancellation", () => {
	const source = `
const http = require('node:http');
const { Duplex } = require('node:stream');
(async () => {
  const rows = [];
  for (const mode of ['returned', 'callback', 'callback-and-return', 'cancelled']) {
    let complete;
    const socket = new Duplex({ read() {}, write(_chunk, _encoding, done) { done(); } });
    let assigned = 0;
    const request = new http.ClientRequest({ createConnection(_options, callback) {
      complete = callback;
      if (mode === 'callback-and-return') callback(null, socket);
      if (mode === 'returned' || mode === 'callback-and-return') return socket;
    } });
    request.on('socket', () => assigned++);
    const errors = [];
    request.on('error', error => errors.push(error));
    if (mode === 'cancelled') request.destroy();
    complete(null, socket);
    complete(null, socket);
    await new Promise(setImmediate);
    rows.push({ mode, assigned, destroyed: socket.destroyed });
    request.destroy();
    await new Promise(setImmediate);
    for (const error of errors) if (error.code !== 'ECONNRESET') throw error;
  }
  console.log(JSON.stringify(rows));
})().catch(error => { console.error(error); process.exitCode = 1; });`;
	const result = spawnSync(process.execPath, ["-e", source], {
		encoding: "utf8",
		timeout: 5_000,
		maxBuffer: 4096,
	});
	expect(result.status, result.error?.message ?? result.stderr).toBe(0);
	expect(JSON.parse(result.stdout)).toEqual([
		{ mode: "returned", assigned: 1, destroyed: false },
		{ mode: "callback", assigned: 1, destroyed: false },
		{ mode: "callback-and-return", assigned: 1, destroyed: false },
		{ mode: "cancelled", assigned: 0, destroyed: true },
	]);
});
