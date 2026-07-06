// Register the native sidecar spawn factory (side effect). After the
// @rivet-dev/agentos-runtime-core SidecarProcess refactor, native spawn is provided by a
// separately-registered factory; importing native-client wires it up so
// SidecarProcess.spawn works in this native runtime.
import "@rivet-dev/agentos-runtime-core/native-client";
import { SidecarProcess } from "@rivet-dev/agentos-runtime-core/sidecar-client";

export {
	NATIVE_SIDECAR_FRAME_TIMEOUT_MS,
	SidecarEventBufferOverflow,
	SidecarProcess,
	SidecarProcessError,
	SidecarProcessExited,
} from "@rivet-dev/agentos-runtime-core/sidecar-client";

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
} from "@rivet-dev/agentos-runtime-core/sidecar-client";

export type {
	SidecarVmConfiguredResponse as SidecarConfigureVmResult,
} from "@rivet-dev/agentos-runtime-core/sidecar-client";
