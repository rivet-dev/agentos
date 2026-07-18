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
import { AgentOs } from "../src/index.js";

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
		onAgentExit: (event) => events.emit("agentExit", event),
	});
	vm.onCronEvent((event) => events.emit("cronEvent", event));

	function trackSession(sessionId: string): void {
		vm.onSessionEvent(sessionId, (event) => events.emit("sessionEvent", event));
	}

	const call = async <T>(
		action: AgentOsConformanceAction,
		...args: unknown[]
	): Promise<T> => {
		switch (action) {
			case "remove":
				return (await vm.remove(
					...(args as Parameters<AgentOs["remove"]>),
				)) as T;
			case "spawn": {
				const [command, processArgs, spawnOptions] = args as [
					string,
					string[],
					Record<string, unknown> | undefined,
				];
				const process = vm.spawn(command, processArgs, spawnOptions);
				vm.onProcessOutput(process.pid, (event) =>
					events.emit("processOutput", event),
				);
				vm.onProcessExit(process.pid, (event) =>
					events.emit("processExit", event),
				);
				return process as T;
			}
			case "openShell": {
				const shell = vm.openShell(
					args[0] as Parameters<AgentOs["openShell"]>[0],
				);
				vm.onShellData(shell.shellId, (event) =>
					events.emit("shellData", event),
				);
				vm.onShellStderr(shell.shellId, (event) =>
					events.emit("shellStderr", event),
				);
				vm.onShellExit(shell.shellId, (event) =>
					events.emit("shellExit", event),
				);
				return shell as T;
			}
			case "httpRequest":
				return (await vm.httpRequest(
					...(args as Parameters<AgentOs["httpRequest"]>),
				)) as T;
			case "scheduleCron": {
				const job = vm.scheduleCron(
					args[0] as Parameters<AgentOs["scheduleCron"]>[0],
				);
				return { id: job.id } as T;
			}
			case "listCronJobs":
				return vm.listCronJobs() as T;
			case "listMounts":
				return (await vm.listMounts()) as T;
			case "listSoftware":
				return (await vm.listSoftware()) as T;
			case "openSession": {
				const [input] = args as Parameters<AgentOs["openSession"]>;
				await vm.openSession(...(args as Parameters<AgentOs["openSession"]>));
				trackSession(input.sessionId ?? "main");
				return undefined as T;
			}
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
