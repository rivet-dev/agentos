#![forbid(unsafe_code)]

//! Agent OS ACP extension protocol types.

use std::collections::HashMap;

use generated::v1::{AcpCreateSessionRequest, AcpResumeSessionRequest, AcpRuntimeKind};
use serde_json::Value;

pub mod generated;

pub const ACP_EXTENSION_NAMESPACE: &str = "dev.rivet.agent-os.acp";
pub const PROTOCOL_NAME: &str = "agentos-acp";
pub const PROTOCOL_VERSION: u16 = 1;

pub const DEFAULT_ACP_PROTOCOL_VERSION: i32 = 1;
pub const DEFAULT_ACP_CLIENT_CAPABILITIES: &str =
    "{\"fs\":{\"readTextFile\":true,\"writeTextFile\":true},\"terminal\":true}";
pub const DEFAULT_ACP_MCP_SERVERS: &str = "[]";
pub const DEFAULT_ACP_CWD: &str = "/workspace";
pub const ACP_PROMPT_TEXT_LIMIT_BYTES: usize = 16 * 1024 * 1024;
pub const ACP_PROMPT_CHUNK_LIMIT: usize = 262_144;

/// Assemble VM-scoped and session-scoped caller instructions at the ACP
/// enforcement point. Clients forward each explicit input without constructing
/// an adapter prompt themselves.
pub fn combine_additional_instructions(
    base: Option<&str>,
    session: Option<&str>,
) -> Option<String> {
    let combined = [base, session]
        .into_iter()
        .flatten()
        .map(str::trim)
        .filter(|part| !part.is_empty())
        .collect::<Vec<_>>()
        .join("\n\n");
    (!combined.is_empty()).then_some(combined)
}

#[derive(Debug, Default)]
pub struct AcpPromptTextAccumulator {
    text: String,
    bytes: usize,
    chunks: usize,
    warned: bool,
}

impl AcpPromptTextAccumulator {
    /// Consume one adapter notification. Returns `true` once, when the capture
    /// first crosses 80% of either bound, so the owning sidecar can emit a
    /// host-visible warning.
    pub fn push_notification(&mut self, notification: &Value) -> Result<bool, String> {
        let Some(chunk) = notification
            .get("params")
            .and_then(|params| params.get("update"))
            .filter(|update| {
                update.get("sessionUpdate").and_then(Value::as_str) == Some("agent_message_chunk")
            })
            .and_then(|update| update.get("content"))
            .and_then(|content| content.get("text"))
            .and_then(Value::as_str)
        else {
            return Ok(false);
        };
        if self.chunks >= ACP_PROMPT_CHUNK_LIMIT {
            return Err(format!(
                "ACP prompt text chunk limit exceeded: at most {ACP_PROMPT_CHUNK_LIMIT} chunks can be captured"
            ));
        }
        let next_len = self.bytes.checked_add(chunk.len()).ok_or_else(|| {
            format!("ACP prompt text exceeds the {ACP_PROMPT_TEXT_LIMIT_BYTES}-byte capture limit")
        })?;
        if next_len > ACP_PROMPT_TEXT_LIMIT_BYTES {
            return Err(format!(
                "ACP prompt text is {next_len} bytes, limit is {ACP_PROMPT_TEXT_LIMIT_BYTES}; reduce agent output"
            ));
        }
        self.text.push_str(chunk);
        self.bytes = next_len;
        self.chunks += 1;
        let near_limit = next_len.saturating_mul(100)
            >= ACP_PROMPT_TEXT_LIMIT_BYTES.saturating_mul(80)
            || self.chunks.saturating_mul(100) >= ACP_PROMPT_CHUNK_LIMIT.saturating_mul(80);
        if near_limit && !self.warned {
            self.warned = true;
            Ok(true)
        } else {
            Ok(false)
        }
    }

    pub fn into_text(self) -> String {
        self.text
    }
}

#[cfg(test)]
mod prompt_text_tests {
    use super::*;
    use serde_json::json;

    fn chunk(text: &str) -> Value {
        json!({
            "method": "session/update",
            "params": {
                "update": {
                    "sessionUpdate": "agent_message_chunk",
                    "content": { "text": text }
                }
            }
        })
    }

    #[test]
    fn prompt_text_limits_fail_at_the_shared_sidecar_boundary() {
        let mut bytes = AcpPromptTextAccumulator {
            bytes: ACP_PROMPT_TEXT_LIMIT_BYTES,
            ..AcpPromptTextAccumulator::default()
        };
        assert!(bytes
            .push_notification(&chunk("x"))
            .expect_err("byte limit")
            .contains("reduce agent output"));

        let mut chunks = AcpPromptTextAccumulator {
            chunks: ACP_PROMPT_CHUNK_LIMIT,
            ..AcpPromptTextAccumulator::default()
        };
        assert!(chunks
            .push_notification(&chunk("x"))
            .expect_err("chunk limit")
            .contains("chunk limit exceeded"));
    }

    #[test]
    fn prompt_text_near_limit_warning_is_edge_triggered() {
        let warning_threshold = ACP_PROMPT_TEXT_LIMIT_BYTES.saturating_mul(80).div_ceil(100);
        let mut capture = AcpPromptTextAccumulator {
            bytes: warning_threshold - 1,
            ..AcpPromptTextAccumulator::default()
        };
        assert!(capture.push_notification(&chunk("x")).expect("cross limit"));
        assert!(!capture
            .push_notification(&chunk("x"))
            .expect("remain near limit"));
    }

    #[test]
    fn additional_instructions_are_assembled_only_in_the_acp_layer() {
        assert_eq!(
            combine_additional_instructions(Some(" base "), Some("session")),
            Some(String::from("base\n\nsession"))
        );
        assert_eq!(combine_additional_instructions(Some("  "), None), None);
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AcpConfigSelection {
    pub config_id: String,
    pub read_only: bool,
}

/// Resolve an adapter-reported configuration category without involving an SDK.
/// Missing categories retain ACP's permissive forwarding behavior by using the
/// category itself as the config id.
pub fn select_config_by_category(
    config_options: &[String],
    category: &str,
) -> Result<AcpConfigSelection, String> {
    for (index, option) in config_options.iter().enumerate() {
        let value: Value = serde_json::from_str(option)
            .map_err(|error| format!("malformed ACP config option {index}: {error}"))?;
        let object = value
            .as_object()
            .ok_or_else(|| format!("ACP config option {index} must be an object"))?;
        if object.get("category").and_then(Value::as_str) != Some(category) {
            continue;
        }
        let config_id = match object.get("id") {
            Some(Value::String(id)) if !id.is_empty() => id.clone(),
            Some(Value::String(_)) | None => category.to_string(),
            Some(_) => return Err(format!("ACP config option {index} id must be a string")),
        };
        let read_only = match object.get("readOnly") {
            Some(Value::Bool(read_only)) => *read_only,
            None => false,
            Some(_) => {
                return Err(format!(
                    "ACP config option {index} readOnly must be a boolean"
                ))
            }
        };
        return Ok(AcpConfigSelection {
            config_id,
            read_only,
        });
    }
    Ok(AcpConfigSelection {
        config_id: category.to_string(),
        read_only: false,
    })
}

pub fn read_only_config_message(agent_type: &str, category: &str) -> String {
    if agent_type == "opencode" && category == "model" {
        String::from(
            "OpenCode reports available models, but model switching must be configured before createSession() because ACP session/set_config_option is not implemented.",
        )
    } else {
        format!("The {category} config option is read-only for {agent_type} sessions.")
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolvedAcpCreateSessionRequest {
    pub agent_type: String,
    pub runtime: AcpRuntimeKind,
    pub cwd: String,
    pub args: Vec<String>,
    pub env: HashMap<String, String>,
    pub protocol_version: i32,
    pub client_capabilities: String,
    pub mcp_servers: String,
    pub skip_os_instructions: bool,
    pub additional_instructions: Option<String>,
}

impl From<AcpCreateSessionRequest> for ResolvedAcpCreateSessionRequest {
    fn from(request: AcpCreateSessionRequest) -> Self {
        Self {
            agent_type: request.agent_type,
            runtime: request.runtime.unwrap_or(AcpRuntimeKind::JavaScript),
            cwd: request.cwd.unwrap_or_else(|| String::from(DEFAULT_ACP_CWD)),
            args: request.args.unwrap_or_default(),
            env: request.env.unwrap_or_default(),
            protocol_version: request
                .protocol_version
                .unwrap_or(DEFAULT_ACP_PROTOCOL_VERSION),
            client_capabilities: request
                .client_capabilities
                .unwrap_or_else(|| String::from(DEFAULT_ACP_CLIENT_CAPABILITIES)),
            mcp_servers: request
                .mcp_servers
                .unwrap_or_else(|| String::from(DEFAULT_ACP_MCP_SERVERS)),
            skip_os_instructions: request.skip_os_instructions.unwrap_or(false),
            additional_instructions: request.additional_instructions,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolvedAcpResumeSessionRequest {
    pub session_id: String,
    pub agent_type: String,
    pub transcript_path: Option<String>,
    pub cwd: String,
    pub env: HashMap<String, String>,
}

impl From<AcpResumeSessionRequest> for ResolvedAcpResumeSessionRequest {
    fn from(request: AcpResumeSessionRequest) -> Self {
        Self {
            session_id: request.session_id,
            agent_type: request.agent_type,
            transcript_path: request.transcript_path,
            cwd: request.cwd.unwrap_or_else(|| String::from(DEFAULT_ACP_CWD)),
            env: request.env.unwrap_or_default(),
        }
    }
}
