import { afterAll, afterEach, beforeAll, describe, expect, it } from "vitest";
import { existsSync } from "node:fs";
import {
	createServer,
	type IncomingMessage,
	type Server,
	type ServerResponse,
} from "node:http";
import { resolve } from "node:path";
import { createWasmVmRuntime } from "@agentos/test-harness";
import {
	allowAll,
	C_BUILD_DIR,
	COMMANDS_DIR,
	createInMemoryFileSystem,
	createKernel,
	describeIf,
} from "@agentos/test-harness";
import type { Kernel } from "@agentos/test-harness";

const WGET_COMMAND_DIRS = [C_BUILD_DIR, COMMANDS_DIR].filter((dir) =>
	existsSync(dir),
);
const hasWgetBinary = WGET_COMMAND_DIRS.some((dir) =>
	existsSync(resolve(dir, "wget")),
);
const WGET_EXEC_TIMEOUT_MS = 10_000;

describeIf(hasWgetBinary, "wget command", () => {
	let kernel: Kernel;
	let server: Server;
	let port: number;

	beforeAll(async () => {
		server = createServer((req: IncomingMessage, res: ServerResponse) => {
			const url = req.url ?? "/";

			if (url === "/file.txt") {
				res.writeHead(200, { "Content-Type": "text/plain" });
				res.end("downloaded content");
				return;
			}

			if (url === "/data.json") {
				res.writeHead(200, { "Content-Type": "application/json" });
				res.end(JSON.stringify({ status: "ok" }));
				return;
			}

			if (url === "/redirect") {
				res.writeHead(302, {
					Location: `http://127.0.0.1:${port}/redirected`,
				});
				res.end();
				return;
			}

			if (url === "/redirected") {
				res.writeHead(200, { "Content-Type": "text/plain" });
				res.end("arrived after redirect");
				return;
			}

			res.writeHead(404, { "Content-Type": "text/plain" });
			res.end("not found");
		});

		await new Promise<void>((resolveListen) =>
			server.listen(0, "127.0.0.1", resolveListen),
		);
		port = (server.address() as import("node:net").AddressInfo).port;
	});

	afterAll(async () => {
		await new Promise<void>((resolveClose) =>
			server.close(() => resolveClose()),
		);
	});

	afterEach(async () => {
		await kernel?.dispose();
	});

	async function mountKernel() {
		const filesystem = createInMemoryFileSystem();
		kernel = createKernel({
			filesystem,
			permissions: allowAll,
			loopbackExemptPorts: [port],
		});
		await kernel.mount(createWasmVmRuntime({ commandDirs: WGET_COMMAND_DIRS }));
		return filesystem;
	}

	it("downloads a file using the URL basename", async () => {
		const filesystem = await mountKernel();

		const result = await kernel.exec(`wget http://127.0.0.1:${port}/file.txt`, {
			timeout: WGET_EXEC_TIMEOUT_MS,
		});

		expect(result.exitCode, result.stderr || result.stdout).toBe(0);
		expect(await filesystem.readTextFile("/workspace/file.txt")).toBe(
			"downloaded content",
		);
	}, 15_000);

	it("-O saves to the requested output path", async () => {
		const filesystem = await mountKernel();

		const result = await kernel.exec(
			`wget -O /output.txt http://127.0.0.1:${port}/data.json`,
			{ timeout: WGET_EXEC_TIMEOUT_MS },
		);

		expect(result.exitCode, result.stderr || result.stdout).toBe(0);
		expect(await filesystem.readTextFile("/output.txt")).toContain(
			'"status":"ok"',
		);
	}, 15_000);

	it("-q suppresses progress output", async () => {
		const filesystem = await mountKernel();

		const result = await kernel.exec(
			`wget -q -O /quiet.txt http://127.0.0.1:${port}/file.txt`,
			{ timeout: WGET_EXEC_TIMEOUT_MS },
		);

		expect(result.exitCode, result.stderr || result.stdout).toBe(0);
		expect(result.stderr).toBe("");
		expect(await filesystem.readTextFile("/quiet.txt")).toBe(
			"downloaded content",
		);
	}, 15_000);

	it("reports failure for a 404 URL", async () => {
		await mountKernel();

		const result = await kernel.exec(
			`wget http://127.0.0.1:${port}/missing.txt`,
			{ timeout: WGET_EXEC_TIMEOUT_MS },
		);

		expect(result.exitCode).not.toBe(0);
		expect(result.stderr).toMatch(/404|not found|error/i);
	}, 15_000);

	it("follows redirects by default", async () => {
		const filesystem = await mountKernel();

		const result = await kernel.exec(
			`wget -O /redirected.txt http://127.0.0.1:${port}/redirect`,
			{ timeout: WGET_EXEC_TIMEOUT_MS },
		);

		expect(result.exitCode, result.stderr || result.stdout).toBe(0);
		expect(await filesystem.readTextFile("/redirected.txt")).toBe(
			"arrived after redirect",
		);
	}, 15_000);
});
