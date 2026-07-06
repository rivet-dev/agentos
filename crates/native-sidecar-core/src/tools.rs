use agentos_sidecar_protocol::protocol::RegisterHostCallbacksRequest;
use serde_json::Value;
use std::collections::{BTreeMap, BTreeSet};
use std::error::Error;
use std::fmt;

pub const DEFAULT_TOOL_TIMEOUT_MS: u64 = 30_000;
pub const MAX_TOOL_TIMEOUT_MS: u64 = 300_000;
pub const MAX_REGISTERED_TOOLKITS: usize = 64;
pub const MAX_REGISTERED_TOOLS_PER_VM: usize = 256;
pub const MAX_TOOLS_PER_TOOLKIT: usize = 64;
pub const MAX_TOOLKIT_NAME_LENGTH: usize = 64;
pub const MAX_TOOL_NAME_LENGTH: usize = 64;
pub const MAX_TOOL_DESCRIPTION_LENGTH: usize = 200;
pub const MAX_TOOL_SCHEMA_BYTES: usize = 16 * 1024;
pub const MAX_TOOL_SCHEMA_DEPTH: usize = 32;
pub const MAX_TOOL_EXAMPLES_PER_TOOL: usize = 16;
pub const MAX_TOOL_EXAMPLE_INPUT_BYTES: usize = 4 * 1024;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ToolRegistrationError {
    InvalidState(String),
    Conflict(String),
}

impl fmt::Display for ToolRegistrationError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidState(message) | Self::Conflict(message) => f.write_str(message),
        }
    }
}

impl Error for ToolRegistrationError {}

pub fn validate_toolkit_registration(
    payload: &RegisterHostCallbacksRequest,
) -> Result<(), ToolRegistrationError> {
    validate_toolkit_name(&payload.name)?;
    if payload.description.is_empty() {
        return Err(ToolRegistrationError::InvalidState(format!(
            "toolkit {} is missing a description",
            payload.name
        )));
    }
    validate_description_length(
        &format!("Toolkit \"{}\"", payload.name),
        &payload.description,
    )?;
    validate_command_aliases("command alias", &payload.command_aliases)?;
    validate_command_aliases("registry command alias", &payload.registry_command_aliases)?;
    for alias in &payload.command_aliases {
        if payload.registry_command_aliases.contains(alias) {
            return Err(ToolRegistrationError::InvalidState(format!(
                "host callback command alias must not also be a registry command alias: {alias}"
            )));
        }
    }
    if payload.callbacks.is_empty() {
        return Err(ToolRegistrationError::InvalidState(format!(
            "toolkit {} must define at least one tool",
            payload.name
        )));
    }
    if payload.callbacks.len() > MAX_TOOLS_PER_TOOLKIT {
        return Err(ToolRegistrationError::InvalidState(format!(
            "toolkit {} defines {} tools, max is {MAX_TOOLS_PER_TOOLKIT}",
            payload.name,
            payload.callbacks.len()
        )));
    }
    for (tool_name, tool) in &payload.callbacks {
        validate_tool_name(tool_name)?;
        if tool.description.is_empty() {
            return Err(ToolRegistrationError::InvalidState(format!(
                "tool {} in toolkit {} is missing a description",
                tool_name, payload.name
            )));
        }
        validate_description_length(
            &format!("Tool \"{}/{}\"", payload.name, tool_name),
            &tool.description,
        )?;
        let tool_input_schema: Value =
            serde_json::from_str(&tool.input_schema).map_err(|error| {
                ToolRegistrationError::InvalidState(format!(
                    "Tool \"{}/{}\" input schema is invalid JSON: {error}",
                    payload.name, tool_name
                ))
            })?;
        validate_tool_schema_shape(
            &format!("Tool \"{}/{}\" input schema", payload.name, tool_name),
            &tool_input_schema,
        )?;
        if let Some(timeout_ms) = tool.timeout_ms {
            if timeout_ms > MAX_TOOL_TIMEOUT_MS {
                return Err(ToolRegistrationError::InvalidState(format!(
                    "Tool \"{}/{}\" timeout is {timeout_ms}ms, max is {MAX_TOOL_TIMEOUT_MS}ms",
                    payload.name, tool_name
                )));
            }
        }
        if tool.examples.len() > MAX_TOOL_EXAMPLES_PER_TOOL {
            return Err(ToolRegistrationError::InvalidState(format!(
                "Tool \"{}/{}\" defines {} examples, max is {MAX_TOOL_EXAMPLES_PER_TOOL}",
                payload.name,
                tool_name,
                tool.examples.len()
            )));
        }
        for (index, example) in tool.examples.iter().enumerate() {
            validate_description_length(
                &format!("Tool \"{}/{}\" example {index}", payload.name, tool_name),
                &example.description,
            )?;
            let example_input: Value = serde_json::from_str(&example.input).map_err(|error| {
                ToolRegistrationError::InvalidState(format!(
                    "Tool \"{}/{}\" example {index} input is invalid JSON: {error}",
                    payload.name, tool_name
                ))
            })?;
            validate_json_byte_length(
                &format!(
                    "Tool \"{}/{}\" example {index} input",
                    payload.name, tool_name
                ),
                &example_input,
                MAX_TOOL_EXAMPLE_INPUT_BYTES,
            )?;
        }
    }
    Ok(())
}

pub fn ensure_toolkit_name_available(
    toolkits: &BTreeMap<String, RegisterHostCallbacksRequest>,
    toolkit_name: &str,
) -> Result<(), ToolRegistrationError> {
    if toolkits.contains_key(toolkit_name) {
        return Err(ToolRegistrationError::Conflict(format!(
            "toolkit already registered: {toolkit_name}"
        )));
    }
    Ok(())
}

pub fn ensure_command_aliases_available(
    toolkits: &BTreeMap<String, RegisterHostCallbacksRequest>,
    payload: &RegisterHostCallbacksRequest,
) -> Result<(), ToolRegistrationError> {
    let requested_command_aliases = payload.command_aliases.iter().collect::<BTreeSet<_>>();
    let requested_registry_aliases = payload
        .registry_command_aliases
        .iter()
        .collect::<BTreeSet<_>>();
    for toolkit in toolkits.values() {
        for alias in &toolkit.command_aliases {
            if requested_command_aliases.contains(alias)
                || requested_registry_aliases.contains(alias)
            {
                return Err(ToolRegistrationError::Conflict(format!(
                    "host callback command alias already registered: {alias}"
                )));
            }
        }
        for alias in &toolkit.registry_command_aliases {
            if requested_command_aliases.contains(alias) {
                return Err(ToolRegistrationError::Conflict(format!(
                    "host callback command alias already registered: {alias}"
                )));
            }
        }
    }
    Ok(())
}

pub fn ensure_toolkit_registry_capacity(
    toolkits: &BTreeMap<String, RegisterHostCallbacksRequest>,
    payload: &RegisterHostCallbacksRequest,
) -> Result<(), ToolRegistrationError> {
    if toolkits.len() >= MAX_REGISTERED_TOOLKITS {
        return Err(ToolRegistrationError::InvalidState(format!(
            "VM already has {} registered toolkits, max is {MAX_REGISTERED_TOOLKITS}",
            toolkits.len()
        )));
    }

    let registered_tools = toolkits
        .values()
        .map(|toolkit| toolkit.callbacks.len())
        .sum::<usize>();
    let total_tools = registered_tools
        .checked_add(payload.callbacks.len())
        .ok_or_else(|| {
            ToolRegistrationError::InvalidState(String::from(
                "registered host callback count overflow",
            ))
        })?;
    if total_tools > MAX_REGISTERED_TOOLS_PER_VM {
        return Err(ToolRegistrationError::InvalidState(format!(
            "VM would have {total_tools} registered host callbacks, max is {MAX_REGISTERED_TOOLS_PER_VM}"
        )));
    }

    Ok(())
}

pub fn registered_tool_command_names(
    toolkits: &BTreeMap<String, RegisterHostCallbacksRequest>,
) -> Vec<String> {
    let mut seen = BTreeSet::new();
    let mut commands = Vec::new();
    for toolkit in toolkits.values() {
        for alias in toolkit
            .registry_command_aliases
            .iter()
            .chain(toolkit.command_aliases.iter())
        {
            if seen.insert(alias.clone()) {
                commands.push(alias.clone());
            }
        }
    }
    commands
}

fn validate_toolkit_name(name: &str) -> Result<(), ToolRegistrationError> {
    if name.len() > MAX_TOOLKIT_NAME_LENGTH {
        return Err(ToolRegistrationError::InvalidState(format!(
            "invalid toolkit name {name}; max length is {MAX_TOOLKIT_NAME_LENGTH}"
        )));
    }
    if name.is_empty()
        || !name
            .chars()
            .all(|ch| ch.is_ascii_lowercase() || ch.is_ascii_digit() || ch == '-')
    {
        return Err(ToolRegistrationError::InvalidState(format!(
            "invalid toolkit name {name}; expected lowercase alphanumeric characters plus hyphens"
        )));
    }
    Ok(())
}

fn validate_tool_name(name: &str) -> Result<(), ToolRegistrationError> {
    if name.len() > MAX_TOOL_NAME_LENGTH {
        return Err(ToolRegistrationError::InvalidState(format!(
            "invalid tool name {name}; max length is {MAX_TOOL_NAME_LENGTH}"
        )));
    }
    if name.is_empty()
        || !name
            .chars()
            .all(|ch| ch.is_ascii_lowercase() || ch.is_ascii_digit() || ch == '-')
    {
        return Err(ToolRegistrationError::InvalidState(format!(
            "invalid tool name {name}; expected lowercase alphanumeric characters plus hyphens"
        )));
    }
    Ok(())
}

fn validate_command_aliases(label: &str, aliases: &[String]) -> Result<(), ToolRegistrationError> {
    let mut seen = BTreeSet::new();
    for alias in aliases {
        validate_command_alias(label, alias)?;
        if !seen.insert(alias) {
            return Err(ToolRegistrationError::InvalidState(format!(
                "duplicate host callback {label}: {alias}"
            )));
        }
    }
    Ok(())
}

fn validate_command_alias(label: &str, alias: &str) -> Result<(), ToolRegistrationError> {
    if alias.is_empty()
        || alias == "."
        || alias == ".."
        || alias.contains('/')
        || alias.contains('\0')
    {
        return Err(ToolRegistrationError::InvalidState(format!(
            "invalid host callback {label}: {alias:?}"
        )));
    }
    Ok(())
}

fn validate_description_length(
    label: &str,
    description: &str,
) -> Result<(), ToolRegistrationError> {
    if description.len() > MAX_TOOL_DESCRIPTION_LENGTH {
        return Err(ToolRegistrationError::InvalidState(format!(
            "{label} description is {} characters, max is {MAX_TOOL_DESCRIPTION_LENGTH}",
            description.len()
        )));
    }
    Ok(())
}

fn validate_tool_schema_shape(label: &str, schema: &Value) -> Result<(), ToolRegistrationError> {
    validate_json_byte_length(label, schema, MAX_TOOL_SCHEMA_BYTES)?;
    validate_json_depth(label, schema, 0)
}

fn validate_json_byte_length(
    label: &str,
    value: &Value,
    limit: usize,
) -> Result<(), ToolRegistrationError> {
    let length = serde_json::to_vec(value)
        .map_err(|error| {
            ToolRegistrationError::InvalidState(format!("{label} is invalid JSON: {error}"))
        })?
        .len();
    if length > limit {
        return Err(ToolRegistrationError::InvalidState(format!(
            "{label} is {length} bytes, max is {limit}"
        )));
    }
    Ok(())
}

fn validate_json_depth(
    label: &str,
    value: &Value,
    depth: usize,
) -> Result<(), ToolRegistrationError> {
    if depth > MAX_TOOL_SCHEMA_DEPTH {
        return Err(ToolRegistrationError::InvalidState(format!(
            "{label} exceeds max JSON depth {MAX_TOOL_SCHEMA_DEPTH}"
        )));
    }

    match value {
        Value::Null | Value::Bool(_) | Value::Number(_) | Value::String(_) => Ok(()),
        Value::Array(values) => {
            for value in values {
                validate_json_depth(label, value, depth + 1)?;
            }
            Ok(())
        }
        Value::Object(object) => {
            for value in object.values() {
                validate_json_depth(label, value, depth + 1)?;
            }
            Ok(())
        }
    }
}
