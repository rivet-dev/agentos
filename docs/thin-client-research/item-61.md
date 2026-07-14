# Item 61 implementation research: preserve complete TypeScript Zod behavior

Status: implementation-ahead research only. No production code, test, protocol,
or tracker status was changed by this research.

## Decision

Allow TypeScript host tools to use transforms, pipes, synchronous custom
refinements, and asynchronous custom refinements. Make both the Zod 4 native
path and the existing Zod 3 converter-compatibility path forward only an
input-side structural JSON Schema to the sidecar, keep the caller's original
Zod schema in the TypeScript host-tool map, and run that original schema exactly
once with `safeParseAsync` immediately before `execute`.

Priority: **P1**. Fix confidence: **high**.

```text
complete caller Zod schema
  |-- structural input JSON Schema --> protocol --> sidecar CLI/help/early shape checks
  `-- original Zod object -----------------------> one host parse --> execute(parsed.data)
```

The sidecar must not interpret or execute Zod effects. A refinement or transform
is arbitrary trusted JavaScript and cannot be represented by JSON Schema or run
by Rust. Keeping it in TypeScript is the intentional exception already recorded
at `docs/thin-client-migration.md:20-21`.

One boundary must remain explicit: do **not** silently enable Zod 3
`preprocess`. Its raw input is unknown and `zod-to-json-schema`'s
`effectStrategy: "input"` describes the schema *after* preprocessing. For
example, `z.preprocess(Number, z.number())` accepts the raw string `"3"`, while
the generated `{ type: "number" }` would make the sidecar reject it before Zod
runs. Continue returning a typed conversion error for preprocess until a
separate permissive structural projection is designed. Item 61 only names
transforms and custom refinements.

## Tracker anchors and current before behavior

The issue, status, and checklist are currently at:

- `docs/thin-client-migration.md:107` -- issue 61;
- `docs/thin-client-migration.md:194` -- `pending`, P1/high confidence; and
- `docs/thin-client-migration.md:286` -- before/after/dedicated-revision checks.

The before behavior is already executable and should be retained as the first
red/green proof:

- `packages/core/src/host-tools-zod.ts:12-20` classifies `effects`,
  `pipeline`, and `pipe` as unsupported;
- `host-tools-zod.ts:118-139,186` detects Zod 4 custom checks and rejects them;
- `packages/core/tests/host-tools-zod.test.ts:192-198` asserts that a custom
  `.refine(...)` throws `HostToolSchemaConversionError`; and
- `packages/core/tests/toolkit-permissions.test.ts:420-459` has to register a
  plain schema and monkey-patch its `safeParse` method afterward to simulate a
  transform, because a real transformed schema cannot register.

The existing converter suite passes before the fix:

```sh
pnpm --dir packages/core exec vitest run tests/host-tools-zod.test.ts
```

That command currently reports five passing tests, including the expected
custom-refinement rejection. During implementation, first replace that
rejection expectation with positive input-projection tests and confirm they fail
against the old production code.

## Exact live code path

### TypeScript registration and callback execution

- `packages/core/src/host-tools.ts:7-17` stores the complete live `ZodType` on
  each `HostTool`.
- `packages/core/src/agent-os.ts:995-1011` calls `zodToJsonSchema` only to build
  the sidecar registration definition.
- `agent-os.ts:1254-1271` serializes those toolkit definitions for VM
  initialization.
- `agent-os.ts:1059-1067,2804-2820` separately retains each original `HostTool`
  in the callback map.
- `agent-os.ts:1013-1057` looks up the original tool and currently calls
  synchronous `safeParse` once before `execute`.

Therefore the required state split already exists. Item 61 only needs to stop
rejecting effects during the structural projection and make the single host
parse async-capable.

### Protocol and runtime serializer

No wire change is needed:

- `packages/runtime-core/src/request-payloads.ts:39-54` models
  `input_schema: unknown` as registration metadata;
- `request-payloads.ts:601-627` serializes that value into JSON UTF-8;
- `crates/sidecar-protocol/protocol/agentos_sidecar_v1.bare:264-280` already
  defines one `RegisteredHostCallbackDefinition.inputSchema: JsonUtf8`; and
- clients, generated codecs, and both sidecars already ship in lockstep.

Do not add a Zod-effects field, transformed schema, or second validation schema
to the protocol. The existing field is the sidecar-facing structural input
schema.

### Native and browser sidecars

No Rust production change is needed:

- `crates/native-sidecar-core/src/tools.rs:36-123` validates registration bounds
  and JSON schema shape without interpreting Zod;
- `crates/native-sidecar/src/tools.rs:288-306` parses the registered JSON and
  performs command input handling before requesting the host callback;
- `native-sidecar/src/tools.rs:354-438` derives object flags from properties,
  required fields, and primitive/array types;
- `native-sidecar/src/tools.rs:494-564` performs the intentionally shallow
  structural check;
- `crates/native-sidecar-browser/src/wire_dispatch.rs:868-900` routes the same
  registration request; and
- `native-sidecar-browser/src/service.rs:1529-1551` uses the shared registration
  validator and stores the same structural schema.

Native Rust coverage already owns structural behavior at
`native-sidecar/src/tools.rs:919-954` and in
`native-sidecar/tests/service.rs:10594-10902`. Do not add Zod terminology or
effect behavior to Rust tests; the new TypeScript real-sidecar test should prove
that structural dispatch and the authoritative host parse compose.

### Rust SDK parity

No Rust SDK change is needed. Rust tools are authored with JSON Schema at
`crates/client/src/config.rs:187-207`, serialized unchanged at
`crates/client/src/agent_os.rs:222-243`, and host-validated before their Rust
callback at `agent_os.rs:1381-1429`. Rust cannot author or execute Zod effects.
Parity here means both clients forward the same structural wire field and both
sidecars consume it identically; Zod authoring/validation is intentionally
TypeScript-specific.

## Verified converter behavior

The following was verified against the installed dependencies
(`zod@4.1.11`, `zod3@3.25.x`, `zod-to-json-schema@3.25.2`):

### Zod 4

Calling native `schema.toJSONSchema({ io: "input" })`:

- converts root and nested `.transform(...)` schemas to their input shape;
- converts `.pipe(outputSchema)` to the input schema and omits output-only
  constraints;
- retains representable input constraints such as `minLength`;
- omits custom refinement functions;
- does not execute a refinement or transform during conversion; and
- works recursively, so no custom Zod schema reconstruction is required.

Calling `toJSONSchema()` without input mode still throws `Transforms cannot be
represented in JSON Schema` for transforms and describes the output side of a
pipe, which is why the option is required.

### Zod 3

The existing fallback accepts:

```ts
{
	$refStrategy: "none",
	target: "jsonSchema7",
	effectStrategy: "input",
	pipeStrategy: "input",
}
```

`effectStrategy: "input"` preserves the pre-transform/refinement shape.
`pipeStrategy: "input"` is essential: the current default `"all"` emits an
`allOf` containing output constraints and would falsely claim that the sidecar
enforces post-pipe semantics. Conversion does not run user callbacks.

Keep `zod-to-json-schema`; Item 49 must not remove it as unused because it is
still the Zod 3 compatibility converter.

## Exact production edits

### 1. `packages/core/src/host-tools-zod.ts`

At lines 4-20, separate true unsupported value shapes from wrappers whose input
shape can be projected:

```ts
const STRUCTURAL_INPUT_WRAPPER_TYPES = new Set(["pipeline", "pipe"]);

const UNSUPPORTED_TYPES = new Set([
	"bigint",
	"date",
	"intersection",
	"tuple",
]);
```

Zod 3 `effects` needs its own branch because `refinement`/`transform` are safe to
project but `preprocess` is not.

Replace `getInnerSchema` at lines 57-62 so Zod 4's string-valued `def.type`
cannot hide a pipe's `def.in` schema:

```ts
function getInnerSchema(schema: ZodType): ZodType | undefined {
	const def = getSchemaDef(schema);
	const inner = def.innerType ?? def.schema ?? def.in ?? def.type;
	return inner && typeof inner === "object"
		? (inner as ZodType)
		: undefined;
}
```

The current order is `innerType ?? schema ?? type ?? in`; on Zod 4 `def.type`
is the literal string `"pipe"`, not an inner schema.

Delete `isCustomRefinement`, `validateChecks`, and the call to
`validateChecks` at line 186. Keep `getChecks`, because record-key validation at
lines 215-235 uses it to reject constrained record keys.

In `validateSchema`, after the existing metadata/unsupported/discriminated-union
checks and before ordinary object recursion, add the input-wrapper handling:

```ts
if (typeName === "effects") {
	const effect = getSchemaDef(schema).effect;
	const effectType =
		effect && typeof effect === "object"
			? String((effect as JsonObject).type ?? "")
			: "";
	if (effectType === "preprocess") {
		throw new HostToolSchemaConversionError(
			path,
			"effects",
			"preprocess raw input cannot be represented structurally",
		);
	}
	if (effectType !== "refinement" && effectType !== "transform") {
		throw new HostToolSchemaConversionError(
			path,
			"effects",
			`unsupported Zod effect kind: ${effectType || "unknown"}`,
		);
	}
	const inner = getInnerSchema(schema);
	if (!inner) {
		throw new HostToolSchemaConversionError(
			path,
			"effects",
			"effect schema is missing its input schema",
		);
	}
	validateSchema(inner, path);
	return;
}

if (STRUCTURAL_INPUT_WRAPPER_TYPES.has(typeName)) {
	const inner = getInnerSchema(schema);
	if (!inner) {
		throw new HostToolSchemaConversionError(
			path,
			displayTypeName(typeName),
			"pipe schema is missing its input schema",
		);
	}
	validateSchema(inner, path);
	return;
}
```

Do not add these wrappers to the existing generic transparent set without the
preprocess distinction. Continue rejecting discriminated unions, tuples,
intersections, dates, bigint, metadata-generated refs, constrained record keys,
and any other shape the sidecar CLI cannot project.

At `generateJsonSchema` (`host-tools-zod.ts:343-363`), request the input side in
both compatibility paths:

```ts
const nativeJsonSchema = (
	schema as ZodType & {
		toJSONSchema?: (options?: { io?: "input" | "output" }) => unknown;
	}
).toJSONSchema?.({ io: "input" });
```

and:

```ts
const generated = zodV3ToJsonSchema(schema as never, {
	$refStrategy: "none",
	target: "jsonSchema7",
	effectStrategy: "input",
	pipeStrategy: "input",
});
```

Keep `findUnsupportedGeneratedKeyword` and `sanitizeJsonSchema`. Add a short
comment above `zodToJsonSchema` saying its result is a sidecar-facing structural
input schema, not the authoritative validator.

### 2. `packages/core/src/agent-os.ts`

At line 1035, make the one authoritative parse async-capable:

```ts
const parsed = await tool.inputSchema.safeParseAsync(payload.input);
```

Leave the existing `!parsed.success` response and `execute(parsed.data)` logic in
place. Do not parse in `toolToSidecarDefinition`, the serializer, or another
helper. The conversion step must never execute caller code.

Keep parsing before the `try` that maps execution failures. An unexpected throw
from a malformed schema should reach the request/transport error path instead of
being mislabeled as a normal validation failure.

### 3. Public documentation

The behavior is user-visible. Add this boundary after
`website/src/content/docs/docs/bindings.mdx:33`:

```md
Refinements and transforms run only on the host. AgentOS forwards their
input-side structural shape to the VM to build CLI flags, then runs the complete
Zod schema exactly once before `execute`. Async refinements and transforms are
supported; validation errors are returned to the calling command.
```

No example source needs to change. `examples/bindings/README.md:12` already
correctly says that Zod validates before the host handler executes.

## Exact tests to change

### Converter unit: `packages/core/tests/host-tools-zod.test.ts`

Remove only the custom-refinement case at lines 192-198 from “rejects other
lossy Zod constructs”. Retain every truly unsupported case.

Add parallel Zod 4 and Zod 3 tests containing:

- a constrained string with a custom refinement and transform;
- an object/root custom refinement;
- a pipe whose output adds a constraint absent from its input; and
- counters proving conversion does not execute effects.

Assert the generated JSON Schema contains the input types and input constraints,
does not contain transform output or the output-only pipe constraint, and leaves
all effect counters at zero. Use one helper assertion for the expected
structural schema so Zod 3/4 parity is obvious.

Also add a Zod 3 preprocess regression:

```ts
expect(() =>
	zodToJsonSchema(
		z3.object({
			value: z3.preprocess((value) => Number(value), z3.number()),
		}),
	),
).toThrow(/preprocess raw input cannot be represented structurally/);
```

This prevents a future broad `effects` allowance from introducing an early
sidecar rejection of input the complete Zod schema accepts.

Before the fix, the new positive cases throw
`HostToolSchemaConversionError`. After the fix, both converter paths return the
same structural input contract.

### One authoritative parse:
`packages/core/tests/toolkit-permissions.test.ts`

Replace the monkey-patch in the test at lines 420-459 with a real schema before
VM creation. Use an asynchronous refinement so the test proves the
`safeParseAsync` change, plus a non-idempotent transform so a second parse is
observable:

```ts
const inputSchema = z
	.object({ value: z.number() })
	.refine(async ({ value }) => value > 0, "value must be positive")
	.transform(({ value }) => {
		transformCount += 1;
		return { value: value + 1 };
	});
```

Register that schema directly. For `{ value: 1 }`, assert registration succeeds,
the transform count is one, and `execute` receives/returns `{ value: 2 }`. Then
invoke the captured handler with `{ value: -1 }`; assert the response contains
`value must be positive`, `execute` is not called again, and the transform count
does not increase.

Do not alter the permission expectations elsewhere in this file; they belong to
Item 62.

### Real sidecar composition:
`packages/core/tests/sidecar-tool-dispatch.test.ts`

Add a dedicated CLI-backed host tool with an asynchronous positive-number
refinement and a transform from `{ value }` to `{ value: value + 1 }`. Invoke it
through `agentos-<toolkit> <tool> --value 3`, not through a captured handler.
Assert:

1. registration succeeds and the sidecar exposes/parses `--value` as a number;
2. the command exits zero;
3. `execute` receives `{ value: 4 }` (zero transforms would yield 3 and two
   transforms would yield 5); and
4. the CLI JSON envelope returns that result.

Run the command again with `--value -1`. It is structurally valid, so it must
reach the host; assert `execute` is skipped, the command exits nonzero, and
stderr contains the custom refinement message. Keep the existing missing-flag
test, which proves sidecar structural rejection still occurs before dispatch.

No Zod behavior test should move into Rust: this item does not move behavior out
of TypeScript. Existing Rust tests remain the owner of generic structural flag
parsing and pre-dispatch rejection.

## Risks and dependencies

- **Item 25 / exactly one parse:** do not restore a second tool dispatcher or
  parse. The real transform test replaces Item 25's monkey-patch workaround.
- **Item 49 / dependency cleanup:** `zod-to-json-schema` remains required for
  Zod 3.
- **Item 62 / permissions:** `toolkit-permissions.test.ts` contains stale
  permission-model assertions; do not mix those edits into Item 61.
- **Async effects:** synchronous `safeParse` throws after encountering a Promise;
  `safeParseAsync` is required even when `execute` is already async.
- **Under-validation is intentional:** the sidecar may accept structurally valid
  input later rejected by a custom refinement. The host Zod parse is
  authoritative.
- **Over-validation is not intentional:** never forward post-transform or
  output-pipe constraints as accepted input semantics.
- **Coercion/preprocess limitation:** Zod coercion and preprocessing can accept
  raw types broader than their generated JSON Schema. Preprocess remains
  explicitly rejected in this bounded fix. Do not claim Item 61 solves every
  possible input coercion; that needs a separate permissive structural-schema
  design and sidecar CLI behavior decision.
- **Examples remain raw inputs:** do not transform `tool.examples` during
  registration. `packages/core/src/host-tools.ts:7-24` currently types examples
  with the parsed `ZodType` output generic even though the sidecar consumes raw
  input examples. That mismatch becomes visible only for shape-changing
  transforms; avoid broadening Item 61 with a public generic redesign, keep its
  tests example-free, and track raw-vs-parsed example typing separately.
- **Security:** every raw callback, including one originating from sidecar CLI
  parsing, must still pass the complete original Zod schema before `execute`.

## Dedicated revision and bounded diff

Implement Item 61 in one new child `jj` revision stacked on the preceding item,
as required by `docs/thin-client-migration.md:25-31`. Expected paths:

```text
packages/core/src/host-tools-zod.ts
packages/core/src/agent-os.ts
packages/core/tests/host-tools-zod.test.ts
packages/core/tests/toolkit-permissions.test.ts
packages/core/tests/sidecar-tool-dispatch.test.ts
website/src/content/docs/docs/bindings.mdx
docs/thin-client-migration.md       # checklist/status only after validation
```

No Rust SDK, sidecar, protocol, generated codec, package dependency, actor, or
public HostTool type should change.

Suggested revision description:

```text
fix(tools): preserve full Zod host behavior
```

## Before/after validation

Focused red/green tests:

```sh
pnpm --dir packages/core exec vitest run tests/host-tools-zod.test.ts
pnpm --dir packages/core exec vitest run tests/toolkit-permissions.test.ts
pnpm --dir packages/core exec vitest run tests/sidecar-tool-dispatch.test.ts
```

Affected package and documentation gates:

```sh
pnpm --dir packages/core check-types
pnpm --dir website build
git diff --check
```

Item 61 is complete only when both Zod versions emit input-side structural
schemas without running effects, transformed/refined tools register, async
refinements are awaited, a real sidecar CLI command composes with exactly one
host transform, refinement-invalid input never reaches `execute`, genuinely
unsupported shapes still fail with typed paths, the docs state the ownership
boundary, and the Item 61 tracker row/checklist are marked done in the dedicated
revision.
