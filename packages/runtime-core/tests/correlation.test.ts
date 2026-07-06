import { describe, expect, test, vi } from "vitest";
import { PendingResponseRegistry } from "../src/correlation.js";

describe("pending response registry", () => {
	test("resolves a registered response by request id", async () => {
		const registry = new PendingResponseRegistry<string>();
		const response = registry.waitForResponse(7, {
			timeoutMs: 1_000,
			timeoutMessage: () => "timed out",
		});

		expect(registry.resolve(7, "ok")).toBe(true);
		await expect(response).resolves.toBe("ok");
		expect(registry.resolve(7, "late")).toBe(false);
	});

	test("rejects a registered response by request id", async () => {
		const registry = new PendingResponseRegistry<string>();
		const response = registry.waitForResponse(9, {
			timeoutMs: 1_000,
			timeoutMessage: () => "timed out",
		});
		const error = new Error("write failed");

		expect(registry.reject(9, error)).toBe(true);
		await expect(response).rejects.toThrow("write failed");
		expect(registry.reject(9, error)).toBe(false);
	});

	test("times out pending responses", async () => {
		vi.useFakeTimers();
		try {
			const registry = new PendingResponseRegistry<string>();
			const response = registry.waitForResponse(11, {
				timeoutMs: 25,
				timeoutMessage: () => "request timed out",
			});
			const expectedRejection = expect(response).rejects.toThrow(
				"request timed out",
			);

			await vi.advanceTimersByTimeAsync(25);
			await expectedRejection;
			expect(registry.resolve(11, "late")).toBe(false);
		} finally {
			vi.useRealTimers();
		}
	});

	test("rejects duplicate request ids", () => {
		const registry = new PendingResponseRegistry<string>();
		const pending = registry.waitForResponse(13, {
			timeoutMs: 1_000,
			timeoutMessage: () => "timed out",
		});
		void pending.catch(() => undefined);

		expect(() =>
			registry.waitForResponse(13, {
				timeoutMs: 1_000,
				timeoutMessage: () => "timed out",
			}),
		).toThrow("response waiter already registered for request 13");

		registry.rejectAll(new Error("cleanup"));
	});

	test("rejects all pending responses", async () => {
		const registry = new PendingResponseRegistry<string>();
		const first = registry.waitForResponse(1, {
			timeoutMs: 1_000,
			timeoutMessage: () => "first timed out",
		});
		const second = registry.waitForResponse(2, {
			timeoutMs: 1_000,
			timeoutMessage: () => "second timed out",
		});

		registry.rejectAll(new Error("transport closed"));

		await expect(first).rejects.toThrow("transport closed");
		await expect(second).rejects.toThrow("transport closed");
		expect(registry.resolve(1, "late")).toBe(false);
		expect(registry.resolve(2, "late")).toBe(false);
	});
});
