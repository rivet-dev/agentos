use agentos_sidecar_protocol::protocol::RegisterHostCallbacksRequest;
use serde_json::Value;
use std::collections::{BTreeMap, BTreeSet};
use std::error::Error;
use std::fmt;

pub const DEFAULT_HOST_FUNCTION_TIMEOUT_MS: u64 = 30_000;
pub const MAX_HOST_FUNCTION_TIMEOUT_MS: u64 = 300_000;
pub const MAX_REGISTERED_HOST_FUNCTION_COLLECTIONS: usize = 64;
pub const MAX_REGISTERED_HOST_FUNCTIONS_PER_VM: usize = 256;
pub const MAX_HOST_FUNCTIONS_PER_COLLECTION: usize = 64;
pub const MAX_HOST_FUNCTION_COLLECTION_NAME_LENGTH: usize = 64;
pub const MAX_HOST_FUNCTION_NAME_LENGTH: usize = 64;
pub const MAX_HOST_FUNCTION_DESCRIPTION_LENGTH: usize = 200;
pub const MAX_HOST_FUNCTION_SCHEMA_BYTES: usize = 16 * 1024;
pub const MAX_HOST_FUNCTION_SCHEMA_DEPTH: usize = 32;
pub const MAX_EXAMPLES_PER_HOST_FUNCTION: usize = 16;
pub const MAX_HOST_FUNCTION_EXAMPLE_INPUT_BYTES: usize = 4 * 1024;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HostFunctionRegistrationError {
    InvalidState(String),
    Conflict(String),
}

impl fmt::Display for HostFunctionRegistrationError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidState(message) | Self::Conflict(message) => f.write_str(message),
        }
    }
}

impl Error for HostFunctionRegistrationError {}

pub fn validate_host_functions_registration(
    payload: &RegisterHostCallbacksRequest,
) -> Result<(), HostFunctionRegistrationError> {
    validate_collection_name(&payload.name)?;
    // Descriptions are optional: a collection has none of its own, and a
    // function's comes from its input schema, which need not carry one.
    validate_description_length(
        &format!("Host function collection \"{}\"", payload.name),
        &payload.description,
    )?;
    validate_command_aliases("command alias", &payload.command_aliases)?;
    validate_command_aliases("registry command alias", &payload.registry_command_aliases)?;
    for alias in &payload.command_aliases {
        if payload.registry_command_aliases.contains(alias) {
            return Err(HostFunctionRegistrationError::InvalidState(format!(
                "host callback command alias must not also be a registry command alias: {alias}"
            )));
        }
    }
    if payload.callbacks.is_empty() {
        return Err(HostFunctionRegistrationError::InvalidState(format!(
            "collection {} must define at least one host function",
            payload.name
        )));
    }
    if payload.callbacks.len() > MAX_HOST_FUNCTIONS_PER_COLLECTION {
        return Err(HostFunctionRegistrationError::InvalidState(format!(
            "collection {} defines {} host functions, max is {MAX_HOST_FUNCTIONS_PER_COLLECTION}",
            payload.name,
            payload.callbacks.len()
        )));
    }
    for (host_function_name, host_function) in &payload.callbacks {
        validate_host_function_name(host_function_name)?;
        validate_description_length(
            &format!("Host function \"{}/{}\"", payload.name, host_function_name),
            &host_function.description,
        )?;
        let host_function_input_schema: Value = serde_json::from_str(&host_function.input_schema)
            .map_err(|error| {
            HostFunctionRegistrationError::InvalidState(format!(
                "Host function \"{}/{}\" input schema is invalid JSON: {error}",
                payload.name, host_function_name
            ))
        })?;
        validate_host_function_schema_shape(
            &format!(
                "Host function \"{}/{}\" input schema",
                payload.name, host_function_name
            ),
            &host_function_input_schema,
        )?;
        if let Some(timeout_ms) = host_function.timeout_ms {
            if timeout_ms > MAX_HOST_FUNCTION_TIMEOUT_MS {
                return Err(HostFunctionRegistrationError::InvalidState(format!(
                    "Host function \"{}/{}\" timeout is {timeout_ms}ms, max is {MAX_HOST_FUNCTION_TIMEOUT_MS}ms",
                    payload.name, host_function_name
                )));
            }
        }
        if host_function.examples.len() > MAX_EXAMPLES_PER_HOST_FUNCTION {
            return Err(HostFunctionRegistrationError::InvalidState(format!(
                "Host function \"{}/{}\" defines {} examples, max is {MAX_EXAMPLES_PER_HOST_FUNCTION}",
                payload.name,
                host_function_name,
                host_function.examples.len()
            )));
        }
        for (index, example) in host_function.examples.iter().enumerate() {
            validate_description_length(
                &format!(
                    "Host function \"{}/{}\" example {index}",
                    payload.name, host_function_name
                ),
                &example.description,
            )?;
            let example_input: Value = serde_json::from_str(&example.input).map_err(|error| {
                HostFunctionRegistrationError::InvalidState(format!(
                    "Host function \"{}/{}\" example {index} input is invalid JSON: {error}",
                    payload.name, host_function_name
                ))
            })?;
            validate_json_byte_length(
                &format!(
                    "Host function \"{}/{}\" example {index} input",
                    payload.name, host_function_name
                ),
                &example_input,
                MAX_HOST_FUNCTION_EXAMPLE_INPUT_BYTES,
            )?;
        }
    }
    Ok(())
}

pub fn ensure_collection_name_available(
    host_functions: &BTreeMap<String, RegisterHostCallbacksRequest>,
    collection_name: &str,
) -> Result<(), HostFunctionRegistrationError> {
    if host_functions.contains_key(collection_name) {
        return Err(HostFunctionRegistrationError::Conflict(format!(
            "host function collection already registered: {collection_name}"
        )));
    }
    Ok(())
}

pub fn ensure_command_aliases_available(
    host_functions: &BTreeMap<String, RegisterHostCallbacksRequest>,
    payload: &RegisterHostCallbacksRequest,
) -> Result<(), HostFunctionRegistrationError> {
    let requested_command_aliases = payload.command_aliases.iter().collect::<BTreeSet<_>>();
    let requested_registry_aliases = payload
        .registry_command_aliases
        .iter()
        .collect::<BTreeSet<_>>();
    for collection in host_functions.values() {
        for alias in &collection.command_aliases {
            if requested_command_aliases.contains(alias)
                || requested_registry_aliases.contains(alias)
            {
                return Err(HostFunctionRegistrationError::Conflict(format!(
                    "host callback command alias already registered: {alias}"
                )));
            }
        }
        for alias in &collection.registry_command_aliases {
            if requested_command_aliases.contains(alias) {
                return Err(HostFunctionRegistrationError::Conflict(format!(
                    "host callback command alias already registered: {alias}"
                )));
            }
        }
    }
    Ok(())
}

pub fn ensure_host_function_registry_capacity(
    host_functions: &BTreeMap<String, RegisterHostCallbacksRequest>,
    payload: &RegisterHostCallbacksRequest,
) -> Result<(), HostFunctionRegistrationError> {
    if host_functions.len() >= MAX_REGISTERED_HOST_FUNCTION_COLLECTIONS {
        return Err(HostFunctionRegistrationError::InvalidState(format!(
            "VM already has {} registered host function collections, max is {MAX_REGISTERED_HOST_FUNCTION_COLLECTIONS}",
            host_functions.len()
        )));
    }

    let registered_host_functions = host_functions
        .values()
        .map(|collection| collection.callbacks.len())
        .sum::<usize>();
    let total_host_functions = registered_host_functions
        .checked_add(payload.callbacks.len())
        .ok_or_else(|| {
            HostFunctionRegistrationError::InvalidState(String::from(
                "registered host callback count overflow",
            ))
        })?;
    if total_host_functions > MAX_REGISTERED_HOST_FUNCTIONS_PER_VM {
        return Err(HostFunctionRegistrationError::InvalidState(format!(
            "VM would have {total_host_functions} registered host callbacks, max is {MAX_REGISTERED_HOST_FUNCTIONS_PER_VM}"
        )));
    }

    Ok(())
}

pub fn registered_host_function_command_names(
    host_functions: &BTreeMap<String, RegisterHostCallbacksRequest>,
) -> Vec<String> {
    let mut seen = BTreeSet::new();
    let mut commands = Vec::new();
    for collection in host_functions.values() {
        for alias in collection
            .registry_command_aliases
            .iter()
            .chain(collection.command_aliases.iter())
        {
            if seen.insert(alias.clone()) {
                commands.push(alias.clone());
            }
        }
    }
    commands
}

fn validate_collection_name(name: &str) -> Result<(), HostFunctionRegistrationError> {
    if name.len() > MAX_HOST_FUNCTION_COLLECTION_NAME_LENGTH {
        return Err(HostFunctionRegistrationError::InvalidState(format!(
            "invalid collection name {name}; max length is {MAX_HOST_FUNCTION_COLLECTION_NAME_LENGTH}"
        )));
    }
    if name.is_empty()
        || !name
            .chars()
            .all(|ch| ch.is_ascii_lowercase() || ch.is_ascii_digit() || ch == '-')
    {
        return Err(HostFunctionRegistrationError::InvalidState(format!(
            "invalid collection name {name}; expected lowercase alphanumeric characters plus hyphens"
        )));
    }
    Ok(())
}

fn validate_host_function_name(name: &str) -> Result<(), HostFunctionRegistrationError> {
    if name.len() > MAX_HOST_FUNCTION_NAME_LENGTH {
        return Err(HostFunctionRegistrationError::InvalidState(format!(
            "invalid host function name {name}; max length is {MAX_HOST_FUNCTION_NAME_LENGTH}"
        )));
    }
    if name.is_empty()
        || !name
            .chars()
            .all(|ch| ch.is_ascii_lowercase() || ch.is_ascii_digit() || ch == '-')
    {
        return Err(HostFunctionRegistrationError::InvalidState(format!(
            "invalid host function name {name}; expected lowercase alphanumeric characters plus hyphens"
        )));
    }
    Ok(())
}

fn validate_command_aliases(
    label: &str,
    aliases: &[String],
) -> Result<(), HostFunctionRegistrationError> {
    let mut seen = BTreeSet::new();
    for alias in aliases {
        validate_command_alias(label, alias)?;
        if !seen.insert(alias) {
            return Err(HostFunctionRegistrationError::InvalidState(format!(
                "duplicate host callback {label}: {alias}"
            )));
        }
    }
    Ok(())
}

fn validate_command_alias(label: &str, alias: &str) -> Result<(), HostFunctionRegistrationError> {
    if alias.is_empty()
        || alias == "."
        || alias == ".."
        || alias.contains('/')
        || alias.contains('\0')
    {
        return Err(HostFunctionRegistrationError::InvalidState(format!(
            "invalid host callback {label}: {alias:?}"
        )));
    }
    Ok(())
}

fn validate_description_length(
    label: &str,
    description: &str,
) -> Result<(), HostFunctionRegistrationError> {
    if description.len() > MAX_HOST_FUNCTION_DESCRIPTION_LENGTH {
        return Err(HostFunctionRegistrationError::InvalidState(format!(
            "{label} description is {} characters, max is {MAX_HOST_FUNCTION_DESCRIPTION_LENGTH}",
            description.len()
        )));
    }
    Ok(())
}

fn validate_host_function_schema_shape(
    label: &str,
    schema: &Value,
) -> Result<(), HostFunctionRegistrationError> {
    validate_json_byte_length(label, schema, MAX_HOST_FUNCTION_SCHEMA_BYTES)?;
    validate_json_depth(label, schema, 0)
}

fn validate_json_byte_length(
    label: &str,
    value: &Value,
    limit: usize,
) -> Result<(), HostFunctionRegistrationError> {
    let length = serde_json::to_vec(value)
        .map_err(|error| {
            HostFunctionRegistrationError::InvalidState(format!("{label} is invalid JSON: {error}"))
        })?
        .len();
    if length > limit {
        return Err(HostFunctionRegistrationError::InvalidState(format!(
            "{label} is {length} bytes, max is {limit}"
        )));
    }
    Ok(())
}

fn validate_json_depth(
    label: &str,
    value: &Value,
    depth: usize,
) -> Result<(), HostFunctionRegistrationError> {
    if depth > MAX_HOST_FUNCTION_SCHEMA_DEPTH {
        return Err(HostFunctionRegistrationError::InvalidState(format!(
            "{label} exceeds max JSON depth {MAX_HOST_FUNCTION_SCHEMA_DEPTH}"
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
