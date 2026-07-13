import { existsSync } from "node:fs";
import { defineConfig, devices } from "@playwright/test";

const executablePath =
	process.env.AGENTOS_CHROME_EXECUTABLE_PATH ??
	(existsSync("/usr/bin/chromium") ? "/usr/bin/chromium" : undefined);

export default defineConfig({
	testDir: "./tests",
	timeout: 180_000,
	use: {
		baseURL: process.env.BROWSER_TERMINAL_BASE_URL ?? "http://127.0.0.1:5173",
		trace: "retain-on-failure",
	},
	projects: [
		{
			name: "chromium",
			use: {
				...devices["Desktop Chrome"],
				...(executablePath ? { launchOptions: { executablePath } } : {}),
			},
		},
	],
});
