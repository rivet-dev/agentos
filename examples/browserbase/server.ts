import browserbase from "@agentos-software/browserbase";
import claude from "@agentos-software/claude-code";
import { agentOS, setup } from "@rivet-dev/agentos";

// Install the `browse` CLI alongside the Claude Code agent. Because `browse`
// reaches the web through the Browserbase cloud, no local browser or sandbox is
// needed — `browse` is exposed inside the VM as a command on `$PATH`.
//
// Mount the local `skills/` folder into Claude Code's skills directory
// (`~/.claude/skills`) so the agent discovers Browserbase's `browse` CLI skill
// and reaches for it on its own, without being told to in the prompt.
const skillsDir = new URL("./skills", import.meta.url).pathname;

const vm = agentOS({
	software: [claude, browserbase],
	mounts: [
		{
			path: "/home/agentos/.claude/skills",
			plugin: { id: "host_dir", config: { hostPath: skillsDir } },
			readOnly: true,
		},
	],
});

export const registry = setup({ use: { vm } });
registry.start();
