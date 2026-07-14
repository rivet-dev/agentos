# Item 35 research: complete Rust ACP results and fail malformed decoding

## Scope and conclusion

Item 35 is a focused Rust SDK boundary fix. The ACP protocol and sidecar already
carry the missing data; the Rust client discards it while rebuilding public
values. The same module also turns several semantic JSON decode failures into
`None` or a shorter vector by using `Value::is_object`, `.ok()`, and
`filter_map`.

The implementation should:

1. expose and forward `adapter_entrypoint` in Rust's public
   `AgentRegistryEntry`;
2. replace every lossy session-state conversion with all-or-error decoding;
3. report those failures through a dedicated `ClientError` variant that retains
   the field/index and `serde_json::Error`; and
4. make fields required by the public Rust/TypeScript types actually required
   during Rust deserialization instead of filling in empty defaults.

No protocol schema or generated protocol file needs to change. TypeScript
already forwards `adapterEntrypoint` and does not shorten a `configOptions`
array, so neither recorded defect requires a TypeScript production change.
Do not add a second TypeScript validator for parity; if stricter adapter-output
validation is later required across languages, it should be done once in the
shared ACP sidecar core.

## Exact current defects

All line numbers below are from Item 34's current working copy and may move by a
few lines when Item 34 is sealed.

### 1. `adapter_entrypoint` is on the wire but dropped by Rust

- `crates/agentos-protocol/protocol/agent_os_acp_v1.bare:46-50` defines
  `AcpAgentEntry { id, installed, adapterEntrypoint }`.
- The generated Rust `AcpAgentEntry` therefore already has
  `adapter_entrypoint`; the shared core fills it in at
  `crates/agentos-sidecar-core/src/engine.rs` in the
  `AcpListAgentsRequest` dispatch branch.
- TypeScript's public `AgentRegistryEntry` includes `adapterEntrypoint` at
  `packages/core/src/agent-os.ts:100-105`, and `listAgents()` copies the wire
  value at `packages/core/src/agent-os.ts:2197-2209`.
- Rust's public `AgentRegistryEntry` has only `id` and `installed` at
  `crates/client/src/session.rs:546-555`.
- Rust `AgentOs::list_agents()` explicitly rebuilds each entry with only those
  two fields at `crates/client/src/session.rs:1233-1249`.

This is not an agent-resolution or filesystem concern. The client should simply
retain the already-resolved guest entrypoint returned by the sidecar.

### 2. Session state is parsed without loss, then silently narrowed

`session_state_from_acp()` at `crates/client/src/session.rs:939-947` correctly
parses each `JsonUtf8` string into `serde_json::Value`, and
`parse_json_vec()` collects all values. The loss occurs later:

- `get_session_modes()` (`session.rs:1547-1552`) filters non-objects and uses
  `.ok()`, converting a malformed present value to `None`.
- `get_session_config_options()` (`session.rs:1579-1589`) uses `filter_map`, so
  one malformed entry is removed while the method returns apparent success with
  a shorter list.
- `get_session_capabilities()` (`session.rs:1593-1604`) filters non-objects and
  uses `.ok()`, converting malformed present capabilities to `None`.
- `get_session_agent_info()` (`session.rs:1608-1614`) does the same for agent
  info.

There is a second silent-normalization source in the public Rust structs:

- `SessionModeState` (`session.rs:641-651`) derives `Default` and has
  `#[serde(default)]` on required `currentModeId` and `availableModes`.
- `SessionConfigOption` (`session.rs:662-690`) has `#[serde(default)]` on the
  required `id`.

Those defaults turn malformed adapter output into invented valid-looking
values. `SessionMode.id`, `ConfigAllowedValue.id`, and `AgentInfo.name` are
already strict; optional fields are already strict when present.

A reliable real-sidecar repro is a `session/new` result containing:

```json
{
  "configOptions": [
    { "id": "valid", "category": "custom" },
    { "id": "bad", "category": "custom", "readOnly": "not-a-boolean" }
  ]
}
```

The shared sidecar accepts both objects because its model-category inspection
only needs the string `id`/`category` fields. Before Item 35, Rust returns a
one-element vector: the second entry fails `SessionConfigOption` deserialization
and `filter_map` silently removes it. After Item 35, the call must fail with a
typed error naming `configOptions[1]`.

## Recommended implementation

### `crates/client/src/error.rs`

Add a public typed variant; keep the actual `serde_json::Error` as the source so
callers can distinguish decode failures without parsing message text:

```rust
/// Trusted sidecar ACP JSON did not match the public Rust response type.
#[error("failed to decode ACP {context}: {source}")]
AcpDecode {
    context: String,
    #[source]
    source: serde_json::Error,
},
```

`context` must identify an exact field (`"modes"`, `"agentCapabilities"`,
`"agentInfo"`) or indexed entry (`"configOptions[1]"`). Do not reuse
`ClientError::Sidecar(String)`: that is the untyped behavior Item 35 is meant to
remove.

### `crates/client/src/session.rs`

1. Extend `AgentRegistryEntry`:

   ```rust
   #[serde(rename = "adapterEntrypoint")]
   pub adapter_entrypoint: String,
   ```

   Then copy `agent.adapter_entrypoint` in the `list_agents()` mapping and fix
   the stale comment claiming the result is only an id.

2. Import `serde::de::DeserializeOwned` and centralize conversion in small pure
   helpers rather than repeating `.ok()`:

   ```rust
   fn decode_acp_value<T: DeserializeOwned>(
       value: Value,
       context: impl Into<String>,
   ) -> Result<T, ClientError> {
       serde_json::from_value(value).map_err(|source| ClientError::AcpDecode {
           context: context.into(),
           source,
       })
   }

   fn decode_optional_acp_value<T: DeserializeOwned>(
       value: Option<Value>,
       context: &'static str,
   ) -> Result<Option<T>, ClientError> {
       value
           .map(|value| decode_acp_value(value, context))
           .transpose()
   }

   fn decode_acp_values<T: DeserializeOwned>(
       values: Vec<Value>,
       context: &'static str,
   ) -> Result<Vec<T>, ClientError> {
       values
           .into_iter()
           .enumerate()
           .map(|(index, value)| {
               decode_acp_value(value, format!("{context}[{index}]"))
           })
           .collect()
   }
   ```

3. Make getters all-or-error:

   - modes: `decode_optional_acp_value(state.modes, "modes")`;
   - config options: `decode_acp_values(state.config_options,
     "configOptions")`;
   - capabilities: decode the optional value first, then retain the existing
     documented normalization that an actually valid empty object means `None`;
   - agent info: `decode_optional_acp_value(state.agent_info, "agentInfo")`.

   There must be no `Value::is_object`, `.ok()`, or `filter_map` left in these
   four paths. Omitted optional wire fields remain `None`; a present malformed
   value is an error. The config list must never return fewer entries than the
   sidecar supplied.

4. Route syntax failures in `parse_optional_json()` and `parse_json_vec()`
   through the same `AcpDecode` variant. Make `parse_json_vec()` enumerate its
   inputs so invalid JSON is also labeled `configOptions[N]`. This gives one
   stable error contract for both invalid JSON text and JSON that does not match
   the typed public shape.

5. Remove silent required-field defaults:

   - remove `Default` from `SessionModeState` and remove `#[serde(default)]`
     from `current_mode_id`/`available_modes`;
   - remove `#[serde(default)]` from `SessionConfigOption.id`;
   - update the comments that currently document the lossy behavior.

   Repository search currently finds no construction through
   `SessionModeState::default()`, so this is not needed internally. Lockstep
   releases have no backward-compatibility constraint.

Keep the public return types. Changing these APIs to `Value` would preserve bad
data but would weaken the SDK and contradict the requested typed decode failure.

## Focused tests

### Required real-boundary coverage: `crates/client/tests/session_e2e.rs`

Split the mock package helper so a test can choose the adapter's
`configOptions`, or add a second dedicated malformed mock package. Preserve the
existing healthy create/prompt/close test.

Add these assertions/tests:

1. In the existing projected-agent assertion, find the exact mock entry and
   assert:

   ```rust
   assert_eq!(
       mock_agent.adapter_entrypoint,
       "/opt/agentos/bin/mock-agent-acp"
   );
   ```

   Against the Item 34 parent this test does not compile because the public Rust
   field is absent. After the fix it proves the value survived sidecar discovery,
   BARE transport, and the public client mapping.

2. Add
   `session_config_decode_rejects_malformed_entry_without_shortening`. Have its
   adapter return the two-entry example above, create the session, then assert
   `get_session_config_options()` returns an error downcastable to:

   ```rust
   ClientError::AcpDecode { context, .. }
       if context == "configOptions[1]"
   ```

   Against the Item 34 parent, the same call succeeds with only the first entry.
   Record that as the before-fix evidence in the tracking checklist. Close the
   session and shut down the VM even after capturing the expected error.

### Cheap deterministic unit coverage: `crates/client/src/session.rs`

Add pure helper tests for all four response categories:

- valid vectors preserve order and exact length;
- a malformed config entry fails at the exact index rather than shortening;
- malformed present modes/capabilities/agentInfo each return `AcpDecode`;
- omitted optional values remain `Ok(None)`;
- missing required `currentModeId`, `availableModes`, and config-option `id`
  fail rather than receiving empty defaults;
- valid capability `{}` remains the documented public `None` after successful
  decoding.

The real-boundary test is still required; unit tests alone would not catch the
missing public `adapter_entrypoint` mapping.

## Validation commands

Run from the repository root after Item 34 is sealed and Item 35 has its own
child `jj` revision:

```sh
cargo fmt --all -- --check
cargo check -p agentos-client
cargo test -p agentos-client --lib
cargo build -p agentos-sidecar
AGENTOS_SIDECAR_BIN="$PWD/target/debug/agentos-sidecar" \
  cargo test -p agentos-client --test session_e2e -- --nocapture
git diff --check
```

The e2e suite is not allowed to pass via a local skip when recording completion;
the test helper already fails if the sidecar binary is absent unless
`AGENT_OS_CLIENT_ALLOW_E2E_SKIPS` is explicitly set.

## Dedicated Item 35 revision scope

Expected paths in Item 35's stacked `jj` revision:

- `crates/client/src/error.rs`
- `crates/client/src/session.rs`
- `crates/client/tests/session_e2e.rs`
- `docs/thin-client-migration.md` (before/after/completion checklist and status)
- `docs/thin-client-research/item-35.md` if research notes are retained in the
  implementation stack

Do not include protocol schemas, generated TypeScript/Rust protocol files,
sidecar implementation files, `Cargo.lock`, or TypeScript production files for
this item. The wire already contains the field and JSON payloads needed for the
fix.

## Risks and review checks

- Adding a field to a public Rust struct and removing `Default` from a public
  type are source-breaking changes. This repository explicitly ships the
  protocol and clients in lockstep with no compatibility guarantee, so that is
  preferable to preserving silent data loss.
- Do not validate filesystem existence or resolve the adapter path in the
  client. The sidecar owns discovery; Rust only copies the returned string.
- Do not convert a malformed config entry into a warning plus partial success.
  The project rule is that failures propagate or are host-visible; partial
  success is precisely the bug.
- Ensure the indexed context comes from the original wire order. Filtering or
  sorting before decoding would make the error misleading.
- Preserve unknown extension fields where the Rust public type already has a
  flattened `extra` map (`SessionMode`, `PromptCapabilities`,
  `AgentCapabilities`, `AgentInfo`). Item 35 should not delete those maps.
- Keep valid `{}` capabilities -> `None`; that is a documented API normalization,
  not a decode failure. Only malformed present values must fail.
