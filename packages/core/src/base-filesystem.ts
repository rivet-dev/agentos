import type { FilesystemEntry } from "./filesystem-snapshot.js";

export interface BaseFilesystemEnvironment {
	env: Record<string, string>;
	prompt: string;
}

export type BaseFilesystemEntry = FilesystemEntry;

export interface BaseFilesystemSnapshot {
	source?: {
		snapshotPath?: string;
		image?: string;
		snapshotCreatedAt?: string;
		builtAt?: string;
		transforms?: string[];
	};
	environment: BaseFilesystemEnvironment;
	filesystem: {
		entries: BaseFilesystemEntry[];
	};
}
