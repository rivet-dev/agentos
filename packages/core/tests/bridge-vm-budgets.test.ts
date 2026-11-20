import { EventEmitter } from "node:events";
import { readFileSync } from "node:fs";
import { runInNewContext } from "node:vm";
import * as ts from "typescript";
import { expect, test } from "vitest";

// Execute actual owning functions with deterministic socket/backpressure doubles.
// These controls do not substitute a second parser or wait for a network timeout.
const bridge = (file: string) =>
	readFileSync(
		new URL(`../../build-tools/bridge-src/${file}`, import.meta.url),
		"utf8",
	);
function declarations(file: string, names: string[]) {
	const source = ts.createSourceFile(
		file,
		bridge(file),
		ts.ScriptTarget.Latest,
		true,
		ts.ScriptKind.TS,
	);
	const found = new Map<string, string>();
	for (const node of source.statements) {
		if (
			ts.isFunctionDeclaration(node) &&
			node.name &&
			names.includes(node.name.text)
		)
			found.set(node.name.text, node.getText(source));
	}
	for (const name of names)
		if (!found.has(name)) throw new Error(`missing actual owner ${name}`);
	return [...found.values()].join("\n");
}
function actualConstants(file: string, names: string[]) {
	const source = ts.createSourceFile(
		file,
		bridge(file),
		ts.ScriptTarget.Latest,
		true,
		ts.ScriptKind.TS,
	);
	const rows: string[] = [];
	for (const statement of source.statements)
		if (ts.isVariableStatement(statement))
			for (const declaration of statement.declarationList.declarations)
				if (names.includes(declaration.name.getText(source)))
					rows.push(`const ${declaration.getText(source)};`);
	return rows.join("\n");
}
const defaults = {
	maxChunkSizeLineBytes: 16416,
	maxPendingResponseBytes: 262144,
	maxReadlineHistoryEntries: 1024,
};

test("readline actual history keeps0/30, enforces default1024 and trusted raised2048", () => {
	const functions = declarations("builtins/readline-terminal.ts", [
		"limitError",
		"createHistory",
	]);
	function owner(maximum: number) {
		return runInNewContext(
			`${actualConstants("builtins/readline-terminal.ts", ["DEFAULT_HISTORY_SIZE", "MAX_HISTORY_SIZE"])};${functions};createHistory`,
			{
				DEFAULT_HISTORY_SIZE: 30,
				BUFFER_MAX_STRING_LENGTH: 0x1fffffe8,
				guestBridgeLimits: () => ({
					...defaults,
					maxReadlineHistoryEntries: maximum,
				}),
				console: { warn() {} },
			},
		);
	}
	expect(() => owner(1024)({ historySize: 1025 })).toThrow(/historySize/);
	const raised = owner(2048)({ historySize: 1025, history: ["seed"] });
	raised.add("next");
	expect([...raised.entries]).toEqual(["next", "seed"]);
	const zero = owner(1024)({ historySize: 0 });
	zero.add("ignored");
	expect([...zero.entries]).toEqual([]);
	const ordinary = owner(1024)({});
	for (let i = 0; i < 31; i++) ordinary.add(`line${i}`);
	expect(ordinary.entries.length).toBe(30);
	const small = owner(1024)({ historySize: 2 });
	for (const line of ["a", "b", "c"]) small.add(line);
	expect([...small.entries]).toEqual(["c", "b"]);
});

function httpOwner(limits: typeof defaults) {
	const functions = declarations("builtins/http.ts", [
		"streamRawHttpResponse",
		"parseRawHttpResponse",
		"appendNormalizedHeader",
		"joinHeaderValue",
		"hasResponseBody",
		"splitTransferEncodingTokens",
		"parseContentLengthHeader",
	]);
	const warnings: string[] = [];
	let response: any;
	class Incoming {
		allow = true;
		chunks: Buffer[] = [];
		ends = 0;
		constructor(
			public head: unknown,
			public resume: () => void,
			public cancel: () => void,
		) {
			response = this;
		}
		push(chunk: Buffer) {
			this.chunks.push(Buffer.from(chunk));
			return this.allow;
		}
		_endBody() {
			this.ends++;
		}
	}
	const stream = runInNewContext(
		`${actualConstants("builtins/http.ts", ["MAX_HTTP_CHUNK_SIZE_LINE_BYTES", "MAX_HTTP_PENDING_RESPONSE_BYTES"])};${functions};streamRawHttpResponse`,
		{
			Buffer,
			IncomingMessage: Incoming,
			guestBridgeLimits: () => limits,
			MAX_HTTP_CHUNK_EXTENSION_BYTES: 16384,
			HTTP_RESPONSE_STREAM_CHUNK_BYTES: 16384,
			MAX_HTTP_BODY_BYTES: 1024 * 1024,
			MAX_HTTP_REQUEST_HEADER_BYTES: 32768,
			MAX_HTTP_REQUEST_HEADERS: 128,
			setTimeout,
			clearTimeout,
			queueMicrotask,
			console: { warn: (m: string) => warnings.push(m) },
		},
	);
	class Socket extends EventEmitter {
		pauses = 0;
		resumes = 0;
		pause() {
			this.pauses++;
		}
		resume() {
			this.resumes++;
		}
		destroy() {}
	}
	return { stream, socket: new Socket(), response: () => response, warnings };
}

test("actual HTTP parser separates raised chunk-line policy from fixed extension compatibility", async () => {
	for (const [cap, success] of [
		[16416, false],
		[32768, true],
	] as const) {
		const o = httpOwner({ ...defaults, maxChunkSizeLineBytes: cap });
		const done = o.stream(o.socket, "GET", 1000, () => {});
		// Attach rejection capture before synchronous fixture events.
		const settled = done.then(
			() => null,
			(error: Error) => error,
		);
		const line = "0".repeat(16416) + "1";
		o.socket.emit(
			"data",
			Buffer.from(
				`HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\n\r\n${line}\r\nA\r\n0\r\n\r\n`,
			),
		);
		const error = await settled;
		if (success) {
			expect(error).toBeNull();
			expect(Buffer.concat(o.response().chunks).toString()).toBe("A");
			expect(o.response().ends).toBe(1);
		} else
			expect(error?.message).toMatch(
				/HTTP chunk size line|limits.http.maxChunkSizeLineBytes/,
			);
		expect(o.socket.listenerCount("data")).toBe(0);
		const extension = httpOwner({ ...defaults, maxChunkSizeLineBytes: cap });
		const extDone = extension
			.stream(extension.socket, "GET", 1000, () => {})
			.then(
				() => null,
				(error: Error) => error,
			);
		extension.socket.emit(
			"data",
			Buffer.from(
				`HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\n\r\n1;${"a".repeat(16385)}\r\nA\r\n0\r\n\r\n`,
			),
		);
		expect((await extDone)?.message).toMatch(/chunk extension/);
	}
});

test("actual paused HTTP response budget rejects finite overflow and raised policy drains exact bytes once", async () => {
	for (const [cap, success] of [
		[3, false],
		[8, true],
	] as const) {
		const o = httpOwner({ ...defaults, maxPendingResponseBytes: cap });
		let terminal: Error | null | undefined;
		const done = o
			.stream(o.socket, "GET", 1000, (response: any) => {
				response.allow = false;
			})
			.then(
				() => {
					terminal = null;
					return null;
				},
				(error: Error) => {
					terminal = error;
					return error;
				},
			);
		o.socket.emit(
			"data",
			Buffer.from("HTTP/1.1 200 OK\r\nContent-Length: 8\r\n\r\nA"),
		);
		expect(o.socket.pauses).toBe(1);
		o.socket.emit("data", Buffer.from("BCDE"));
		if (success) {
			o.response().allow = true;
			o.response().resume();
			await Promise.resolve();
			expect(o.socket.resumes).toBe(1);
			o.socket.emit("data", Buffer.from("FGH"));
			expect(await done).toBeNull();
			expect(Buffer.concat(o.response().chunks).toString()).toBe("ABCDEFGH");
			expect(o.response().ends).toBe(1);
		} else {
			await Promise.resolve();
			const observed = terminal;
			// RED remains immediate and clean on the old hardcoded owner: do
			// not wait for its one-second timeout or leave fixture listeners.
			if (observed === undefined)
				o.socket.emit(
					"error",
					new Error("fixture cleanup: configured rejection missing"),
				);
			await done;
			expect(observed?.message).toMatch(
				/limits.http.maxPendingResponseBytes.*maximum=3/,
			);
		}
		expect(o.socket.listenerCount("data")).toBe(0);
	}
});
