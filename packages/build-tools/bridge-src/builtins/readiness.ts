import { exposeCustomGlobal } from "../global-exposure.js";

type CapabilityIdentity = {
	capabilityId?: bigint | number | string;
	capabilityGeneration?: bigint | number | string;
};

type ReadinessTarget = (flags: number) => void;

const readinessTargets = new Map<string, ReadinessTarget>();

function readinessKey(
	capabilityId: bigint | number | string,
	capabilityGeneration: bigint | number | string,
): string {
	return `${String(capabilityId)}:${String(capabilityGeneration)}`;
}

export function registerCapabilityReadiness(
	identity: CapabilityIdentity | null | undefined,
	target: ReadinessTarget,
): boolean {
	if (
		identity?.capabilityId === void 0 ||
		identity.capabilityGeneration === void 0
	) {
		return false;
	}
	readinessTargets.set(
		readinessKey(identity.capabilityId, identity.capabilityGeneration),
		target,
	);
	return true;
}

export function unregisterCapabilityReadiness(
	identity: CapabilityIdentity | null | undefined,
): void {
	if (
		identity?.capabilityId === void 0 ||
		identity.capabilityGeneration === void 0
	) {
		return;
	}
	readinessTargets.delete(
		readinessKey(identity.capabilityId, identity.capabilityGeneration),
	);
}

function agentOSReadyDispatch(
	capabilityId: bigint,
	capabilityGeneration: bigint,
	flags: number,
): boolean {
	// Missing targets are expected for a close racing guest teardown. A stale
	// generation cannot resolve to a replacement capability because generation
	// is part of the key.
	const target = readinessTargets.get(
		readinessKey(capabilityId, capabilityGeneration),
	);
	if (!target) return false;
	target(flags);
	return true;
}

exposeCustomGlobal("_agentOSReadyDispatch", agentOSReadyDispatch);
