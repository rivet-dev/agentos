// docs:start boot
import { createClient } from "@rivet-dev/agentos/client";
import type { registry } from "./server";

const client = createClient<typeof registry>({ endpoint: "http://localhost:6420" });
const handle = client.vm.getOrCreate("my-agent");

const result = await handle.exec("echo hello");
console.log(result.stdout); // "hello\n"
// docs:end boot

// ── Filesystem ────────────────────────────────────────────────────
async function filesystem() {
  // docs:start filesystem
  await handle.writeFile("/home/agentos/hello.txt", "Hello, world!");
  const content = await handle.readFile("/home/agentos/hello.txt");
  console.log(new TextDecoder().decode(content));

  await handle.mkdir("/home/agentos/src");
  await handle.writeFiles([
    { path: "/home/agentos/src/index.ts", content: "console.log('hi');" },
    { path: "/home/agentos/src/utils.ts", content: "export const add = (a: number, b: number) => a + b;" },
  ]);

  const entries = await handle.readdirRecursive("/home/agentos");
  for (const entry of entries) {
    console.log(entry.type, entry.path);
  }
  // docs:end filesystem
}

// ── Processes ─────────────────────────────────────────────────────
async function processes() {
  // docs:start processes
  // One-shot execution
  const result = await handle.exec("ls -la /home/agentos");
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
  // docs:end processes
}

// ── Agent sessions ────────────────────────────────────────────────
async function agentSessions() {
  // docs:start sessions
  const conn = handle.connect();

  // Stream session events (each event is a JSON-RPC notification)
  conn.on("sessionEvent", (data) => {
    console.log(data.sessionId, data.event.method, data.event.params);
  });

  // Observe permission requests from the agent
  conn.on("permissionRequest", (data) => {
    console.log("Permission:", data.sessionId, data.request.description);
  });

  // createSession() resolves to the session ID string.
  const sessionId = await handle.createSession("pi", {
    env: { ANTHROPIC_API_KEY: process.env.ANTHROPIC_API_KEY! },
  });

  // Send a prompt. sendPrompt() resolves to { response, text }, where `text` is
  // the accumulated agent message text and `response` is the raw JSON-RPC response.
  const { text } = await handle.sendPrompt(sessionId, "Write a hello world script");
  console.log(text);

  await handle.closeSession(sessionId);
  // docs:end sessions
}

// ── Networking ────────────────────────────────────────────────────
async function networking() {
  // docs:start networking
  // Start a server inside the VM
  await handle.writeFile(
    "/tmp/app.mjs",
    'import http from "http"; http.createServer((req, res) => res.end("hello")).listen(3000);',
  );
  await handle.spawn("node", ["/tmp/app.mjs"]);

  // Fetch from it
  const response = await handle.vmFetch(3000, "/");
  console.log(new TextDecoder().decode(response.body));
  // docs:end networking
}

// ── Cron jobs ─────────────────────────────────────────────────────
async function cronJobs() {
  // docs:start cron
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
      cwd: "/workspace",
    },
  });

  const conn = handle.connect();
  conn.on("cronEvent", (data) => {
    console.log("Cron event:", data.event.type, data.event.jobId);
  });

  console.log(await handle.listCronJobs());
  // docs:end cron
}

export {
  filesystem,
  processes,
  agentSessions,
  networking,
  cronJobs,
};
