// docs:start read-write
import { vm } from "./client.js";

// Write a file (string or Uint8Array)
await vm.filesystem.writeFile({
	path: "/home/agentos/hello.txt",
	content: "Hello, world!",
});

// Read a file (returns Uint8Array)
const content = await vm.filesystem.readFile({
	path: "/home/agentos/hello.txt",
});
console.log(new TextDecoder().decode(content));
// docs:end read-write

// docs:start batch
// Batch write (creates parent directories automatically)
const writeResults = await vm.filesystem.writeFiles({
	entries: [
		{ path: "/home/agentos/src/index.ts", content: "console.log('hello');" },
		{
			path: "/home/agentos/src/utils.ts",
			content: "export function add(a: number, b: number) { return a + b; }",
		},
	],
});

// Batch read
const readResults = await vm.filesystem.readFiles({
	paths: ["/home/agentos/src/index.ts", "/home/agentos/src/utils.ts"],
});
for (const result of readResults) {
	console.log(
		result.path,
		new TextDecoder().decode(result.content ?? new Uint8Array()),
	);
}
// docs:end batch

// docs:start directories
// Create a directory
await vm.filesystem.mkdir({ path: "/home/agentos/projects", recursive: false });

// List directory contents
const entries = await vm.filesystem.readdir({ path: "/home/agentos/projects" });

// Recursive listing (entries carry path, type, and sizeBytes)
const tree = await vm.filesystem.readdirRecursive({
	path: "/home/agentos",
	exclude: [],
});
for (const entry of tree) {
	const name = entry.path.split("/").pop() ?? entry.path;
	console.log(entry.type, entry.path, name);
}
// docs:end directories

// docs:start metadata
// Check if a path exists
const fileExists = await vm.filesystem.exists({
	path: "/home/agentos/hello.txt",
});

// Get file metadata
const info = await vm.filesystem.stat({ path: "/home/agentos/hello.txt" });
console.log(info.sizeBytes, info.isDirectory, info.mtimeMs);
// docs:end metadata

// docs:start move-delete
// Move/rename
await vm.filesystem.move({
	from: "/home/agentos/old.txt",
	to: "/home/agentos/new.txt",
});

// Delete a file
await vm.filesystem.remove({ path: "/home/agentos/new.txt", recursive: false });

// Delete a directory recursively
await vm.filesystem.remove({ path: "/home/agentos/temp", recursive: true });
// docs:end move-delete

// Keep batch + directory results referenced for the type-check.
void writeResults;
void entries;
void fileExists;
