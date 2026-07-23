// docs:start quickstart
import { createClient } from "@rivet-dev/agentos/client";
import type { registry } from "./server";

const client = createClient<typeof registry>({
	endpoint: "http://localhost:6420",
});
const agent = client.vm.getOrCreate("my-agent");
const apiKey = process.env.COMMAND_CODE_API_KEY;
if (!apiKey) throw new Error("COMMAND_CODE_API_KEY is required");

await agent.mkdir("/workspace", { recursive: true });
const result = await agent.execArgv(
	"cmd",
	[
		"-p",
		"What files are in the current directory?",
		"--output-format",
		"json",
		"--trust",
		"--yolo",
		"--skip-onboarding",
		"--no-auto-update",
	],
	{
		cwd: "/workspace",
		env: { COMMAND_CODE_API_KEY: apiKey },
	},
);
if (result.exitCode !== 0) throw new Error(result.stderr || result.stdout);
console.log(result.stdout);
// docs:end quickstart
