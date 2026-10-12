import { spawnSync } from "node:child_process";
import { afterAll, beforeAll, expect, test } from "vitest";
import { AgentOs } from "../src/index.js";

let sidecar: Awaited<ReturnType<typeof AgentOs.createSidecar>>;
let vm: AgentOs;

beforeAll(async () => {
	sidecar = await AgentOs.createSidecar();
	vm = await AgentOs.create({
		sidecar: { kind: "explicit", handle: sidecar },
		defaultSoftware: false,
		permissions: {
			fs: "allow",
			process: "allow",
			childProcess: "allow",
			network: "deny",
		},
	});
});

afterAll(async () => {
	try {
		await vm?.dispose();
	} finally {
		await sidecar?.dispose();
	}
});

const cases = [
	{
		name: "binary readable demand",
		input: Buffer.from([0, 255, 128, 65, 226, 130, 172]),
		size: null,
		encoding: null,
	},
	{
		name: "sized reads with a short EOF tail",
		input: Buffer.from([0, 255, 128, 65, 66, 67, 68, 69]),
		size: 3,
		encoding: null,
	},
	{
		name: "explicit read beyond the default watermark",
		input: Buffer.from(Array.from({ length: 65545 }, (_, i) => i % 251)),
		size: 65540,
		encoding: null,
	},
	{
		name: "empty readable EOF",
		input: Buffer.alloc(0),
		size: null,
		encoding: null,
	},
	...(["utf8", "utf16le", "latin1", "ascii", "hex", "base64"] as const).map(
		(encoding) => ({
			name: `${encoding} decoding and final tail`,
			input: Buffer.from([0, 255, 128, 226, 130, 172, 65]),
			size: null,
			encoding,
		}),
	),
];

for (const { name, input, size, encoding } of cases) {
	test(`${name} matches native Node`, async () => {
		const program = `
const chunks = [];
const sizes = [];
const encoding = ${JSON.stringify(encoding)};
if (encoding) process.stdin.setEncoding(encoding);
process.stdin.on('readable', () => {
  let chunk;
  while ((chunk = process.stdin.read(${size === null ? "" : String(size)})) !== null) {
    if (!encoding && !Buffer.isBuffer(chunk)) throw new Error('Unencoded stdin must return Buffer');
    chunks.push(chunk);
    sizes.push(chunk.length);
  }
});
process.stdin.on('end', () => console.log(JSON.stringify({
  value: encoding ? chunks.join('') : Buffer.concat(chunks).toString('hex'),
  ${size === null ? "" : "sizes,"}
  ended: true
})));
`;
		const native = spawnSync(process.execPath, ["-e", program], {
			input,
			encoding: "utf8",
			timeout: 5_000,
			maxBuffer: 256 * 1024,
		});
		expect(native.error).toBeUndefined();
		expect(native.status, native.stderr).toBe(0);
		const expected = JSON.parse(native.stdout);
		expect(expected.ended).toBe(true);
		if (encoding === null) expect(expected.value).toBe(input.toString("hex"));
		const guest = await vm.process.execFile("node", ["-e", program], {
			stdin: input,
			timeoutMs: 5_000,
			output: { capture: "all", maxBufferedBytes: 256 * 1024 },
		});
		expect(guest.exitCode, guest.stderr).toBe(0);
		expect(guest.stdoutTruncated).toBe(false);
		expect(guest.stdout).not.toBe("");
		expect(JSON.parse(guest.stdout)).toEqual(expected);
	}, 15_000);
}

const collectStdin = `
const chunks = [];
await new Promise(resolve => {
  process.stdin.on('data', chunk => chunks.push(chunk));
  process.stdin.on('end', resolve);
});
console.log(Buffer.concat(chunks).toString('hex'));
`;

for (const kind of ["Buffer", "Uint8Array"] as const) {
	for (const mode of ["initial", "streamed"] as const) {
		test(`${mode} ${kind} stdin exposes only the supplied byte view`, async () => {
			// Fill the entire backing store with synthetic surrounding data. This
			// detects a boundary leak without ever printing unrelated host memory.
			const backing = new ArrayBuffer(32);
			new Uint8Array(backing).fill(0xa5);
			const input =
				kind === "Buffer"
					? Buffer.from(backing, 9, 4)
					: new Uint8Array(backing, 9, 4);
			input.set([0, 255, 128, 65]);
			if (mode === "initial") {
				const result = await vm.javascript.execute(collectStdin, {
					stdin: input,
					timeoutMs: 5_000,
					output: { capture: "all" },
				});
				expect(result.outcome, result.stderr).toBe("succeeded");
				expect(result.stdout).toBe("00ff8041\n");
			} else {
				const process = await vm.javascript.spawn(collectStdin, {
					timeoutMs: 5_000,
					output: { retainEvents: true },
				});
				await vm.process.writeStdin(process.pid, input);
				await vm.process.closeStdin(process.pid);
				expect(await vm.process.wait(process.pid)).toMatchObject({
					exitCode: 0,
				});
				const output = await vm.process.readOutput(process.pid);
				const stdout = output.events
					.filter((event) => event.channel === "stdout")
					.map((event) => new TextDecoder().decode(event.chunk))
					.join("");
				expect(stdout).toBe("00ff8041\n");
			}
		}, 15_000);
	}
}
