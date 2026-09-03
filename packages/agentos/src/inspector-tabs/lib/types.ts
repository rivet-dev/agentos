import type { Output } from "../../generated/contract";

export interface SoftwareBundle {
	name: string;
	slug: string;
	version: string;
	source: "rivet-dev" | "user";
	binaries: string[];
}

export interface KernelProcessInfo {
	generation: number | bigint;
	process: Output.ActorProcessId | null;
	pid: number;
	ppid: number;
	pgid: number;
	sid: number;
	driver: string;
	command: string;
	args: string[];
	cwd: string;
	status: "running" | "exited";
	exitCode: number | null;
	startTime: number;
	exitTime: number | null;
}

export interface ProcessTreeNode extends KernelProcessInfo {
	children: ProcessTreeNode[];
}

export interface ProcessOutputPayload {
	pid: number;
	stream: "stdout" | "stderr";
	data: unknown;
	seq: number;
}

export interface ProcessExitPayload {
	pid: number;
	exitCode: number;
}

export interface ShellDataPayload {
	shellId: string;
	data: unknown;
	seq: number | bigint;
}

export interface ShellExitPayload {
	shellId: string;
	exitCode: number;
}

export interface SignedPreviewUrl {
	path: string;
	token: string;
	port: number;
	expiresAt: number;
}

export interface FsEntry {
	name: string;
	path: string;
	dir: boolean;
	size?: number;
	symlink?: boolean;
	virtual?: boolean;
}

export interface FileContent {
	path: string;
	sizeBytes: number;
	mtimeMs: number;
	text: string | null;
	bytes: Uint8Array | null;
	oversize: boolean;
	special?: boolean;
}

export interface MountInfo {
	path: string;
	kind: string;
	readOnly: boolean;
	config?: unknown | null;
}

export interface ShellInfo {
	shellId: string;
	openedAt: number;
}

export type ShellReplayMode = "none" | "screen" | "scrollback";

export interface ShellSnapshot {
	shellId: string;
	mode: ShellReplayMode;
	seq: number | bigint;
	truncated: boolean;
	cols: number;
	rows: number;
	data: Uint8Array;
}

export type RuntimeHealth = Output.VmStatusSnapshot;
export type VmShutdownPayload = Output.VmShutdown;
