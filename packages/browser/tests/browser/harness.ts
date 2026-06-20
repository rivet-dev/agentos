import { expect, type Page } from "@playwright/test";
import type { ExecOptions, TimingMitigation } from "../../src/runtime.js";

export type HarnessStdioEvent = {
	channel: "stdout" | "stderr";
	message: string;
};

export type HarnessCreateRuntimeOptions = {
	filesystem?: "memory" | "opfs";
	opfsNamespace?: string;
	timingMitigation?: TimingMitigation;
	payloadLimits?: {
		base64TransferBytes?: number;
		jsonPayloadBytes?: number;
	};
	useDefaultNetwork?: boolean;
};

export type HarnessCreateRuntimeResponse = {
	crossOriginIsolated: boolean;
	runtimeId: string;
	workerUrl: string;
};

export type HarnessExecResponse = {
	crossOriginIsolated: boolean;
	result: {
		code: number;
		errorMessage?: string;
	};
	stdio: HarnessStdioEvent[];
};

export type HarnessTerminatePendingResponse = {
	outcome: "resolved" | "rejected";
	resultCode: number | null;
	errorMessage: string | null;
	debug: {
		disposed: boolean;
		pendingCount: number;
		signalState: number[];
		workerOnmessage: "null" | "set";
		workerOnerror: "null" | "set";
	};
};

export type HarnessExtensionDispatchResponse =
	| {
			ok: true;
			namespace: string;
			payload: number[];
	  }
	| {
			ok: false;
			errorMessage: string;
			errorCode?: string;
	  };

export type HarnessSmokeResponse = HarnessExecResponse & {
	workerUrl: string;
};

type SecureExecBrowserHarness = {
	createRuntime(
		options?: HarnessCreateRuntimeOptions,
	): Promise<HarnessCreateRuntimeResponse>;
	exec(
		runtimeId: string,
		code: string,
		options?: ExecOptions,
	): Promise<HarnessExecResponse>;
	disposeRuntime(runtimeId: string): Promise<void>;
	disposeAllRuntimes(): Promise<void>;
	releaseOpfsNamespace(namespace: string): Promise<void>;
	listOpfsNamespaces(): Promise<string[]>;
	terminatePendingExec(
		runtimeId: string,
		code: string,
		delayMs?: number,
	): Promise<HarnessTerminatePendingResponse>;
	dispatchExtensionRequest(
		runtimeId: string,
		namespace: string,
		payload: number[],
	): Promise<HarnessExtensionDispatchResponse>;
	smoke(): Promise<HarnessSmokeResponse>;
};

declare global {
	interface Window {
		__secureExecBrowserHarness?: SecureExecBrowserHarness;
	}
}

export async function openHarnessPage(page: Page): Promise<void> {
	await page.goto("/frontend/runtime-harness.html");
	await expect(page.locator("#harness-status")).toHaveText("ready");
}

export async function createRuntime(
	page: Page,
	options?: HarnessCreateRuntimeOptions,
): Promise<HarnessCreateRuntimeResponse> {
	return page.evaluate(async (optionsArg) => {
		const harness = window.__secureExecBrowserHarness;
		if (!harness) {
			throw new Error("Browser harness is unavailable on window");
		}
		return harness.createRuntime(optionsArg);
	}, options);
}

export async function execRuntime(
	page: Page,
	runtimeId: string,
	code: string,
	options?: ExecOptions,
): Promise<HarnessExecResponse> {
	return page.evaluate(
		async ({ runtimeId: runtimeIdArg, code: codeArg, options: optionsArg }) => {
			const harness = window.__secureExecBrowserHarness;
			if (!harness) {
				throw new Error("Browser harness is unavailable on window");
			}
			return harness.exec(runtimeIdArg, codeArg, optionsArg);
		},
		{ runtimeId, code, options },
	);
}

export async function disposeRuntime(
	page: Page,
	runtimeId: string,
): Promise<void> {
	await page.evaluate(async (runtimeIdArg) => {
		const harness = window.__secureExecBrowserHarness;
		if (!harness) {
			return;
		}
		await harness.disposeRuntime(runtimeIdArg);
	}, runtimeId);
}

export async function disposeAllRuntimes(page: Page): Promise<void> {
	await page.evaluate(async () => {
		const harness = window.__secureExecBrowserHarness;
		if (!harness) {
			return;
		}
		await harness.disposeAllRuntimes();
	});
}

export async function releaseOpfsNamespace(
	page: Page,
	namespace: string,
): Promise<void> {
	await page.evaluate(async (namespaceArg) => {
		const harness = window.__secureExecBrowserHarness;
		if (!harness) {
			throw new Error("Browser harness is unavailable on window");
		}
		await harness.releaseOpfsNamespace(namespaceArg);
	}, namespace);
}

export async function listOpfsNamespaces(page: Page): Promise<string[]> {
	return page.evaluate(async () => {
		const harness = window.__secureExecBrowserHarness;
		if (!harness) {
			throw new Error("Browser harness is unavailable on window");
		}
		return harness.listOpfsNamespaces();
	});
}

export async function terminatePendingExec(
	page: Page,
	runtimeId: string,
	code: string,
	delayMs?: number,
): Promise<HarnessTerminatePendingResponse> {
	return page.evaluate(
		async ({ runtimeId: runtimeIdArg, code: codeArg, delayMs: delayMsArg }) => {
			const harness = window.__secureExecBrowserHarness;
			if (!harness) {
				throw new Error("Browser harness is unavailable on window");
			}
			return harness.terminatePendingExec(runtimeIdArg, codeArg, delayMsArg);
		},
		{ runtimeId, code, delayMs },
	);
}

export async function dispatchExtensionRequest(
	page: Page,
	runtimeId: string,
	namespace: string,
	payload: number[],
): Promise<HarnessExtensionDispatchResponse> {
	return page.evaluate(
		async ({
			runtimeId: runtimeIdArg,
			namespace: namespaceArg,
			payload: payloadArg,
		}) => {
			const harness = window.__secureExecBrowserHarness;
			if (!harness) {
				throw new Error("Browser harness is unavailable on window");
			}
			return harness.dispatchExtensionRequest(
				runtimeIdArg,
				namespaceArg,
				payloadArg,
			);
		},
		{ runtimeId, namespace, payload },
	);
}

export async function smokeHarness(page: Page): Promise<HarnessSmokeResponse> {
	return page.evaluate(async () => {
		const harness = window.__secureExecBrowserHarness;
		if (!harness) {
			throw new Error("Browser harness is unavailable on window");
		}
		return harness.smoke();
	});
}

export function getLastStdioMessage(
	response: HarnessExecResponse,
	channel: HarnessStdioEvent["channel"],
): string {
	const message = response.stdio
		.filter((event) => event.channel === channel)
		.at(-1)?.message;
	if (!message) {
		throw new Error(`Missing ${channel} output in harness response`);
	}
	return message;
}
