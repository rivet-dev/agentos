import { mkdir } from "node:fs/promises";
import { join } from "node:path";
import { expect, type Page, test } from "@playwright/test";

declare global {
	interface Window {
		__agentOSTerminalDemo?: {
			screens(): Record<string, string>;
			write(shellId: string, text: string): Promise<void>;
		};
	}
}

interface ShellHarness {
	screen(): Promise<string>;
	write(text: string): Promise<void>;
}

const proofDirectory = process.env.AGENTOS_BROWSER_TERMINAL_PROOF_DIR;

async function captureProof(page: Page, name: string): Promise<void> {
	if (!proofDirectory) return;
	await mkdir(proofDirectory, { recursive: true });
	await page.screenshot({
		path: join(proofDirectory, `${name}.png`),
		fullPage: true,
	});
}

async function runShellCommand(
	shell: ShellHarness,
	command: string,
	timeout = 30_000,
): Promise<string> {
	// A mounted xterm can accept input before the Actor-delivered initial prompt.
	// Wait for that prompt so it cannot be mistaken for this command's completion.
	await expect
		.poll(async () => (await shell.screen()).trimEnd().split("\n").at(-1) ?? "", {
			timeout,
		})
		.toMatch(/sh-[0-9.]+\$\s*$/);
	const beforeCommand = await shell.screen();
	await shell.write(`${command}\r`);
	await expect
		.poll(() => shell.screen(), { timeout })
		.not.toBe(beforeCommand);
	// The restored interactive prompt is the real completion boundary. In
	// particular, wait for it before sending another command after a WASI child.
	await expect
		.poll(async () => (await shell.screen()).trimEnd().split("\n").at(-1) ?? "", {
			timeout,
		})
		.toMatch(/sh-[0-9.]+\$\s*$/);
	const screen = await shell.screen();
	expect(screen).not.toContain("WARN could not retrieve pid for child process");
	return screen;
}

async function proveVimGitAndChildPids(
	shell: ShellHarness,
	prefix: "ACTOR" | "BROWSER",
): Promise<void> {
	const slug = prefix.toLowerCase();
	const script = `/tmp/${slug}-vim-script.sh`;

	await shell.write(
		`vim -N -u NONE -i NONE -n --cmd 'set t_u7= t_RV= t_RF= t_RB=' ${script}\r`,
	);
	await expect
		.poll(() => shell.screen(), { timeout: 60_000 })
		.toContain("All");
	await shell.write(
		`i#!/bin/sh\recho ${prefix}_VIM_SCRIPT_E2E_OK\u001b:wq\r`,
	);
	await expect
		.poll(() => shell.screen(), { timeout: 30_000 })
		.toMatch(/sh-[0-9.]+\$/);

	await runShellCommand(shell, `chmod +x ${script}`);
	const scriptScreen = await runShellCommand(shell, script);
	expect(scriptScreen).toContain(`${prefix}_VIM_SCRIPT_E2E_OK`);

	const repository = `/tmp/${slug}-git`;
	await runShellCommand(shell, `rm -rf ${repository}`);
	await runShellCommand(shell, `mkdir ${repository}`);
	const initScreen = await runShellCommand(
		shell,
		`git init ${repository}`,
		60_000,
	);
	expect(initScreen).toContain("Initialized empty Git repository");
	await runShellCommand(
		shell,
		`echo ${prefix}_GIT_E2E_OK > ${repository}/proof.txt`,
	);
	await runShellCommand(
		shell,
		`git -C ${repository} add proof.txt`,
		60_000,
	);
	await runShellCommand(
		shell,
		`git -C ${repository} -c user.name=AgentOS -c user.email=agentos@example.com commit -m proof`,
		60_000,
	);
	const hashScreen = await runShellCommand(
		shell,
		`git -C ${repository} rev-parse HEAD`,
		60_000,
	);
	expect(hashScreen).toMatch(/[0-9a-f]{40}/);
	const gitScreen = await runShellCommand(shell, `cat ${repository}/proof.txt`);
	expect(gitScreen).toContain(`${prefix}_GIT_E2E_OK`);
	expect(gitScreen).not.toContain("fatal:");
}

test("runs shell, Vim, Git, and Pi through the Actor API PTY", async ({ page }) => {
	await page.addInitScript(() => localStorage.clear());
	await page.goto("/actor.html");
	await page.getByRole("button", { name: "+ New VM" }).click();
	await expect(page.getByText("connected")).toBeVisible({ timeout: 30_000 });

	await page.getByRole("button", { name: "+ shell" }).click();
	await page.waitForFunction(
		() =>
			Object.keys(window.__agentOSTerminalDemo?.screens() ?? {}).length === 1,
	);
	const actorShellId = await page.evaluate(() =>
		Object.keys(window.__agentOSTerminalDemo?.screens() ?? {})[0],
	);
	expect(actorShellId).toBeTruthy();
	const actorShell: ShellHarness = {
			screen: () =>
				page.evaluate(
					(id) => window.__agentOSTerminalDemo?.screens()[id] ?? "",
					actorShellId,
				),
			write: (text) =>
				page.evaluate(
					async ({ id, data }) => {
						await window.__agentOSTerminalDemo?.write(id, data);
					},
					{ id: actorShellId, data: text },
				),
	};
	const initialScreen = await runShellCommand(
		actorShell,
		"/bin/echo AGENTOS_SHELL_E2E_OK",
	);
	expect(
		initialScreen.split("\n").some((line) => line.trim() === "AGENTOS_SHELL_E2E_OK"),
	).toBe(true);
	await proveVimGitAndChildPids(actorShell, "ACTOR");
	await captureProof(page, "actor-api-shell-vim-git");

	await page.getByRole("button", { name: "+ pi" }).click();
	await page.waitForFunction(
		() =>
			Object.values(window.__agentOSTerminalDemo?.screens() ?? {}).some(
				(screen) =>
					screen.includes("pi v0.60.0") && screen.includes("/workspace"),
			),
		undefined,
		{ timeout: 45_000 },
	);
	const piShellId = await page.evaluate(() =>
		Object.entries(window.__agentOSTerminalDemo?.screens() ?? {}).find(
			([, screen]) => screen.includes("pi v0.60.0"),
		)?.[0],
	);
	expect(piShellId).toBeTruthy();
	await page.evaluate(
		async ({ shellId }) => {
			await window.__agentOSTerminalDemo?.write(
				shellId,
				"Reply exactly ACTOR_PI_E2E_OK\r",
			);
		},
		{ shellId: piShellId },
	);
	await page.waitForFunction(
		() =>
			Object.values(window.__agentOSTerminalDemo?.screens() ?? {}).some(
				(screen) =>
					screen.includes("[MOCK actor model") &&
					screen.includes("ACTOR_PI_E2E_OK"),
			),
		undefined,
		{ timeout: 45_000 },
	);

	const screens = await page.evaluate(() =>
		Object.values(window.__agentOSTerminalDemo?.screens() ?? {}),
	);
	expect(screens.some((screen) => screen.includes("pi v0.60.0"))).toBe(true);
	expect(
		screens.some(
			(screen) =>
				screen.includes("[MOCK actor model") &&
				screen.includes("ACTOR_PI_E2E_OK"),
		),
	).toBe(true);
});

test("runs shell, Vim, Git, and Pi on browser-local PTYs", async ({
	page,
}) => {
	await page.goto("/browser.html");
	await expect(page.getByText("IN-BROWSER VM", { exact: true })).toBeVisible();
	await expect(
		page.getByText("No Actor API. Runtime and PTYs execute in this tab."),
	).toBeVisible();

	await page.getByRole("button", { name: "+ shell" }).click();
	const shellFrame = page.frameLocator(
		'iframe[title="Browser-local shell terminal"]',
	);
	await expect(shellFrame.locator("#status")).toHaveText("running", {
		timeout: 60_000,
	});
	const shellId = await page.evaluate(
		() => Object.keys(window.__agentOSBrowserTerminalDemo?.screens() ?? {})[0],
	);
	expect(shellId).toBeTruthy();
	await page.evaluate(
		async ({ id }) => {
			await window.__agentOSBrowserTerminalDemo?.write(
				id,
				"/bin/echo AGENTOS_BROWSER_VM_E2E_OK | /bin/tr A-Z a-z\r",
			);
		},
		{ id: shellId },
	);
	await page.waitForFunction(() =>
		Object.values(window.__agentOSBrowserTerminalDemo?.screens() ?? {}).some(
			(screen) => screen.includes("agentos_browser_vm_e2e_ok"),
		),
	);
	await proveVimGitAndChildPids(
		{
			screen: () =>
				page.evaluate(
					(id) => window.__agentOSBrowserTerminalDemo?.screens()[id] ?? "",
					shellId,
				),
			write: (text) =>
				page.evaluate(
					async ({ id, data }) => {
						await window.__agentOSBrowserTerminalDemo?.write(id, data);
					},
					{ id: shellId, data: text },
				),
		},
		"BROWSER",
	);
	await captureProof(page, "in-browser-shell-vim-git");

	await page.getByRole("button", { name: "+ pi" }).click();
	const piFrame = page.frameLocator(
		'iframe[title="Browser-local pi terminal"]',
	);
	await expect(piFrame.locator("#status")).toHaveText("running", {
		timeout: 60_000,
	});
	await expect
		.poll(
			async () =>
				piFrame.locator("body").evaluate(() => window.__piTui?.screen() ?? ""),
			{ timeout: 30_000 },
		)
		.toContain("pi v0.60.0");
	const piId = await page.evaluate(() =>
		Object.keys(window.__agentOSBrowserTerminalDemo?.screens() ?? {}).find(
			(id) => id.includes("-pi-"),
		),
	);
	expect(piId).toBeTruthy();
	const answer = await page.evaluate(
		async ({ id }) =>
			window.__agentOSBrowserTerminalDemo?.askPi(
				id,
				"Reply exactly BROWSER_PI_E2E_OK",
			),
		{ id: piId },
	);
	expect(answer).toMatchObject({ promptAnswered: true });
	await expect
		.poll(
			async () =>
				piFrame.locator("body").evaluate(() => window.__piTui?.screen() ?? ""),
			{ timeout: 30_000 },
		)
		.toContain("BROWSER_PI_E2E_OK");
});
