// Register the native sidecar spawn factory (side effect). After the
// @secure-exec/core SidecarProcess refactor, native spawn is provided by a
// separately-registered factory; importing native-client wires it up so
// SidecarProcess.spawn works in this native runtime.
import "@secure-exec/core/native-client";

export {
	NATIVE_SIDECAR_FRAME_TIMEOUT_MS,
	SidecarProcess,
	SidecarProcess as NativeSidecarProcessClient,
	SidecarEventBufferOverflow,
	SidecarProcessError,
	SidecarProcessExited,
} from "@secure-exec/core/sidecar-client";

export type {
	AuthenticatedSession,
	CreatedVm,
	ExtEnvelope,
	GuestFilesystemStat,
	RootFilesystemEntry,
	RootFilesystemLowerDescriptor,
	SidecarEventSelector,
	SidecarFsPermissionRule,
	SidecarMountDescriptor,
	SidecarMountPluginDescriptor,
	SidecarPatternPermissionRule,
	SidecarPermissionMode,
	SidecarPermissionScope,
	SidecarPermissionsPolicy,
	SidecarProcessSnapshotEntry,
	SidecarProjectedModuleDescriptor,
	SidecarRegisteredHostCallbackDefinition,
	SidecarRegisteredHostCallbackExample,
	SidecarRequestFrame,
	SidecarRequestHandler,
	SidecarRequestPayload,
	SidecarResponseFrame,
	SidecarResponsePayload,
	SidecarRulePermissions,
	SidecarSessionState,
	SidecarSignalHandlerRegistration,
	SidecarSignalState,
	SidecarSocketStateEntry,
	SidecarSoftwareDescriptor,
	SidecarSpawnOptions,
	SidecarSpawnOptions as NativeSidecarSpawnOptions,
	SidecarZombieTimerCount,
} from "@secure-exec/core/sidecar-client";
