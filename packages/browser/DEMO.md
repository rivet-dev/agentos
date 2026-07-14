# Agent OS in the browser — demo

A set of runnable demos for the **converged Agent OS sidecar compiled to
WebAssembly**. They cover ACP wire handling, a real Brush shell on a kernel PTY,
and the upstream Pi CLI/TUI running on that browser PTY.

## Run it

```bash
cd packages/browser
# Builds the wasm-web package + the ACP codec bundle, then serves the demo.
node ./scripts/build-wasm-test-assets.mjs
PORT=43175 node ./tests/browser-wasm/serve.mjs
# ACP:      http://localhost:43175/demo.html (click Run demo)
# shell:    http://localhost:43175/real-terminal.html, then run
#           window.__realTerminal.start() in DevTools
# Pi TUI:   http://localhost:43175/pi-tui.html, then run
#           window.__piTui.start() in DevTools
```

Or run it headless (the demo page is verified by Playwright):

```bash
pnpm --filter @rivet-dev/agentos-browser test:browser-wasm
```

## What the demos prove

1. `demo.html` boots `agentos-sidecar-browser`, authenticates over the BARE wire
   protocol, and round-trips an ACP `get_session_state` request.
2. `real-terminal.html` launches the real Brush WASM shell, connects xterm to a
   kernel PTY master, and runs guest commands from browser keystrokes.
3. `pi-tui.html` launches `@mariozechner/pi-coding-agent`'s real CLI bundle with
   TTY stdio and renders its interactive TUI through xterm. The strict gate
   fails if visible Pi TUI output does not appear.

The test pages expose small read-only drivers on `window` so Playwright can
assert PTY descriptors and terminal contents. They do not replace the real
terminal input path: keystrokes still flow xterm → PTY master → guest, and output
returns through the PTY.

## Pi model behavior

Pi's TUI and prompt pipeline are covered separately:

- `AGENTOS_REQUIRE_REAL_PI_TUI=1` requires the upstream Pi TUI to boot visibly
  on a browser PTY without requiring a model credential.
- `pi-prompt.spec.ts` drives a full prompt through Pi with a deterministic local
  provider, proving the agent/model request pipeline without external secrets.
- `AGENTOS_REQUIRE_REAL_PI_MODEL=1` is the opt-in gate for Chrome's real on-device
  `LanguageModel`; it is skipped when that browser capability is not explicitly
  required.

For the actor-backed browser UI that opens native AgentOS VMs, real shell tabs,
and Pi tabs, see `examples/browser-terminal`.
