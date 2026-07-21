import { JavaScriptRuntime } from "@rivet-dev/agentos-javascript";

// Boot a fully virtualized VM. The guest runs inside the kernel isolation
// boundary, and any child processes it spawns are themselves kernel-managed
// processes - they never spawn real host processes.
const rt = await JavaScriptRuntime.create();

try {
  // Guest code runs as an ES module inside the VM. It uses the standard
  // node:child_process module to spawn a command available in the VM and read
  // its output. Here we spawn `sh -c` to echo a message, and `node` to report
  // its own version - both run as kernel-managed child processes.
  const { stdout, stderr, exitCode } = await rt.execute(`
    import { execFileSync } from "node:child_process";

    // Spawn a shell command and capture its stdout.
    const shellOut = execFileSync("sh", ["-c", "echo hello from a child process"], {
      encoding: "utf8",
    });
    console.log("sh output:", shellOut.trim());

    // Spawn node as a child process and read its version.
    const nodeVersion = execFileSync("node", ["--version"], {
      encoding: "utf8",
    });
    console.log("child node version:", nodeVersion.trim());
  `);

  console.log("exitCode:", exitCode);
  if (stderr.trim()) console.log("guest stderr:", stderr.trim());
  console.log("guest stdout:");
  console.log(stdout.trim());
} finally {
  await rt.dispose();
}
