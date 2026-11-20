export function checkExecutionCpuLimit(limit: number | undefined): void {
	if (limit !== undefined) {
		throw unsupportedExecutionOption(
			"Per-run cpuTimeLimitMs is unsupported by the native sidecar; configure VM limits.jsRuntime.cpuTimeLimitMs or limits.wasm.activeCpuTimeLimitMs instead",
		);
	}
}

export function checkExecutionTimingMitigation(
	mode: "off" | "freeze" | undefined,
): void {
	if (mode !== undefined && mode !== "off") {
		throw unsupportedExecutionOption(
			"Per-run timingMitigation is unsupported by the native sidecar; configure VM-level highResolutionTime to control clock precision",
		);
	}
}

function unsupportedExecutionOption(message: string): Error {
	return Object.assign(new Error(message), {
		code: "ERR_AGENTOS_UNSUPPORTED_EXECUTION_OPTION",
	});
}
