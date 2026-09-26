import type { ProcessMemorySnapshot } from "./memory.js";

type RetainedSample = {
	backend: string;
	retained: ProcessMemorySnapshot;
	retainedDelta: ProcessMemorySnapshot;
};

export function retainedMemoryMedian(
	samples: RetainedSample[],
	backend: string,
	counter: "rssBytes" | "pssBytes",
): number {
	const values = samples.filter((sample) => sample.backend === backend)
		.map((sample) => sample.retained[counter]).sort((a, b) => a - b);
	if (values.length === 0) throw new Error(`no retained memory samples for ${backend}`);
	const middle = Math.floor(values.length / 2);
	return values.length % 2 ? values[middle] : (values[middle - 1] + values[middle]) / 2;
}
