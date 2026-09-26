import { vm } from "./client.js";

const conn = vm.connect();
await conn.ready;
const terminal = await conn.terminal.open({ options: { args: [], env: {} } });

conn.on("terminal.output", (data) => {
	if (data.terminal.shellId !== terminal.shellId) return;
	// data.terminal: { generation, shellId }
	// data.data: Uint8Array
	const text = new TextDecoder().decode(data.data);
	process.stdout.write(text);
});
