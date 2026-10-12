// Native snapshot setup injects the Rust defaults before bridge evaluation.
// Native code captures and removes the installer before snapshot userland or
// guest entry. No policy global or guest-callable setter is consulted later.
type GuestBridgeLimits = Readonly<{
	maxChunkSizeLineBytes: number;
	maxPendingResponseBytes: number;
	maxReadlineHistoryEntries: number;
}>;
const freeze = Object.freeze;
const copy = (value: GuestBridgeLimits): GuestBridgeLimits =>
	freeze({
		maxChunkSizeLineBytes: value.maxChunkSizeLineBytes,
		maxPendingResponseBytes: value.maxPendingResponseBytes,
		maxReadlineHistoryEntries: value.maxReadlineHistoryEntries,
	});
let limits = copy(
	(
		globalThis as typeof globalThis & {
			__agentOSBridgeLimits: GuestBridgeLimits;
		}
	).__agentOSBridgeLimits,
);
Object.defineProperty(globalThis, "__agentOSBridgeLimitInstaller", {
	configurable: true,
	enumerable: false,
	writable: false,
	value: (value: GuestBridgeLimits) => {
		limits = copy(value);
	},
});
export function guestBridgeLimits(): GuestBridgeLimits {
	return limits;
}
