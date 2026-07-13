import {
	fromGeneratedRootFilesystemEntry,
	type LiveRootFilesystemEntry,
	type LiveRootFilesystemEntryEncoding,
} from "./filesystem.js";
import { fromGeneratedExtEnvelope, type LiveExtEnvelope } from "./ext.js";
import * as protocol from "./generated-protocol.js";
import { parseJsonUtf8 } from "./json.js";
import type { LiveCronOverlap } from "./request-payloads.js";
import { bigIntToSafeNumber } from "./numbers.js";
import {
	fromGeneratedFilesystemOperation,
	fromGeneratedGuestFilesystemOperation,
	fromGeneratedRootFilesystemEntryEncoding,
	fromGeneratedSignalDispositionAction,
	type LiveFilesystemOperation,
	type LiveGuestFilesystemOperation,
	type LiveSignalDispositionAction,
} from "./protocol-maps.js";
import {
	fromGeneratedGuestFilesystemStat,
	fromGeneratedProcessSnapshotEntry,
	fromGeneratedSocketStateEntry,
	type LiveGuestFilesystemStat,
	type LiveProcessSnapshotEntry,
	type LiveSocketStateEntry,
} from "./state.js";

/** A directory child with its file type, from `read_dir` (no extra lstat). */
export interface LiveGuestDirEntry {
	name: string;
	path: string;
	isDirectory: boolean;
	isSymbolicLink: boolean;
	size: bigint;
}

export interface LiveSignalHandlerRegistration {
	action: LiveSignalDispositionAction;
	mask: number[];
	flags: number;
}

export interface LiveQueueSnapshotEntry {
	name: string;
	category: string;
	depth: number;
	high_water: number;
	capacity: number;
	fill_percent: number;
}

export interface LiveResourceSnapshot {
	running_processes: number;
	exited_processes: number;
	fd_tables: number;
	open_fds: number;
	pipes: number;
	pipe_buffered_bytes: number;
	ptys: number;
	pty_buffered_input_bytes: number;
	pty_buffered_output_bytes: number;
	sockets: number;
	socket_listeners: number;
	socket_connections: number;
	socket_buffered_bytes: number;
	socket_datagram_queue_len: number;
	queue_snapshots: LiveQueueSnapshotEntry[];
}

export interface LiveProjectedCommand {
	name: string;
	guest_path: string;
}

export interface LivePackageCommands {
	package_name: string;
	commands: string[];
}

export interface LiveAgentosProjectedAgent {
	id: string;
	acp_entrypoint: string;
	adapter_entrypoint: string;
}

export interface LiveCronAlarm {
	generation: number;
	next_alarm_ms?: number;
}

export interface LiveCronJobEntry {
	id: string;
	schedule: string;
	action: unknown;
	overlap: LiveCronOverlap;
	last_run_ms?: number;
	next_run_ms?: number;
	run_count: number;
	running: boolean;
}

export interface LiveCronRun {
	run_id: string;
	job_id: string;
	action: unknown;
}

export interface LiveCronEventRecord {
	kind: "fire" | "complete" | "error";
	job_id: string;
	time_ms: number;
	duration_ms?: number;
	error?: string;
}

export interface LiveCronDispatch {
	alarm: LiveCronAlarm;
	runs: LiveCronRun[];
	events: LiveCronEventRecord[];
}

export type LiveResponsePayload =
	| {
			type: "authenticated";
			sidecar_id: string;
			connection_id: string;
			max_frame_bytes: number;
	  }
	| {
			type: "session_opened";
			session_id: string;
			owner_connection_id: string;
	  }
	| {
			type: "vm_created";
			vm_id: string;
			guest_cwd: string;
			guest_env: Record<string, string>;
	  }
	| {
			type: "vm_initialized";
			vm_id: string;
			guest_cwd: string;
			guest_env: Record<string, string>;
			applied_mounts: number;
			projected_commands: LiveProjectedCommand[];
			agents: LiveAgentosProjectedAgent[];
			host_callbacks: Array<{
				registration: string;
				command_count: number;
			}>;
	  }
	| {
			type: "vm_configured";
			applied_mounts: number;
			projected_commands: LiveProjectedCommand[];
			agents: LiveAgentosProjectedAgent[];
	  }
	| {
			type: "package_linked";
			projected_commands: LiveProjectedCommand[];
			agents: LiveAgentosProjectedAgent[];
	  }
	| {
			type: "provided_commands_response";
			packages: LivePackageCommands[];
	  }
	| { type: "cron_scheduled"; id: string; alarm: LiveCronAlarm }
	| { type: "cron_jobs"; jobs: LiveCronJobEntry[]; alarm: LiveCronAlarm }
	| {
			type: "cron_cancelled";
			id: string;
			cancelled: boolean;
			alarm: LiveCronAlarm;
	  }
	| {
			type: "cron_wake";
			alarm: LiveCronAlarm;
			runs: LiveCronRun[];
			events: LiveCronEventRecord[];
	  }
	| {
			type: "cron_run_completed";
			alarm: LiveCronAlarm;
			runs: LiveCronRun[];
			events: LiveCronEventRecord[];
	  }
	| { type: "cron_state_exported"; state: string }
	| {
			type: "cron_state_imported";
			alarm: LiveCronAlarm;
			runs: LiveCronRun[];
			events: LiveCronEventRecord[];
	  }
	| {
			type: "host_callbacks_registered";
			registration: string;
			command_count: number;
	  }
	| {
			type: "layer_created";
			layer_id: string;
	  }
	| {
			type: "layer_sealed";
			layer_id: string;
	  }
	| {
			type: "snapshot_imported";
			layer_id: string;
	  }
	| {
			type: "snapshot_exported";
			layer_id: string;
			entries: LiveRootFilesystemEntry[];
	  }
	| {
			type: "overlay_created";
			layer_id: string;
	  }
	| {
			type: "root_filesystem_bootstrapped";
			entry_count: number;
	  }
	| {
			type: "guest_filesystem_result";
			operation: LiveGuestFilesystemOperation;
			path: string;
			content?: string;
			encoding?: LiveRootFilesystemEntryEncoding;
			entries?: LiveGuestDirEntry[];
			stat?: LiveGuestFilesystemStat;
			exists?: boolean;
			target?: string;
	  }
	| {
			type: "guest_kernel_result";
			payload: ArrayBuffer;
	  }
	| {
			type: "root_filesystem_snapshot";
			entries: LiveRootFilesystemEntry[];
	  }
	| {
			type: "vm_disposed";
			vm_id: string;
	  }
	| {
			type: "process_started";
			process_id: string;
			pid?: number;
	  }
	| {
			type: "stdin_written";
			process_id: string;
			accepted_bytes: number;
	  }
	| {
			type: "pty_resized";
			process_id: string;
			cols: number;
			rows: number;
	  }
	| {
			type: "stdin_closed";
			process_id: string;
	  }
	| {
			type: "process_killed";
			process_id: string;
	  }
	| {
			type: "process_snapshot";
			processes: LiveProcessSnapshotEntry[];
	  }
	| ({
			type: "resource_snapshot";
	  } & LiveResourceSnapshot)
	| {
			type: "listener_snapshot";
			listener?: LiveSocketStateEntry;
	  }
	| {
			type: "bound_udp_snapshot";
			socket?: LiveSocketStateEntry;
	  }
	| {
			type: "vm_fetch_result";
			response_json: string;
	  }
	| {
			type: "signal_state";
			process_id: string;
			handlers: Record<string, LiveSignalHandlerRegistration>;
	  }
	| {
			type: "zombie_timer_count";
			count: number;
	  }
	| {
			type: "filesystem_result";
			operation: LiveFilesystemOperation;
			status: string;
			payload_size_bytes: number;
	  }
	| {
			type: "persistence_state";
			key: string;
			found: boolean;
			payload_size_bytes: number;
	  }
	| {
			type: "persistence_flushed";
			key: string;
			committed_bytes: number;
	  }
	| {
			type: "rejected";
			code: string;
			message: string;
	  }
	| {
			type: "ext_result";
			envelope: LiveExtEnvelope;
	  };

export function fromGeneratedResponsePayload(
	payload: protocol.ResponsePayload,
): LiveResponsePayload {
	switch (payload.tag) {
		case "AuthenticatedResponse":
			return {
				type: "authenticated",
				sidecar_id: payload.val.sidecarId,
				connection_id: payload.val.connectionId,
				max_frame_bytes: payload.val.maxFrameBytes,
			};
		case "SessionOpenedResponse":
			return {
				type: "session_opened",
				session_id: payload.val.sessionId,
				owner_connection_id: payload.val.ownerConnectionId,
			};
		case "VmCreatedResponse":
			return {
				type: "vm_created",
				vm_id: payload.val.vmId,
				guest_cwd: payload.val.guestCwd,
				guest_env: Object.fromEntries(payload.val.guestEnv),
			};
		case "VmInitializedResponse":
			return {
				type: "vm_initialized",
				vm_id: payload.val.vmId,
				guest_cwd: payload.val.guestCwd,
				guest_env: Object.fromEntries(payload.val.guestEnv),
				applied_mounts: payload.val.appliedMounts,
				projected_commands: payload.val.projectedCommands.map((command) => ({
					name: command.name,
					guest_path: command.guestPath,
				})),
				agents: payload.val.agents.map(fromGeneratedAgentosProjectedAgent),
				host_callbacks: payload.val.hostCallbacks.map((registration) => ({
					registration: registration.registration,
					command_count: registration.commandCount,
				})),
			};
		case "VmDisposedResponse":
			return { type: "vm_disposed", vm_id: payload.val.vmId };
		case "RootFilesystemBootstrappedResponse":
			return {
				type: "root_filesystem_bootstrapped",
				entry_count: payload.val.entryCount,
			};
		case "VmConfiguredResponse":
			return {
				type: "vm_configured",
				applied_mounts: payload.val.appliedMounts,
				projected_commands: payload.val.projectedCommands.map((command) => ({
					name: command.name,
					guest_path: command.guestPath,
				})),
				agents: payload.val.agents.map(fromGeneratedAgentosProjectedAgent),
			};
		case "PackageLinkedResponse":
			return {
				type: "package_linked",
				projected_commands: payload.val.projectedCommands.map((command) => ({
					name: command.name,
					guest_path: command.guestPath,
				})),
				agents: payload.val.agents.map(fromGeneratedAgentosProjectedAgent),
			};
		case "ProvidedCommandsResponse":
			return {
				type: "provided_commands_response",
				packages: payload.val.packages.map((pkg) => ({
					package_name: pkg.packageName,
					commands: [...pkg.commands],
				})),
			};
		case "CronScheduledResponse":
			return {
				type: "cron_scheduled",
				id: payload.val.id,
				alarm: fromGeneratedCronAlarm(payload.val.alarm),
			};
		case "CronJobsResponse":
			return {
				type: "cron_jobs",
				jobs: payload.val.jobs.map(fromGeneratedCronJobEntry),
				alarm: fromGeneratedCronAlarm(payload.val.alarm),
			};
		case "CronCancelledResponse":
			return {
				type: "cron_cancelled",
				id: payload.val.id,
				cancelled: payload.val.cancelled,
				alarm: fromGeneratedCronAlarm(payload.val.alarm),
			};
		case "CronWakeResponse":
			return {
				type: "cron_wake",
				alarm: fromGeneratedCronAlarm(payload.val.alarm),
				runs: payload.val.runs.map(fromGeneratedCronRun),
				events: payload.val.events.map(fromGeneratedCronEvent),
			};
		case "CronRunCompletedResponse":
			return {
				type: "cron_run_completed",
				alarm: fromGeneratedCronAlarm(payload.val.alarm),
				runs: payload.val.runs.map(fromGeneratedCronRun),
				events: payload.val.events.map(fromGeneratedCronEvent),
			};
		case "CronStateExportedResponse":
			return {
				type: "cron_state_exported",
				state: payload.val.state,
			};
		case "CronStateImportedResponse":
			return {
				type: "cron_state_imported",
				alarm: fromGeneratedCronAlarm(payload.val.alarm),
				runs: payload.val.runs.map(fromGeneratedCronRun),
				events: payload.val.events.map(fromGeneratedCronEvent),
			};
		case "HostCallbacksRegisteredResponse":
			return {
				type: "host_callbacks_registered",
				registration: payload.val.registration,
				command_count: payload.val.commandCount,
			};
		case "LayerCreatedResponse":
			return { type: "layer_created", layer_id: payload.val.layerId };
		case "LayerSealedResponse":
			return { type: "layer_sealed", layer_id: payload.val.layerId };
		case "SnapshotImportedResponse":
			return { type: "snapshot_imported", layer_id: payload.val.layerId };
		case "SnapshotExportedResponse":
			return {
				type: "snapshot_exported",
				layer_id: payload.val.layerId,
				entries: payload.val.entries.map(fromGeneratedRootFilesystemEntry),
			};
		case "OverlayCreatedResponse":
			return { type: "overlay_created", layer_id: payload.val.layerId };
		case "GuestFilesystemResultResponse":
			return {
				type: "guest_filesystem_result",
				operation: fromGeneratedGuestFilesystemOperation(payload.val.operation),
				path: payload.val.path,
				...(payload.val.content !== null
					? { content: payload.val.content }
					: {}),
				...(payload.val.encoding !== null
					? {
							encoding: fromGeneratedRootFilesystemEntryEncoding(
								payload.val.encoding,
							),
						}
					: {}),
				...(payload.val.entries !== null
					? {
							entries: payload.val.entries.map((entry) => ({
								name: entry.name,
								path: entry.path,
								isDirectory: entry.isDirectory,
								isSymbolicLink: entry.isSymbolicLink,
								size: entry.size,
							})),
						}
					: {}),
				...(payload.val.stat !== null
					? { stat: fromGeneratedGuestFilesystemStat(payload.val.stat) }
					: {}),
				...(payload.val.exists !== null ? { exists: payload.val.exists } : {}),
				...(payload.val.target !== null ? { target: payload.val.target } : {}),
			};
		case "GuestKernelResultResponse":
			return {
				type: "guest_kernel_result",
				payload: payload.val.payload,
			};
		case "RootFilesystemSnapshotResponse":
			return {
				type: "root_filesystem_snapshot",
				entries: payload.val.entries.map(fromGeneratedRootFilesystemEntry),
			};
		case "ProcessStartedResponse":
			return {
				type: "process_started",
				process_id: payload.val.processId,
				...(payload.val.pid !== null ? { pid: payload.val.pid } : {}),
			};
		case "StdinWrittenResponse":
			return {
				type: "stdin_written",
				process_id: payload.val.processId,
				accepted_bytes: bigIntToSafeNumber(
					payload.val.acceptedBytes,
					"stdin_written.accepted_bytes",
				),
			};
		case "PtyResizedResponse":
			return {
				type: "pty_resized",
				process_id: payload.val.processId,
				cols: payload.val.cols,
				rows: payload.val.rows,
			};
		case "StdinClosedResponse":
			return { type: "stdin_closed", process_id: payload.val.processId };
		case "ProcessKilledResponse":
			return { type: "process_killed", process_id: payload.val.processId };
		case "ProcessSnapshotResponse":
			return {
				type: "process_snapshot",
				processes: payload.val.processes.map(fromGeneratedProcessSnapshotEntry),
			};
		case "ResourceSnapshotResponse":
			return {
				type: "resource_snapshot",
				running_processes: bigIntToSafeNumber(
					payload.val.runningProcesses,
					"resource_snapshot.running_processes",
				),
				exited_processes: bigIntToSafeNumber(
					payload.val.exitedProcesses,
					"resource_snapshot.exited_processes",
				),
				fd_tables: bigIntToSafeNumber(
					payload.val.fdTables,
					"resource_snapshot.fd_tables",
				),
				open_fds: bigIntToSafeNumber(
					payload.val.openFds,
					"resource_snapshot.open_fds",
				),
				pipes: bigIntToSafeNumber(payload.val.pipes, "resource_snapshot.pipes"),
				pipe_buffered_bytes: bigIntToSafeNumber(
					payload.val.pipeBufferedBytes,
					"resource_snapshot.pipe_buffered_bytes",
				),
				ptys: bigIntToSafeNumber(payload.val.ptys, "resource_snapshot.ptys"),
				pty_buffered_input_bytes: bigIntToSafeNumber(
					payload.val.ptyBufferedInputBytes,
					"resource_snapshot.pty_buffered_input_bytes",
				),
				pty_buffered_output_bytes: bigIntToSafeNumber(
					payload.val.ptyBufferedOutputBytes,
					"resource_snapshot.pty_buffered_output_bytes",
				),
				sockets: bigIntToSafeNumber(
					payload.val.sockets,
					"resource_snapshot.sockets",
				),
				socket_listeners: bigIntToSafeNumber(
					payload.val.socketListeners,
					"resource_snapshot.socket_listeners",
				),
				socket_connections: bigIntToSafeNumber(
					payload.val.socketConnections,
					"resource_snapshot.socket_connections",
				),
				socket_buffered_bytes: bigIntToSafeNumber(
					payload.val.socketBufferedBytes,
					"resource_snapshot.socket_buffered_bytes",
				),
				socket_datagram_queue_len: bigIntToSafeNumber(
					payload.val.socketDatagramQueueLen,
					"resource_snapshot.socket_datagram_queue_len",
				),
				queue_snapshots: payload.val.queueSnapshots.map((queue) => ({
					name: queue.name,
					category: queue.category,
					depth: bigIntToSafeNumber(
						queue.depth,
						"resource_snapshot.queue.depth",
					),
					high_water: bigIntToSafeNumber(
						queue.highWater,
						"resource_snapshot.queue.high_water",
					),
					capacity: bigIntToSafeNumber(
						queue.capacity,
						"resource_snapshot.queue.capacity",
					),
					fill_percent: bigIntToSafeNumber(
						queue.fillPercent,
						"resource_snapshot.queue.fill_percent",
					),
				})),
			};
		case "ListenerSnapshotResponse":
			return {
				type: "listener_snapshot",
				...(payload.val.listener !== null
					? { listener: fromGeneratedSocketStateEntry(payload.val.listener) }
					: {}),
			};
		case "BoundUdpSnapshotResponse":
			return {
				type: "bound_udp_snapshot",
				...(payload.val.socket !== null
					? { socket: fromGeneratedSocketStateEntry(payload.val.socket) }
					: {}),
			};
		case "SignalStateResponse":
			return {
				type: "signal_state",
				process_id: payload.val.processId,
				handlers: Object.fromEntries(
					[...payload.val.handlers].map(([signal, registration]) => [
						String(signal),
						{
							action: fromGeneratedSignalDispositionAction(registration.action),
							mask: Array.from(registration.mask),
							flags: registration.flags,
						},
					]),
				),
			};
		case "ZombieTimerCountResponse":
			return {
				type: "zombie_timer_count",
				count: bigIntToSafeNumber(
					payload.val.count,
					"zombie_timer_count.count",
				),
			};
		case "FilesystemResultResponse":
			return {
				type: "filesystem_result",
				operation: fromGeneratedFilesystemOperation(payload.val.operation),
				status: payload.val.status,
				payload_size_bytes: bigIntToSafeNumber(
					payload.val.payloadSizeBytes,
					"filesystem_result.payload_size_bytes",
				),
			};
		case "PermissionDecisionResponse":
			throw new Error(
				"unsupported bare response payload tag: permission_decision",
			);
		case "PersistenceStateResponse":
			return {
				type: "persistence_state",
				key: payload.val.key,
				found: payload.val.found,
				payload_size_bytes: bigIntToSafeNumber(
					payload.val.payloadSizeBytes,
					"persistence_state.payload_size_bytes",
				),
			};
		case "PersistenceFlushedResponse":
			return {
				type: "persistence_flushed",
				key: payload.val.key,
				committed_bytes: bigIntToSafeNumber(
					payload.val.committedBytes,
					"persistence_flushed.committed_bytes",
				),
			};
		case "RejectedResponse":
			return {
				type: "rejected",
				code: payload.val.code,
				message: payload.val.message,
			};
		case "VmFetchResponse":
			return {
				type: "vm_fetch_result",
				response_json: payload.val.responseJson,
			};
		case "ExtEnvelope":
			return {
				type: "ext_result",
				envelope: fromGeneratedExtEnvelope(payload.val),
			};
	}
}

function fromGeneratedCronAlarm(value: protocol.CronAlarm): LiveCronAlarm {
	return {
		generation: bigIntToSafeNumber(value.generation, "cron_alarm.generation"),
		...(value.nextAlarmMs === null
			? {}
			: {
					next_alarm_ms: bigIntToSafeNumber(
						value.nextAlarmMs,
						"cron_alarm.next_alarm_ms",
					),
				}),
	};
}

function fromGeneratedCronOverlap(
	value: protocol.CronOverlap,
): LiveCronOverlap {
	switch (value) {
		case protocol.CronOverlap.Allow:
			return "allow";
		case protocol.CronOverlap.Skip:
			return "skip";
		case protocol.CronOverlap.Queue:
			return "queue";
	}
}

function fromGeneratedCronJobEntry(
	value: protocol.CronJobEntry,
): LiveCronJobEntry {
	return {
		id: value.id,
		schedule: value.schedule,
		action: parseJsonUtf8(value.action, "cron_job.action"),
		overlap: fromGeneratedCronOverlap(value.overlap),
		...(value.lastRunMs === null
			? {}
			: {
					last_run_ms: bigIntToSafeNumber(
						value.lastRunMs,
						"cron_job.last_run_ms",
					),
				}),
		...(value.nextRunMs === null
			? {}
			: {
					next_run_ms: bigIntToSafeNumber(
						value.nextRunMs,
						"cron_job.next_run_ms",
					),
				}),
		run_count: bigIntToSafeNumber(value.runCount, "cron_job.run_count"),
		running: value.running,
	};
}

function fromGeneratedCronRun(value: protocol.CronRun): LiveCronRun {
	return {
		run_id: value.runId,
		job_id: value.jobId,
		action: parseJsonUtf8(value.action, "cron_run.action"),
	};
}

function fromGeneratedCronEvent(
	value: protocol.CronEventRecord,
): LiveCronEventRecord {
	const kind = (() => {
		switch (value.kind) {
			case protocol.CronEventKind.Fire:
				return "fire" as const;
			case protocol.CronEventKind.Complete:
				return "complete" as const;
			case protocol.CronEventKind.Error:
				return "error" as const;
		}
	})();
	return {
		kind,
		job_id: value.jobId,
		time_ms: bigIntToSafeNumber(value.timeMs, "cron_event.time_ms"),
		...(value.durationMs === null
			? {}
			: {
					duration_ms: bigIntToSafeNumber(
						value.durationMs,
						"cron_event.duration_ms",
					),
				}),
		...(value.error === null ? {} : { error: value.error }),
	};
}

export function fromGeneratedCronDispatch(
	value: protocol.CronDispatchEvent,
): LiveCronDispatch {
	return {
		alarm: fromGeneratedCronAlarm(value.alarm),
		runs: value.runs.map(fromGeneratedCronRun),
		events: value.events.map(fromGeneratedCronEvent),
	};
}

function fromGeneratedAgentosProjectedAgent(
	agent: protocol.AgentosProjectedAgent,
): LiveAgentosProjectedAgent {
	return {
		id: agent.id,
		acp_entrypoint: agent.acpEntrypoint,
		adapter_entrypoint: agent.adapterEntrypoint,
	};
}
