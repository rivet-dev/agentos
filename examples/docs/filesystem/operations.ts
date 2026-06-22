import { createClient } from "@rivet-dev/agentos/client";
import type { registry } from "./server";

const client = createClient<typeof registry>({ endpoint: "http://localhost:6420" });
const agent = client.vm.getOrCreate("my-agent");

// --- Read and write ---

// Write a file (string or Uint8Array)
await agent.writeFile("/home/user/hello.txt", "Hello, world!");

// Read a file (returns Uint8Array)
const content = await agent.readFile("/home/user/hello.txt");
console.log(new TextDecoder().decode(content));

// --- Batch read and write ---

// Batch write (creates parent directories automatically)
const writeResults = await agent.writeFiles([
  { path: "/home/user/src/index.ts", content: "console.log('hello');" },
  { path: "/home/user/src/utils.ts", content: "export function add(a: number, b: number) { return a + b; }" },
]);

// Batch read
const readResults = await agent.readFiles([
  "/home/user/src/index.ts",
  "/home/user/src/utils.ts",
]);
for (const result of readResults) {
  console.log(result.path, new TextDecoder().decode(result.content ?? new Uint8Array()));
}

// --- Directories ---

// Create a directory
await agent.mkdir("/home/user/projects");

// List directory contents
const entries = await agent.readdir("/home/user/projects");

// Recursive listing with metadata
const tree = await agent.readdirRecursive("/home/user", {
  maxDepth: 3,
  exclude: ["node_modules"],
});
for (const entry of tree) {
  console.log(entry.type, entry.path, entry.size);
}

// --- File metadata ---

// Check if a path exists
const fileExists = await agent.exists("/home/user/hello.txt");

// Get file metadata
const info = await agent.stat("/home/user/hello.txt");
console.log(info.size, info.isDirectory, info.mtimeMs);

// --- Move and delete ---

// Move/rename
await agent.move("/home/user/old.txt", "/home/user/new.txt");

// Delete a file
await agent.delete("/home/user/new.txt");

// Delete a directory recursively
await agent.delete("/home/user/temp", { recursive: true });

// Keep batch + directory results referenced for the type-check.
void writeResults;
void entries;
void fileExists;
