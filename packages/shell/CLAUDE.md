# agentos-shell

- Run the local interactive shell from the repo root with `just shell`. If workspace dependency links are missing, the recipe repairs the partially installed workspace with `pnpm install --force`. It also rebuilds missing shell software and direct runtime entrypoints, then visibly builds the sidecar at `target/debug/agentos-sidecar` before opening the guest prompt. Cold registry and sidecar builds can take a while; later runs are incremental.
- Keep `just shell` a transparent shim over the shell CLI: forward its arguments without injecting CLI flags. The CLI itself defaults stdin attachment and TTY mode on; callers can opt out with `--no-interactive` or `--no-tty`.
- `just shell` always uses the in-repo AgentOS sidecar it just built, even if the host environment exports `AGENTOS_SIDECAR_BIN`. To deliberately test another binary, invoke the shell CLI directly with that override.
- `just shell --actor` additionally builds and pins the in-repo actor plugin. Actor development requires the Rivet repo at `../r6` (override with `AGENTOS_R6_ROOT`); the recipe installs the filtered `rivetkit` dependency graph there when its `tsx` loader is missing.
- Do not implement or route through a custom/synthetic shell, prompt, line editor, or command parser; interactive shell mode must launch native Bash through the terminal/PTY path so behavior matches `docker run -it bash`.
- Keep `agentos-shell` loading every command-providing package from the in-repo `software/`; when that registry changes, update the imports, package dependencies, and smoke coverage here.
