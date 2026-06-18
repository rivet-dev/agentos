# codedb WASI fork

Repo-side WASI wrapper for [`justrach/codedb`](https://github.com/justrach/codedb).

- Upstream snapshot: `04a21fb`
- Upstream license: BSD-3-Clause
- Local license file: `LICENSE.upstream`

What this fork keeps:

- `explore.zig` indexing and outline logic
- `index.zig` word and trigram search
- `style.zig` tree formatting

What this fork intentionally drops:

- daemon startup
- HTTP server
- MCP server
- watcher thread
- child-process spawning
- POSIX file locking

Supported CLI shape today:

- `codedb tree <root>`
- `codedb outline <root> <path>`
- `codedb find <root> <symbol>`
- `codedb search <root> <query> [max]`
- `codedb word <root> <word>`
- `codedb deps <root> <path>`
- `codedb read <root> <path>`

Build:

```bash
zig build -Dtarget=wasm32-wasi -Doptimize=ReleaseSmall
cp -f zig-out/bin/codedb.wasm zig-out/bin/codedb
```

The extra copy step matches the registry command-dir convention, which expects a file named `codedb` without the `.wasm` suffix.
