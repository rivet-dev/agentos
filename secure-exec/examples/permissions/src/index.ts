// docs:start default
import { evaluate } from "secure-exec";

const request = `(async () => {
	const response = await fetch("https://example.com");
	await response.text();
	return response.status;
})()`;

// The network is denied unless you allow it.
const denied = await evaluate<number>(request, {
	output: { capture: "stderr" },
});
if (denied.outcome !== "succeeded") {
	console.log(denied.outcome, denied.stderr?.split("\n")[0]); // failed TypeError: fetch failed
}
// docs:end default

// docs:start allow
// VM options such as `permissions` go on the call. A policy is merged over the
// defaults, so granting the network keeps everything else working.
const allowed = await evaluate<number>(request, {
	permissions: { network: "allow" },
});
console.log(allowed.outcome === "succeeded" ? allowed.value : allowed.error); // 200
// docs:end allow

// docs:start scoped
// Allow a single host instead of the whole network.
const scoped = await evaluate<number>(request, {
	permissions: {
		network: {
			default: "deny",
			rules: [
				{
					mode: "allow",
					operations: ["*"],
					patterns: ["dns://example.com", "tcp://example.com:*"],
				},
			],
		},
	},
});
console.log(scoped.outcome === "succeeded" ? scoped.value : scoped.error); // 200
// docs:end scoped
