// Requires the built registry command. Optional native oracle uses the same
// upstream release; fixture expectations remain checked without a host gawk.
import { spawnSync } from "node:child_process";
import { existsSync, readFileSync } from "node:fs";
import { mkdtemp, rm, writeFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { fileURLToPath } from "node:url";
import { afterAll, beforeAll, describe, expect, test } from "vitest";
import {
	AgentOs,
	createSnapshotExport,
} from "../../../packages/core/src/index.js";

const wasmPath =
	process.env.AGENTOS_GAWK_TEST_BIN ??
	fileURLToPath(new URL("../bin/awk", import.meta.url));
const nativePath =
	process.env.AGENTOS_GAWK_NATIVE_BIN ??
	fileURLToPath(
		new URL(
			"../../../toolchain/c/build/gawk-native/gawk-5.4.1/gawk",
			import.meta.url,
		),
	);
const shellPath = fileURLToPath(
	new URL("../../coreutils/bin/sh", import.meta.url),
);
const catPath = fileURLToPath(
	new URL("../../coreutils/bin/cat", import.meta.url),
);
const killPath = fileURLToPath(
	new URL("../../coreutils/bin/kill", import.meta.url),
);
if (process.env.AGENTOS_GAWK_REQUIRE_NATIVE === "1") {
	for (const path of [wasmPath, nativePath, shellPath, catPath, killPath])
		if (!existsSync(path))
			throw new Error(`GNU awk acceptance artifact missing: ${path}`);
}
const fixtures = { "a.txt": "alpha 3\nbeta 5\n", "b.txt": "gamma 7\n" };
const cases = [
	{
		name: "BEGIN and END with omitted stdin",
		args: ['BEGIN { print "begin" } END { print "end" }'],
		stdout: "begin\nend\n",
	},
	{
		name: "empty stdin reaches EOF",
		args: ['{ print $0 } END { print "EOF", NR }'],
		stdin: "",
		stdout: "EOF 0\n",
	},
	{
		name: "finite stdin without final newline",
		args: ['{ print NR, $0 } END { print "EOF", NR }'],
		stdin: "one\ntwo",
		stdout: "1 one\n2 two\nEOF 2\n",
	},
	{
		name: "multi-file NR and FNR",
		args: [
			'{ print FILENAME, NR, FNR, $0 } END { print "END", NR, FNR }',
			"a.txt",
			"b.txt",
		],
		stdout: "a.txt 1 1 alpha 3\na.txt 2 2 beta 5\nb.txt 3 1 gamma 7\nEND 3 1\n",
	},
	{
		name: "getline and close reopen a file",
		args: [
			'BEGIN { while ((getline line < "a.txt") > 0) print "G:" line; close("a.txt"); print (getline line < "a.txt"); print line } END { print NR, FNR }',
		],
		stdout: "G:alpha 3\nG:beta 5\n1\nalpha 3\n0 0\n",
	},
	{
		name: "getline from finite stdin updates counters",
		args: [
			'BEGIN { getline; print "FIRST", NR, FNR, $0 } { print "REST", NR, FNR, $0 } END { print "END", NR }',
		],
		stdin: "first\nsecond\n",
		stdout: "FIRST 1 1 first\nREST 2 2 second\nEND 2\n",
	},
	{
		name: "regex, substitution, and printf",
		args: [
			'/[[:alpha:]]+/ { gsub(/[0-9]+/, "N", $0); printf "%02d:%-6s:%.2f\\n", NR, toupper($1), 7 / 2 }',
		],
		stdin: "abc12 xx\ndef34 yy\n",
		stdout: "01:ABCN  :3.50\n02:DEFN  :3.50\n",
	},
	{
		name: "GNU BEGINFILE and ENDFILE",
		args: [
			'BEGINFILE { print "OPEN", FILENAME, FNR } { n++ } ENDFILE { print "CLOSE", FILENAME, FNR } END { print n }',
			"a.txt",
			"b.txt",
		],
		stdout: "OPEN a.txt 0\nCLOSE a.txt 2\nOPEN b.txt 0\nCLOSE b.txt 1\n3\n",
	},
	{
		name: "GNU gensub and array sorting",
		args: [
			'BEGIN { print gensub(/([a-z]+)([0-9]+)/, "\\\\2-\\\\1", "g", "abc12"); a[1]="z"; a[2]="a"; n=asort(a); for(i=1;i<=n;i++) print a[i] }',
		],
		stdout: "12-abc\na\nz\n",
	},
	{
		name: "exit runs END and preserves status",
		args: ['BEGIN { print "begin"; exit 7 } END { print "end" }'],
		stdout: "begin\nend\n",
		status: 7,
	},
	{
		name: "system executes the POSIX shell and reports status",
		args: [
			'BEGIN { print "STATUS", system("exit 7"); print "OK", system("printf shell-ok") }',
		],
		stdout: "STATUS 7\nshell-okOK 0\n",
	},
	{
		name: "getline reads a command pipe and closes it",
		args: [
			'BEGIN { cmd="printf \'pipe-one\\npipe-two\\n\'"; while ((cmd | getline line) > 0) print "PIPE:" line; print "CLOSE", close(cmd) }',
		],
		stdout: "PIPE:pipe-one\nPIPE:pipe-two\nCLOSE 0\n",
	},
	{
		name: "UTF-8 length substring and regex use explicit locale",
		args: ['BEGIN { s="éΩ"; print length(s), substr(s,2,1), match(s,/Ω/) }'],
		stdout: "2 Ω 2\n",
		locale: "C.UTF-8",
	},
	{
		name: "command write pipe flushes and returns child status",
		args: [
			'BEGIN { cmd="/bin/cat"; print "one" | cmd; print "two" | cmd; print "CLOSE", close(cmd) }',
		],
		stdout: "one\ntwo\nCLOSE 0\n",
	},
	{
		name: "two-way command pipe closes input before reading",
		args: [
			'BEGIN { cmd="/bin/cat"; print "duplex" |& cmd; close(cmd,"to"); cmd |& getline line; print line; print "CLOSE", close(cmd) }',
		],
		stdout: "duplex\nCLOSE 0\n",
	},
	{
		name: "system preserves signal termination status",
		args: ['BEGIN { print system("/bin/kill -TERM $$") }'],
		stdout: "271\n",
	},
];

describe.skipIf(!existsSync(wasmPath))("upstream GNU awk", () => {
	let vm: AgentOs;
	let nativeCwd: string;
	beforeAll(async () => {
		if (existsSync(nativePath)) {
			const version = spawnSync(nativePath, ["--version"], {
				encoding: "utf8",
				timeout: 5_000,
			});
			expect(version.status, version.stderr).toBe(0);
			expect(version.stdout).toMatch(/^GNU Awk 5\.4\.1/);
		}
		nativeCwd = await mkdtemp(join(tmpdir(), "agentos-gnu-awk-"));
		for (const [name, value] of Object.entries(fixtures))
			await writeFile(join(nativeCwd, name), value);
		vm = await AgentOs.create({
			defaultSoftware: false,
			permissions: { fs: "allow", process: "allow", childProcess: "allow" },
			rootFilesystem: {
				lowers: [
					createSnapshotExport([
						{ path: "/bin", type: "directory", mode: "0755" },
						{
							path: "/bin/kill",
							type: "file",
							mode: "0755",
							content: readFileSync(killPath).toString("base64"),
							encoding: "base64",
						},
						{ path: "/work", type: "directory", mode: "0755" },
						{
							path: "/bin/cat",
							type: "file",
							mode: "0755",
							content: readFileSync(catPath).toString("base64"),
							encoding: "base64",
						},
						{
							path: "/bin/sh",
							type: "file",
							mode: "0755",
							content: readFileSync(shellPath).toString("base64"),
							encoding: "base64",
						},
						{
							path: "/bin/awk",
							type: "file",
							mode: "0755",
							content: readFileSync(wasmPath).toString("base64"),
							encoding: "base64",
						},
						...Object.entries(fixtures).map(([name, content]) => ({
							path: `/work/${name}`,
							type: "file" as const,
							mode: "0644",
							content,
							encoding: "utf8" as const,
						})),
					]),
				],
			},
		});
	}, 30_000);
	afterAll(async () => {
		await vm?.dispose();
		if (nativeCwd) await rm(nativeCwd, { recursive: true });
	});

	for (const fixture of cases)
		test(fixture.name, async () => {
			const expected = {
				stdout: fixture.stdout,
				stderr: "",
				status: "status" in fixture ? fixture.status : 0,
			};
			if (existsSync(nativePath)) {
				const native = spawnSync(nativePath, fixture.args, {
					cwd: nativeCwd,
					env: {
						...process.env,
						LC_ALL: "locale" in fixture ? fixture.locale : "C",
					},
					input: "stdin" in fixture ? fixture.stdin : "",
					encoding: "utf8",
					timeout: 5_000,
					maxBuffer: 64 * 1024,
				});
				expect(native.error).toBeUndefined();
				expect({
					stdout: native.stdout,
					stderr: native.stderr,
					status: native.status,
				}).toEqual(expected);
			}
			const result = await vm.process.execFile("/bin/awk", fixture.args, {
				cwd: "/work",
				env: { LC_ALL: "locale" in fixture ? fixture.locale : "C" },
				timeoutMs: 5_000,
				output: { capture: "all", maxBufferedBytes: 64 * 1024 },
				...("stdin" in fixture ? { stdin: fixture.stdin } : {}),
			});
			expect({
				stdout: result.stdout,
				stderr: result.stderr,
				status: result.exitCode,
			}).toEqual(expected);
		}, 10_000);

	for (const fixture of [
		{
			name: "read pipe with stdin closed",
			fd: 0,
			program:
				'BEGIN { cmd="printf closed"; cmd | getline line; print line; print close(cmd) }',
			stdout: "closed\n0\n",
			stderr: "",
		},
		{
			name: "write pipe with stdout closed",
			fd: 1,
			program: 'BEGIN { cmd="/bin/cat >&2"; print "closed" | cmd; close(cmd) }',
			stdout: "",
			stderr: "closed\n",
		},
		{
			name: "two-way pipe with stdin closed",
			fd: 0,
			program:
				'BEGIN { cmd="/bin/cat"; print "closed" |& cmd; close(cmd,"to"); cmd |& getline line; print line; print close(cmd) }',
			stdout: "closed\n0\n",
			stderr: "",
		},
		{
			name: "two-way pipe with stdout closed",
			fd: 1,
			program:
				'BEGIN { cmd="/bin/cat >&2"; print "closed" |& cmd; close(cmd,"to"); cmd |& getline line; close(cmd) }',
			stdout: "",
			stderr: "closed\n",
		},
	])
		test(fixture.name, async () => {
			const script = `exec ${fixture.fd}<&-; exec "$1" "$2"`;
			const expected = {
				stdout: fixture.stdout,
				stderr: fixture.stderr,
				status: 0,
			};
			if (existsSync(nativePath)) {
				const native = spawnSync(
					"/bin/sh",
					["-c", script, "probe", nativePath, fixture.program],
					{
						encoding: "utf8",
						timeout: 5_000,
						maxBuffer: 64 * 1024,
						env: { ...process.env, LC_ALL: "C" },
					},
				);
				expect(native.error).toBeUndefined();
				expect({
					stdout: native.stdout,
					stderr: native.stderr,
					status: native.status,
				}).toEqual(expected);
			}
			const result = await vm.process.execFile(
				"/bin/sh",
				["-c", script, "probe", "/bin/awk", fixture.program],
				{
					env: { LC_ALL: "C" },
					timeoutMs: 5_000,
					output: { capture: "all", maxBufferedBytes: 64 * 1024 },
				},
			);
			expect({
				stdout: result.stdout,
				stderr: result.stderr,
				status: result.exitCode,
			}).toEqual(expected);
		}, 10_000);

	test("system reports missing shell as status 127", async () => {
		await vm.filesystem.move("/bin/sh", "/bin/saved-sh");
		try {
			const result = await vm.process.execFile(
				"/bin/awk",
				['BEGIN { print system("exit 7") }'],
				{ timeoutMs: 5_000, output: { capture: "all" } },
			);
			expect({
				stdout: result.stdout,
				stderr: result.stderr,
				status: result.exitCode,
			}).toEqual({ stdout: "127\n", stderr: "", status: 0 });
		} finally {
			await vm.filesystem.move("/bin/saved-sh", "/bin/sh");
		}
	});

	for (const fixture of [
		{
			name: "read pipe",
			program:
				'BEGIN { cmd="printf hi"; print (cmd | getline line); print close(cmd) }',
			stdout: "0\n127\n",
		},
		{
			name: "two-way pipe",
			program:
				'BEGIN { cmd="cat"; print (cmd |& getline line); print close(cmd) }',
			stdout: "0\n127\n",
		},
		{
			name: "buffered nonfatal write pipe",
			program:
				'BEGIN { PROCINFO["BUFFERPIPE"]=1; cmd="cat"; PROCINFO[cmd,"NONFATAL"]=1; print "hi" | cmd; print close(cmd) }',
			stdout: "127\n",
		},
	])
		test(`missing shell preserves ${fixture.name} completion`, async () => {
			await vm.filesystem.move("/bin/sh", "/bin/saved-sh");
			try {
				const result = await vm.process.execFile(
					"/bin/awk",
					[fixture.program],
					{ timeoutMs: 5_000, output: { capture: "all" } },
				);
				expect({
					stdout: result.stdout,
					stderr: result.stderr,
					status: result.exitCode,
				}).toEqual({ stdout: fixture.stdout, stderr: "", status: 0 });
			} finally {
				await vm.filesystem.move("/bin/saved-sh", "/bin/sh");
			}
		});

	test("identifies itself as GNU awk", async () => {
		const result = await vm.process.execFile("/bin/awk", ["--version"], {
			timeoutMs: 5_000,
			output: { capture: "all" },
		});
		expect(result.exitCode, result.stderr).toBe(0);
		expect(result.stdout).toMatch(/^GNU Awk 5\.4\.1/);
	});
	test("syntax and missing-file errors return failure", async () => {
		for (const args of [["BEGIN {"], ["{ print }", "missing.txt"]]) {
			const result = await vm.process.execFile("/bin/awk", args, {
				cwd: "/work",
				timeoutMs: 5_000,
				output: { capture: "all" },
			});
			if (existsSync(nativePath)) {
				const native = spawnSync(nativePath, args, {
					cwd: nativeCwd,
					env: { ...process.env, LC_ALL: "C" },
					encoding: "utf8",
					timeout: 5_000,
				});
				const diagnostic = (value: string) =>
					value.replace(/^(?:gawk|awk):/gm, "awk:");
				expect(native.error).toBeUndefined();
				expect(result.exitCode).toBe(native.status);
				expect(result.stdout).toBe(native.stdout);
				expect(diagnostic(result.stderr)).toBe(diagnostic(native.stderr));
			}
			expect(result.exitCode).not.toBe(0);
			expect(result.stderr).toContain(
				args.length === 1
					? "unexpected newline or end of string"
					: "missing.txt",
			);
		}
	});
});
