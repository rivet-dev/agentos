import { readFileSync } from "node:fs";
import { runInNewContext } from "node:vm";
import { expect, test } from "vitest";

const source = readFileSync(
	new URL("../../build-tools/bridge-src/builtins/fs.ts", import.meta.url),
	"utf8",
);

function readFileWithBridge(readFile: () => Promise<string>) {
	// Execute the production entrypoint and cancellation helpers independently
	// of the guest bootstrap; only the asynchronous host I/O is substituted.
	const names = [
		"createAbortError",
		"validateAbortSignal",
		"throwIfAborted",
		"fsReadFileAsync",
	];
	const functions = names.map((name) => {
		const declaration = source.match(
			new RegExp(`(?:async )?function ${name}\\([^]*?^}`, "m"),
		);
		if (!declaration) throw new Error(`Missing production function ${name}`);
		return declaration[0];
	});
	return runInNewContext(`${functions.join("\n")}\nfsReadFileAsync`, {
		validateEncodingOption() {},
		normalizePathLike: (path: string) => path,
		_fsAsync: { readFile, readFileBinary: readFile },
		import_buffer: { Buffer },
		bridgeErrorCode: (error: { code?: string }) => error.code,
	}) as (
		path: string,
		options: { encoding?: string; signal: AbortSignal },
	) => Promise<string>;
}

test.each([
	"utf8",
	undefined,
])("async path read (%s) rejects cancellation while awaiting host I/O", async (encoding) => {
	let finish!: (value: string) => void;
	const read = readFileWithBridge(
		() =>
			new Promise((resolve) => {
				finish = resolve;
			}),
	);
	const controller = new AbortController();
	const reason = new Error("cancel pending read");
	const result = read("/mounted/file", { encoding, signal: controller.signal });
	const rejected = expect(result).rejects.toMatchObject({
		name: "AbortError",
		code: "ABORT_ERR",
		cause: reason,
	});
	controller.abort(reason);
	finish("contents must not be delivered after cancellation");
	await rejected;
});

test("pre-aborted path read never starts host I/O", async () => {
	let calls = 0;
	const read = readFileWithBridge(async () => {
		calls++;
		return "contents";
	});
	const controller = new AbortController();
	controller.abort();
	await expect(
		read("/mounted/file", { encoding: "utf8", signal: controller.signal }),
	).rejects.toMatchObject({ code: "ABORT_ERR" });
	expect(calls).toBe(0);
});
