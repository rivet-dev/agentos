# Browserbase CDP shell

This experiment is the smallest Browserbase version of `just shell`. AgentOS
core, the VFS, Brush shell, and both PTYs run inside the remote browser tab. The
local controller only starts the asset server, exposes it with a temporary
Cloudflare Quick Tunnel, creates a Browserbase session, and forwards raw terminal
bytes over the Chrome DevTools Protocol (CDP).

There is deliberately no xterm or DOM terminal. Browser output calls a CDP
`Runtime` binding; input uses CDP `Runtime.evaluate`. The browser VM runs real
WASM builds of Brush, Vim, Git, Bash, and the small command set listed in
`src/browser.ts`.

At boot, the command artifacts are written into
`/opt/agentos/pkgs/browser-base-shell/0.0.1/bin` in the guest VFS and linked from
`/opt/agentos/bin` and `/bin`. Brush performs ordinary `PATH` lookup; the shared
browser process host then reads and compiles the selected executable from its
guest path. There are no empty executable markers and child commands are not
selected by a demo-specific basename switch. This is deliberately close to the
native `/opt/agentos` layout, although this minimal experiment stages individual
artifacts rather than sending packed `.aospkg` files through `configure_vm`.

## Run

Browserbase credentials must be explicitly populated in the shell that launches
the demo. The experiment never reads credentials from a home-directory file.
`cloudflared` must also be on `PATH`.

```sh
cd examples/experiments/browser-base-shell
export BROWSERBASE_API_KEY='<your Browserbase API key>'
export BROWSERBASE_PROJECT_ID='<your Browserbase project ID>'
pnpm start
```

`pnpm start` builds Vim when it is missing. It also rebuilds Brush whenever its
tracked command sources or browser PID patch change, then records an ignored
fingerprint so subsequent starts remain fast. Set `AGENTOS_VIM_COMMAND` only
when deliberately testing a different prebuilt Vim.

The workspace-level `pnpm build` runs the TypeScript gate and does not require
native/browser WASM toolchains. `pnpm build:demo` assembles a deployable bundle
from checked-in command artifacts. Only `pnpm start` and `pnpm test:e2e` refresh
the local Brush/Vim artifacts needed for an interactive run.

The legacy `BROWSER_BASE_API_KEY` and `BROWSER_BASE_PROJECT_ID` spellings are
also accepted when already used by an existing environment.

Use the shell normally. For example, create and directly run a script:

```sh
printf '#!/bin/sh\necho HELLO_FROM_BROWSER\n' > /tmp/example.sh
chmod +x /tmp/example.sh
/tmp/example.sh
```

The process host implements Linux-style shebang dispatch. A text file without a
shebang still correctly fails direct execution with `ENOEXEC`; invoke that kind
of file as `bash /tmp/example.sh`. Exit the demo with `Ctrl-]`; `Ctrl-C` is
intentionally forwarded to the browser PTY.

Git supports the AgentOS VM subset: local `init`, `add`, `commit`, `rev-parse`,
`branch`, `checkout`, and local or unauthenticated HTTP(S) `clone`. It does not
pretend to implement unsupported porcelain such as `status`, `log`, or `show`;
see `registry/native/crates/libs/git/README.md` for the exact surface.

The first run starts a detached Cloudflare Quick Tunnel to Vite's fixed local
port `4178`. Later runs reuse that process and URL; exiting the shell stops Vite
and releases Browserbase but deliberately leaves `cloudflared` running. Override
the port with `AGENTOS_BROWSERBASE_PORT`. Inspect or stop the daemon explicitly:

```sh
pnpm tunnel:status
pnpm tunnel:stop
```

Run the real Browserbase E2E check with:

```sh
pnpm test:e2e
```

The test proves a shell pipeline, PTY interrupt handling, redirect persistence,
Bash execution, direct shebang execution, interactive Vim editing and save,
executing the Vim-authored script, a local Git commit plus `rev-parse`, real
child PIDs without the former warning, and cross-origin isolation. Browserbase
cannot reach the developer machine's `localhost`; the persistent Quick Tunnel
gives the remote browser a stable route to the per-run local Vite server.
