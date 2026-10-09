Pyodide runtime bundle for the agentos Python sidecar.

Bundled runtime files:
- `pyodide.mjs`
- `pyodide.asm.js`
- `pyodide.asm.wasm`
- `pyodide-lock.json`
- `python_stdlib.zip`

Bundled offline package wheels:
- `click-8.3.1-py3-none-any.whl`
- `micropip-0.11.0-py3-none-any.whl`
- `numpy-2.2.5-cp313-cp313-pyodide_2025_0_wasm32.whl`
- `pandas-2.3.3-cp313-cp313-pyodide_2025_0_wasm32.whl`
- `python_dateutil-2.9.0.post0-py2.py3-none-any.whl`
- `pytz-2025.2-py2.py3-none-any.whl`
- `six-1.17.0-py2.py3-none-any.whl`

Bundle size as vendored in this directory:
- Core Pyodide runtime: 12,283,621 bytes
- Offline package wheels: 8,347,517 bytes
- Total: 20,631,138 bytes (19.68 MiB)

`python-runner.mjs` points `indexURL` at this local directory.

The offline wheels need no install step: the first `import` of one of their top-level modules (for example `import pandas`) unpacks the wheel and its bundled dependencies.

Dynamic package installs:
- Other `pyodide-lock.json` packages (for example Pillow) download from `https://cdn.jsdelivr.net/pyodide/v0.29.3/full/`, the Pyodide CDN for this runtime version. Downloads go through the VM network policy, so the VM needs a network grant for that host. `AGENTOS_PYODIDE_PACKAGE_BASE_URL` overrides the base URL, for example with a mirror. Downloaded wheels are cached in the Pyodide package cache.
- Bundled wheels, including `micropip`, always load from the local asset directory, so package-manager bootstrap and bundled packages do not depend on external network access.
- `pip install` fails with a non-zero exit when a requested package does not load.
- `await micropip.install("https://.../package.whl")` goes through the Python runner's bridge-backed fetch path, which means network permissions are enforced by the agentos kernel rather than bypassing it.

Debug timing output:
- Set `AGENTOS_PYTHON_WARMUP_DEBUG=1` on a Python execution request to emit `__AGENTOS_PYTHON_WARMUP_METRICS__:` JSON lines on stderr.
- The Rust execution engine emits a `phase:"prewarm"` line that reports whether warmup executed or reused the cached compile-cache path, plus the measured warmup duration in milliseconds.
- `python-runner.mjs` emits a `phase:"startup"` line just before guest code runs, including total startup time, `loadPyodide()` time, and whether the source was inline code, a file, or prewarm-only.

Startup targets:
- Cold start target: first request in a fresh cache should keep the combined prewarm plus startup path under `3000ms` on commodity hardware.
- Warm start target: cached follow-up requests should keep the `phase:"startup"` time under `500ms`.
