declare module "./opencode-acp.mjs" {
	export const AcpCommand: {
		handler(args: {
			port: number;
			hostname: string;
			mdns: boolean;
			"mdns-domain": string;
			cors: string[];
			cwd: string;
		}): Promise<void>;
	};
}

export {};
