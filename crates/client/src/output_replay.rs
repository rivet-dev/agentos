use crate::{ClientError, ResourceLimitDetails};

pub(crate) const DEFAULT_OUTPUT_REPLAY_PAGE_EVENTS: usize = 256;
pub(crate) const DEFAULT_OUTPUT_REPLAY_PAGE_BYTES: usize = 768 * 1024;
const MAX_WIRE_PAGE_VALUE: usize = u32::MAX as usize;

/// Ordinary process/terminal replay uses zero on the wire to request the
/// sidecar's configured default. Explicit zero is never valid public input.
pub(crate) fn wire_page_limits(
    max_events: Option<usize>,
    max_bytes: Option<usize>,
    operation: &'static str,
) -> Result<(usize, usize), ClientError> {
    if let Some(value) = max_events {
        validate_page_limit(value, operation, "maxEvents", "events")?;
    }
    if let Some(value) = max_bytes {
        validate_page_limit(value, operation, "maxBytes", "bytes")?;
    }
    Ok((max_events.unwrap_or(0), max_bytes.unwrap_or(0)))
}

/// Byte paging for the separate language-execution replay adapter.
pub(crate) fn page_limits(
    max_events: Option<usize>,
    max_bytes: Option<usize>,
    operation: &'static str,
) -> Result<(usize, usize), ClientError> {
    let max_events = max_events.unwrap_or(DEFAULT_OUTPUT_REPLAY_PAGE_EVENTS);
    validate_page_limit(max_events, operation, "maxEvents", "events")?;
    let max_bytes = max_bytes.unwrap_or(DEFAULT_OUTPUT_REPLAY_PAGE_BYTES);
    validate_page_limit(max_bytes, operation, "maxBytes", "bytes")?;
    Ok((max_events, max_bytes))
}

fn validate_page_limit(
    value: usize,
    operation: &'static str,
    field: &'static str,
    unit: &'static str,
) -> Result<(), ClientError> {
    if value > 0 && value <= MAX_WIRE_PAGE_VALUE {
        return Ok(());
    }
    Err(ClientError::ResourceLimit {
        code: String::from("ERR_AGENTOS_RESOURCE_LIMIT"),
        message: format!(
            "{operation} {field} requested {value}; transport range is 1..={MAX_WIRE_PAGE_VALUE}"
        ),
        details: Box::new(ResourceLimitDetails {
            limit_name: Some(String::from("process_output_replay_wire_value")),
            configured_limit: Some(MAX_WIRE_PAGE_VALUE as u64),
            requested: Some(value as u64),
            unit: Some(unit.to_owned()),
            scope: Some(String::from("request")),
            operation: Some(operation.to_owned()),
            configuration_path: Some(field.to_owned()),
            retryable: Some(false),
            ..ResourceLimitDetails::default()
        }),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn page_limit_failures_are_typed() {
        let error = page_limits(Some(0), None, "process.output.read").unwrap_err();
        let ClientError::ResourceLimit { details, .. } = error else {
            panic!("expected resource limit");
        };
        assert_eq!(details.configuration_path.as_deref(), Some("maxEvents"));
        assert_eq!(details.configured_limit, Some(u32::MAX as u64));
    }

    #[test]
    fn omitted_wire_bounds_use_sidecar_defaults_but_explicit_zero_is_invalid() {
        assert_eq!(
            wire_page_limits(None, None, "process.output.read").unwrap(),
            (0, 0)
        );
        assert_eq!(
            wire_page_limits(Some(12), None, "process.output.read").unwrap(),
            (12, 0)
        );
        assert!(wire_page_limits(None, Some(0), "terminal.output.read").is_err());
        assert!(wire_page_limits(Some(0), None, "process.output.read").is_err());
    }

    #[test]
    fn page_limits_can_be_raised_above_defaults() {
        assert_eq!(
            page_limits(
                Some(DEFAULT_OUTPUT_REPLAY_PAGE_EVENTS + 1),
                Some(DEFAULT_OUTPUT_REPLAY_PAGE_BYTES + 1),
                "process.output.read"
            )
            .unwrap(),
            (
                DEFAULT_OUTPUT_REPLAY_PAGE_EVENTS + 1,
                DEFAULT_OUTPUT_REPLAY_PAGE_BYTES + 1
            )
        );
    }
}
