# Sandbox Mounting

Extend agentOS with full sandboxes for heavy workloads like browsers, desktop automation, and compilation.

For heavy workloads like browsers, desktop automation, and compilation, pair agentOS with a full sandbox on demand. Its filesystem mounts into the VM as a native directory, and its process management is exposed as [bindings](/docs/bindings), all provider-agnostic through [Sandbox Agent](https://sandboxagent.dev).

## Why use agentOS with a sandbox?

agentOS is an alternative to sandboxes that covers most use cases, but some workloads need a full sandbox for special kinds of software (browsers, desktop automation, heavy compilation). Sandbox mounting lets you lazily start a sandbox on demand, only when it is needed, and project it into the VM. The hybrid model means one agent session can handle both lightweight coding tasks and heavy system operations, using the right tool for each.

See [agentOS vs Sandbox](/docs/versus-sandbox) for a detailed comparison.

## When to use a sandbox

- **Native binaries** not yet supported in the agentOS runtime.
- **Browsers and desktop automation**: Playwright, Puppeteer, Selenium, or anything that needs a display server.
- **Heavy compilation**: Large builds or native toolchains that require a full Linux environment.
- **GUI applications**: Desktop apps, VNC sessions, or any workload that needs a graphical environment.
- **Node.js packages with native extensions** (e.g. `sharp`, `bcrypt`, `better-sqlite3`) that require a full build toolchain.

Start with the default agentOS VM for all workloads, and only spin up a sandbox when a task genuinely requires one. Sandboxes are billed per second of uptime, so start them on demand and tear them down when the task is done.

## Getting started

The sandbox integration ships as the `@rivet-dev/agentos-sandbox` package. It works through two mechanisms:

- **Filesystem mount**: Projects the sandbox into the VM as a native directory, like mounting a hard drive on your own machine. Read and write files through the mount directly.
- **Bindings**: Exposes sandbox process management as [bindings](/docs/bindings). Execute commands on the sandbox from within the VM.

Both are powered by [Sandbox Agent](https://sandboxagent.dev), and you can swap providers without changing agent code. Install both packages:

```bash
npm install @rivet-dev/agentos-sandbox sandbox-agent
```

`createSandboxFs` and `createSandboxBindings` come from `@rivet-dev/agentos-sandbox`. `SandboxAgent` and the provider helpers (such as `docker`) come from the `sandbox-agent` package.

## Calling the mounted bindings

Once the sandbox is mounted, write code through the filesystem and run it inside the sandbox. The sandbox bindings are exposed inside the VM as a CLI command, so you call it through the same `exec`/`spawn` surface as any other command.

## Bindings reference

The bindings expose these commands inside the VM:

```bash
# Run a command synchronously
agentos-sandbox run-command --command "npm install" --cwd "/app"

# Start a background process
agentos-sandbox create-process --command "npm" --args "run" --args "dev"

# List running processes
agentos-sandbox list-processes

# Get process output
agentos-sandbox get-process-logs --id "proc_abc123"

# Stop or kill a process
agentos-sandbox stop-process --id "proc_abc123"
agentos-sandbox kill-process --id "proc_abc123"

# Send input to an interactive process
agentos-sandbox send-input --id "proc_abc123" --data "yes"
```

## Sandbox providers

The extension works with any [Sandbox Agent](https://sandboxagent.dev) provider. See the [Sandbox Agent documentation](https://sandboxagent.dev) for available providers and setup instructions.