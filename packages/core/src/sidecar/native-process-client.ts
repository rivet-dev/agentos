// Register the native sidecar spawn factory (side effect). After the
// @secure-exec/core SidecarProcess refactor, native spawn is provided by a
// separately-registered factory; importing native-client wires it up so
// SidecarProcess.spawn works in this native runtime.
import "@secure-exec/core/native-client";
import { SidecarProcess } from "@secure-exec/core/sidecar-client";

export {
	NATIVE_SIDECAR_FRAME_TIMEOUT_MS,
	SidecarEventBufferOverflow,
	SidecarProcess,
	SidecarProcessError,
	SidecarProcessExited,
} from "@secure-exec/core/sidecar-client";

export const NativeSidecarProcessClient = SidecarProcess;

export type {
	AuthenticatedSession,
	CreatedVm,
	ExtEnvelope,
	GuestFilesystemStat,
	RootFilesystemEntry,
	RootFilesystemLowerDescriptor,
	SidecarEventSelector,
	SidecarFsPermissionRule,
	SidecarLinkPackageResult,
	SidecarMountDescriptor,
	SidecarMountPluginDescriptor,
	SidecarPatternPermissionRule,
	SidecarPermissionMode,
	SidecarPermissionScope,
	SidecarPermissionsPolicy,
	SidecarProcessSnapshotEntry,
	SidecarProjectedAgent,
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

export type {
	SidecarVmConfiguredResponse as SidecarConfigureVmResult,
} from "@secure-exec/core/sidecar-client";
