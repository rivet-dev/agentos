import { createClient } from "@rivet-dev/agentos/client";
import type { registry } from "./server";

const client = createClient<typeof registry>({ endpoint: "http://localhost:6420" });
const agent = client.vm.getOrCreate("my-agent");

// ── Quick start ───────────────────────────────────────────────────
async function quickStart() {
  // docs:start quickstart
  const sessionId = await agent.createSession("claude", {
    env: { ANTHROPIC_API_KEY: process.env.ANTHROPIC_API_KEY! },
  });

  const { text } = await agent.sendPrompt(
    sessionId,
    "What files are in the current directory?",
  );
  console.log(text);
  // docs:end quickstart
}

// ── Skills ────────────────────────────────────────────────────────
//
// Write a SKILL.md into the agent's skills directory before creating the
// session and the agent discovers it automatically.
async function withSkill() {
  // docs:start skill
  const skill = `---
name: commit-style
description: How to write commit messages in this project.
---

Write commit messages in the imperative mood and keep the subject under 50 characters.
`;

  // Write the skill before creating the session
  await agent.mkdir("/home/agentos/.claude/skills/commit-style");
  await agent.writeFile("/home/agentos/.claude/skills/commit-style/SKILL.md", skill);

  // Claude Code discovers the skill automatically
  const sessionId = await agent.createSession("claude", {
    env: { ANTHROPIC_API_KEY: process.env.ANTHROPIC_API_KEY! },
  });
  // docs:end skill
  console.log(sessionId);
}

// ── MCP servers ───────────────────────────────────────────────────
//
// Claude Code reads its MCP server list from `~/.claude.json`. Write that file
// into the VM before creating the session — local child-process servers and
// remote URLs are both supported.
async function withMcp() {
  // Pre-install the MCP server so `npx` is silent — first-run install output
  // would otherwise corrupt the MCP stdio handshake ("Connection closed").
  await agent.exec("npm install -g @modelcontextprotocol/server-filesystem");

  // docs:start mcp
  const mcpConfig = {
    mcpServers: {
      filesystem: {
        command: "npx",
        args: ["-y", "@modelcontextprotocol/server-filesystem", "/home/agentos"],
      },
      remote: {
        type: "sse",
        url: "https://mcp.example.com/sse",
        headers: { Authorization: "Bearer my-token" },
      },
    },
  };
  await agent.writeFile("/home/agentos/.claude.json", JSON.stringify(mcpConfig));

  const sessionId = await agent.createSession("claude", {
    env: { ANTHROPIC_API_KEY: process.env.ANTHROPIC_API_KEY! },
  });
  // docs:end mcp
  console.log(sessionId);
}

// ── Skills + MCP together ─────────────────────────────────────────
async function withSkillAndMcp() {
  const skill = `---
name: commit-style
description: How to write commit messages in this project.
---

Write commit messages in the imperative mood and keep the subject under 50 characters.
`;

  await agent.mkdir("/home/agentos/.claude/skills/commit-style");
  await agent.writeFile("/home/agentos/.claude/skills/commit-style/SKILL.md", skill);

  // Pre-install the MCP server so `npx` is silent — first-run install output
  // would otherwise corrupt the MCP stdio handshake ("Connection closed").
  await agent.exec("npm install -g @modelcontextprotocol/server-filesystem");

  // Claude Code reads its MCP servers from ~/.claude.json (see `withMcp`).
  const mcpConfig = {
    mcpServers: {
      filesystem: {
        command: "npx",
        args: ["-y", "@modelcontextprotocol/server-filesystem", "/home/agentos"],
      },
    },
  };
  await agent.writeFile("/home/agentos/.claude.json", JSON.stringify(mcpConfig));

  const sessionId = await agent.createSession("claude", {
    env: { ANTHROPIC_API_KEY: process.env.ANTHROPIC_API_KEY! },
  });

  const { text } = await agent.sendPrompt(
    sessionId,
    "Stage everything and write a commit message following the project skill.",
  );
  console.log(text);
}

export { quickStart, withSkill, withMcp, withSkillAndMcp };
