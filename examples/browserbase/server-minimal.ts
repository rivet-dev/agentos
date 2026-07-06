import browserbase from "@agentos-software/browserbase";
import pi from "@agentos-software/pi";
import { agentOS, setup } from "@rivet-dev/agentos";

// `browse` is exposed inside the VM as a command on `$PATH`.
const vm = agentOS({
	software: [pi, browserbase],
});

export const registry = setup({ use: { vm } });
registry.start();
