// Nightly: exercises the registry awk command through attached process execution.
import gawk from "@agentos-software/gawk";
import { afterAll, beforeAll, describe, expect, test } from "vitest";
import { AgentOs } from "../src/index.js";
import { ALLOW_ALL_VM_PERMISSIONS } from "./helpers/permissions.js";

describe("attached process execution stdin lifecycle", () => {
	let vm: AgentOs | undefined;

	beforeAll(async () => {
		vm = await AgentOs.create({
			permissions: ALLOW_ALL_VM_PERMISSIONS,
			software: [gawk],
		});
	});

	afterAll(async () => {
		await vm?.dispose();
	});

	test.each([
		{ label: "omitted", stdin: undefined },
		{ label: "explicitly empty", stdin: "" },
	])("BEGIN-only awk completes with $label stdin", async ({ stdin }) => {
		if (!vm) throw new Error("VM was not created");
		const result = await vm.process.exec("awk 'BEGIN { print \"AWK\" }'", {
			timeoutMs: 5_000,
			output: { capture: "all" },
			...(stdin === undefined ? {} : { stdin }),
		});

		expect(result).toMatchObject({
			outcome: "succeeded",
			exitCode: 0,
			stdout: "AWK\n",
			stderr: "",
			stdoutTruncated: false,
			stderrTruncated: false,
		});
	});

	test.each([
		{ label: "omitted", stdin: undefined, stdout: "EOF\n" },
		{ label: "explicitly empty", stdin: "", stdout: "EOF\n" },
		{
			label: "nonempty without a trailing newline",
			stdin: "one\ntwo",
			stdout: "GOT:one\nGOT:two\nEOF\n",
		},
	])("input-reading awk receives EOF with $label stdin", async ({
		stdin,
		stdout,
	}) => {
		if (!vm) throw new Error("VM was not created");
		const result = await vm.process.exec(
			'awk \'{ print "GOT:" $0 } END { print "EOF" }\'',
			{
				timeoutMs: 5_000,
				output: { capture: "all" },
				...(stdin === undefined ? {} : { stdin }),
			},
		);

		expect(result).toMatchObject({
			outcome: "succeeded",
			exitCode: 0,
			stdout,
			stderr: "",
			stdoutTruncated: false,
			stderrTruncated: false,
		});
	});
});
