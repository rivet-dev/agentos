import { type ChildProcess, spawn } from "node:child_process";
import { once } from "node:events";
import {
	cpSync,
	existsSync,
	mkdirSync,
	mkdtempSync,
	rmSync,
	symlinkSync,
	writeFileSync,
} from "node:fs";
import { createServer } from "node:net";
import { tmpdir } from "node:os";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import { afterAll, beforeAll, describe, expect, it } from "vitest";
import {
	ACTOR_E2E_NAMESPACE,
	ACTOR_E2E_TOKEN,
	type ActorRuntimeHandle,
	startActorRuntime,
} from "../../agentos/tests/helpers/actor-runtime.js";

const RUN_E2E = process.env.AGENTOS_ACTOR_E2E === "1";
const packageRoot = dirname(dirname(fileURLToPath(import.meta.url)));
const workspaceRoot = resolve(packageRoot, "../..");
const exampleRoot = join(workspaceRoot, "examples/flue");
const rivetFlueRoot = process.env.AGENTOS_FLUE_RIVET_PACKAGE;
const flueForkRoot = process.env.AGENTOS_FLUE_FORK_ROOT;
const MAX_LOG_BYTES = 1024 * 1024;

describe.skipIf(!RUN_E2E)("Flue + Rivet + agentOS real actor E2E", () => {
	let runtime: ActorRuntimeHandle;
	let storagePath: string;
	let fixtureRoot: string;
	let server: RunningServer | undefined;

	beforeAll(async () => {
		if (!rivetFlueRoot || !flueForkRoot) {
			throw new Error(
				"Set AGENTOS_FLUE_RIVET_PACKAGE to the local @rivet-dev/flue package and AGENTOS_FLUE_FORK_ROOT to the local rivet-dev/flue checkout.",
			);
		}
		storagePath = mkdtempSync(join(tmpdir(), "agentos-flue-combined-e2e-"));
		fixtureRoot = join(storagePath, "example");
		createFixture(fixtureRoot, rivetFlueRoot, flueForkRoot);
		await run(
			process.execPath,
			[join(flueForkRoot, "packages/cli/bin/flue.mjs"), "build"],
			fixtureRoot,
		);
		runtime = await startActorRuntime(storagePath);
	}, 180_000);

	afterAll(async () => {
		await server?.stop();
		await runtime?.stop();
		if (storagePath) rmSync(storagePath, { recursive: true, force: true });
	}, 30_000);

	it(
		"persists filesystem and exec output after reconnecting through the native Flue router",
		async () => {
			const poolName = `agentos-flue-${Date.now()}`;
			await configurePool(runtime.endpoint, poolName);
			const port = await getFreePort();
			const instanceId = `combined-${Date.now()}`;

			server = startServer(fixtureRoot, runtime.endpoint, poolName, port);
			await waitForServer(port, server);
			await postPrompt(port, instanceId, "write the persistent file", server);
			await waitForHistory(
				port,
				instanceId,
				["persisted-through-agentos", "The persistent file was written."],
				server,
			);

			// Drop every Flue-side connection and registry resource. Starting the
			// generated server again must reconnect the same Flue context to the
			// same agentOS actor and its actor-owned filesystem.
			await server.stop();
			server = startServer(fixtureRoot, runtime.endpoint, poolName, port);
			await waitForServer(port, server);
			await postPrompt(port, instanceId, "read the persistent file", server);
			const history = await waitForHistory(
				port,
				instanceId,
				[
					"persisted-through-agentos",
					"The persistent file survived reconnecting.",
				],
				server,
			);

			expect(JSON.stringify(history)).toContain("cat persisted.txt");
		},
		180_000,
	);
});

function createFixture(
	root: string,
	localRivetFlueRoot: string,
	localFlueForkRoot: string,
): void {
	cpSync(exampleRoot, root, {
		recursive: true,
		filter: (source) =>
			!["dist", ".turbo", "node_modules"].includes(source.split("/").at(-1) ?? ""),
	});
	writeFileSync(
		join(root, "tsconfig.json"),
		JSON.stringify({
			compilerOptions: {
				lib: ["ESNext", "DOM"],
				module: "ESNext",
				moduleResolution: "Bundler",
				strict: true,
				target: "ES2023",
				types: ["node"],
			},
			include: ["**/*.ts"],
		}),
	);
	writeDeterministicAgent(root);
	const modules = join(root, "node_modules");
	mkdirSync(join(modules, "@flue"), { recursive: true });
	mkdirSync(join(modules, "@rivet-dev"), { recursive: true });

	link(
		join(localFlueForkRoot, "packages/cli"),
		join(modules, "@flue/cli"),
	);
	link(
		join(localFlueForkRoot, "packages/runtime"),
		join(modules, "@flue/runtime"),
	);
	copyPublishedPackage(
		localRivetFlueRoot,
		join(modules, "@rivet-dev/flue"),
	);
	copyPublishedPackage(packageRoot, join(modules, "@rivet-dev/agentos-flue"));
	copyPublishedPackage(
		join(workspaceRoot, "packages/agentos"),
		join(modules, "@rivet-dev/agentos"),
	);
	link(
		join(workspaceRoot, "packages/core"),
		join(modules, "@rivet-dev/agentos-core"),
	);
	link(
		join(localRivetFlueRoot, "node_modules/rivetkit"),
		join(modules, "rivetkit"),
	);
}

function writeDeterministicAgent(root: string): void {
	writeFileSync(
		join(root, "agents/assistant.ts"),
		`import { createAgent, registerProvider } from "@flue/runtime";
import { fauxAssistantMessage, registerFauxProvider } from "@flue/runtime/adapter-kit";
import { agentOSSandbox } from "@rivet-dev/agentos-flue";
import { registry } from "../actors.js";

export const route = async (_context, next) => next();

const provider = registerFauxProvider({ provider: "agentos-flue-e2e" });
const respond = (context) => {
    const latestUser = context.messages.findLast((message) => message.role === "user");
    const userText = latestUser?.role === "user"
      ? typeof latestUser.content === "string"
        ? latestUser.content
        : latestUser.content.map((block) => block.type === "text" ? block.text : "").join("")
      : "";
    if (context.messages.at(-1)?.role !== "user") {
      return fauxAssistantMessage(userText.includes("read")
        ? "The persistent file survived reconnecting."
        : "The persistent file was written.");
    }
    const reading = userText.includes("read");
    return fauxAssistantMessage({
      type: "toolCall",
      id: reading ? "read-persistent-file" : "write-persistent-file",
      name: "bash",
      arguments: {
        command: reading
          ? "cat persisted.txt"
          : "printf 'persisted-through-agentos' > persisted.txt && cat persisted.txt",
      },
    }, { stopReason: "toolUse" });
};
provider.setResponses(Array.from({ length: 8 }, () => respond));
const model = provider.getModel();
registerProvider(model.provider, { api: provider.api, baseUrl: model.baseUrl });

export default createAgent(() => ({
  model: model.provider + "/" + model.id,
  sandbox: agentOSSandbox({ actor: "vm", registry }),
}));
`,
	);
}

function copyPublishedPackage(source: string, target: string): void {
	if (!existsSync(join(source, "dist"))) {
		throw new Error(`Build local package before E2E: ${source}`);
	}
	mkdirSync(target, { recursive: true });
	cpSync(join(source, "dist"), join(target, "dist"), { recursive: true });
	cpSync(join(source, "package.json"), join(target, "package.json"));
}

function link(source: string, target: string): void {
	if (!existsSync(source)) throw new Error(`Missing local package: ${source}`);
	symlinkSync(source, target, "dir");
}

async function configurePool(endpoint: string, poolName: string): Promise<void> {
	const headers = { Authorization: `Bearer ${ACTOR_E2E_TOKEN}` };
	const datacentersResponse = await fetch(
		`${endpoint}/datacenters?namespace=${ACTOR_E2E_NAMESPACE}`,
		{ headers },
	);
	if (!datacentersResponse.ok) {
		throw new Error(`failed to list E2E datacenters: ${datacentersResponse.status}`);
	}
	const datacenter = (
		(await datacentersResponse.json()) as {
			datacenters: Array<{ name: string }>;
		}
	).datacenters[0]?.name;
	if (!datacenter) throw new Error("E2E Engine returned no datacenter");
	const response = await fetch(
		`${endpoint}/runner-configs/${poolName}?namespace=${ACTOR_E2E_NAMESPACE}`,
		{
			method: "PUT",
			headers: { ...headers, "content-type": "application/json" },
			body: JSON.stringify({ datacenters: { [datacenter]: { normal: {} } } }),
		},
	);
	if (!response.ok) {
		throw new Error(`failed to configure E2E pool: ${await response.text()}`);
	}
}

interface RunningServer {
	child: ChildProcess;
	logs(): string;
	stop(): Promise<void>;
}

function startServer(
	root: string,
	endpoint: string,
	poolName: string,
	port: number,
): RunningServer {
	const child = spawn(process.execPath, [join(root, "dist/server.mjs")], {
		cwd: root,
		env: {
			...process.env,
			AGENTOS_SIDECAR_BIN: resolve(workspaceRoot, "target/debug/agentos-sidecar"),
			FLUE_MODE: "local",
			PORT: String(port),
			RIVET_ENDPOINT: endpoint,
			RIVET_NAMESPACE: ACTOR_E2E_NAMESPACE,
			RIVET_POOL: poolName,
			RIVET_TOKEN: ACTOR_E2E_TOKEN,
			RIVETKIT_ENGINE_SPAWN: "never",
		},
		stdio: ["ignore", "pipe", "pipe"],
	});
	let output = "";
	for (const stream of [child.stdout, child.stderr]) {
		stream?.setEncoding("utf8");
		stream?.on("data", (chunk: string) => {
			output = (output + chunk).slice(-MAX_LOG_BYTES);
		});
	}
	return {
		child,
		logs: () => output,
		async stop() {
			if (child.exitCode !== null || child.signalCode !== null) return;
			child.kill("SIGTERM");
			await Promise.race([
				once(child, "exit"),
				new Promise((resolveStop) => setTimeout(resolveStop, 10_000)),
			]);
			if (child.exitCode === null && child.signalCode === null) {
				child.kill("SIGKILL");
				await once(child, "exit");
			}
		},
	};
}

async function waitForServer(
	port: number,
	server: RunningServer,
): Promise<void> {
	await retry(async () => {
		if (server.child.exitCode !== null) {
			throw new Error(`Flue server exited\n${server.logs()}`);
		}
		const response = await fetch(`http://127.0.0.1:${port}/`);
		return response.status < 500;
	}, server);
}

async function postPrompt(
	port: number,
	instanceId: string,
	body: string,
	server: RunningServer,
): Promise<void> {
	await retry(async () => {
		const response = await fetch(
			`http://127.0.0.1:${port}/agents/assistant/${instanceId}`,
			{
				method: "POST",
				headers: { "content-type": "application/json" },
				body: JSON.stringify({ kind: "user", body }),
			},
		);
		if (response.status === 202) return true;
		if (response.status < 500) {
			throw new Error(`prompt failed: ${response.status} ${await response.text()}`);
		}
		return false;
	}, server);
}

async function waitForHistory(
	port: number,
	instanceId: string,
	expected: string[],
	server: RunningServer,
): Promise<unknown> {
	let history: unknown;
	await retry(async () => {
		const response = await fetch(
			`http://127.0.0.1:${port}/agents/assistant/${instanceId}?view=history`,
		);
		if (!response.ok) return false;
		history = await response.json();
		const text = JSON.stringify(history);
		return expected.every((value) => text.includes(value));
	}, server, 120);
	return history;
}

async function retry(
	runAttempt: () => Promise<boolean>,
	server: RunningServer,
	attempts = 60,
): Promise<void> {
	let lastError: unknown;
	for (let attempt = 0; attempt < attempts; attempt++) {
		try {
			if (await runAttempt()) return;
		} catch (error) {
			lastError = error;
		}
		await new Promise((resolveDelay) => setTimeout(resolveDelay, 500));
	}
	throw new Error(
		`E2E condition timed out: ${String(lastError ?? "not ready")}\n${server.logs()}`,
	);
}

async function run(
	command: string,
	args: string[],
	cwd: string,
): Promise<void> {
	const child = spawn(command, args, {
		cwd,
		stdio: ["ignore", "pipe", "pipe"],
	});
	let output = "";
	for (const stream of [child.stdout, child.stderr]) {
		stream.setEncoding("utf8");
		stream.on("data", (chunk: string) => {
			output += chunk;
		});
	}
	const [code] = (await once(child, "exit")) as [number | null];
	if (code !== 0) throw new Error(`${command} failed (${code})\n${output}`);
}

async function getFreePort(): Promise<number> {
	const server = createServer();
	server.listen(0, "127.0.0.1");
	await once(server, "listening");
	const address = server.address();
	if (!address || typeof address === "string") {
		throw new Error("failed to allocate E2E port");
	}
	server.close();
	await once(server, "close");
	return address.port;
}
