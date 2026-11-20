import { afterEach, expect, test, vi } from "vitest";
import { boundedNativeFixture } from "./helpers/native-owner-scope.js";

afterEach(() => vi.useRealTimers());

// Deterministic fixture-safety controls; no sidecar, sleeps or real timeout wait.
test("pending fixture body still enters cleanup and final owned-child termination", async () => {
	vi.useFakeTimers();
	const events: string[] = [];
	const result = boundedNativeFixture({
		bodyMs: 20,
		cleanupMs: 20,
		finishMs: 20,
		body: () => new Promise<void>(() => {}),
		async cleanup() {
			events.push("cleanup");
		},
		async finish() {
			events.push("kill/reap");
		},
	}).catch((error) => error);
	await vi.advanceTimersByTimeAsync(20);
	expect(await result).toBeInstanceOf(Error);
	expect(events).toEqual(["cleanup", "kill/reap"]);
});

test("pending cleanup cannot prevent final termination and original assertion is preserved", async () => {
	vi.useFakeTimers();
	const original = new Error("original body assertion");
	const events: string[] = [];
	const result = boundedNativeFixture({
		bodyMs: 20,
		cleanupMs: 20,
		finishMs: 20,
		async body() {
			throw original;
		},
		cleanup: () => new Promise<void>(() => {}),
		async finish() {
			events.push("kill/reap");
		},
	}).catch((error) => error);
	await vi.advanceTimersByTimeAsync(20);
	const error = await result;
	expect(error).toBeInstanceOf(AggregateError);
	expect(error.errors[0]).toBe(original);
	expect(String(error.errors[1])).toContain("fixture cleanup exceeded");
	expect(events).toEqual(["kill/reap"]);
});

test("late pending body continuation is fenced after cleanup without starting another operation", async () => {
	vi.useFakeTimers();
	let release!: () => void;
	const gate = new Promise<void>((resolve) => {
		release = resolve;
	});
	const events: string[] = [];
	const result = boundedNativeFixture({
		bodyMs: 20,
		cleanupMs: 20,
		finishMs: 20,
		async body(phase) {
			await phase.step(() => gate, 100, "parked body RPC");
			events.push("after RPC");
			await phase.step(
				async () => {
					events.push("later operation");
				},
				10,
				"forbidden later operation",
			);
		},
		async cleanup() {
			events.push("cleanup");
		},
		async finish() {
			events.push("kill/reap");
		},
	}).catch((error) => error);
	await vi.advanceTimersByTimeAsync(20);
	await result;
	release();
	await vi.advanceTimersByTimeAsync(0);
	expect(events).toEqual(["cleanup", "kill/reap"]);
});

test("expected pending-disposal rejection does not retire an otherwise active fixture phase", async () => {
	const pending = Object.assign(new Error("creation pending"), {
		code: "ERR_AGENTOS_SIDECAR_CREATION_PENDING",
	});
	let subsequent = false;
	await boundedNativeFixture({
		bodyMs: 100,
		cleanupMs: 100,
		finishMs: 100,
		async body(phase) {
			await expect(
				phase.step(
					() => Promise.reject(pending),
					50,
					"expected pending disposal",
				),
			).rejects.toBe(pending);
			await phase.step(
				async () => {
					subsequent = true;
				},
				50,
				"peer health RPC",
			);
		},
		async cleanup() {},
		async finish() {},
	});
	expect(subsequent).toBe(true);
});
