#!/usr/bin/env node
// Owned local integration only. This script never installs, builds, or downloads.
import assert from "node:assert/strict";
import { spawn } from "node:child_process";
import { createHash, randomBytes } from "node:crypto";
import { createReadStream, createWriteStream } from "node:fs";
import {
	chmod,
	mkdir,
	mkdtemp,
	open,
	opendir,
	readFile,
	readlink,
	stat,
	writeFile,
} from "node:fs/promises";
import { createServer as createHttpServer } from "node:http";
import { createServer as createNetServer } from "node:net";
import { isAbsolute, join, resolve } from "node:path";
import { performance } from "node:perf_hooks";
import { setTimeout as delay } from "node:timers/promises";
import { fileURLToPath } from "node:url";

const supervisor = fileURLToPath(
	new URL("./hosted-owned-supervisor.py", import.meta.url),
);
const MAX_LOG_BYTES = 4 * 1024 * 1024;
const MAX_JSON_BYTES = 1024 * 1024;

async function jsonFile(path, maximum = MAX_JSON_BYTES) {
	assert(
		(await stat(path)).size <= maximum,
		`ERR_FIXTURE_JSON_LIMIT: ${path} exceeds ${maximum}`,
	);
	return JSON.parse(await readFile(path, "utf8"));
}
async function sha256(path) {
	const hash = createHash("sha256");
	for await (const chunk of createReadStream(path)) hash.update(chunk);
	return hash.digest("hex");
}
async function pinned(input) {
	assert(isAbsolute(input.path), "artifact paths must be absolute");
	assert.match(input.sha256, /^[0-9a-f]{64}$/);
	assert.equal(
		await sha256(input.path),
		input.sha256,
		`artifact changed: ${input.path}`,
	);
	return { ...input, bytes: (await stat(input.path)).size };
}
async function listen(server, port = 0) {
	await new Promise((accept, reject) => {
		server.once("error", reject);
		server.listen(port, "127.0.0.1", accept);
	});
	return server.address().port;
}
async function closeServer(server) {
	await new Promise((accept, reject) =>
		server.close((error) => (error ? reject(error) : accept())),
	);
}
async function reserveEnginePorts() {
	for (let attempt = 0; attempt < 16; attempt++) {
		const reservations = [];
		try {
			const first = createNetServer();
			reservations.push(first);
			const port = await listen(first);
			if (port > 65525) throw Error("neighbor port overflow");
			for (const offset of [1, 10]) {
				const next = createNetServer();
				reservations.push(next);
				await listen(next, port + offset);
			}
			return { port, reservations };
		} catch (error) {
			for (const item of reservations)
				if (item.listening) await closeServer(item);
			if (attempt === 15)
				throw new Error(
					"ERR_FIXTURE_PORT_RESERVATION: sixteen attempts failed",
					{ cause: error },
				);
		}
	}
	throw Error("unreachable port reservation");
}
function baseEnvironment() {
	// Inherit no credentials, proxy configuration, injection hooks, or telemetry exporters.
	const env = {
		PATH: process.env.PATH ?? "/usr/bin:/bin",
		LANG: "C.UTF-8",
		TZ: "UTC",
	};
	if (process.env.HOME) env.HOME = process.env.HOME;
	return env;
}

async function ownedProcess(label, argv, env, directory, maxMs, cwd) {
	assert.match(label, /^[a-z0-9-]{1,48}$/);
	assert(
		argv.length > 0 &&
			argv.length <= 64 &&
			argv.every((arg) => typeof arg === "string"),
	);
	const statusPath = join(directory, `${label}.status.json`);
	const child = spawn(
		"python3",
		[
			supervisor,
			"--status",
			statusPath,
			"--max-ms",
			String(Math.max(1000, Math.min(900000, Math.floor(maxMs)))),
			"--",
			...argv,
		],
		{
			detached: true,
			env,
			cwd,
			stdio: ["ignore", "pipe", "pipe"],
		},
	);
	let processExit;
	let failure;
	const logDone = [];
	const closed = new Promise((accept) => {
		child.once("error", (error) => {
			failure = error;
			accept();
		});
		child.once("close", (code, signal) => {
			processExit = { code, signal };
			accept();
		});
	});
	for (const [name, stream] of [
		["stdout", child.stdout],
		["stderr", child.stderr],
	]) {
		const log = createWriteStream(join(directory, `${label}.${name}.log`), {
			flags: "wx",
			highWaterMark: 64 * 1024,
		});
		logDone.push(
			new Promise((accept) => {
				log.once("finish", accept);
				log.once("close", accept);
			}),
		);
		stream.on("error", (error) => {
			failure = error;
			child.kill("SIGTERM");
		});
		let bytes = 0;
		stream.on("data", (chunk) => {
			const available = Math.max(0, MAX_LOG_BYTES - bytes);
			bytes += chunk.length;
			if (available > 0 && !log.write(chunk.subarray(0, available))) {
				stream.pause();
				log.once("drain", () => stream.resume());
			}
			if (bytes > MAX_LOG_BYTES && !failure) {
				failure = Error(
					`ERR_FIXTURE_LOG_LIMIT: ${label}.${name} exceeded ${MAX_LOG_BYTES} bytes`,
				);
				child.kill("SIGTERM");
			}
		});
		stream.on("end", () => log.end());
		log.on("error", (error) => {
			failure = error;
			child.kill("SIGTERM");
		});
	}
	async function status() {
		if (failure) throw failure;
		try {
			return await jsonFile(statusPath, 128 * 1024);
		} catch (error) {
			if (error.code === "ENOENT" && !processExit) return undefined;
			throw error;
		}
	}
	async function waitChild(timeoutMs, allowNonzero = false) {
		const end = performance.now() + timeoutMs;
		while (performance.now() < end) {
			const value = await status();
			if (value?.childExit !== null && value?.childExit !== undefined) {
				if (!allowNonzero)
					assert.equal(
						value.childExit,
						0,
						`${label} exited: ${JSON.stringify(value)}`,
					);
				return value;
			}
			if (processExit)
				throw Error(
					`${label} supervisor exited before child status: ${JSON.stringify(processExit)}`,
				);
			await delay(50);
		}
		throw Error(
			`ERR_FIXTURE_OPERATION_DEADLINE: ${label} exceeded ${timeoutMs}ms`,
		);
	}
	async function stop() {
		if (!processExit) {
			child.kill("SIGUSR1"); // Supervisor signals its unreaped child, never a recycled PID.
			try {
				await waitChild(40_000, true);
			} catch (error) {
				failure ??= error;
			}
			child.kill("SIGTERM"); // Anchor terminates/reaps its complete owned process group.
			await Promise.race([
				closed,
				delay(8_000, undefined, { ref: false }).then(() => {
					throw Error(`${label} owned group failed to close`);
				}),
			]);
		}
		await Promise.race([
			Promise.all(logDone),
			delay(5_000, undefined, { ref: false }).then(() => {
				throw Error(`${label} log flush deadline`);
			}),
		]);
		const value = await jsonFile(statusPath, 128 * 1024);
		assert(!value.forcedGroupKill, `${label} required forced group kill`);
		assert(
			!value.deadlineReached,
			`${label} reached supervisor whole deadline`,
		);
		if (failure) throw failure;
		assert(
			value.childExit === 0 ||
				(label === "engine" && value.childExit === -15 && value.softStopSent),
			`${label} abnormal child exit: ${JSON.stringify(value)}`,
		);
		return { label, ...value, supervisorExit: processExit };
	}
	return { label, status, waitChild, stop };
}
async function ownsListener(status, port) {
	const pid = status.childPid;
	const identity = await readFile(`/proc/${pid}/stat`, "ascii");
	const fields = identity.slice(identity.lastIndexOf(")") + 2).split(/\s+/);
	assert(
		fields[19] === status.childStartTicks &&
			Number(fields[1]) === status.supervisorPid &&
			Number(fields[2]) === status.pgid,
		"ERR_FIXTURE_PROCESS_IDENTITY: owned engine identity changed",
	);
	const inodes = new Set();
	let descriptors = 0;
	for await (const entry of await opendir(`/proc/${pid}/fd`)) {
		assert(
			++descriptors <= 4096,
			"ERR_FIXTURE_FD_CENSUS_LIMIT: owned engine exceeded 4096 descriptors",
		);
		try {
			const value = await readlink(`/proc/${pid}/fd/${entry.name}`);
			const match = /^socket:\[(\d+)\]$/.exec(value);
			if (match) inodes.add(match[1]);
		} catch (error) {
			if (error.code !== "ENOENT") throw error;
		}
	}
	const file = await open(`/proc/${pid}/net/tcp`, "r");
	const chunks = [];
	let size = 0;
	try {
		while (true) {
			const bytes = Buffer.alloc(65536);
			const read = await file.read(bytes, 0, bytes.length, null);
			if (!read.bytesRead) break;
			size += read.bytesRead;
			assert(size <= 512 * 1024, "ERR_FIXTURE_SOCKET_CENSUS_LIMIT");
			chunks.push(bytes.subarray(0, read.bytesRead));
		}
	} finally {
		await file.close();
	}
	const expected = `0100007F:${port.toString(16).toUpperCase().padStart(4, "0")}`;
	return Buffer.concat(chunks)
		.toString("ascii")
		.split("\n")
		.some((line) => {
			const fields = line.trim().split(/\s+/);
			return (
				fields[1] === expected && fields[3] === "0A" && inodes.has(fields[9])
			);
		});
}
async function startMirror(artifacts) {
	assert(
		artifacts.length > 0 && artifacts.length <= 16,
		"require 1..16 matching software artifacts",
	);
	const manifest = { schemaVersion: 1, artifacts: [] };
	const routes = new Map();
	for (const item of artifacts) {
		assert.match(item.name, /^[a-z0-9][a-z0-9-]{0,63}$/);
		assert(
			!manifest.artifacts.some((value) => value.name === item.name),
			"duplicate software name",
		);
		const packagePath = `packages/${item.name}/${item.sha256}.aospkg`;
		manifest.artifacts.push({
			name: item.name,
			digest: `sha256:${item.sha256}`,
			size: item.bytes,
			path: packagePath,
		});
		for (const release of ["0.0.1", "latest"])
			routes.set(`/${release}/software/${packagePath}`, item);
	}
	const bytes = Buffer.from(JSON.stringify(manifest));
	let active = 0;
	const server = createHttpServer((request, response) => {
		if (active >= 16) {
			response.writeHead(503).end("ERR_FIXTURE_HTTP_CONCURRENCY");
			return;
		}
		if (request.method !== "GET" && request.method !== "HEAD") {
			response.writeHead(405).end();
			return;
		}
		const path = new URL(request.url, "http://127.0.0.1").pathname;
		if (/^\/(0\.0\.1|latest)\/software\/manifest\.json$/.test(path)) {
			response.writeHead(200, {
				"content-type": "application/json",
				"content-length": bytes.length,
			});
			response.end(request.method === "HEAD" ? undefined : bytes);
			return;
		}
		const item = routes.get(path);
		if (!item) {
			response.writeHead(404).end();
			return;
		}
		response.writeHead(200, {
			"content-type": "application/octet-stream",
			"content-length": item.bytes,
		});
		if (request.method === "HEAD") {
			response.end();
			return;
		}
		active++;
		const stream = createReadStream(item.path);
		response.once("close", () => {
			active--;
			stream.destroy();
		});
		stream.on("error", (error) => response.destroy(error));
		stream.pipe(response);
	});
	server.maxConnections = 32;
	server.maxRequestsPerSocket = 32;
	server.requestTimeout = 30_000;
	server.headersTimeout = 5_000;
	server.keepAliveTimeout = 1_000;
	const port = await listen(server);
	return { server, url: `http://127.0.0.1:${port}/`, manifest };
}
async function requestJson(endpoint, path, token, method = "GET", body) {
	const response = await fetch(new URL(path, endpoint), {
		method,
		redirect: "error",
		signal: AbortSignal.timeout(2_000),
		headers: {
			authorization: `Bearer ${token}`,
			"content-type": "application/json",
		},
		body: body === undefined ? undefined : JSON.stringify(body),
	});
	const reader = response.body.getReader();
	let size = 0;
	const chunks = [];
	while (true) {
		const { value, done } = await reader.read();
		if (done) break;
		size += value.length;
		if (size > MAX_JSON_BYTES) {
			await reader.cancel();
			throw Error("ERR_FIXTURE_HTTP_RESPONSE_LIMIT");
		}
		chunks.push(value);
	}
	const data = Buffer.concat(chunks).toString("utf8");
	assert(
		response.ok,
		`${method} ${path}: ${response.status} ${data.slice(0, 1024)}`,
	);
	return JSON.parse(data);
}
async function main(configPath) {
	const config = await jsonFile(configPath);
	const actorBudgetArgs = [];
	if (config.workerActorBudgetsJson !== undefined) {
		assert(typeof config.workerActorBudgetsJson === "string", "workerActorBudgetsJson must be a JSON string");
		JSON.parse(config.workerActorBudgetsJson);
		// The worker owns semantic validation. This fixture only forwards explicit startup input.
		actorBudgetArgs.push(`--actor-budgets-json=${config.workerActorBudgetsJson}`);
	}
	assert(
		Array.isArray(config.commands) &&
			config.commands.length > 0 &&
			config.commands.length <= 8,
	);
	const labels = new Set(["engine", "worker"]);
	for (const command of config.commands) {
		assert.match(command.label, /^[a-z0-9-]{1,48}$/);
		assert(
			!labels.has(command.label),
			"fixture labels must be unique and not engine/worker",
		);
		labels.add(command.label);
		assert(
			Array.isArray(command.argv) && isAbsolute(command.argv[0]),
			"probe executable must be absolute",
		);
		await pinned({ path: command.argv[0], sha256: command.executableSha256 });
	}
	assert(
		Array.isArray(config.inputs) &&
			config.inputs.length > 0 &&
			config.inputs.length <= 1024,
		"require1..1024 explicit SDK/test/source input pins",
	);
	const inputs = [];
	for (const input of config.inputs) inputs.push(await pinned(input));
	assert(isAbsolute(config.repo) && isAbsolute(config.evidenceParent));
	const wholeMs = config.wholeMs ?? 600_000;
	assert(wholeMs >= 60_000 && wholeMs <= 900_000);
	assert(
		Array.isArray(config.software) &&
			config.software.length > 0 &&
			config.software.length <= 16,
		"require 1..16 pinned software artifacts",
	);
	const engine = await pinned(config.engine);
	const worker = await pinned(config.worker);
	const artifacts = await Promise.all(config.software.map(pinned));
	const directory = await mkdtemp(join(config.evidenceParent, "hosted-local-"));
	await chmod(directory, 0o700);
	const state = join(directory, "state");
	await mkdir(state);
	const token = randomBytes(24).toString("hex");
	const namespace = `hosted-${randomBytes(8).toString("hex")}`;
	const poolName = `worker-${randomBytes(8).toString("hex")}`;
	const groups = [];
	const cleanup = [];
	const report = {
		engine,
		worker,
		artifacts,
		inputs,
		namespace,
		poolName,
		commands: [],
		cleanup,
		acceptance: "incomplete",
	};
	let mirror;
	let reservations = [];
	const started = performance.now();
	const remaining = () => Math.max(0, wholeMs - (performance.now() - started));
	const base = baseEnvironment();
	try {
		mirror = await startMirror(artifacts);
		const ports = await reserveEnginePorts();
		reservations = ports.reservations;
		const endpoint = `http://127.0.0.1:${ports.port}`;
		report.endpoint = endpoint;
		report.softwareManifest = mirror.manifest;
		const engineConfig = join(state, "engine.json");
		await writeFile(
			engineConfig,
			JSON.stringify({
				auth: {
					admin_token: token,
					insecure_allow_unauthenticated: false,
					jwt: { enabled: true },
				},
				telemetry: { enabled: false },
				topology: {
					datacenter_label: 1,
					datacenters: {
						default: {
							datacenter_label: 1,
							is_leader: true,
							public_url: endpoint,
							peer_url: `http://127.0.0.1:${ports.port + 1}`,
						},
					},
				},
			}),
			{ mode: 0o600 },
		);
		for (const socket of reservations) await closeServer(socket);
		reservations = [];
		const engineEnv = {
			...base,
			RIVETKIT_STORAGE_PATH: state,
			RIVET__GUARD__HOST: "127.0.0.1",
			RIVET__GUARD__PORT: String(ports.port),
			RIVET__API_PEER__HOST: "127.0.0.1",
			RIVET__API_PEER__PORT: String(ports.port + 1),
			RIVET__METRICS__HOST: "127.0.0.1",
			RIVET__METRICS__PORT: String(ports.port + 10),
			RIVET__STORAGE__LOG_DIRECTORY: join(state, "depot-log"),
			RIVET__OBJECT_STORAGE__PRIMARY__FILE_SYSTEM__PATH: join(
				state,
				"depot-objects",
			),
			RIVET__STORAGE__BALANCING__ENABLED: "false",
			RIVET__STORAGE__AUTO_PROVISION_PARTITIONS: "1",
			RIVET__STORAGE__LOG_SHARD__MAX_DISK_BYTES: "4294967296",
			RIVET__STORAGE__COLD_STORE__DISK_CACHE_BYTES: "0",
			RIVET__STORAGE__WRITE_TO_COLD_AFTER_MS: "5000",
			RIVET__STORAGE__STOP_TIMEOUT_MS: "6000",
			RIVET__STORAGE__STOP_TEARDOWN_MS: "20000",
			RIVET__PEGBOARD__NODE_LOST_THRESHOLD: "7000",
			RIVET__RUNTIME__WORKER_SHUTDOWN_DURATION: "1",
			RIVET__RUNTIME__GUARD_SHUTDOWN_DURATION: "1",
			RIVET__RUNTIME__FORCE_SHUTDOWN_DURATION: "30",
		};
		await mkdir(join(state, "depot-log"));
		const engineProcess = await ownedProcess(
			"engine",
			[engine.path, "--config", engineConfig],
			engineEnv,
			directory,
			remaining(),
			config.repo,
		);
		groups.push(engineProcess);
		let health;
		const readyEnd = performance.now() + Math.min(30_000, remaining());
		while (performance.now() < readyEnd) {
			const engineStatus = await engineProcess.status();
			assert(
				engineStatus?.childExit == null,
				"owned engine exited before readiness",
			);
			if (!engineStatus || !(await ownsListener(engineStatus, ports.port))) {
				await delay(100);
				continue;
			}
			try {
				health = await requestJson(endpoint, "/health", token);
				if (health.runtime === "engine") break;
			} catch (error) {
				report.lastEngineReadinessError = String(error);
			}
			await delay(100);
		}
		assert.equal(health?.runtime, "engine", "owned engine readiness deadline");
		assert.equal(
			health.version,
			config.engine.expectedVersion,
			"cached engine version differs from explicit expected version",
		);
		assert.equal(
			health.architecture,
			"v3",
			"owned fixture requires the3.0.7 v3 architecture",
		);
		// Health is liveness, not storage command readiness. Retry only these
		// bounded reads; namespace/pool creation mutations are never replayed.
		let storageReady = false;
		const storageEnd = performance.now() + Math.min(30_000, remaining());
		while (performance.now() < storageEnd) {
			assert(
				(await engineProcess.status())?.childExit == null,
				"owned engine exited before storage readiness",
			);
			try {
				await requestJson(endpoint, "/ready", token);
				const namespaces = await requestJson(
					endpoint,
					"/namespaces?limit=1",
					token,
				);
				assert(
					Array.isArray(namespaces.namespaces),
					"owned namespace readiness response has no catalog page",
				);
				storageReady = true;
				break;
			} catch (error) {
				report.lastStorageReadinessError = String(error);
			}
			await delay(100);
		}
		assert(storageReady, "owned storage command readiness deadline");
		report.health = health;
		await requestJson(endpoint, "/namespaces", token, "POST", {
			name: namespace,
		});
		// Use the compiled3.0.7 public shim's canonical node/pool contract.
		// The admin credential only bootstraps this fresh owned namespace/pool
		// and mints separate node and application JWTs.
		await requestJson(
			endpoint,
			`/pools/${poolName}?namespace=${namespace}`,
			token,
			"PUT",
			{
				drain_on_version_upgrade: false,
			},
		);
        // Published3.0.7 loopback serving always updates the pool before
        // connecting. This extra namespace-wide permission is fixture-only;
        // the production credential example deliberately does not grant it.
		const workerGrants = [
			{ resource: "node", target: "any", operations: ["create"] },
            { resource: "pool", target: "any", operations: ["update"] },
			{ resource: "actor", target: "any", operations: ["create", "read"] },
			{ resource: "actor_gateway", target: "any", operations: ["create", "read"] },
		];
		const applicationGrants = [
			{
				resource: "actor",
				target: "any",
				operations: ["create", "read", "update", "delete", "list"],
			},
			{ resource: "actor_gateway", target: "any", operations: ["create", "read"] },
		];
		const workerCredential = await requestJson(
			endpoint,
			"/auth/tokens",
			token,
			"POST",
			{
				namespace,
				duration: 1800,
				subject: "owned-node",
				grants: workerGrants,
			},
		);
		const applicationCredential = await requestJson(
			endpoint,
			"/auth/tokens",
			token,
			"POST",
			{
				namespace,
				duration: 1800,
				subject: "owned-application",
				grants: applicationGrants,
			},
		);
		assert(
			typeof workerCredential.token === "string" &&
				typeof applicationCredential.token === "string",
		);
		report.authentication = {
			workerGrants,
			applicationGrants,
			workerExpiresTs: workerCredential.expires_ts,
			applicationExpiresTs: applicationCredential.expires_ts,
			insecureAllowUnauthenticated: false,
            fixtureOnlyPoolUpdate: { namespace, target: "any", exactPoolIsolation: false },
		};
		const workerEnv = {
			...base,
			...(config.workerEnv ?? {}),
			RIVET_ENDPOINT: endpoint,
			RIVET_NAMESPACE: namespace,
			RIVET_TOKEN: workerCredential.token,
			RIVET_POOL: poolName,
			RIVETKIT_ENGINE_SPAWN: "never",
			RIVETKIT_ENGINE_AUTO_DOWNLOAD: "0",
			RIVETKIT_STORAGE_PATH: state,
			AGENTOS_SIDECAR_BIN: worker.path,
			AGENTOS_SOFTWARE_REGISTRY_URL: mirror.url,
			AGENTOS_ALLOW_INSECURE_LOCAL_PACKAGE_HTTP: "1",
		};
		const actor = await ownedProcess(
			"worker",
			[
				worker.path,
				"actor",
				"--worker-shutdown-timeout-ms=30000",
				`--package-cache-dir=${join(state, "packages")}`,
				"--package-cache-max-bytes=536870912",
				"--package-cache-max-entries=32",
				...actorBudgetArgs,
			],
			workerEnv,
			directory,
			remaining(),
			config.repo,
		);
		groups.push(actor);
		const workerReadyEnd = performance.now() + Math.min(45_000, remaining());
		let names;
		while (performance.now() < workerReadyEnd) {
			assert(
				(await actor.status())?.childExit == null,
				"owned actor worker exited before registration",
			);
			try {
				names = await requestJson(
					endpoint,
					`/actors/names?namespace=${namespace}&limit=16`,
					token,
				);
				if (names.names?.agentOS) break;
			} catch (error) {
				report.lastWorkerReadinessError = String(error);
			}
			await delay(100);
		}
		assert(names?.names?.agentOS, "owned worker registration deadline");
		const sources = mirror.manifest.artifacts.map((artifact) => ({
			url: new URL(`0.0.1/software/${artifact.path}`, mirror.url).href,
			digest: artifact.digest,
		}));
		const probeEnv = {
			...workerEnv,
			AGENTOS_HOSTED_TEST_REQUIRE_LOCAL: "1",
			AGENTOS_HOSTED_TEST_ENDPOINT: endpoint,
			AGENTOS_HOSTED_TEST_NAMESPACE: namespace,
			AGENTOS_HOSTED_TEST_TOKEN: applicationCredential.token,
			AGENTOS_HOSTED_TEST_ADMIN_TOKEN: token,
			AGENTOS_HOSTED_TEST_NODE_TOKEN: workerCredential.token,
			AGENTOS_HOSTED_TEST_POOL: poolName,
			AGENTOS_HOSTED_TEST_SOFTWARE_JSON: JSON.stringify(sources),
			AGENTOS_HOSTED_TEST_BAD_TOKEN: randomBytes(24).toString("hex"),
		};
		for (const command of config.commands) {
			assert(remaining() >= 1000, "ERR_FIXTURE_WHOLE_DEADLINE");
			const budget = Math.min(
				command.timeoutMs ?? 120_000,
				300_000,
				remaining(),
			);
			const probe = await ownedProcess(
				command.label,
				command.argv,
				probeEnv,
				directory,
				budget,
				config.repo,
			);
			groups.push(probe);
			const outcome = await probe.waitChild(budget);
			report.commands.push({
				label: command.label,
				argv: command.argv,
				executableSha256: command.executableSha256,
				outcome,
			});
			cleanup.push(await probe.stop());
			groups.pop();
		}
		report.acceptance =
			"configured commands passed; auth coverage is only what those commands actually assert";
	} catch (error) {
		report.failure = { message: String(error), stack: error.stack };
	} finally {
		for (const group of groups.reverse()) {
			try {
				cleanup.push(await group.stop());
			} catch (error) {
				cleanup.push({ label: group.label, failure: String(error) });
				report.acceptance = "failed cleanup";
			}
		}
		for (const socket of reservations)
			if (socket.listening) await closeServer(socket);
		if (mirror) {
			mirror.server.closeAllConnections();
			await closeServer(mirror.server);
		}
		try {
			report.finalPins = {
				engine: await sha256(engine.path),
				worker: await sha256(worker.path),
				software: await Promise.all(
					artifacts.map(async (item) => ({
						name: item.name,
						sha256: await sha256(item.path),
					})),
				),
			};
			report.finalExecutablePins = [];
			for (const command of config.commands) {
				const actual = await sha256(command.argv[0]);
				assert.equal(
					actual,
					command.executableSha256,
					`probe executable changed: ${command.label}`,
				);
				report.finalExecutablePins.push({
					label: command.label,
					path: command.argv[0],
					sha256: actual,
				});
			}
			report.finalInputPins = [];
			for (const input of inputs) {
				const actual = await sha256(input.path);
				assert.equal(
					actual,
					input.sha256,
					`fixture input changed: ${input.path}`,
				);
				report.finalInputPins.push({ path: input.path, sha256: actual });
			}
			assert.equal(report.finalPins.engine, engine.sha256);
			assert.equal(report.finalPins.worker, worker.sha256);
			for (const item of report.finalPins.software)
				assert.equal(
					item.sha256,
					artifacts.find((value) => value.name === item.name).sha256,
				);
		} catch (error) {
			report.finalPinFailure = String(error);
			report.acceptance = "failed provenance verification";
		}
		await writeFile(
			join(directory, "report.json"),
			`${JSON.stringify(report, null, 2)}\n`,
			{ mode: 0o600 },
		);
	}
	console.log(
		JSON.stringify({
			evidence: directory,
			acceptance: report.acceptance,
			failure: report.failure?.message,
		}),
	);
	assert(
		!report.failure &&
			!report.finalPinFailure &&
			!cleanup.some((value) => value.failure),
		"owned hosted fixture failed; inspect private evidence",
	);
}

if (process.argv.length !== 4 || process.argv[2] !== "--run") {
	console.error(
		"Usage: node scripts/run-hosted-local.mjs --run /absolute/fixture-config.json (starts owned local processes)",
	);
	process.exitCode = 2;
} else {
	await main(resolve(process.argv[3]));
}
