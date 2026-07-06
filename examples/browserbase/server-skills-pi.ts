import browserbase from "@agentos-software/browserbase";
import pi from "@agentos-software/pi";
import { agentOS, setup } from "@rivet-dev/agentos";

// Mount the local `skills/` folder into Pi's skills directory
// (`~/.pi/agent/skills`) so the agent can discover the `browse` CLI skill.
const skillsDir = new URL("./skills", import.meta.url).pathname;

const vm = agentOS({
	software: [pi, browserbase],
	mounts: [
		{
			path: "/home/agentos/.pi/agent/skills",
			plugin: { id: "host_dir", config: { hostPath: skillsDir } },
			readOnly: true,
		},
	],
});

export const registry = setup({ use: { vm } });
registry.start();
