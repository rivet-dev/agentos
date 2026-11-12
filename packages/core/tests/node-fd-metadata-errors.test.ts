import { readFileSync } from "node:fs";
import { expect, test } from "vitest";

const source = readFileSync(
	new URL("../../build-tools/bridge-src/builtins/fs.ts", import.meta.url),
	"utf8",
);
const start = source.indexOf("function fdMetadataCall(");
const end = source.indexOf("function bridgeCall(", start);
const call = new Function(
	"bridgeErrorCode",
	"POSIX_ERRNO",
	"errnoForCode",
	`${source.slice(start, end)}; return fdMetadataCall;`,
)(
	(error: { code: string }) => error.code,
	{ EBADF: 9, EPERM: 1 },
	(code: string) => (code === "EBADF" ? -9 : -1),
);

test.each([
	"fchmod",
	"fchown",
])("descriptor metadata error preserves identity and adds %s context", (syscall) => {
	const failure = Object.assign(new Error("bad descriptor"), {
		code: "EBADF",
		details: { fd: 7 },
	});
	expect(() =>
		call(() => {
			throw failure;
		}, syscall),
	).toThrow(failure);
	expect(failure).toMatchObject({
		code: "EBADF",
		errno: -9,
		syscall,
		details: { fd: 7 },
	});
});

test("descriptor metadata leaves non-POSIX failures intact", () => {
	const failure = Object.assign(new Error("bridge stopped"), {
		code: "ERR_AGENTOS_TRANSPORT",
	});
	expect(() =>
		call(() => {
			throw failure;
		}, "fchmod"),
	).toThrow(failure);
	expect(failure).not.toHaveProperty("syscall");
});
