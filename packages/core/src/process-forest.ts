/** Internal topology stays separate from public, potentially overlapping display PIDs. */
export interface ProcessForestRecord<T> {
	info: T;
	key: string;
	parentKey: string | null;
}

export function buildProcessForest<T, R>(
	records: ProcessForestRecord<T>[],
	project: (info: T, children: R[]) => R,
): R[] {
	const keys = new Set(records.map((record) => record.key));
	const children = new Map<string, ProcessForestRecord<T>[]>();
	const roots: ProcessForestRecord<T>[] = [];
	for (const record of records) {
		if (record.parentKey !== null && keys.has(record.parentKey)) {
			const siblings = children.get(record.parentKey) ?? [];
			siblings.push(record);
			children.set(record.parentKey, siblings);
		} else {
			roots.push(record);
		}
	}
	const seen = new Set<string>();
	const visit = (record: ProcessForestRecord<T>): R => {
		seen.add(record.key);
		const descendants = (children.get(record.key) ?? [])
			.filter((child) => !seen.has(child.key))
			.map(visit);
		return project(record.info, descendants);
	};
	return roots.map(visit);
}
