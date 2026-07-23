# Command Code CLI

Run the genuine Command Code v1 CLI inside an agentOS VM.

## Quick start

The registry package projects `command-code@1.1.0` as `cmd`, `cmdc`, `command-code`, and `commandcode`. It uses the genuine upstream CLI, rebundled for the VM with compatibility patches recorded in the package's `agentos-build.json`. It is a command package, not an ACP agent. Invoke it with `execArgv` or attach a terminal for interactive use.

The CLI is sourced from the npm release, whose package metadata is `UNLICENSED`; the public Command Code GitHub repository does not contain the CLI implementation. Review the upstream terms before publishing or redistributing the packed artifact.

## Credentials

Create a Command Code API key and pass it to the command as `COMMAND_CODE_API_KEY`, sourced from your server's environment. The credential stays inside the VM process environment and is read by the packaged Command Code CLI.

See Command Code's [documentation](https://commandcode.ai/docs) for account and model details.

## Verified command surface

The agentOS end-to-end test runs the packaged command in a real VM and verifies:

- version and help output;
- unauthenticated status JSON and the documented headless authentication exit code;
- `.agents/skills/` discovery with `cmd skills list --debug`;
- project-scoped HTTP MCP add/get configuration and the resulting `.mcp.json`;
- all four projected command aliases.

The opt-in live test also verifies API-key authentication and reaches Command Code's hosted model endpoint. The available test key reached the billing boundary but had insufficient credits, so a complete generated model response has not been validated. Interactive help was also exercised through an agentOS terminal, but a complete interactive login and model turn has not been validated.

## Runtime support matrix

Legend: **Supported** is documented by Command Code on native Linux; **Tested** was exercised end to end in an agentOS VM; **Expected** has the required agentOS primitive but has not been exercised through an authenticated Command Code turn; **Partial** names an important limitation; **Unsupported** lacks the execution model the CLI requires.

| Command Code surface | Native Linux | agentOS VM | Direct Cloudflare Worker isolate |
| --- | --- | --- | --- |
| CLI startup, `--version`, `--help`, aliases | Supported | **Tested** | **Unsupported**: no standalone Node CLI/process entrypoint |
| API-key authentication | Supported | **Tested** with an opt-in live test | **Unsupported** as a full CLI |
| Browser login / MCP OAuth browser flow | Supported | **Partial**: automatic host-browser opening is not validated; API keys are preferred | **Unsupported** as a local CLI |
| Interactive TUI and TTY input | Supported | **Partial**: terminal startup/help tested; full interactive turn untested | **Unsupported**: `node:tty`, `readline`, and REPL are non-functional stubs |
| Headless `-p`, JSON/NDJSON, piped stdin, exit codes | Supported | **Partial**: unauthenticated and authenticated billing boundaries tested; funded generation and streaming untested | **Unsupported** as a full CLI |
| File read, grep, glob | Supported | **Expected**, not tested through a model turn | **Unsupported** for this CLI: Worker VFS has no glob APIs and no normal project tree |
| File edit/write | Supported (permission gated) | **Expected**, subject to VM filesystem policy | **Unsupported** for a durable project: only request-local `/tmp` is writable |
| Shell commands and hooks | Supported (permission gated) | **Expected**, subject to VM process policy; hook execution untested | **Unsupported**: `node:child_process` is a non-functional stub |
| Git and managed worktrees | Supported | **Expected** when Git is installed; not tested with Command Code | **Unsupported**: no child processes or durable project filesystem |
| Persisted sessions, resume, fork, rewind | Supported | **Expected** on the VM filesystem; not tested | **Unsupported**: request-local VFS is not persistent |
| Skills and `.agents/skills/` | Supported | **Tested** for project discovery/listing | **Unsupported** as a CLI; bundled files alone do not provide the required process model |
| MCP configuration | Supported | **Tested** for project-scoped HTTP add/get | **Unsupported** as a full CLI |
| MCP HTTP/SSE tool use | Supported | **Expected**, subject to VM network policy; live server untested | **Unsupported** as a full CLI, even though Workers can make HTTP requests |
| MCP stdio servers | Supported | **Expected**, subject to installed commands and process policy; untested | **Unsupported**: no child processes |
| Mods and local package installation | Supported | **Partial**: not validated; native dependencies may require compatible agentOS software | **Unsupported**: no package manager or child processes |
| Clipboard copy and image paste | Supported when a host clipboard is available | **Unsupported**: the VM has no host clipboard and native N-API clipboard binding | **Unsupported**: no host terminal clipboard |
| Auto-update | Supported | **Unsupported** for the projected registry package; pass `--no-auto-update` | **Unsupported** |

This Cloudflare column evaluates running the npm CLI directly in a standard Worker isolate with `nodejs_compat`. It does not describe an official Command Code Worker implementation—none was found—and it does not cover [Cloudflare Sandbox](https://developers.cloudflare.com/agents/tools/sandbox/), which is a separate container product with a real filesystem and shell.

Sources: [Command Code CLI reference](https://commandcode.ai/docs/reference/cli), [headless mode](https://commandcode.ai/docs/core-concepts/headless), [MCP](https://commandcode.ai/docs/mcp), [skills](https://commandcode.ai/docs/skills), [hooks](https://commandcode.ai/docs/hooks), [Cloudflare Node.js compatibility](https://developers.cloudflare.com/workers/runtime-apis/nodejs/), and [Cloudflare Worker filesystem](https://developers.cloudflare.com/workers/runtime-apis/nodejs/fs/).

## Permissions and updates

`--yolo` lets Command Code edit files and run shell commands without its own prompts. Those operations remain subject to the VM's filesystem, process, network, and resource policies. Only use it with trusted prompts and least-privilege VM configuration.

The registry package is immutable at `/opt/agentos`, so use `--no-auto-update` (or `COMMANDCODE_SKIP_UPDATES=1`) and update through a newer registry package instead.