// Shared "Copy Agent Prompt" text. A ready-to-paste prompt that points a coding
// agent at agentOS so it can scaffold a server + client and verify them end to
// end. Used by the homepage hero, the use-cases CTA, AND the quickstart docs —
// keep this as the single source of truth so the wording never drifts between
// surfaces. (The quickstart MDX imports AGENT_PROMPT and renders it.)
export const AGENT_PROMPT = `Help me get started with agentOS (\`@rivet-dev/agentos\`).

agentOS is the agent-facing runtime for running coding agents — Claude Code, Codex, Pi, and OpenCode — inside fast, isolated VMs. It's a faster, lighter, cheaper alternative to sandboxes, with agent orchestration built in.

Please do the following in this project:

1. Scaffold a minimal agentOS server and client. Install \`@rivet-dev/agentos\` and a software package such as \`@agentos-software/pi\`. Create a server that defines an \`agentOS()\` VM actor with that software and starts the registry via \`setup\`. Create a client that uses \`createClient\` from \`@rivet-dev/agentos/client\`, calls \`vm.getOrCreate(...)\`, chooses a session ID, opens a durable agent session with \`openSession({ sessionId, agent: "pi" })\`, and sends a prompt with \`prompt({ sessionId, content })\`.

2. Actually test it end-to-end — don't stop at "it compiles". Start the server, connect the client, create the session, send a prompt, and confirm a real response comes back from the agent. Verify the full server↔client round-trip works.

3. Read the documentation at https://agentos-sdk.dev/docs to learn more, and consult it whenever you get stuck.

If you get stuck and the docs don't unblock you, tell me to ask for help in Discord (https://rivet.dev/discord) and to open a GitHub issue (https://github.com/rivet-dev/agentos/issues).`;
