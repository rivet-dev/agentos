import {
	createTypeScriptTools,
	type ProjectCompileResult,
	type TypeCheckResult,
	type TypeScriptTools,
} from "@rivet-dev/agentos-typescript";
import type { AgentOs } from "@rivet-dev/agentos-core";

export function createQuickstartTools(agentOs: AgentOs): TypeScriptTools {
	return createTypeScriptTools({
		agentOs,
	});
}

void createQuickstartTools;
void (null as ProjectCompileResult | null);
void (null as TypeCheckResult | null);
