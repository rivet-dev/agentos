import type { AgentOs } from "../src/agent-os.js";
import type {
	CodeExecutionResult,
	DetachedExecution,
} from "../src/language-execution.js";

declare const vm: AgentOs;

async function checkNestedApi(): Promise<void> {
	const attached: CodeExecutionResult = await vm.process.exec("true");
	const detached: DetachedExecution = await vm.process.exec("true", {
		detached: true,
	});
	const spawned: { pid: number } = vm.process.spawn("true", []);
	void attached;
	void detached;
	void spawned;

	await vm.process.execFile("true", []);
	vm.process.get(1);
	vm.process.list();
	vm.process.listAll();
	vm.process.tree();
	await vm.process.wait(1);
	vm.process.stop(1);
	vm.process.kill(1);
	await vm.process.writeStdin(1, "input");
	await vm.process.closeStdin(1);

	await vm.javascript.execute("");
	await vm.javascript.evaluate("1");
	await vm.javascript.executeFile("/workspace/main.js");
	await vm.javascript.typescript.execute("");
	await vm.javascript.typescript.evaluate("1");
	await vm.javascript.typescript.executeFile("/workspace/main.ts");
	await vm.javascript.typescript.check("");
	await vm.javascript.typescript.checkProject();
	await vm.javascript.npm.install();
	await vm.javascript.npm.runScript("test");
	await vm.javascript.npm.runPackage("typescript");

	await vm.python.execute("");
	await vm.python.evaluate("1");
	await vm.python.executeFile("/workspace/main.py");
	await vm.python.executeModule("main");
	await vm.python.install();

	await vm.executions.get("execution");
	await vm.executions.list();
	await vm.executions.wait("execution");
	await vm.executions.cancel("execution");
	await vm.executions.signal("execution", "SIGTERM");
	await vm.executions.reset("execution");
	await vm.executions.delete("execution");
	await vm.executions.writeStdin("execution", "input");
	await vm.executions.closeStdin("execution");
	await vm.executions.resizePty("execution", { cols: 80, rows: 24 });
	await vm.executions.readOutput("execution");

	const terminal = vm.terminal.open();
	await vm.terminal.write(terminal.shellId, "input");
	vm.terminal.resize(terminal.shellId, 80, 24);
	await vm.terminal.wait(terminal.shellId);
	vm.terminal.close(terminal.shellId);

	await vm.filesystem.readFile("/workspace/file");
	await vm.filesystem.writeFile("/workspace/file", "content");
	await vm.filesystem.readFiles(["/workspace/file"]);
	await vm.filesystem.writeFiles([
		{ path: "/workspace/file", content: "content" },
	]);
	await vm.filesystem.stat("/workspace/file");
	await vm.filesystem.mkdir("/workspace/dir");
	await vm.filesystem.readdir("/workspace");
	await vm.filesystem.readdirEntries("/workspace");
	await vm.filesystem.readdirRecursive("/workspace");
	await vm.filesystem.exists("/workspace/file");
	await vm.filesystem.move("/workspace/from", "/workspace/to");
	await vm.filesystem.remove("/workspace/file");
	await vm.filesystem.export({ maxBytes: 1024 });
	await vm.filesystem.unmount("/workspace/mount");
	await vm.filesystem.listMounts();

	await vm.software.list();
	await vm.agents.list();
	await vm.sessions.get();
	await vm.sessions.list();
	await vm.sessions.delete();
	await vm.sessions.unload();
	await vm.sessions.cancelPrompt();
	await vm.sessions.readHistory();
	await vm.sessions.getConfig();
	await vm.sessions.getCapabilities();
	await vm.sessions.getAgentInfo();
	vm.cron.list();

	// New execution APIs were renamed, not retained as flat aliases.
	// @ts-expect-error Use javascript.execute().
	vm.executeJavaScript("");
	// @ts-expect-error Use python.execute().
	vm.executePython("");
	// @ts-expect-error Use executions.list().
	vm.listExecutions();
}

void checkNestedApi;
