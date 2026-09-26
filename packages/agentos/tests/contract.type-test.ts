import type {
	AgentOsActorCreateInput,
	AgentOsActorHandle,
	AgentOsClient,
	ConfigPatchOutput,
	ConfigSetOutput,
	FilesystemReaddirRecursiveOutput,
	FilesystemStatOutput,
	NetworkFetchStreamStartOutput,
	ProcessTreeOutput,
	SoftwareInstallInput,
	VmStatusOutput,
} from "../src/index";
import { agentOsSource } from "../src/inspector-tabs/lib/source";

declare const actor: AgentOsActorHandle;
declare const client: AgentOsClient;
const actorById: AgentOsActorHandle = client.getForId("agentOS", "actor-id");
void actorById;

const createInput: AgentOsActorCreateInput = {
	config: {
		environment: { CI: "true" },
		preview: { defaultTtlMs: 30_000 },
		filesystem: {},
		software: [{ url: "https://packages.example/tool.aospkg" }],
	},
};
void createInput;

const status: Promise<VmStatusOutput> = actor.vm.status({});
const config: Promise<ConfigSetOutput> = actor.config.set({ config: {} });
const patched: Promise<ConfigPatchOutput> = actor.config.patch({
	patch: { environment: { DEBUG: "1" } },
});
const ran = actor.process.run({
	command: "sh",
	args: ["-c", "printf hello"],
	options: { env: {} },
});
const replay = actor.process.output.read({
	process: { generation: 1, pid: 7 },
	maxEvents: 32,
	maxBytes: 64 * 1024,
});
const stream: Promise<NetworkFetchStreamStartOutput> =
	actor.network.fetchStream.start({
		request: {
			port: 3000,
			path: "/health",
			method: "GET",
			headers: {},
		},
	});
void status;
void config;
void patched;
void ran;
void replay;
void stream;

// Serde-defaulted fields remain optional throughout nested public inputs.
void actor.process.run({ command: "true" });
void actor.process.run({ command: "true", options: {} });
void actor.process.spawn({ command: "true" });
void actor.terminal.open({});
void actor.cron.schedule({ expression: "* * * * *", command: "true" });
void actor.network.fetch({ request: { port: 3000, path: "/health" } });
void actor.filesystem.export({});
void actor.javascript.execute({ source: "1 + 1" });
void actor.javascript.execute({
	source: "1 + 1",
	options: { inline: { process: { output: {} } } },
});
void actor.javascript.npm.install({});
void actor.typescript.check({ source: "const value: number = 1" });
void actor.python.install({ options: {} });

// Exact integer handles, revisions, and cursors can be returned to the actor
// without coercing a potentially large bigint to an imprecise number.
declare const observedRevision: ConfigSetOutput["revision"];
void actor.config.patch({ expectedRevision: observedRevision, patch: {} });
void actor.config.set({ config: { limits: { agentosPackages: {} } } });
void actor.config.set({ config: { limits: { tls: {}, execution: {} } } });
void actor.config.set({
	config: {
		limits: {
			tls: { maxBufferedBytes: 2048 },
			execution: {
				completedTtlMs: 60_000,
				maxCompletedExecutions: 128n,
				liveExecutionWarningThreshold: 32,
			},
		},
	},
});
void actor.config.set({
	config: { limits: { agentosPackages: { maxMounts: 8192 } } },
});
void actor.config.set({
	config: { limits: { agentosPackages: { maxMounts: 8192n } } },
});
void actor.process.output.read({
	process: { generation: 1n, pid: 7 },
	after: 9_007_199_254_740_993n,
});

const remoteSoftware: SoftwareInstallInput = {
	source: { url: "https://packages.example/tool.aospkg" },
};
void remoteSoftware;

// Output DTOs name byte/time units and keep process exits structured at every depth.
declare const fileStat: FilesystemStatOutput;
const fileSizeBytes: number | bigint = fileStat.sizeBytes;
declare const directoryEntry: FilesystemReaddirRecursiveOutput[number];
const entrySizeBytes: number | bigint = directoryEntry.sizeBytes;
const entryType: "file" | "directory" | "symlink" = directoryEntry.type;
declare const processNode: ProcessTreeOutput["roots"][number];
const processStartTimeMs: number = processNode.startTimeMs;
const processExitTimeMs: number | null | undefined = processNode.exitTimeMs;
const processExitCode: number | undefined = processNode.exit?.exitCode;
const processHandle:
	| { generation: number | bigint; pid: number }
	| null
	| undefined = processNode.process;
const processChild: ProcessTreeOutput["roots"][number] | undefined =
	processNode.children[0];
if (processHandle) {
	void actor.process.get({ process: processHandle });
	void actor.process.output.read({ process: processHandle });
	void actor.process.signal({ process: processHandle, signal: "SIGTERM" });
	void agentOsSource.processOutputReader(
		processHandle.generation,
		processHandle.pid,
	)(9_007_199_254_740_993n);
}
// @ts-expect-error guest tree nodes can lack a process handle
void actor.process.signal({ process: processNode.process, signal: "SIGTERM" });
void [
	fileSizeBytes,
	entrySizeBytes,
	entryType,
	processStartTimeMs,
	processExitTimeMs,
	processExitCode,
	processHandle,
	processChild,
];
// @ts-expect-error byte counts use sizeBytes in actor metadata
fileStat.size;
// @ts-expect-error recursive directory byte counts use sizeBytes
directoryEntry.size;
// @ts-expect-error the recursive entry discriminant is type, not entryType
directoryEntry.entryType;
// @ts-expect-error tree timestamps carry millisecond units
processNode.startTime;
// @ts-expect-error tree timestamps carry millisecond units
processNode.exitTime;
// @ts-expect-error tree exits are structured
processNode.exitCode;

const filesystemDefaults: AgentOsActorCreateInput = {
	config: {
		filesystem: {
			root: { type: "actor-sqlite" },
			mounts: [
				{
					path: "/data",
					backend: { type: "actor-sqlite" },
				},
			],
		},
	},
};
void filesystemDefaults;

const hostSoftware: SoftwareInstallInput = {
	// @ts-expect-error hosted software cannot read a host path
	source: { path: "/tmp/tool.aospkg" },
};
void hostSoftware;

// @ts-expect-error agents and sessions are absent from the hosted actor contract
actor.sessions.create({});

// @ts-expect-error reserved maintenance actions are not public client methods
actor.__agentos.cron.invoke({});

// @ts-expect-error shell-string process.exec was removed from the actor API
actor.process.exec({ command: "echo hello" });
