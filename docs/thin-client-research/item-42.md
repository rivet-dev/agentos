# Item 42 research — use Linux cwd semantics and no compiler bootstrap files

Status: implementation-ready research only. This note does not modify production
code, tests, or the Item 42 tracker status.

Refreshed against the empty Item 42 revision `suwmustu` on 2026-07-14, whose
Item 41 parent is `qmzytqsv` / `3c4ab15d`. Priority: **P2**. Confidence:
**high**.

## Recommendation

Remove all TypeScript-compiler transport files, not only the redundant
`mkdir("/tmp")`. Send the compiler request through the existing bounded process
stdin protocol and run the fixed compiler runner with `node -e`. Inside that
guest process, use `process.cwd()` as the only base for project, config, and
synthetic-source paths.

Also correct explicit relative execution cwd once in shared sidecar behavior.
Native currently turns `cwd: "project"` into `/project`; browser forwards the
unresolved string `project`. Both must resolve it against the VM cwd, exactly as
Linux resolves a relative `chdir` target. TypeScript and Rust clients should
continue to forward an explicit cwd unchanged and preserve omission.

The resulting flow is:

```text
TypeScript request
    -> Execute { argv: ["node", "-e", fixedRunner], stdin: request JSON,
                 cwd: explicit caller value or omitted }
    -> shared sidecar cwd resolution
    -> Node process.cwd()
    -> TypeScript compiler
```

Rename the remaining secure-exec-era synthetic source name to
`__agentos_typescript_input__.ts`.

This is smaller and more Linux-native than the old recommendation to retain two
files under `/tmp`: source requests no longer need guest filesystem writes,
there is nothing to bootstrap or clean up, and project emit retains only the
filesystem writes that compilation functionally requested.

## Current issues and exact anchors

### The compiler client bootstraps and transports through the filesystem

`packages/typescript/src/index.ts:137-206` currently:

1. calls `agentOs.mkdir("/tmp", { recursive: true })` and wraps failure in a
   package-specific bootstrap error;
2. allocates an ID from client-global `nextRuntimeRequestId` at line 85;
3. writes a request JSON file and a generated CommonJS runner under `/tmp`;
4. executes the runner file; and
5. performs two existence probes and deletes the files in `finally` through
   `removeGuestFileIfExists` at lines 300-307.

None of that file lifecycle is compiler behavior. `execArgv` already accepts
`stdin`; the sidecar owns its bounded input route and closes it before wait. A
fixed `node -e` runner can collect request JSON from `process.stdin`. User source
remains on stdin, not argv, so request size does not inflate the process command
line. Use the stream interface rather than `fs.readFileSync(0)`: AgentOS's guest
`node:fs` shim special-cases numeric stdio for `readSync`, not `readFileSync`.

The current `execArgv` options at lines 165-169 correctly preserve cwd omission:

```ts
{
	...(request.options.cwd === undefined
		? {}
		: { cwd: request.options.cwd }),
}
```

Keep that presence behavior and add `stdin`; do not fill `/workspace`, `/root`,
or any other cwd default in the package.

### The compiler applies cwd twice

The embedded `compilerRuntimeMain` independently runs this in both
`resolveProjectConfig` and `createSourceProgram` at current lines 387 and 422:

```ts
const cwd = path.resolve(options.cwd ?? "/root");
```

The caller's `cwd` has already been supplied to process creation. Resolving the
same relative string again inside that process duplicates policy and can append
the directory twice. Both helpers should instead use:

```ts
const cwd = process.cwd();
```

`options.cwd` remains public because it selects the execution cwd; it should not
also be a compiler path base after the process starts.

### Relative execution cwd is divergent in the sidecars

The existing note incorrectly assumed the sidecar already resolved a relative
execution cwd against the VM cwd. Current code proves otherwise:

- Native `resolve_guest_execution_cwd` at
  `crates/native-sidecar/src/execution.rs:9889-9893` passes every present string
  directly to `normalize_path`; `normalize_path("project")` produces `/project`.
- Browser execute dispatch at
  `crates/native-sidecar-browser/src/wire_dispatch.rs:2019-2034` uses a present
  payload cwd verbatim, so the same request remains `project`.
- Shared filesystem path resolution already has the correct rule in
  `crates/native-sidecar-core/src/guest_fs.rs:23-34`: absolute paths normalize
  from root; relative paths join the VM cwd; empty paths return `ENOENT`.

Reuse that shared rule rather than adding a TypeScript-specific workaround or
copying another native/browser path function.

### Legacy synthetic filename and example bootstrap

`createSourceProgram` at `packages/typescript/src/index.ts:423-426` defaults an
in-memory source to `__secure_exec_typescript_input__.ts`. The migration map at
`scripts/secure-exec-agentos-map.json:305` already names the intended
`__agentos_typescript_input__` replacement; this literal was missed.

The runnable example also calls `vm.mkdir("/root", { recursive: true })` at
`packages/secure-exec-example-ai-agent-type-check/src/index.ts:64` immediately
before writing its output. `/root` is already part of the Linux base. The checked
copy at `docs/features/typescript.mdx:78` repeats that unnecessary bootstrap.

## Existing Linux base and process contracts

Do not add `/tmp`, `/root`, `/workspace`, or TypeScript defaults to a client or a
new RPC:

- `crates/native-sidecar/src/vm.rs:72-117` declares `/tmp` mode `01777`, `/root`,
  and `/workspace` in the sidecar-owned root bootstrap table.
- The same table is applied to native roots at lines 1472-1485 and shadow roots
  at lines 1995-2010, including when the default base layer is disabled.
- `resolve_guest_cwd` at `crates/native-sidecar/src/vm.rs:1928-1932` resolves an
  omitted VM cwd to `/workspace`.
- `KernelVmConfig::new` at `crates/kernel/src/kernel.rs:130-143` uses the same
  kernel default.
- Native `prepare_guest_runtime_env` at
  `crates/native-sidecar/src/execution.rs:10869-10985` sets `PWD` to the resolved
  guest cwd and supplies `TMPDIR=/tmp` only as runtime environment.
- `packages/core/tests/kernel-bootstrap-base.test.ts:18-38` proves `/tmp` is
  `01777` with and without the bundled base.
- `packages/core/tests/wasm-commands.test.ts:47-51` proves omitted execution cwd
  is `/workspace`.

Sidecar bootstrap is trusted runtime initialization and does not consume guest
filesystem permission. A VM policy that denies guest `/tmp` writes must not
prevent the sidecar from providing the normal Linux directory.

## Exact implementation edits

### `packages/typescript/src/index.ts`

Delete `nextRuntimeRequestId` at current line 85. Replace
`runCompilerInAgentOs` at current lines 137-206 with the same response/error
handling but one execution call:

```ts
async function runCompilerInAgentOs(
	agentOs: AgentOs,
	request: CompilerRequest,
): Promise<CompilerResponse> {
	let result;
	try {
		result = await agentOs.execArgv(
			"node",
			["-e", buildCompilerRuntimeScript()],
			{
				stdin: JSON.stringify(request),
				...(request.options.cwd === undefined
					? {}
					: { cwd: request.options.cwd }),
			},
		);
	} catch (error) {
		throw new Error(`TypeScript runner execution failed: ${String(error)}`, {
			cause: error,
		});
	}

	if (result.stdout.trim()) {
		try {
			return parseRuntimeResponse(result.stdout);
		} catch (error) {
			throw new Error(
				`failed to decode TypeScript runner response: ${String(error)}`,
				{ cause: error },
			);
		}
	}
	if (result.exitCode !== 0) {
		throw new Error(
			`TypeScript runtime exited ${result.exitCode}${
				result.stderr.trim() ? `: ${result.stderr.trim()}` : ""
			}`,
		);
	}
	throw new Error("TypeScript runtime produced no response");
}
```

Change `buildCompilerRuntimeScript(requestPath: string)` at current line 251 to
take no argument. Remove its outer request-file `node:fs` import and execute the
existing compiler/envelope body from the stdin end handler:

```js
let requestJson = "";
process.stdin.setEncoding("utf8");
process.stdin.on("data", (chunk) => {
	requestJson += chunk;
});
process.stdin.on("end", () => {
	try {
		const request = JSON.parse(requestJson);
		const ts = loadTypeScriptCompiler(request.compilerSpecifier);
		const __name = (target) => target;
		const result = (${compilerRuntimeMain.toString()})(request, ts);
		process.stdout.write(JSON.stringify({ ok: true, result }));
	} catch (error) {
		process.stdout.write(JSON.stringify({
			ok: false,
			errorMessage:
				error instanceof Error ? (error.stack ?? error.message) : String(error),
		}));
		process.exitCode = 1;
	}
});
process.stdin.resume();
```

Keep that existing `compilerRuntimeMain.toString()` interpolation; do not
duplicate its body or make user data part of generated source.

Delete `removeGuestFileIfExists` at current lines 300-307. There are no compiler
transport files left to probe, delete, or retain IDs for.

In `compilerRuntimeMain`:

- replace both `path.resolve(options.cwd ?? "/root")` expressions with
  `process.cwd()`;
- change `__secure_exec_typescript_input__.ts` to
  `__agentos_typescript_input__.ts`;
- keep `ProjectCompilerOptions.cwd` and `SourceCompilerOptions.cwd` unchanged;
- keep `fs.mkdirSync(path.dirname(fileName), { recursive: true })` in project
  emit. It creates caller-requested output directories and is functional
  compiler behavior.

### Shared and native sidecar cwd resolution

In `crates/native-sidecar-core/src/guest_fs.rs`, make the existing
`resolve_guest_path` function public. Keep its absolute/relative/empty semantics;
generalize the empty-path text from `guest filesystem path is empty` to `guest
path is empty`, and update the local assertion at current line 609. Do not create
another resolver. Re-export it from
`crates/native-sidecar-core/src/lib.rs:63-67`.

In `crates/native-sidecar/src/execution.rs`:

1. import `resolve_guest_path` from `agentos_native_sidecar_core`;
2. make `resolve_guest_execution_cwd` return
   `Result<String, SidecarError>`;
3. for an explicit cwd, call `resolve_guest_path(&vm.guest_cwd, value)` and map
   its POSIX error through the existing `guest_kernel_core_error`; for omission,
   clone `vm.guest_cwd`;
4. in `resolve_execution_cwds`, attempt the existing in-sandbox host-path
   compatibility branch **only when `Path::new(raw_cwd).is_absolute()`**;
   relative values such as `project`, `./project`, and `../project` must always
   take the guest resolver, regardless of the sidecar process's host cwd;
5. make `resolve_execution_cwds` return a `Result`, wrapping the guarded
   in-sandbox host-path compatibility return in `Ok(...)` and using `?` for the
   shared guest-path fallback; and
6. add `?` at its two callers at current lines 9377-9378 and 9430.

Do not otherwise disturb the host-path compatibility branch, host shadow
mapping, entrypoint resolution, `PWD`, or omitted-cwd behavior. The absolute
guard is important even though today's lexical `normalize_host_path("project")`
normally does not fall under an absolute `vm.host_cwd`: guest semantics must not
depend on the sidecar's launch directory or a future normalization change. This
edit only changes explicit relative guest paths and makes empty cwd fail instead
of becoming `/`.

### Browser sidecar cwd resolution

In `crates/native-sidecar-browser/src/wire_dispatch.rs`, import
`resolve_guest_path`. In `execute`, after the existing shell/PTY/timeout request
validation at current lines 1916-1950 but before process-ID allocation:

1. read the VM's sidecar-owned base cwd once with `self.sidecar.guest_cwd`;
2. preserve it when `payload.cwd` is omitted;
3. resolve a present cwd with `resolve_guest_path(&vm_cwd, cwd)`; and
4. reject a lookup/resolution failure before allocating a process ID or browser
   execution context.

Delete the late verbatim/omission match at current lines 2019-2034 and pass the
already-resolved `guest_cwd` into `StartExecutionRequest`. This avoids creating a
context that must then be released merely to discover an invalid cwd.

No sidecar protocol or generated binding changes are needed. `ExecuteRequest.cwd`
already carries an optional caller override.

### Example and documentation

In `packages/secure-exec-example-ai-agent-type-check/src/index.ts`, delete only
the `/root` mkdir at current line 64. Keep the explicit generated source/output
paths; an absolute `/root` path is valid Linux behavior and the base owns it.

Delete the matching line from the checked runnable block at
`docs/features/typescript.mdx:78`.

Add the user-facing contract to `packages/typescript/README.md` and, after the
setup section, `docs/features/typescript.mdx`:

- requests run inside the caller-owned VM;
- omitted cwd uses that VM's resolved process cwd, normally `/workspace`;
- explicit absolute or relative cwd is resolved once by process creation, and
  compiler-relative paths use that resulting `process.cwd()`;
- source requests are transported over process stdin and do not create temp
  files; and
- project compilation may still read project files and write requested outputs.

In
`packages/secure-exec-example-ai-agent-type-check/package.json`, change the
scoped script to:

```json
"check-types": "tsc --noEmit -p tsconfig.json && pnpm verify-docs"
```

This makes the already-present checked-example contract part of root `pnpm
check-types` without adding a root orchestration script or workflow step.

Do not rename the example directory in Item 42. Do not edit
`scripts/secure-exec-agentos-map.json`; it already records the intended literal
rename.

## Before tests

Add the desired regression tests first and run them against the completed Item
41 parent. Record the failing test names, output, parent jj revision, and command
in Item 42's tracker checklist before changing production code.

Use these exact proposed test names and parent-failure commands after adding the
tests but before the production edit:

```sh
pnpm --dir packages/typescript exec vitest run \
  tests/typescript-tools.integration.test.ts \
  -t "uses stdin without compiler transport files and inherits the VM cwd"

pnpm --dir packages/typescript exec vitest run \
  tests/typescript-tools.integration.test.ts \
  -t "resolves a relative project cwd once against the VM cwd"

cargo test -p agentos-native-sidecar-browser --test wire_dispatch \
  browser_wire_dispatcher_handles_lifecycle_and_execution_frames -- --exact
```

The first must fail on Item 41 with the `/tmp` preparation diagnostic, the
second must fail because native resolves/duplicates the relative cwd, and the
third must fail because browser records `project` verbatim. If a test does not
fail for that reason, correct the characterization before implementation rather
than accepting a false-green parent.

### No compiler transport writes and omitted cwd

In `packages/typescript/tests/typescript-tools.integration.test.ts`, create a
dedicated VM with the normal `nodeModulesMount` and a filesystem rule that denies
all compiler transport mutations under `/tmp`:

```ts
const restrictedVm = await AgentOs.create({
	defaultSoftware: false,
	mounts: [nodeModulesMount(join(workspaceRoot, "node_modules"))],
	limits: { jsRuntime: { v8HeapLimitMb: 256, cpuTimeLimitMs: 5_000 } },
	permissions: {
		fs: {
			default: "allow",
			rules: [
				{
					mode: "deny",
					operations: ["write", "create_dir", "rm"],
					paths: ["/tmp", "/tmp/**"],
				},
			],
		},
	},
});
```

Call `typecheckSource` without cwd or filePath:

```ts
const tools = createTypeScriptTools({ agentOs: restrictedVm });
const result = await tools.typecheckSource({
	sourceText: "const value: string = 1;\n",
});
const diagnostic = result.diagnostics.find(({ code }) => code === 2322);
expect(result.success).toBe(false);
expect(diagnostic?.filePath).toBe(
	"/workspace/__agentos_typescript_input__.ts",
);
expect(
	(await restrictedVm.readdir("/tmp")).filter((name) =>
		name.startsWith("agentos-typescript-"),
	),
).toEqual([]);
```

Construct `tools` with `restrictedVm` directly and dispose that VM in `finally`;
do not replace the shared fixture and accidentally deny legitimate project emit.

On the parent this fails before compilation with code 0 and
`failed to prepare TypeScript runner directory`. Afterward it proves stdin
transport, no `/tmp` mutation, the sidecar's omitted `/workspace` cwd, the new
synthetic filename, and the retained TypeScript diagnostic in one test. Sidecar
root bootstrap still succeeds because guest permissions are applied to guest
operations, not trusted base construction.

### Relative cwd resolves once

In the same integration file, create
`/workspace/project/{tsconfig.json,src/index.ts}` and assert:

```ts
await vm.mkdir("/workspace/project/src", { recursive: true });
await vm.writeFile(
	"/workspace/project/tsconfig.json",
	JSON.stringify({
		compilerOptions: { module: "commonjs", target: "es2022" },
		include: ["src/**/*.ts"],
	}),
);
await vm.writeFile(
	"/workspace/project/src/index.ts",
	"export const value: number = 7;\n",
);
await expect(tools.typecheckProject({ cwd: "project" })).resolves.toEqual({
	success: true,
	diagnostics: [],
});
```

The parent fails: native resolves the process cwd from root rather than the VM
cwd, and the compiler then reapplies `project`. The fixed test proves the client
forwards the string, native sidecar resolves `/workspace/project`, and the guest
compiler trusts `process.cwd()` without a second resolution.

### Browser parity

In
`crates/native-sidecar-browser/tests/wire_dispatch.rs::browser_wire_dispatcher_handles_lifecycle_and_execution_frames`:

- include `/workspace/project` as a directory in the bootstrap entries;
- change the execution request's current absolute cwd at line 1242 to
  `Some(String::from("project"))`; and
- change the later process-snapshot cwd assertion at current line 1331 to
  `/workspace/project`.

That desired assertion fails on the parent because browser records `project`
verbatim. Afterward it proves browser uses the same shared Linux rule.

The existing shared guest filesystem test at
`crates/native-sidecar-core/src/guest_fs.rs:587-610` already characterizes
relative join, `.`/`..` normalization, and empty-path `ENOENT`. Extend it or add
a focused direct `resolve_guest_path` test when making the function public; do
not copy fixtures into native and browser helpers.

## After tests and validation

Keep the existing absolute `/root` project cases in
`typescript-tools.integration.test.ts`; they prove explicit absolute overrides
still work. Rename `uses the caller-owned VM and removes its temporary runner
files` to `uses the caller-owned VM without temporary runner files`; retain its
caller-owned `/tmp/caller-owned.txt` and empty `agentos-typescript-*` assertions.

Run:

```sh
pnpm --dir packages/typescript exec vitest run \
  tests/typescript-tools.integration.test.ts
pnpm --dir packages/typescript check-types
pnpm --dir packages/typescript build
pnpm --dir packages/typescript test:smoke

cargo test -p agentos-native-sidecar-core resolves_guest
cargo test -p agentos-native-sidecar-browser --test wire_dispatch \
  browser_wire_dispatcher_handles_lifecycle_and_execution_frames -- --exact
cargo test -p agentos-native-sidecar --lib
cargo check -p agentos-native-sidecar -p agentos-native-sidecar-browser

pnpm --dir packages/secure-exec-example-ai-agent-type-check check-types
pnpm --dir packages/core exec vitest run tests/kernel-bootstrap-base.test.ts
cargo fmt --check
git diff --check
```

Final absence checks:

```sh
! rg -n '__secure_exec_typescript_input__|options\.cwd \?\? "/root"|mkdir\("/tmp"|agentos-typescript-request-|agentos-typescript-runner-' \
  packages/typescript
! rg -n 'mkdir\("/root"' \
  packages/secure-exec-example-ai-agent-type-check/src/index.ts \
  docs/features/typescript.mdx
```

Update the Item 42 tracker acceptance row only during implementation: the before
box cites both failing TypeScript regressions and the browser parity regression;
the after box cites stdin/no-temp behavior, native/browser relative-cwd parity,
and absolute/omitted cwd coverage; confidence becomes high. Mark completion only
after the dedicated jj revision and focused gates pass.

## CI impact

No workflow YAML or `scripts/ci.sh` edit is needed:

- regular CI runs workspace TypeScript build/check-types and `pnpm test`, which
  cover the compiler package and the example's newly chained docs verifier;
- regular Rust CI runs workspace clippy with all targets, compiling every
  changed shared/native/browser Rust test target;
- `scripts/ci.sh` runs the same workspace clippy plus TypeScript workspace
  checks; and
- nightly `cargo test --workspace` runs the shared/native/browser Rust tests.

The focused shared, native, and browser commands remain mandatory before sealing
because regular GitHub/local CI compile these crates but do not explicitly run
their test packages. Adding workflow steps solely for this bounded cwd change is
unnecessary while focused validation and nightly own the behavioral tests.

## Dependencies, risks, and non-goals

- **Stack order:** Item 42 must be one revision on top of completed Item 41.
- **Item 43:** the next item retains implemented process `cwd` options, and its
  research explicitly excludes the TypeScript `filePath` examples. Preserve
  this item's cwd contract when rebasing; no direct path overlap is expected.
- **Item 49:** later example dependency cleanup touches the same example package
  but not this source behavior. Preserve the scoped docs-verifier wiring.
- **Fixed argv size:** only the fixed generated runner is passed to `node -e`;
  user source/request JSON is stdin. Do not interpolate user source into argv.
- **Bounded stdin:** rely on the existing sidecar process-input bounds and typed
  failures. Do not add an unbounded client buffer or fallback temp file.
- **Functional filesystem access remains:** project typecheck reads files and
  project emit may create output directories/write outputs. Item 42 removes
  transport/bootstrap writes, not caller-requested compiler I/O.
- **Missing cwd:** an explicit nonexistent or empty cwd should fail through
  normal process/POSIX behavior. Do not create it in the client or compiler.
- **Host-path compatibility:** native has an existing in-sandbox absolute host
  cwd branch for runtime staging. Preserve it, but gate it to absolute input;
  relative cwd is always guest-relative and must use the shared resolver.
- **No protocol feature:** stdin and optional execute cwd already exist. No BARE,
  generated binding, Rust client, lockfile, or secure-exec mirror change is
  required.
- **No TypeScript sidecar special case:** `/tmp`, cwd, and synthetic compiler
  names must not be hard-coded into sidecar behavior.

## Dedicated stacked jj revision

Create exactly one revision on top of completed Item 41, keeping the existing
stack bookmark and shared working copy. Suggested description:

```text
refactor(typescript): use stdin and Linux process cwd
```

Expected path scope:

```text
packages/typescript/src/index.ts
packages/typescript/tests/typescript-tools.integration.test.ts
packages/typescript/README.md
crates/native-sidecar-core/src/guest_fs.rs
crates/native-sidecar-core/src/lib.rs
crates/native-sidecar/src/execution.rs
crates/native-sidecar-browser/src/wire_dispatch.rs
crates/native-sidecar-browser/tests/wire_dispatch.rs
packages/secure-exec-example-ai-agent-type-check/src/index.ts
packages/secure-exec-example-ai-agent-type-check/package.json
docs/features/typescript.mdx
docs/thin-client-migration.md
```

No sidecar protocol/generated binding, Rust/TypeScript client API, root package
script, workflow YAML, lockfile, or compatibility mirror path should appear.
