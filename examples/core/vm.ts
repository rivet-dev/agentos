// docs:start boot
import { AgentOs } from "@rivet-dev/agentos-core";
import pi from "@agentos-software/pi";

// Create a VM directly with the core package — no actor runtime, no
// client/server split. `AgentOs.create()` boots the VM in-process.
const vm = await AgentOs.create({ software: [pi] });

const result = await vm.exec("echo hello");
console.log(result.stdout); // "hello\n"
// docs:end boot

// ── Filesystem ────────────────────────────────────────────────────
async function filesystem() {
  // docs:start filesystem
  await vm.writeFile("/home/agentos/hello.txt", "Hello, world!");
  const content = await vm.readFile("/home/agentos/hello.txt");
  console.log(new TextDecoder().decode(content));

  await vm.mkdir("/home/agentos/src");
  await vm.writeFiles([
    { path: "/home/agentos/src/index.ts", content: "console.log('hi');" },
    { path: "/home/agentos/src/utils.ts", content: "export const add = (a: number, b: number) => a + b;" },
  ]);

  const entries = await vm.readdirRecursive("/home/agentos");
  for (const entry of entries) {
    console.log(entry.type, entry.path);
  }
  // docs:end filesystem
}

// ── Processes ─────────────────────────────────────────────────────
async function processes() {
  // docs:start processes
  // One-shot execution
  const result = await vm.exec("ls -la /home/agentos");
  console.log(result.stdout);

  // Long-running process with streaming output. spawn() returns synchronously;
  // stdout/stderr are delivered through the callbacks you pass in.
  await vm.writeFile(
    "/tmp/server.mjs",
    'import http from "http"; http.createServer((req, res) => res.end("ok")).listen(3000); console.log("listening");',
  );
  const { pid } = vm.spawn("node", ["/tmp/server.mjs"], {
    onStdout: (data) => console.log("stdout:", new TextDecoder().decode(data)),
  });

  vm.onProcessExit(pid, (exitCode) => console.log("exited:", exitCode));

  // Write to stdin
  await vm.writeProcessStdin(pid, "some input\n");

  // Stop or kill
  vm.stopProcess(pid);
  // docs:end processes
}

// ── Agent sessions ────────────────────────────────────────────────
async function agentSessions() {
  // docs:start sessions
  // createSession() resolves to a session record. All session operations take
  // its `sessionId`.
  const { sessionId } = await vm.createSession("pi", {
    env: { ANTHROPIC_API_KEY: process.env.ANTHROPIC_API_KEY! },
  });

  // Stream session events (each event is a JSON-RPC notification).
  vm.onSessionEvent(sessionId, (event) => {
    console.log(event.method, event.params);
  });

  // Observe permission requests from the agent.
  vm.onPermissionRequest(sessionId, (request) => {
    console.log("Permission:", request.description ?? request.permissionId);
  });

  // Send a prompt. prompt() resolves to { response, text }, where `text` is the
  // accumulated agent message text and `response` is the raw JSON-RPC response.
  const { text } = await vm.prompt(sessionId, "Write a hello world script");
  console.log(text);

  vm.closeSession(sessionId);
  // docs:end sessions
}

// ── Networking ────────────────────────────────────────────────────
async function networking() {
  // docs:start networking
  // Start a server inside the VM
  await vm.writeFile(
    "/tmp/app.mjs",
    'import http from "http"; http.createServer((req, res) => res.end("hello")).listen(3000);',
  );
  vm.spawn("node", ["/tmp/app.mjs"]);

  // Fetch from it — fetch(port, Request) reaches services running in the VM.
  const response = await vm.fetch(3000, new Request("http://vm/"));
  console.log(await response.text());
  // docs:end networking
}

// ── Cron jobs ─────────────────────────────────────────────────────
async function cronJobs() {
  // docs:start cron
  const job = vm.scheduleCron({
    id: "cleanup",
    schedule: "0 * * * *",
    action: { type: "exec", command: "rm", args: ["-rf", "/tmp/cache"] },
  });
  console.log("Scheduled:", job.id);

  // Run an agent session on a schedule
  vm.scheduleCron({
    schedule: "0 9 * * *",
    action: {
      type: "session",
      agentType: "pi",
      prompt: "Review the logs and summarize any errors",
      options: { cwd: "/workspace" },
    },
  });

  vm.onCronEvent((event) => {
    console.log("Cron event:", event.type, event.jobId);
  });

  console.log(vm.listCronJobs());
  // docs:end cron
}

export { filesystem, processes, agentSessions, networking, cronJobs };
