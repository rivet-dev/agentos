// Shared "Copy Agent Prompt" text. A ready-to-paste prompt that points a coding
// agent at Agent OS so it can install the package and scaffold a session from
// the docs. Used by the homepage hero and the use-cases CTA — keep one copy so
// the wording never drifts between them.
export const AGENT_PROMPT = `Help me get started with Agent OS (\`@agent-os/core\`).

Agent OS is the agent-facing runtime for running coding agents — Claude Code, Codex, Pi, OpenCode, and Amp — inside fast, isolated VMs. It's a faster, lighter, cheaper alternative to sandboxes, with agent orchestration built in.

Read the docs at https://agentos-sdk.dev/docs, then install it with \`npm install @agent-os/core\` and scaffold a minimal agent session in this project.`;
