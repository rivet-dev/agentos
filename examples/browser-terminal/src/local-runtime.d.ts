interface BrowserLocalPiResult {
	error?: string;
	promptAnswered?: boolean;
}

interface Window {
	__agentOSTerminalConfig?: {
		software: "smoke" | "full";
	};
	__realTerminal?: {
		start(): Promise<{ masterFd: number; slaveFd: number }>;
		write(data: string): Promise<void>;
		screen(): string;
		output(): string;
		dispose(): Promise<void>;
	};
	__piTui?: {
		start(): Promise<BrowserLocalPiResult>;
		ask(prompt: string): Promise<BrowserLocalPiResult>;
		write(data: string): Promise<void>;
		screen(): string;
		dispose(): Promise<void>;
	};
}
