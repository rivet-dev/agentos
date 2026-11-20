import { bounded } from "./native-owner-fixture.js";

export interface NativeFixturePhase {
	step<T>(work: () => Promise<T>, timeoutMs: number, label: string): Promise<T>;
}

// The fixture, not Vitest's outer timeout, owns unwind. No earlier pending
// operation or disposal can keep the final owned-child termination phase out.
export async function boundedNativeFixture(input: {
	body(phase: NativeFixturePhase): Promise<void>;
	cleanup(): Promise<void>;
	finish(): Promise<void>;
	bodyMs: number;
	cleanupMs: number;
	finishMs: number;
}): Promise<void> {
	let active = true;
	const deadline = performance.now() + input.bodyMs;
	const retired = () => new Error("native ownership fixture phase retired");
	const phase: NativeFixturePhase = {
		async step(work, timeoutMs, label) {
			if (!active) throw retired();
			const remaining = deadline - performance.now();
			if (remaining <= 0) throw retired();
			let timer: ReturnType<typeof setTimeout> | undefined;
			try {
				const value = await Promise.race([
					Promise.resolve().then(work),
					new Promise<never>((_, reject) => {
						timer = setTimeout(
							() => {
								active = false;
								reject(
									new Error(`${label} exceeded its bounded fixture phase`),
								);
							},
							Math.min(timeoutMs, remaining),
						);
					}),
				]);
				if (!active) throw retired();
				return value;
			} finally {
				clearTimeout(timer);
			}
		},
	};
	const errors: unknown[] = [];
	try {
		await bounded(
			Promise.resolve().then(() => input.body(phase)),
			input.bodyMs,
			"native ownership fixture body",
		);
	} catch (error) {
		errors.push(error);
	} finally {
		active = false;
		try {
			await bounded(
				Promise.resolve().then(input.cleanup),
				input.cleanupMs,
				"native ownership fixture cleanup",
			);
		} catch (error) {
			errors.push(error);
		} finally {
			try {
				await bounded(
					Promise.resolve().then(input.finish),
					input.finishMs,
					"native ownership fixture owned-child termination",
				);
			} catch (error) {
				errors.push(error);
			}
		}
	}
	if (errors.length === 1) throw errors[0];
	if (errors.length > 1)
		throw new AggregateError(errors, "native ownership fixture failed");
}
