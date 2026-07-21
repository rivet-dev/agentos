// Run shell commands inside the VM.

import { AgentOs } from "@rivet-dev/agentos";

const vm = await AgentOs.create();

// Simple commands
const echo = await vm.process.exec("echo 'Hello from the shell!'");
console.log(echo.stdout);

// Pipes
const piped = await vm.process.exec("echo 'hello world' | tr a-z A-Z");
console.log("Piped:", piped.stdout.trim());

// File manipulation
await vm.process.exec("echo 'line 1' > /tmp/test.txt");
await vm.process.exec("echo 'line 2' >> /tmp/test.txt");
await vm.process.exec("echo 'line 3' >> /tmp/test.txt");

const cat = await vm.process.exec("cat /tmp/test.txt");
console.log("File contents:");
console.log(cat.stdout);

// grep
const grep = await vm.process.exec("grep '2' /tmp/test.txt");
console.log("Grep result:", grep.stdout.trim());

console.log("Exit code:", grep.exitCode);

await vm.dispose();
