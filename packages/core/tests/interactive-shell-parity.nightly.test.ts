import { Terminal } from "@xterm/headless";
import { afterEach, describe, expect, it } from "vitest";
import { AgentOs } from "../src/index.js";

const PROMPT = "sh-0.4$ ";

describe("public interactive shell parity", () => {
	let vm: AgentOs | undefined;
	let screen: Terminal | undefined;

	afterEach(async () => {
		await vm?.dispose();
		screen?.dispose();
	});

	async function openShell() {
		vm = await AgentOs.create();
		screen = new Terminal({ cols: 160, rows: 40, allowProposedApi: true });
		const { shellId } = vm.terminal.open({ cols: 160, rows: 40 });
		vm.onShellData(shellId, ({ data }) => screen!.write(data));
		const screenshotTrimmed = () => {
			const buffer = screen!.buffer.active;
			const lines: string[] = [];
			for (let row = 0; row < screen!.rows; row++) {
				lines.push(
					buffer.getLine(buffer.viewportY + row)?.translateToString(true) ?? "",
				);
			}
			while (lines.at(-1) === "") lines.pop();
			return lines.join("\n");
		};
		const waitFor = async (text: string, occurrence = 1, timeout = 5_000) => {
			try {
				await expect
					.poll(() => screenshotTrimmed().split(text).length - 1, {
						timeout,
						interval: 20,
					})
					.toBeGreaterThanOrEqual(occurrence);
			} catch (error) {
				throw new Error(
					`Waiting for ${JSON.stringify(text)}: ${screenshotTrimmed()}`,
					{ cause: error },
				);
			}
		};
		await waitFor(PROMPT);
		return {
			shell: { write: (data: string) => vm!.terminal.write(shellId, data) },
			resize: (cols: number, rows: number) =>
				vm!.terminal.resize(shellId, cols, rows),
			wait: () => vm!.terminal.wait(shellId),
			waitFor,
			screenshotTrimmed,
		};
	}

	it.each([
		["sequence", "printf 'first\\n'; printf 'second\\n'", ["first", "second"]],
		[
			"conditional",
			"false && printf 'unexpected\\n'; true && printf 'allowed\\n'",
			["allowed"],
		],
		["pipeline", "printf 'lower\\n' | tr a-z A-Z", ["LOWER"]],
		[
			"subshell",
			"(printf 'inside\\n'); printf 'outside\\n'",
			["inside", "outside"],
		],
		[
			"command substitution",
			"printf '<%s>\\n' \"$(printf substituted)\"",
			["<substituted>"],
		],
	] satisfies [string, string, string[]][])(
		"executes %s syntax through the terminal",
		async (_name, command, expected) => {
			const shell = await openShell();
			await shell.shell.write(`${command}\n`);
			await shell.waitFor(PROMPT, 2, 10_000);
			const lines = shell.screenshotTrimmed().split("\n");
			for (const line of expected) {
				expect(
					lines.filter((value) => value === line),
					shell.screenshotTrimmed(),
				).toHaveLength(1);
			}
			expect(lines).not.toContain("unexpected");
		},
		20_000,
	);

	it("reports an unknown command and preserves shell status and readiness", async () => {
		const shell = await openShell();
		await shell.shell.write("missing_parity_command\n");
		await shell.waitFor(PROMPT, 2, 10_000);
		const diagnosticLines = shell
			.screenshotTrimmed()
			.split("\n")
			.filter((line) => !line.includes(PROMPT));
		expect(
			diagnosticLines.some(
				(line) =>
					line.includes("missing_parity_command") &&
					/not found|unknown command/i.test(line),
			),
			diagnosticLines.join("\n"),
		).toBe(true);
		await shell.shell.write("printf 'status=%s\\n' \"$?\"\n");
		await shell.waitFor(PROMPT, 3, 10_000);
		expect(
			shell.screenshotTrimmed().split("\n"),
			shell.screenshotTrimmed(),
		).toContain("status=127");
	}, 25_000);

	it("completes a unique filename and executes the completed command", async () => {
		const shell = await openShell();
		await vm!.filesystem.writeFile(
			"/tmp/parity_unique_document.txt",
			"completed-file\n",
		);
		await shell.shell.write("cat /tmp/parity_unique_doc");
		await shell.waitFor("cat /tmp/parity_unique_doc");
		await shell.shell.write("\t");
		await shell.waitFor("/tmp/parity_unique_document.txt");
		await shell.shell.write("\n");
		await shell.waitFor(PROMPT, 2, 10_000);
		expect(shell.screenshotTrimmed().split("\n")).toContain("completed-file");
	}, 20_000);

	it("completes a unique command name and executes it", async () => {
		const shell = await openShell();
		await shell.shell.write("printen");
		await shell.waitFor("printen");
		await shell.shell.write("\t");
		await shell.waitFor("printenv");
		await shell.shell.write(" PATH\n");
		await shell.waitFor(PROMPT, 2, 10_000);
		const outputLines = shell
			.screenshotTrimmed()
			.split("\n")
			.filter((line) => !line.includes(PROMPT));
		expect(
			outputLines.some((line) => line.split(":").includes("/opt/agentos/bin")),
		).toBe(true);
	}, 20_000);
	it("preserves variables and cwd across commands", async () => {
		const shell = await openShell();
		await shell.shell.write("export PARITY_VALUE=persisted; cd /tmp\n");
		await shell.waitFor(PROMPT, 2);
		await shell.shell.write('printf \'%s:%s\\n\' "$PARITY_VALUE" "$PWD"\n');
		await shell.waitFor(PROMPT, 3);
		expect(shell.screenshotTrimmed().split("\n")).toContain("persisted:/tmp");
	});

	it("discards partial input on Ctrl-C and accepts the next command", async () => {
		const shell = await openShell();
		await shell.shell.write("unfinished");
		await shell.waitFor("unfinished");
		await shell.shell.write("\x03");
		await shell.waitFor(PROMPT, 2);
		await shell.shell.write("printf 'ready-again\\n'\n");
		await shell.waitFor(PROMPT, 3);
		expect(shell.screenshotTrimmed().split("\n")).toContain("ready-again");
	});

	it("exits with status zero on EOF at an empty prompt", async () => {
		const shell = await openShell();
		await shell.shell.write("\x04");
		expect(await shell.wait()).toBe(0);
	});

	it("exposes resized PTY dimensions to a foreground program", async () => {
		const shell = await openShell();
		shell.resize(100, 30);
		await shell.shell.write(
			"node -e 'console.log(process.stdout.rows, process.stdout.columns)'\n",
		);
		await shell.waitFor(PROMPT, 2);
		expect(
			shell.screenshotTrimmed().split("\n"),
			shell.screenshotTrimmed(),
		).toContain("30 100");
	});

	it("runs a caught resize handler and keeps the terminal read usable", async () => {
		const shell = await openShell();
		await shell.shell.write("trap 'printf \"caught-resize\\n\"' WINCH\n");
		await shell.waitFor(PROMPT, 2);
		shell.resize(101, 31);
		await shell.shell.write("printf 'after-resize\\n'\n");
		await shell.waitFor(PROMPT, 3);
		const lines = shell.screenshotTrimmed().split("\n");
		expect(lines.filter((line) => line === "caught-resize")).toHaveLength(1);
		expect(lines).toContain("after-resize");
	});

	it("preserves explicit-command output bytes and warning-like text", async () => {
		vm = await AgentOs.create();
		await vm.filesystem.writeFile(
			"/tmp/terminal-bytes.mjs",
			`
const warning = "WARN could not retrieve pid for child process";
process.stdout.write(Buffer.from([0xe2]));
setTimeout(() => {
  process.stdout.write(Buffer.from([0x82, 0xac, 0x0a]));
  process.stdout.write(warning + "\\n");
}, 25);
`,
		);
		const { shellId } = vm.terminal.open({
			command: "node",
			args: ["/tmp/terminal-bytes.mjs"],
		});
		const chunks: Uint8Array[] = [];
		vm.onShellData(shellId, ({ data }) => chunks.push(data));
		expect(await vm.terminal.wait(shellId)).toBe(0);
		const output = Buffer.concat(chunks);
		expect(output.subarray(0, 3)).toEqual(Buffer.from([0xe2, 0x82, 0xac]));
		expect(output.toString("utf8")).toContain(
			"WARN could not retrieve pid for child process",
		);
	});

	it.each([
		false,
		true,
	])("preserves large ordered terminal writes with raw=%s", async (raw) => {
		vm = await AgentOs.create();
		const { shellId } = vm.terminal.open({
			command: "node",
			args: [
				"-e",
				`
if (${raw}) process.stdin.setRawMode(true);
process.stdout.write("a\\nb\\n".repeat(32768));
process.stderr.write("stderr\\n");
`,
			],
		});
		const chunks: Uint8Array[] = [];
		vm.onShellData(shellId, ({ data }) => chunks.push(data));
		expect(await vm.terminal.wait(shellId)).toBe(0);
		const newline = raw ? "\n" : "\r\n";
		const output = Buffer.concat(chunks);
		const expected = Buffer.from(
			`a${newline}b${newline}`.repeat(32768) + `stderr${newline}`,
		);
		expect(output.byteLength).toBe(expected.byteLength);
		expect(Buffer.compare(output, expected)).toBe(0);
	}, 20_000);
});
