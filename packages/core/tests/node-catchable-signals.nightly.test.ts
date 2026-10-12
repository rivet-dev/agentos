import { spawnSync } from "node:child_process";
import { constants } from "node:os";
import { expect, test } from "vitest";
import { AgentOs } from "../src/index.js";

// SIGUSR1 is already covered elsewhere; sending it to native Node may enable
// its inspector. Keep this control confined to ordinary local signal delivery.
const signals = Object.entries(constants.signals)
	.filter(
		([name, number]) =>
			number <= 31 &&
			!["SIGKILL", "SIGSTOP", "SIGUSR1", "SIGIOT", "SIGPOLL"].includes(name),
	)
	.sort((left, right) => left[1] - right[1])
	.map(([name]) => name);

function program() {
	const child = `
const signals = ${JSON.stringify(signals)};
let received = 0;
const hold = setInterval(() => {}, 1000);
for (const name of signals) process.on(name, () => {
  console.log('caught:' + name);
  received++;
  if (received === signals.length) { clearInterval(hold); console.log('done'); }
});
console.log('ready');
`;
	return `
const { spawn } = require('node:child_process');
const signals = ${JSON.stringify(signals)};
const child = spawn(process.execPath, ['-e', ${JSON.stringify(child)}]);
let pending = '', next = 0;
const deadline = setTimeout(() => { child.kill('SIGKILL'); throw new Error('signal matrix deadline'); }, 5000);
child.stdout.on('data', (bytes) => {
  process.stdout.write(bytes);
  pending += bytes.toString();
  let newline;
  while ((newline = pending.indexOf('\\n')) !== -1) {
    const line = pending.slice(0, newline);
    pending = pending.slice(newline + 1);
    if ((line === 'ready' || line.startsWith('caught:')) && next < signals.length) {
      process.kill(child.pid, signals[next++]);
    }
  }
});
child.stderr.on('data', (bytes) => process.stderr.write(bytes));
child.on('error', (error) => { throw error; });
child.on('close', (code) => { clearTimeout(deadline); process.exitCode = code ?? 1; });
`;
}

test("guest child signal delivery catches the same supported Linux signals as native Node", async () => {
	const native = spawnSync(process.execPath, ["-e", program()], {
		encoding: "utf8",
		timeout: 7_000,
		env: { PATH: process.env.PATH },
	});
	expect(native.error).toBeUndefined();
	expect(native.status, native.stderr).toBe(0);
	const vm = await AgentOs.create({ permissions: { network: "deny" } });
	try {
		const result = await vm.process.execFile("node", ["-e", program()], {
			timeoutMs: 8_000,
			output: { capture: "all" },
		});
		expect(result.exitCode, result.stderr + "\n" + result.stdout).toBe(0);
		expect(result.stdout).toBe(native.stdout);
	} finally {
		await vm.dispose();
	}
}, 20_000);
