import coreutils from "@agentos-software/coreutils";
import {
	type AgentOsConformanceAction,
	type AgentOsConformanceBackend,
	type AgentOsConformanceEvent,
	CONFORMANCE_ACP_ADAPTER,
	CONFORMANCE_AGENT_NAME,
	defineAgentOsConformanceSuite,
} from "@rivet-dev/agentos-test-harness/agent-os-conformance";
import { createProjectedAgentPackage } from "@rivet-dev/agentos-test-harness/projected-agent-package";
import { AgentOs, type PermissionReply } from "../src/index.js";

class EventBus {
	readonly handlers = new Map<string, Set<(payload: any) => void>>();

	on(event: string, handler: (payload: any) => void): () => void {
		let handlers = this.handlers.get(event);
		if (!handlers) {
			handlers = new Set();
			this.handlers.set(event, handlers);
		}
		handlers.add(handler);
		return () => handlers?.delete(handler);
	}

	emit(event: string, payload: unknown): void {
		for (const handler of this.handlers.get(event) ?? []) handler(payload);
	}
}

async function createCoreBackend(): Promise<AgentOsConformanceBackend> {
	const events = new EventBus();
	const agentPackage = createProjectedAgentPackage({
		name: CONFORMANCE_AGENT_NAME,
		adapterScript: CONFORMANCE_ACP_ADAPTER,
	});
	const mounts = [
		{
			path: "/conformance-mount",
			plugin: {
				id: "host_dir" as const,
				config: {
					hostPath: agentPackage.packageDir,
					readOnly: true,
				},
			},
			readOnly: true,
		},
	];
	const vm = await AgentOs.create({
		defaultSoftware: false,
		software: [coreutils, agentPackage.software],
		mounts,
		onAgentExit: (event) =>
			events.emit("agentCrashed", { sessionId: event.sessionId, event }),
	});
	vm.onCronEvent((event) => events.emit("cronEvent", event));

	function trackSession(sessionId: string): void {
		vm.onSessionEvent(sessionId, (event) =>
			events.emit("sessionEvent", { sessionId, event }),
		);
		vm.onPermissionRequest(sessionId, (request) =>
			events.emit("permissionRequest", { sessionId, request }),
		);
	}

	const call = async <T>(
		action: AgentOsConformanceAction,
		...args: unknown[]
	): Promise<T> => {
		switch (action) {
			case "readdirEntries": {
				const path = args[0] as string;
				const names = await vm.readdir(path);
				return (await Promise.all(
					names.map(async (name) => {
						const stat = await vm.stat(`${path}/${name}`);
						return {
							name,
							isDirectory: stat.isDirectory,
							isSymbolicLink: stat.isSymbolicLink,
						};
					}),
				)) as T;
			}
			case "deleteFile":
				return (await vm.delete(
					...(args as Parameters<AgentOs["delete"]>),
				)) as T;
			case "spawn": {
				const [command, processArgs, spawnOptions] = args as [
					string,
					string[],
					Record<string, unknown> | undefined,
				];
				const process = vm.spawn(command, processArgs, {
					...spawnOptions,
					onStdout: (data) =>
						events.emit("processOutput", {
							pid: process.pid,
							stream: "stdout",
							data,
						}),
					onStderr: (data) =>
						events.emit("processOutput", {
							pid: process.pid,
							stream: "stderr",
							data,
						}),
				});
				void vm
					.waitProcess(process.pid)
					.then((exitCode) =>
						events.emit("processExit", { pid: process.pid, exitCode }),
					);
				return process as T;
			}
			case "openShell": {
				const shell = vm.openShell({
					...(args[0] as Record<string, unknown> | undefined),
					onStderr: (data) =>
						events.emit("shellStderr", { shellId: shell.shellId, data }),
				});
				vm.onShellData(shell.shellId, (data) =>
					events.emit("shellData", { shellId: shell.shellId, data }),
				);
				void vm
					.waitShell(shell.shellId)
					.then((exitCode) =>
						events.emit("shellExit", { shellId: shell.shellId, exitCode }),
					);
				return shell as T;
			}
			case "vmFetch": {
				const [port, url, requestOptions] = args as [
					number,
					string,
					(
						| {
								method?: string;
								headers?: Record<string, string>;
								body?: string | Uint8Array;
						  }
						| undefined
					),
				];
				const response = await vm.fetch(
					port,
					new Request(url, {
						method: requestOptions?.method,
						headers: requestOptions?.headers,
						body: requestOptions?.body,
					}),
				);
				return {
					status: response.status,
					statusText: response.statusText,
					headers: Object.fromEntries(response.headers.entries()),
					body: new Uint8Array(await response.arrayBuffer()),
				} as T;
			}
			case "scheduleCron": {
				const job = vm.scheduleCron(
					args[0] as Parameters<AgentOs["scheduleCron"]>[0],
				);
				return { id: job.id } as T;
			}
			case "listCronJobs":
				return vm.listCronJobs() as T;
			case "listMounts":
				return mounts.map((mount) => ({
					path: mount.path,
					kind: mount.plugin.id,
					readOnly: mount.readOnly,
					config: mount.plugin.config,
				})) as T;
			case "listSoftware":
				return (await vm.providedCommands()) as T;
			case "createSession": {
				const result = await vm.createSession(
					...(args as Parameters<AgentOs["createSession"]>),
				);
				trackSession(result.sessionId);
				return result.sessionId as T;
			}
			case "resumeSession": {
				const result = await vm.resumeSession(
					...(args as Parameters<AgentOs["resumeSession"]>),
				);
				trackSession(result.sessionId);
				return result as T;
			}
			case "sendPrompt":
				return (await vm.prompt(args[0] as string, args[1] as string)) as T;
			case "cancelPrompt":
				return (await vm.cancelSession(args[0] as string)) as T;
			case "setMode":
				return (await vm.setSessionMode(
					args[0] as string,
					args[1] as string,
				)) as T;
			case "getModes":
				return vm.getSessionModes(args[0] as string) as T;
			case "setModel":
				return (await vm.setSessionModel(
					args[0] as string,
					args[1] as string,
				)) as T;
			case "setThoughtLevel":
				return (await vm.setSessionThoughtLevel(
					args[0] as string,
					args[1] as string,
				)) as T;
			case "respondPermission":
				return (await vm.respondPermission(
					args[0] as string,
					args[1] as string,
					args[2] as PermissionReply,
				)) as T;
			default: {
				const method = (vm as any)[action];
				if (typeof method !== "function") {
					throw new Error(`Core backend does not implement ${action}`);
				}
				return (await method.apply(vm, args)) as T;
			}
		}
	};

	return {
		call,
		on: (event: AgentOsConformanceEvent, handler: (payload: any) => void) =>
			events.on(event, handler),
		async dispose() {
			await vm.dispose();
			agentPackage.cleanup();
		},
	};
}

defineAgentOsConformanceSuite({
	name: "AgentOS Core actor-surface conformance",
	createBackend: createCoreBackend,
});
