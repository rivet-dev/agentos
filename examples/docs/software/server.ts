import { agentOS, setup } from "@rivet-dev/agentos";
import pi from "./software/pi";

// Declare which software packages are available inside the VM. Each entry is an
// imported package; together they determine which CLI commands the agent can
// run. Common utilities (coreutils, sed, grep, gawk, findutils, diffutils, tar,
// and gzip) ship by default; `pi` is the agent itself.
const vm = agentOS({
	software: [pi],
});

export const registry = setup({ use: { vm } });

registry.start();
