import { afterEach, describe, expect, test, vi } from "vitest";
import { z } from "zod";
import { AgentOs } from "../src/index.js";
import { NativeSidecarProcessClient } from "../src/sidecar/rpc-client.js";

// ---------------------------------------------------------------------------
// The sidecar owns hostFunction.invoke permission enforcement. These tests
// capture the trusted callback handler installed by AgentOs.create() and verify
// that it dispatches only the callback key the sidecar already authorized.
// ---------------------------------------------------------------------------

type CapturedHandler = (request: any) => Promise<any> | any;

async function createVmCapturingHandler(
	options: Parameters<typeof AgentOs.create>[0],
): Promise<{ vm: AgentOs; handler: CapturedHandler }> {
	let captured: CapturedHandler | null = null;
	const original =
		NativeSidecarProcessClient.prototype.setSidecarRequestHandler;
	const spy = vi
		.spyOn(NativeSidecarProcessClient.prototype, "setSidecarRequestHandler")
		.mockImplementation(function (
			this: NativeSidecarProcessClient,
			handler: any,
			vmId?: string,
		) {
			if (handler) {
				captured = handler as CapturedHandler;
			}
			// Still install on the real client so the VM behaves normally.
			return original.call(this, handler, vmId);
		});
	try {
		const vm = await AgentOs.create(options);
		if (!captured) {
			throw new Error(
				"AgentOs.create did not install a sidecar request handler",
			);
		}
		return { vm, handler: captured };
	} finally {
		spy.mockRestore();
	}
}

function hostCallbackFrame(callbackKey: string, input: unknown) {
	return {
		frame_type: "sidecar_request" as const,
		request_id: 1,
		payload: {
			type: "host_callback" as const,
			invocation_id: "guest-forged-1",
			callback_key: callbackKey,
			input,
			timeout_ms: 30_000,
		},
	};
}

// Registry callbacks use a command envelope, but only the reserved `agentos`
// callback key may select that dispatch path.
function commandHostCallbackFrame(command: string, args: string[]) {
	return {
		frame_type: "sidecar_request" as const,
		request_id: 1,
		payload: {
			type: "host_callback" as const,
			invocation_id: "guest-forged-cmd-1",
			callback_key: "agentos",
			input: {
				type: "command",
				command,
				args,
				cwd: "/home/agentos",
			},
			timeout_ms: 30_000,
		},
	};
}

const mathFunctions = {
	add: {
		inputSchema: z
			.object({
				a: z.number(),
				b: z.number(),
			})
			.describe("Add two numbers"),
		execute: ({ a, b }) => ({ sum: a + b }),
	},
};

const duplicateMathFunctions = {
	multiply: {
		inputSchema: z
			.object({
				a: z.number(),
				b: z.number(),
			})
			.describe("Multiply two numbers"),
		execute: ({ a, b }) => ({ product: a * b }),
	},
};

async function runCommand(vm: AgentOs, command: string, args: string[]) {
	const stdoutChunks: string[] = [];
	const stderrChunks: string[] = [];
	const { pid } = await vm.process.spawn(command, args, {
		onStdout: (chunk) => {
			stdoutChunks.push(new TextDecoder().decode(chunk));
		},
		onStderr: (chunk) => {
			stderrChunks.push(new TextDecoder().decode(chunk));
		},
	});

	return {
		exitCode: (await vm.process.wait(pid)).exitCode,
		stdout: stdoutChunks.join(""),
		stderr: stderrChunks.join(""),
	};
}

describe("hostFunction collection permissions", () => {
	let vm: AgentOs | null = null;

	afterEach(async () => {
		await vm?.dispose();
		vm = null;
	});

	test("rejects two collection keys that resolve to the same command name", async () => {
		await expect(
			AgentOs.create({
				hostFunctions: { math: mathFunctions, Math: duplicateMathFunctions },
			}),
		).rejects.toThrow(/both resolve to the command name "math"/);
	});

	test("allows hostFunction collection invocation with default permissions", async () => {
		vm = await AgentOs.create({
			hostFunctions: { math: mathFunctions },
		});

		const result = await runCommand(vm, "agentos-math", [
			"add",
			"--a",
			"2",
			"--b",
			"3",
		]);
		expect(result.exitCode).toBe(0);
		expect(JSON.parse(result.stdout)).toEqual({
			ok: true,
			result: { sum: 5 },
		});
	});

	test("denies hostFunction collection invocation when hostFunction permissions deny it", async () => {
		vm = await AgentOs.create({
			hostFunctions: { math: mathFunctions },
			permissions: {
				fs: "allow",
				childProcess: "allow",
				hostFunction: { default: "deny", rules: [] },
			},
		});

		const result = await runCommand(vm, "agentos-math", [
			"add",
			"--a",
			"5",
			"--b",
			"7",
		]);
		expect(result.exitCode, JSON.stringify(result)).toBe(1);
		expect(result.stdout).toBe("");
		expect(result.stderr).toContain("hostFunction.invoke");
		expect(result.stderr).toContain("math:add");
	});

	test("allows hostFunction collection invocation when a matching hostFunction permission is granted", async () => {
		vm = await AgentOs.create({
			hostFunctions: { math: mathFunctions },
			permissions: {
				fs: "allow",
				childProcess: "allow",
				hostFunction: {
					default: "deny",
					rules: [
						{
							mode: "allow",
							operations: ["invoke"],
							patterns: ["math:add"],
						},
					],
				},
			},
		});

		const result = await runCommand(vm, "agentos-math", [
			"add",
			"--a",
			"5",
			"--b",
			"7",
		]);
		expect(result.exitCode).toBe(0);
		expect(JSON.parse(result.stdout)).toEqual({
			ok: true,
			result: { sum: 12 },
		});
	});
});

describe("host-function collection permissions: raw host_callback RPC path", () => {
	let vm: AgentOs | null = null;

	afterEach(async () => {
		await vm?.dispose();
		vm = null;
	});

	test("command-shaped function input cannot redirect an authorized callback", async () => {
		const executed: string[] = [];
		const functions = {
			allowed: {
				inputSchema: z.object({
					type: z.literal("command"),
					command: z.string(),
					args: z.array(z.string()),
					cwd: z.string(),
				}),
				execute: () => {
					executed.push("allowed");
					return "allowed";
				},
			},
			danger: {
				inputSchema: z.object({}),
				execute: () => {
					executed.push("danger");
					return "danger";
				},
			},
		};

		const created = await createVmCapturingHandler({
			hostFunctions: { math: functions },
		});
		vm = created.vm;

		const response = await created.handler(
			hostCallbackFrame("math:allowed", {
				type: "command",
				command: "agentos-math",
				args: ["danger"],
				cwd: "/workspace",
			}),
		);

		expect(executed).toEqual(["allowed"]);
		expect(response.type).toBe("host_callback_result");
		expect(response.result).toBe("allowed");
		expect(response.error).toBeUndefined();
	});

	// AOSFS-1 (P1, J.1/J.2): the raw host_callback RPC path is fully
	// guest-controlled, including the `input` object. The guest can stuff extra
	// keys, a `__proto__` payload, and a `constructor` key into `input` to try to
	// (a) leak raw unvalidated fields into the host-side `execute`, or (b) pollute
	// Object.prototype on the host. The handler runs `hostFunction.inputSchema.safeParse`
	// and passes ONLY `parsed.data` to execute; a strict/stripping Zod object must
	// hand `execute` exactly the declared keys and nothing else, and no prototype
	// pollution may occur. Asserts the system strips the hostile/extra keys.
	test("host_callback strips hostile/extra input keys; execute receives only validated Zod data and no prototype pollution", async () => {
		const seen: unknown[] = [];
		const collection = {
			add: {
				inputSchema: z
					.object({ a: z.number(), b: z.number() })
					.describe("Add two numbers"),
				execute: (input) => {
					// Capture exactly what execute is handed.
					seen.push(input);
					const { a, b } = input;
					return { sum: a + b };
				},
			},
		};

		const created = await createVmCapturingHandler({
			hostFunctions: { math: collection },
			permissions: {
				fs: "allow",
				childProcess: "allow",
				hostFunction: {
					default: "deny",
					rules: [
						{ mode: "allow", operations: ["invoke"], patterns: ["math:add"] },
					],
				},
			},
		});
		vm = created.vm;

		// Hostile input: declared keys + extra fields + a prototype-pollution
		// payload. Build via JSON so __proto__ is a real own enumerable key (the
		// exact shape an untrusted guest sends over the wire).
		const hostileInput = JSON.parse(
			'{"a":2,"b":3,"evilField":"leak-me","secret":"do-not-pass","__proto__":{"polluted":"yes"},"constructor":{"prototype":{"polluted2":"yes"}}}',
		);

		const response = await created.handler(
			hostCallbackFrame("math:add", hostileInput),
		);

		// The hostFunction ran (policy allows math:add) and produced the correct result.
		expect(response.type).toBe("host_callback_result");
		expect(response.error).toBeUndefined();
		expect(response.result).toEqual({ sum: 5 });

		// execute saw EXACTLY the declared keys — no leaked hostile/extra fields.
		expect(seen).toHaveLength(1);
		const handed = seen[0] as Record<string, unknown>;
		expect(Object.keys(handed).sort()).toEqual(["a", "b"]);
		expect(handed.a).toBe(2);
		expect(handed.b).toBe(3);
		expect(handed).not.toHaveProperty("evilField");
		expect(handed).not.toHaveProperty("secret");

		// No prototype pollution of Object.prototype on the host.
		expect(({} as Record<string, unknown>).polluted).toBeUndefined();
		expect(({} as Record<string, unknown>).polluted2).toBeUndefined();
		expect(Object.hasOwn(Object.prototype, "polluted")).toBe(false);
	});

	// AOSFS-2 (P2): a guest can send schema-failing input on the raw host_callback
	// RPC path (which does NOT go through the CLI argv parser / sidecar-hostFunction
	// dispatch validation at sidecar-host-function-dispatch:108). The handler must
	// safeParse and return a validation error WITHOUT invoking execute.
	test("host_callback rejects schema-failing input without invoking execute", async () => {
		const executed: unknown[] = [];
		const collection = {
			add: {
				inputSchema: z
					.object({ a: z.number(), b: z.number() })
					.describe("Add two numbers"),
				execute: ({ a, b }) => {
					executed.push({ a, b });
					return { sum: a + b };
				},
			},
		};

		const created = await createVmCapturingHandler({
			hostFunctions: { math: collection },
			permissions: {
				fs: "allow",
				childProcess: "allow",
				hostFunction: {
					default: "deny",
					rules: [
						{ mode: "allow", operations: ["invoke"], patterns: ["math:add"] },
					],
				},
			},
		});
		vm = created.vm;

		// `a` is the wrong type; `b` is missing entirely.
		const response = await created.handler(
			hostCallbackFrame("math:add", { a: "not-a-number" }),
		);

		expect(executed).toHaveLength(0);
		expect(response.type).toBe("host_callback_result");
		expect(response.result).toBeUndefined();
		expect(typeof response.error).toBe("string");
		// Zod validation message (number expected / required), not a thrown crash.
		expect(response.error).toMatch(/number|expected|required|invalid|nan/i);
	});

	test("registry callback rejects a mismatched command envelope", async () => {
		const executed: unknown[] = [];
		const spyFunctions = {
			add: {
				inputSchema: z
					.object({ a: z.number(), b: z.number() })
					.describe("Add two numbers"),
				execute: ({ a, b }) => {
					executed.push({ a, b });
					return { sum: a + b };
				},
			},
		};

		const created = await createVmCapturingHandler({
			hostFunctions: { math: spyFunctions },
		});
		vm = created.vm;

		const response = await created.handler(
			commandHostCallbackFrame("agentos-math", ["add", "--a", "2", "--b", "3"]),
		);

		expect(executed).toHaveLength(0);
		expect(response.type).toBe("host_callback_result");
		expect(response.result).toBeUndefined();
		expect(typeof response.error).toBe("string");
		expect(response.error).toMatch(/invalid registry callback/i);
	});
});

describe("host functions on a shared sidecar", () => {
	test("keeps each VM's callbacks after a sibling is disposed or has no host functions", async () => {
		const sidecar = await AgentOs.createSidecar();
		const vms: AgentOs[] = [];
		const permissions = {
			fs: "allow",
			childProcess: "allow",
			hostFunction: "allow",
		} as const;
		const invoke = async (vm: AgentOs, id: string) => {
			const result = await runCommand(vm, "agentos-identity", ["who"]);
			expect(result.exitCode, result.stderr).toBe(0);
			expect(JSON.parse(result.stdout)).toEqual({ ok: true, result: { id } });
		};
		try {
			for (const id of ["a", "b", "c"]) {
				vms.push(
					await AgentOs.create({
						sidecar: { kind: "explicit", handle: sidecar },
						defaultSoftware: false,
						permissions,
						hostFunctions: {
							identity: {
								who: { inputSchema: z.object({}), execute: () => ({ id }) },
							},
						},
					}),
				);
			}
			await invoke(vms[0], "a");
			await invoke(vms[1], "b");
			await invoke(vms[2], "c");
			await vms[1].dispose();
			await invoke(vms[0], "a");
			vms.push(
				await AgentOs.create({
					sidecar: { kind: "explicit", handle: sidecar },
					defaultSoftware: false,
					permissions,
				}),
			);
			await invoke(vms[0], "a");
			await invoke(vms[2], "c");
		} finally {
			try {
				await Promise.all(vms.map((vm) => vm.dispose()));
			} finally {
				await sidecar.dispose();
			}
		}
	});
});
