#!/usr/bin/env node
process.env.OPENCODE_DISABLE_CONFIG_DEP_INSTALL ??= "1";
process.env.OPENCODE_DISABLE_EMBEDDED_WEB_UI ??= "1";

// @ts-expect-error Generated at build time by scripts/build-opencode-acp.mjs.
const { AcpCommand } = (await import("./opencode-acp/acp.js")) as {
	AcpCommand: {
		handler(args: {
			port: number;
			hostname: string;
			mdns: boolean;
			"mdns-domain": string;
			cors: string[];
			cwd: string;
		}): Promise<void>;
	};
};

await AcpCommand.handler({
	port: 0,
	hostname: "127.0.0.1",
	mdns: false,
	"mdns-domain": "opencode.local",
	cors: [],
	cwd: process.cwd(),
});

export {};
