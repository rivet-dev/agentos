import {
	allowAll,
	createNodeDriver,
	createNodeHostCommandExecutor,
	createNodeRuntimeDriverFactory,
	NodeRuntime,
	type NodeRuntimeOptions,
} from "secure-exec";
import { createInMemoryFileSystem } from "@rivet-dev/agentos-core/test/runtime";

export function createQuickstartOptions(): NodeRuntimeOptions {
	const filesystem = createInMemoryFileSystem();
	const systemDriver = createNodeDriver({
		filesystem,
		permissions: allowAll,
		commandExecutor: createNodeHostCommandExecutor(),
	});

	return {
		systemDriver,
		runtimeDriverFactory: createNodeRuntimeDriverFactory(),
	};
}

void NodeRuntime;
