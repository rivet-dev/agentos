import type { AgentOs } from "@rivet-dev/agentos-core";
import { createTypeScriptTools } from "@rivet-dev/agentos-typescript";
import { describe, expect, it, vi } from "vitest";

describe("@rivet-dev/agentos-typescript transport", () => {
	it("preserves an omitted compiler cwd on the sidecar request", async () => {
		const execArgv = vi.fn().mockResolvedValue({
			exitCode: 0,
			stdout: JSON.stringify({
				ok: true,
				result: { success: true, diagnostics: [] },
			}),
			stderr: "",
		});
		const tools = createTypeScriptTools({
			agentOs: { execArgv } as unknown as AgentOs,
		});

		await expect(
			tools.typecheckSource({ sourceText: "const value = 1;\n" }),
		).resolves.toEqual({ success: true, diagnostics: [] });

		expect(execArgv).toHaveBeenCalledOnce();
		const [, , options] = execArgv.mock.calls[0] ?? [];
		expect(options).toHaveProperty("stdin");
		expect(options).not.toHaveProperty("cwd");
	});
});
