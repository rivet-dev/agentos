// Regression for https://github.com/rivet-dev/agentos/issues/1959
//
// A WASM pipeline stage reading from a pipe used to block the sidecar reactor
// thread inside a synchronous kernel read (the `process.fd_read` sync RPC).
// While parked, the reactor could not run the upstream stage to completion, so
// the writer never closed its pipe end and the reader only unblocked when the
// `maxBlockingReadMs` watchdog fired — once per pipeline stage. A low cap
// turned that stall into a guest-visible `Would block` / EAGAIN and a nonzero
// exit; a high cap turned it into a multi-second-per-stage stall.
//
// The fix makes WASM `process.fd_read` non-blocking (mirroring the sibling
// `process.fd_write` path): the reactor stays free to run the writer, the
// reader observes EOF promptly, and the pipeline completes with correct output
// whose latency no longer depends on `maxBlockingReadMs`.
import { describe, expect, test } from "vitest";
import { AgentOs } from "../src/index.js";

describe("issue-1959: pipe reader no longer stalls for maxBlockingReadMs", () => {
	// A low cap is the sharpest probe: before the fix this stage read raced the
	// watchdog and surfaced `sed: Would block` (rc=1). After the fix the cap is
	// irrelevant — the read waits for the writer and observes EOF cleanly.
	test("a fast pipeline succeeds under a low maxBlockingReadMs (no watchdog EAGAIN)", async () => {
		const vm = await AgentOs.create({
			limits: { resources: { maxBlockingReadMs: 500 } },
		});
		try {
			const envSortSed = await vm.exec("env | sort | sed -n '1,2p'", {
				cwd: "/workspace",
				timeoutMs: 60_000,
			});
			expect(
				envSortSed.exitCode,
				`env|sort|sed stderr=${envSortSed.stderr}`,
			).toBe(0);
			expect(envSortSed.stderr.toLowerCase()).not.toMatch(
				/would block|temporarily unavailable/,
			);
			expect(envSortSed.stdout.length).toBeGreaterThan(0);

			// Exact outputs prove no bytes are dropped across the pipe.
			const seq = await vm.exec("seq 1 5 | paste -sd,", {
				cwd: "/workspace",
				timeoutMs: 60_000,
			});
			expect(seq.exitCode, `seq stderr=${seq.stderr}`).toBe(0);
			expect(seq.stdout).toBe("1,2,3,4,5\n");

			const large = await vm.exec("seq 1 5000 | tail -n 1", {
				cwd: "/workspace",
				timeoutMs: 60_000,
			});
			expect(large.exitCode, `large stderr=${large.stderr}`).toBe(0);
			expect(large.stdout).toBe("5000\n");
		} finally {
			await vm.dispose();
		}
	}, 120_000);

	// The defining property of the fix: pipeline latency does not scale with
	// maxBlockingReadMs. Before the fix, raising the cap made a three-stage
	// pipeline pay the cap once per stage (~3x). After it, the two runs are
	// close regardless of the cap.
	test("three-stage pipeline latency is independent of maxBlockingReadMs", async () => {
		const timePipeline = async (maxBlockingReadMs: number) => {
			const vm = await AgentOs.create({
				limits: { resources: { maxBlockingReadMs } },
			});
			try {
				const startedAt = Date.now();
				const r = await vm.exec("env | sort | sed -n '1,2p'", {
					cwd: "/workspace",
					timeoutMs: 60_000,
				});
				expect(r.exitCode, `cap=${maxBlockingReadMs} stderr=${r.stderr}`).toBe(
					0,
				);
				return Date.now() - startedAt;
			} finally {
				await vm.dispose();
			}
		};

		const lowCapMs = 500;
		const highCapMs = 8000;
		const lowElapsed = await timePipeline(lowCapMs);
		const highElapsed = await timePipeline(highCapMs);

		// If the per-stage stall were still present, the high-cap run would pay
		// roughly an extra stage-worth of watchdog time. Allow generous slack for
		// unoptimized (debug) builds and VM warmup while still catching a stall:
		// a single extra stage at the 8s cap would blow past this bound.
		expect(
			highElapsed,
			`low(${lowCapMs}ms cap)=${lowElapsed}ms high(${highCapMs}ms cap)=${highElapsed}ms`,
		).toBeLessThan(lowElapsed + highCapMs);
	}, 180_000);
});
