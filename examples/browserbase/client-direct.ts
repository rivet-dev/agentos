import { createAgentOsClient } from "@rivet-dev/agentos";

const packageUrl = process.env.AGENTOS_BROWSERBASE_PACKAGE_URL;
if (!packageUrl) {
	throw new Error(
		"set AGENTOS_BROWSERBASE_PACKAGE_URL to the immutable .aospkg URL",
	);
}

const client = createAgentOsClient({
	endpoint: "http://localhost:6420",
});
const vm = client.agentOS.getOrCreate(["examples", "browserbase"], {
	createWithInput: {
		config: {
			software: [
				{
					url: packageUrl,
					digest: process.env.AGENTOS_BROWSERBASE_PACKAGE_DIGEST,
				},
			],
		},
	},
});

// Drive `browse` directly through the VM's process API. `browse cloud fetch`
// retrieves a page through the Browserbase cloud and returns it as JSON with the
// page rendered as markdown. `browse` reads its credentials from the command
// environment, which we pass through `run`.
const env = {
	BROWSERBASE_API_KEY: process.env.BROWSERBASE_API_KEY!,
	BROWSERBASE_PROJECT_ID: process.env.BROWSERBASE_PROJECT_ID!,
};

const { stdout } = await vm.process.run({
	command: "browse",
	args: ["cloud", "fetch", "https://example.com"],
	options: { env, captureStdio: true },
});
if (!stdout) {
	throw new Error("Browserbase command returned no output");
}

const page = JSON.parse(stdout) as { statusCode: number; content: string };
console.log(`fetched status ${page.statusCode}`);
console.log(page.content);
