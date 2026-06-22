import { createRivetKit } from "@rivet-dev/agentos/react";
import { useState } from "react";
import type { registry } from "./server";

const { useActor } = createRivetKit<typeof registry>("http://localhost:6420");

export function Agent() {
  const [log, setLog] = useState("");
  const agent = useActor({ name: "vm", key: "my-agent" });

  // Stream agent events into component state
  agent.useEvent("sessionEvent", (data) => {
    setLog((prev) => prev + JSON.stringify(data.event) + "\n");
  });

  async function run() {
    // In production, inject credentials on the server (see /docs/llm-credentials)
    const session = await agent.connection?.createSession("pi", {
      env: { ANTHROPIC_API_KEY: process.env.VITE_ANTHROPIC_API_KEY! },
    });
    if (!session) return;
    await agent.connection?.sendPrompt(
      session.sessionId,
      "Write a hello world script to /home/user/hello.js",
    );
  }

  return (
    <div>
      <button onClick={run}>Run agent</button>
      <pre>{log}</pre>
    </div>
  );
}
