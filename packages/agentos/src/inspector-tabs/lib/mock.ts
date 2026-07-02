// Dummy/offline mode. Load the bundle with `?mock=1` and every actor action is
// served from in-memory fixtures (`mock-data.ts`) instead of a live actor — so
// the inspector tabs can be developed and QA'd with no running VM. Pick which
// tab to render with `?tab=<id>` (filesystem | processes | software | mounts |
// transcript) when the URL has no `custom-tabs/<id>` path segment to read.
//
//   <bundle>/?mock=1&tab=filesystem
//
import { mockActions } from "./mock-data";

const params = new URLSearchParams(window.location.search);

/** True when the bundle was loaded in dummy mode (`?mock=1`; `?mock=0` opts out). */
export function isMockMode(): boolean {
	return params.has("mock") && params.get("mock") !== "0";
}

/** Tab id from `?tab=<id>` — the mock-mode fallback for the path-derived id. */
export function mockTabId(): string | undefined {
	return params.get("tab") ?? undefined;
}

/** Resolve an action against the fixtures, with a small delay so loading /
 * Suspense states are exercised the same way a real round-trip would. */
export async function mockCallAction<T>(name: string, args: unknown[]): Promise<T> {
	const handler = mockActions[name];
	if (!handler) {
		throw new Error(`mock mode: no fixture for action "${name}" (add one in mock-data.ts)`);
	}
	await new Promise((resolve) => setTimeout(resolve, 80));
	return handler(args) as T;
}
