import type { PromptResult } from "../../src/session-api.js";

type TextContentBlock = Extract<
	NonNullable<PromptResult["message"]>["content"][number],
	{ type: "text" }
>;

export function promptResultText(result: PromptResult): string {
	return (
		result.message?.content
			.filter((block): block is TextContentBlock => block.type === "text")
			.map((block) => block.text)
			.join("") ?? ""
	);
}
