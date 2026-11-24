import {
	createTypeScriptTools,
	type ProjectCompileResult,
	type TypeCheckResult,
	type TypeScriptTools,
} from "../src/internal/typescript-tools.js";
import {
	createNodeDriver,
	createNodeRuntimeDriverFactory,
} from "../src/runtime-compat.js";

export function createQuickstartTools(): TypeScriptTools {
	return createTypeScriptTools({
		systemDriver: createNodeDriver(),
		runtimeDriverFactory: createNodeRuntimeDriverFactory(),
	});
}

void createQuickstartTools;
void (null as ProjectCompileResult | null);
void (null as TypeCheckResult | null);
