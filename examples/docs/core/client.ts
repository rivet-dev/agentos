import { createClient } from "@rivet-dev/agentos/client";
import type { registry } from "./server";

const client = createClient<typeof registry>({ endpoint: "http://localhost:6420" });
const handle = client.vm.getOrCreate("my-agent");

// ── Run a command ─────────────────────────────────────────────────
async function bootAndExec() {
  const result = await handle.exec("echo hello");
  console.log(result.stdout); // "hello\n"
}

// ── Filesystem ────────────────────────────────────────────────────
async function filesystem() {
  await handle.writeFile("/home/user/hello.txt", "Hello, world!");
  const content = await handle.readFile("/home/user/hello.txt");
  console.log(new TextDecoder().decode(content));

  await handle.mkdir("/home/user/src");
  await handle.writeFiles([
    { path: "/home/user/src/index.ts", content: "console.log('hi');" },
    { path: "/home/user/src/utils.ts", content: "export const add = (a: number, b: number) => a + b;" },
  ]);

  const entries = await handle.readdirRecursive("/home/user");
  for (const entry of entries) {
    console.log(entry.type, entry.path);
  }
}

// ── Processes ─────────────────────────────────────────────────────
async function processes() {
  // One-shot execution
  const result = await handle.exec("ls -la /home/user");
  console.log(result.stdout);

  // Long-running process with streaming output
  await handle.writeFile(
    "/tmp/server.mjs",
    'import http from "http"; http.createServer((req, res) => res.end("ok")).listen(3000); console.log("listening");',
  );
  const { pid } = await handle.spawn("node", ["/tmp/server.mjs"]);

  const conn = handle.connect();
  conn.on("processOutput", (data) => {
    if (data.pid === pid && data.stream === "stdout") {
      console.log("stdout:", new TextDecoder().decode(data.data));
    }
  });
  conn.on("processExit", (data) => {
    if (data.pid === pid) console.log("exited:", data.exitCode);
  });

  // Write to stdin
  await handle.writeProcessStdin(pid, "some input\n");

  // Stop or kill
  await handle.stopProcess(pid);
}

// ── Agent sessions ────────────────────────────────────────────────
async function agentSessions() {
  const conn = handle.connect();

  // Stream events (each event is a JSON-RPC notification)
  conn.on("sessionEvent", (data) => {
    console.log(data.event.method, data.event.params);
  });

  // Handle permissions
  conn.on("permissionRequest", (data) => {
    console.log("Permission:", data.request.description);
    // Reply with "once", "always", or "reject"
    void handle.respondPermission(data.sessionId, data.request.permissionId, "once");
  });

  const session = await handle.createSession("pi", {
    env: { ANTHROPIC_API_KEY: process.env.ANTHROPIC_API_KEY! },
  });

  // Send a prompt. sendPrompt() resolves to { response, text }, where `text` is
  // the accumulated agent message text and `response` is the raw JSON-RPC response.
  const { text } = await handle.sendPrompt(session.sessionId, "Write a hello world script");
  console.log(text);

  // Configure the session
  await handle.setModel(session.sessionId, "claude-sonnet-4-6");
  await handle.setMode(session.sessionId, "plan");

  await handle.closeSession(session.sessionId);
}

// ── Interactive shell ─────────────────────────────────────────────
async function interactiveShell() {
  const { shellId } = await handle.openShell();

  const conn = handle.connect();
  conn.on("shellData", (data) => {
    if (data.shellId === shellId) {
      process.stdout.write(new TextDecoder().decode(data.data));
    }
  });

  await handle.writeShell(shellId, "echo hello from shell\n");

  // Resize terminal
  await handle.resizeShell(shellId, 120, 40);

  await handle.closeShell(shellId);
}

// ── Networking ────────────────────────────────────────────────────
async function networking() {
  // Start a server inside the VM
  await handle.writeFile(
    "/tmp/app.mjs",
    'import http from "http"; http.createServer((req, res) => res.end("hello")).listen(3000);',
  );
  await handle.spawn("node", ["/tmp/app.mjs"]);

  // Fetch from it
  const response = await handle.vmFetch(3000, "/");
  console.log(new TextDecoder().decode(response.body));
}

// ── Cron jobs ─────────────────────────────────────────────────────
async function cronJobs() {
  const { id } = await handle.scheduleCron({
    id: "cleanup",
    schedule: "0 * * * *",
    action: { type: "exec", command: "rm", args: ["-rf", "/tmp/cache"] },
  });
  console.log("Scheduled:", id);

  // Run an agent session on a schedule
  await handle.scheduleCron({
    schedule: "0 9 * * *",
    action: {
      type: "session",
      agentType: "pi",
      prompt: "Review the logs and summarize any errors",
      options: { cwd: "/home/user" },
    },
  });

  const conn = handle.connect();
  conn.on("cronEvent", (data) => {
    console.log("Cron event:", data.event.id, data.event.schedule);
  });

  console.log(await handle.listCronJobs());
}

export {
  bootAndExec,
  filesystem,
  processes,
  agentSessions,
  interactiveShell,
  networking,
  cronJobs,
};
