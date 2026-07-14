// docs:start core-readme-quickstart
import pi from "@agentos-software/pi";
import { AgentOs } from "@rivet-dev/agentos-core";

const apiKey = process.env.ANTHROPIC_API_KEY;
if (!apiKey) {
	throw new Error("ANTHROPIC_API_KEY is required");
}

const vm = await AgentOs.create({ software: [pi] });

try {
	const { sessionId } = await vm.createSession("pi", {
		env: { ANTHROPIC_API_KEY: apiKey },
	});

	try {
		const { text } = await vm.prompt(
			sessionId,
			"Write a hello world in TypeScript",
		);
		console.log(text);
	} finally {
		await vm.closeSession(sessionId);
	}
} finally {
	await vm.dispose();
}
// docs:end core-readme-quickstart
