import { createRivetKit } from "@rivet-dev/agentos/react";
import { useState } from "react";
import type { registry } from "./server";

const { useActor } = createRivetKit<typeof registry>("http://localhost:6420");

export function Agent() {
  const [log, setLog] = useState("");
  const agent = useActor({ name: "vm", key: "my-agent" });

  // Stream agent events into component state
  agent.useEvent("sessionEvent", (event) => {
	setLog((prev) => prev + JSON.stringify(event) + "\n");
  });

  async function run() {
    // In production, inject credentials on the server (see /docs/llm-credentials)
    const connection = agent.connection;
    if (!connection) return;

    await connection.openSession({
      agent: "pi",
      env: { ANTHROPIC_API_KEY: process.env.VITE_ANTHROPIC_API_KEY! },
    });
    await connection.prompt({
      content: [
        {
          type: "text",
          text: "Write a hello world script to /workspace/hello.js",
        },
      ],
    });
  }

  return (
    <div>
      <button onClick={run}>Run agent</button>
      <pre>{log}</pre>
    </div>
  );
}
