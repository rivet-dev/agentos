// docs:start read-write
import { createClient } from "@rivet-dev/agentos/client";
import type { registry } from "./server";

const client = createClient<typeof registry>({ endpoint: "http://localhost:6420" });
const agent = client.vm.getOrCreate("my-agent");

// Write a file (string or Uint8Array)
await agent.writeFile("/home/agentos/hello.txt", "Hello, world!");

// Read a file (returns Uint8Array)
const content = await agent.readFile("/home/agentos/hello.txt");
console.log(new TextDecoder().decode(content));
// docs:end read-write

// docs:start batch
// Batch write (creates parent directories automatically)
const writeResults = await agent.writeFiles([
  { path: "/home/agentos/src/index.ts", content: "console.log('hello');" },
  { path: "/home/agentos/src/utils.ts", content: "export function add(a: number, b: number) { return a + b; }" },
]);

// Batch read
const readResults = await agent.readFiles([
  "/home/agentos/src/index.ts",
  "/home/agentos/src/utils.ts",
]);
for (const result of readResults) {
  console.log(result.path, new TextDecoder().decode(result.content ?? new Uint8Array()));
}
// docs:end batch

// docs:start directories
// Create a directory
await agent.mkdir("/home/agentos/projects");

// List directory contents
const entries = await agent.readdir("/home/agentos/projects");

// Recursive listing (entries carry path, type, and size)
const tree = await agent.readdirRecursive("/home/agentos");
for (const entry of tree) {
  const name = entry.path.split("/").pop() ?? entry.path;
  console.log(entry.type, entry.path, name);
}
// docs:end directories

// docs:start metadata
// Check if a path exists
const fileExists = await agent.exists("/home/agentos/hello.txt");

// Get file metadata
const info = await agent.stat("/home/agentos/hello.txt");
console.log(info.size, info.isDirectory, info.mtimeMs);
// docs:end metadata

// docs:start move-delete
// Move/rename
await agent.move("/home/agentos/old.txt", "/home/agentos/new.txt");

// Delete a file
await agent.deleteFile("/home/agentos/new.txt");

// Delete a directory recursively
await agent.deleteFile("/home/agentos/temp", { recursive: true });
// docs:end move-delete

// Keep batch + directory results referenced for the type-check.
void writeResults;
void entries;
void fileExists;
