import { vm } from "./client.js";

// Write into the VFS (creates parent dirs). Accepts string | Uint8Array.
await vm.filesystem.writeFile({ path: "/home/agentos/out.txt", content: "hi" });

// Read back to the host as raw bytes.
const bytes = await vm.filesystem.readFile({ path: "/home/agentos/out.txt" });
console.log(new TextDecoder().decode(bytes));
