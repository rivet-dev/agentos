//! End-to-end coverage for sidecar-owned system-prompt injection at `create_session`.
//!
//! The base prompt is no longer baked into a guest file (`/etc/agentos/instructions.md` is gone);
//! the Agent OS client passes create-time additions and generated tool docs to the wrapper sidecar,
//! which assembles them with the base prompt and injects the result into the launched adapter's argv
//! (`--append-system-prompt` for `pi`). This test resolves a tiny mock ACP adapter through the real
//! module-access path, launches a `pi` session, and asserts the adapter actually observed the
//! injected prompt in `process.argv`.

mod common;

use std::collections::BTreeMap;
use std::path::Path;
use std::sync::Arc;

use agentos_client::config::{
    AgentOsConfig, AgentOsSidecarConfig, FsPermissions, HostTool, PatternPermissions,
    PermissionMode, Permissions, ToolKit,
};
use agentos_client::{AgentOs, CreateSessionOptions};
use serde_json::json;
use uuid::Uuid;

/// A mock ACP adapter that answers `initialize` / `session/new` and echoes its own `process.argv`
/// (minus `node` + script path) in the initialize `agentInfo.argv` so the test can read it from the
/// session's agent info without depending on guest-to-kernel file sync timing.
const MOCK_ACP_ADAPTER: &str = r#"
let buffer = "";
process.stdin.resume();
process.stdin.on("data", (chunk) => {
  buffer += chunk instanceof Uint8Array ? new TextDecoder().decode(chunk) : String(chunk);
  while (true) {
    const idx = buffer.indexOf("\n");
    if (idx === -1) break;
    const line = buffer.slice(0, idx);
    buffer = buffer.slice(idx + 1);
    if (!line.trim()) continue;
    const msg = JSON.parse(line);
    if (msg.id === undefined) continue;
    let result;
    switch (msg.method) {
      case "initialize":
        result = {
          protocolVersion: 1,
          agentInfo: { name: "mock-acp", version: "1.0.0", argv: process.argv.slice(2) },
        };
        break;
      case "session/new":
        result = { sessionId: "__MOCK_SESSION_ID__" };
        break;
      default:
        process.stdout.write(
          JSON.stringify({ jsonrpc: "2.0", id: msg.id, error: { code: -32601, message: "Method not found" } }) + "\n",
        );
        continue;
    }
    process.stdout.write(JSON.stringify({ jsonrpc: "2.0", id: msg.id, result }) + "\n");
  }
});

setInterval(() => {}, 1000);
"#;

const ADDITIONAL_MARKER: &str = "rust-client-extra-instructions";

/// Allow-all permissions so the mock adapter can spawn, read its module-access bin, and write the
/// argv probe file.
fn allow_all_permissions() -> Permissions {
    Permissions {
        fs: Some(FsPermissions::Mode(PermissionMode::Allow)),
        network: Some(PatternPermissions::Mode(PermissionMode::Allow)),
        child_process: Some(PatternPermissions::Mode(PermissionMode::Allow)),
        process: Some(PatternPermissions::Mode(PermissionMode::Allow)),
        env: Some(PatternPermissions::Mode(PermissionMode::Allow)),
        binding: Some(PatternPermissions::Mode(PermissionMode::Allow)),
    }
}

/// Lay out a fake `node_modules/@agentos-software/pi` whose `bin` resolves to the mock adapter,
/// so the client's `resolve_package_bin("pi")` path projects it into the guest at
/// `/root/node_modules/@agentos-software/pi/adapter.mjs` and the sidecar launches it.
fn write_mock_pi_adapter(module_access_cwd: &std::path::Path) {
    let package_dir = module_access_cwd
        .join("node_modules")
        .join("@rivet-dev")
        .join("agentos-pi");
    std::fs::create_dir_all(&package_dir).expect("create mock adapter package dir");
    std::fs::write(
        package_dir.join("package.json"),
        r#"{ "name": "@agentos-software/pi", "version": "0.0.0", "bin": "./adapter.mjs" }"#,
    )
    .expect("write mock adapter package.json");
    let adapter = MOCK_ACP_ADAPTER.replace(
        "__MOCK_SESSION_ID__",
        &format!("mock-session-{}", Uuid::new_v4()),
    );
    std::fs::write(package_dir.join("adapter.mjs"), adapter)
        .expect("write mock adapter entrypoint");
}

async fn launch_pi_session_and_read_argv(options: CreateSessionOptions) -> Vec<String> {
    launch_pi_session_with_tools_and_read_argv(options, Vec::new()).await
}

async fn launch_pi_session_with_tools_and_read_argv(
    options: CreateSessionOptions,
    tool_kits: Vec<ToolKit>,
) -> Vec<String> {
    let module_access_dir =
        std::env::temp_dir().join(format!("agentos-client-os-instructions-{}", Uuid::new_v4()));
    write_mock_pi_adapter(&module_access_dir);

    let argv = run_session(&module_access_dir, options, tool_kits).await;

    std::fs::remove_dir_all(&module_access_dir).ok();
    argv
}

async fn run_session(
    module_access_dir: &Path,
    options: CreateSessionOptions,
    tool_kits: Vec<ToolKit>,
) -> Vec<String> {
    let os = AgentOs::create(AgentOsConfig {
        module_access_cwd: Some(module_access_dir.to_string_lossy().into_owned()),
        sidecar: Some(AgentOsSidecarConfig::Shared {
            pool: Some(format!("os-instructions-{}", Uuid::new_v4())),
        }),
        tool_kits,
        permissions: Some(allow_all_permissions()),
        ..Default::default()
    })
    .await
    .expect("create VM with module access for mock adapter");

    let session = os
        .create_session("pi", options)
        .await
        .expect("create pi session against mock adapter");

    let agent_info = os
        .get_session_agent_info(&session.session_id)
        .expect("mock adapter should report agent info");
    let argv: Vec<String> = serde_json::from_value(
        agent_info
            .extra
            .get("argv")
            .cloned()
            .expect("mock adapter should echo argv in agentInfo"),
    )
    .expect("argv probe is a JSON string array");

    os.shutdown().await.expect("shutdown VM");
    argv
}

fn injected_prompt(argv: &[String]) -> &str {
    let idx = argv
        .iter()
        .position(|arg| arg == "--append-system-prompt")
        .unwrap_or_else(|| panic!("argv should contain --append-system-prompt, got {argv:?}"));
    argv.get(idx + 1)
        .unwrap_or_else(|| panic!("--append-system-prompt should be followed by a value: {argv:?}"))
        .as_str()
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn create_session_injects_assembled_system_prompt() {
    if !common::sidecar_available() {
        panic!(
            "create_session_injects_assembled_system_prompt: sidecar binary is not built; build it with `cargo build -p agentos-sidecar`"
        );
    }
    common::ensure_sidecar_env();

    let argv = launch_pi_session_and_read_argv(CreateSessionOptions {
        additional_instructions: Some(ADDITIONAL_MARKER.to_string()),
        ..Default::default()
    })
    .await;

    let prompt = injected_prompt(&argv);
    assert!(
        prompt.contains("# agentOS"),
        "base OS instructions are injected: {prompt:?}"
    );
    assert!(
        prompt.contains(ADDITIONAL_MARKER),
        "create-time additional instructions are appended: {prompt:?}"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn create_session_injects_host_tool_reference_from_client_config() {
    if !common::sidecar_available() {
        panic!(
            "create_session_injects_host_tool_reference_from_client_config: sidecar binary is not built; build it with `cargo build -p agentos-sidecar`"
        );
    }
    common::ensure_sidecar_env();

    let argv = launch_pi_session_with_tools_and_read_argv(
        CreateSessionOptions::default(),
        vec![ToolKit {
            name: "weather".to_string(),
            description: "Weather lookup tools.".to_string(),
            tools: vec![HostTool {
                name: "forecast".to_string(),
                description: "Get a forecast.".to_string(),
                input_schema: json!({
                    "type": "object",
                    "properties": {
                        "zipCode": { "type": "string" },
                    },
                    "required": ["zipCode"],
                }),
                timeout_ms: None,
                execute: Arc::new(|_input| Box::pin(async { Ok(json!({ "ok": true })) })),
            }],
        }],
    )
    .await;

    let prompt = injected_prompt(&argv);
    assert!(
        prompt.contains("## Available Host Tools"),
        "client-generated tool reference is injected: {prompt:?}"
    );
    assert!(
        prompt.contains("`agentos-weather forecast --zip-code <string>`"),
        "tool reference includes CLI command and schema-derived flags: {prompt:?}"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn create_session_skip_os_instructions_drops_base_but_keeps_additional() {
    if !common::sidecar_available() {
        panic!(
            "create_session_skip_os_instructions_drops_base_but_keeps_additional: sidecar binary is not built; build it with `cargo build -p agentos-sidecar`"
        );
    }
    common::ensure_sidecar_env();

    let argv = launch_pi_session_and_read_argv(CreateSessionOptions {
        skip_os_instructions: true,
        additional_instructions: Some(ADDITIONAL_MARKER.to_string()),
        env: BTreeMap::new(),
        ..Default::default()
    })
    .await;

    let prompt = injected_prompt(&argv);
    assert!(
        !prompt.contains("# agentOS"),
        "skip_os_instructions drops the base prompt: {prompt:?}"
    );
    assert!(
        prompt.contains(ADDITIONAL_MARKER),
        "skip_os_instructions still injects additional instructions: {prompt:?}"
    );
}
