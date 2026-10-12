import type { AgentOsActorHandle } from "@rivet-dev/agentos";

// The caller owns the VM handle and its client.
export async function readAndWriteFiles(vm: AgentOsActorHandle) {
	await vm.filesystem.writeFile({
		path: "/workspace/hello.txt",
		content: "Hello!",
	});
	const content = await vm.filesystem.readFile({
		path: "/workspace/hello.txt",
		maxBytes: 1024,
	});
	const written = await vm.filesystem.writeFiles({
		entries: [
			{ path: "/workspace/one.txt", content: "one" },
			{ path: "/workspace/two.txt", content: "two" },
		],
	});
	for (const result of written) {
		if (!result.success) throw new Error(`${result.path}: ${result.error}`);
	}
	const files = await vm.filesystem.readFiles({
		paths: ["/workspace/one.txt", "/workspace/two.txt"],
		maxBytes: 1024,
	});
	for (const file of files) {
		if (file.error) throw new Error(`${file.path}: ${file.error}`);
	}
	return { text: new TextDecoder().decode(content), files };
}
