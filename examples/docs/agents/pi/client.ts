import { createClient } from "@rivet-dev/agentos/client";
import type { registry } from "./server";

const client = createClient<typeof registry>({ endpoint: "http://localhost:6420" });
const agent = client.vm.getOrCreate("my-agent");

// ── Quick start ───────────────────────────────────────────────────
async function quickStart() {
  const session = await agent.createSession("pi", {
    env: { ANTHROPIC_API_KEY: process.env.ANTHROPIC_API_KEY! },
  });

  const { text } = await agent.sendPrompt(
    session.sessionId,
    "What files are in the current directory?",
  );
  console.log(text);
}

// ── Skills ────────────────────────────────────────────────────────
//
// Write a SKILL.md into the agent's skills directory before creating the
// session. Skills are only loaded when at least one Pi extension is also
// present, so write an extension alongside the skill.
async function withSkill() {
  const skill = `---
name: commit-style
description: How to write commit messages in this project.
---

Write commit messages in the imperative mood and keep the subject under 50 characters.
`;

  await agent.mkdir("/home/agentos/.pi/agent/skills/commit-style", { recursive: true });
  await agent.writeFile("/home/agentos/.pi/agent/skills/commit-style/SKILL.md", skill);

  // Skills are only loaded when a Pi extension is also present
  await agent.mkdir("/home/agentos/.pi/agent/extensions", { recursive: true });
  await agent.writeFile("/home/agentos/.pi/agent/extensions/enable-skills.js", "export default function(pi) {}\n");

  const session = await agent.createSession("pi", {
    env: { ANTHROPIC_API_KEY: process.env.ANTHROPIC_API_KEY! },
  });
  console.log(session.sessionId);
}

// ── MCP servers ───────────────────────────────────────────────────
//
// Expose extra tools to the agent with `mcpServers` — local child-process
// servers and remote URLs are both supported.
async function withMcp() {
  // Pre-install the MCP server so `npx` is silent — first-run install output
  // would otherwise corrupt the MCP stdio handshake ("Connection closed").
  await agent.exec("npm install -g @modelcontextprotocol/server-filesystem");

  const session = await agent.createSession("pi", {
    env: { ANTHROPIC_API_KEY: process.env.ANTHROPIC_API_KEY! },
    mcpServers: [
      {
        type: "local",
        command: "npx",
        args: ["-y", "@modelcontextprotocol/server-filesystem", "/home/agentos"],
        env: {},
      },
      {
        type: "remote",
        url: "https://mcp.example.com/sse",
        headers: { Authorization: "Bearer my-token" },
      },
    ],
  });
  console.log(session.sessionId);
}

// ── Skills + MCP together ─────────────────────────────────────────
async function withSkillAndMcp() {
  const skill = `---
name: commit-style
description: How to write commit messages in this project.
---

Write commit messages in the imperative mood and keep the subject under 50 characters.
`;

  await agent.mkdir("/home/agentos/.pi/agent/skills/commit-style", { recursive: true });
  await agent.writeFile("/home/agentos/.pi/agent/skills/commit-style/SKILL.md", skill);

  // Skills are only loaded when a Pi extension is also present
  await agent.mkdir("/home/agentos/.pi/agent/extensions", { recursive: true });
  await agent.writeFile("/home/agentos/.pi/agent/extensions/enable-skills.js", "export default function(pi) {}\n");

  // Pre-install the MCP server so `npx` is silent — first-run install output
  // would otherwise corrupt the MCP stdio handshake ("Connection closed").
  await agent.exec("npm install -g @modelcontextprotocol/server-filesystem");

  const session = await agent.createSession("pi", {
    env: { ANTHROPIC_API_KEY: process.env.ANTHROPIC_API_KEY! },
    mcpServers: [
      {
        type: "local",
        command: "npx",
        args: ["-y", "@modelcontextprotocol/server-filesystem", "/home/agentos"],
        env: {},
      },
    ],
  });

  const { text } = await agent.sendPrompt(
    session.sessionId,
    "Stage everything and write a commit message following the project skill.",
  );
  console.log(text);
}

export { quickStart, withSkill, withMcp, withSkillAndMcp };
