use agentos_sidecar_protocol::protocol::RegisterHostCallbacksRequest;
use serde_json::Value;
use std::collections::BTreeMap;
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
    if toolkits.is_empty() {
        return Vec::new();
    }
    let mut commands = Vec::with_capacity(toolkits.len() + 1);
    commands.push(String::from("agentos"));
    for toolkit_name in toolkits.keys() {
        commands.push(format!("agentos-{toolkit_name}"));
    }
    commands
}

/// Render the sidecar-owned CLI reference injected into ACP agent prompts.
/// Native and browser adapters share this formatter so registered host tools
/// are described identically on both runtimes.
pub fn build_host_tool_reference(
    toolkits: &BTreeMap<String, RegisterHostCallbacksRequest>,
) -> Result<String, ToolRegistrationError> {
    if toolkits.is_empty() {
        return Ok(String::new());
    }

    let mut lines = vec![
        String::from("## Available Host Tools"),
        String::new(),
        String::from("Run `agentos list-tools` to see all available tools."),
        String::new(),
    ];
    for (toolkit_name, toolkit) in toolkits {
        lines.extend([
            format!("### {toolkit_name}"),
            String::new(),
            toolkit.description.clone(),
            String::new(),
        ]);
        for (tool_name, tool) in &toolkit.callbacks {
            let schema = serde_json::from_str::<Value>(&tool.input_schema).map_err(|error| {
                ToolRegistrationError::InvalidState(format!(
                    "registered tool {toolkit_name}:{tool_name} has an invalid input schema: {error}"
                ))
            })?;
            let signature = describe_tool_flags_payload(&schema)
                .iter()
                .filter_map(|flag| {
                    let name = flag.get("name")?.as_str()?;
                    let value_type = flag.get("type")?.as_str()?;
                    let required = flag.get("required")?.as_bool()?;
                    Some(if required {
                        format!("{name} <{value_type}>")
                    } else {
                        format!("[{name} <{value_type}>]")
                    })
                })
                .collect::<Vec<_>>()
                .join(" ");
            let suffix = (!signature.is_empty())
                .then(|| format!(" {signature}"))
                .unwrap_or_default();
            lines.push(format!(
                "- `agentos-{toolkit_name} {tool_name}{suffix}` — {}",
                tool.description
            ));
        }
        lines.push(String::new());

        let tools_with_examples = toolkit
            .callbacks
            .iter()
            .filter(|(_, tool)| !tool.examples.is_empty())
            .collect::<Vec<_>>();
        if !tools_with_examples.is_empty() {
            lines.extend([String::from("**Examples:**"), String::new()]);
            for (tool_name, tool) in tools_with_examples {
                for example in &tool.examples {
                    let input =
                        serde_json::from_str::<Value>(&example.input).map_err(|error| {
                            ToolRegistrationError::InvalidState(format!(
                                "registered tool {toolkit_name}:{tool_name} has an invalid example input: {error}"
                            ))
                        })?;
                    let arguments = tool_input_to_flags(&input);
                    let suffix = (!arguments.is_empty())
                        .then(|| format!(" {arguments}"))
                        .unwrap_or_default();
                    lines.push(format!(
                        "- {}: `agentos-{toolkit_name} {tool_name}{suffix}`",
                        example.description
                    ));
                }
            }
            lines.push(String::new());
        }
        lines.extend([
            format!("Run `agentos-{toolkit_name} <tool> --help` for details."),
            String::new(),
        ]);
    }
    Ok(lines.join("\n"))
}

fn describe_tool_flags_payload(schema: &Value) -> Vec<Value> {
    let properties = schema
        .get("properties")
        .and_then(Value::as_object)
        .cloned()
        .unwrap_or_default();
    let required = schema
        .get("required")
        .and_then(Value::as_array)
        .map(|items| {
            items
                .iter()
                .filter_map(Value::as_str)
                .collect::<std::collections::BTreeSet<_>>()
        })
        .unwrap_or_default();
    properties
        .iter()
        .map(|(field_name, field_schema)| {
            serde_json::json!({
                "name": format!("--{}", camel_to_kebab(field_name)),
                "type": describe_tool_flag_type(field_schema),
                "required": required.contains(field_name.as_str()),
            })
        })
        .collect()
}

fn describe_tool_flag_type(schema: &Value) -> String {
    match json_schema_type(schema) {
        Some("array") => format!(
            "{}[]",
            schema
                .get("items")
                .and_then(json_schema_type)
                .unwrap_or("string")
        ),
        Some("string") => schema
            .get("enum")
            .and_then(Value::as_array)
            .map(|values| values.iter().filter_map(Value::as_str).collect::<Vec<_>>())
            .filter(|values| !values.is_empty())
            .map(|values| values.join("|"))
            .unwrap_or_else(|| String::from("string")),
        Some(other) => other.to_owned(),
        None => String::from("string"),
    }
}

fn json_schema_type(schema: &Value) -> Option<&str> {
    schema.get("type").and_then(Value::as_str)
}

fn camel_to_kebab(value: &str) -> String {
    let mut output = String::with_capacity(value.len());
    for (index, ch) in value.chars().enumerate() {
        if ch.is_ascii_uppercase() {
            if index > 0 {
                output.push('-');
            }
            output.push(ch.to_ascii_lowercase());
        } else {
            output.push(ch);
        }
    }
    output
}

fn tool_input_to_flags(input: &Value) -> String {
    let Some(input) = input.as_object() else {
        return String::new();
    };
    input
        .iter()
        .flat_map(|(key, value)| {
            let flag = format!("--{}", camel_to_kebab(key));
            match value {
                Value::Bool(true) => vec![flag],
                Value::Bool(false) => vec![format!("--no-{}", camel_to_kebab(key))],
                Value::Array(items) => items
                    .iter()
                    .map(|item| format!("{flag} {}", tool_cli_string(item)))
                    .collect(),
                _ => vec![format!("{flag} {}", tool_cli_string(value))],
            }
        })
        .collect::<Vec<_>>()
        .join(" ")
}

fn tool_cli_string(value: &Value) -> String {
    match value {
        Value::String(value) => value.clone(),
        other => other.to_string(),
    }
}

pub fn toolkit_command_name(toolkit_name: &str) -> String {
    format!("agentos-{toolkit_name}")
}

pub fn is_registry_command(command_name: &str) -> bool {
    command_name == "agentos"
}

pub fn toolkit_name_for_command<'a>(
    toolkits: &'a BTreeMap<String, RegisterHostCallbacksRequest>,
    command_name: &str,
) -> Option<&'a str> {
    toolkits.keys().find_map(|toolkit_name| {
        if toolkit_command_name(toolkit_name) == command_name {
            Some(toolkit_name.as_str())
        } else {
            None
        }
    })
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
