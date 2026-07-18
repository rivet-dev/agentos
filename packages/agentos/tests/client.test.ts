import { readFileSync, readdirSync } from "node:fs";
import { createClient as createRivetClient } from "rivetkit/client";
import { describe, expect, test } from "vitest";
import { createClient } from "../src/client.js";

describe("AgentOS client convenience re-export", () => {
	test("exports vanilla RivetKit without wrappers or decorators", () => {
		expect(createClient).toBe(createRivetClient);
		const source = readFileSync(
			new URL("../src/client.ts", import.meta.url),
			"utf8",
		);
		for (const forbidden of [
			"new Proxy",
			"decorateActorHandle",
			"decorateActorConnection",
			"AgentOsActorHandle",
			"AgentOsClient",
		]) {
			expect(source).not.toContain(forbidden);
		}
	});

	test("public sources do not reintroduce removed proxy or nested event APIs", () => {
		const roots = [
			new URL("../src", import.meta.url),
			new URL("../../core/src", import.meta.url),
		];
		const source = roots
			.flatMap((root) =>
				readdirSync(root, { recursive: true, withFileTypes: true })
					.filter((entry) => entry.isFile() && entry.name.endsWith(".ts"))
					.map((entry) =>
						readFileSync(new URL(entry.parentPath + "/" + entry.name, "file:"), "utf8"),
					),
			)
			.join("\n");
		for (const forbidden of [
			"agentOsHandle",
			"decorateActorHandle",
			"decorateActorConnection",
			"AgentOsActorHandle",
			".actor.preview",
			".actor.lifecycle",
			".update.sessionUpdate",
			".request.options",
			".response.outcome",
		]) {
			expect(source).not.toContain(forbidden);
		}
	});
});
