import type { PermissionsPolicy } from "@rivet-dev/agentos-runtime-core/vm-config";
import type { Permissions } from "../runtime.js";

function serializeFilesystemScope(
	scope: Exclude<Permissions["fs"], string | undefined>,
) {
	return {
		...(scope.default === undefined ? {} : { default: scope.default }),
		rules: scope.rules.map((rule) => ({ ...rule })),
	};
}

function serializePatternScope(
	scope: Exclude<
		| Permissions["network"]
		| Permissions["childProcess"]
		| Permissions["process"]
		| Permissions["env"]
		| Permissions["binding"],
		string | undefined
	>,
) {
	return {
		...(scope.default === undefined ? {} : { default: scope.default }),
		rules: scope.rules.map((rule) => ({ ...rule })),
	};
}

export function serializePermissionsForSidecar(
	permissions: Permissions,
): PermissionsPolicy;
export function serializePermissionsForSidecar(): undefined;
export function serializePermissionsForSidecar(
	permissions?: Permissions,
): PermissionsPolicy | undefined {
	if (!permissions) {
		return undefined;
	}

	return {
		fs:
			typeof permissions.fs === "string" || !permissions.fs
				? permissions.fs
				: serializeFilesystemScope(permissions.fs),
		network:
			typeof permissions.network === "string" || !permissions.network
				? permissions.network
				: serializePatternScope(permissions.network),
		childProcess:
			typeof permissions.childProcess === "string" || !permissions.childProcess
				? permissions.childProcess
				: serializePatternScope(permissions.childProcess),
		process:
			typeof permissions.process === "string" || !permissions.process
				? permissions.process
				: serializePatternScope(permissions.process),
		env:
			typeof permissions.env === "string" || !permissions.env
				? permissions.env
				: serializePatternScope(permissions.env),
		binding:
			typeof permissions.binding === "string" || !permissions.binding
				? permissions.binding
				: serializePatternScope(permissions.binding),
	};
}
