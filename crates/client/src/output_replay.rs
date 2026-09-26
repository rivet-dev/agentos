use std::collections::VecDeque;

use crate::process::ProcessStream;
use crate::{ClientError, ResourceLimitDetails};

pub(crate) const OUTPUT_REPLAY_EVENT_LIMIT: usize = 1_024;
pub(crate) const OUTPUT_REPLAY_BYTE_LIMIT: usize = 1024 * 1024;
pub(crate) const OUTPUT_REPLAY_PAGE_EVENT_LIMIT: usize = 256;
pub(crate) const OUTPUT_REPLAY_PAGE_BYTE_LIMIT: usize = 768 * 1024;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct OutputReplayEvent {
    pub sequence: u64,
    pub stream: ProcessStream,
    pub data: Vec<u8>,
    pub timestamp_ms: i64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct OutputReplayPage {
    pub events: Vec<OutputReplayEvent>,
    pub next_cursor: Option<u64>,
    pub has_more: bool,
    pub truncated: bool,
}

/// One bounded sequenced byte replay used by processes, language spawns, and
/// terminals. Keeping cursor and truncation behavior here prevents each API
/// adapter from inventing subtly different pull semantics.
pub(crate) struct OutputReplayBuffer {
    events: VecDeque<OutputReplayEvent>,
    retained_bytes: usize,
    next_sequence: u64,
    truncated_before: Option<u64>,
}

impl OutputReplayBuffer {
    pub(crate) fn new() -> Self {
        Self {
            events: VecDeque::new(),
            retained_bytes: 0,
            next_sequence: 0,
            truncated_before: None,
        }
    }

    pub(crate) fn push(&mut self, stream: ProcessStream, data: &[u8]) -> OutputReplayEvent {
        let sequence = self.next_sequence;
        self.next_sequence = self.next_sequence.saturating_add(1);
        let event = OutputReplayEvent {
            sequence,
            stream,
            data: data.to_vec(),
            timestamp_ms: epoch_ms_now(),
        };
        if data.len() > OUTPUT_REPLAY_BYTE_LIMIT {
            self.truncated_before = Some(sequence);
            return event;
        }
        self.events.push_back(event.clone());
        self.retained_bytes = self.retained_bytes.saturating_add(data.len());
        while self.events.len() > OUTPUT_REPLAY_EVENT_LIMIT
            || self.retained_bytes > OUTPUT_REPLAY_BYTE_LIMIT
        {
            if let Some(removed) = self.events.pop_front() {
                self.retained_bytes = self.retained_bytes.saturating_sub(removed.data.len());
                self.truncated_before = Some(removed.sequence);
            } else {
                break;
            }
        }
        event
    }

    pub(crate) fn read(
        &self,
        after: Option<u64>,
        max_events: usize,
        max_bytes: usize,
    ) -> OutputReplayPage {
        let cursor = after;
        let include_all = after.is_none();
        let after = after.unwrap_or(0);
        let mut bytes = 0usize;
        let mut events = Vec::new();
        let mut has_more = false;
        for event in &self.events {
            if !include_all && event.sequence <= after {
                continue;
            }
            if events.len() == max_events || bytes.saturating_add(event.data.len()) > max_bytes {
                has_more = true;
                break;
            }
            bytes += event.data.len();
            events.push(event.clone());
        }
        let requested_next = if include_all {
            0
        } else {
            after.saturating_add(1)
        };
        OutputReplayPage {
            next_cursor: events.last().map(|event| event.sequence).or(cursor),
            events,
            has_more,
            truncated: self
                .truncated_before
                .is_some_and(|sequence| sequence >= requested_next),
        }
    }
}

pub(crate) fn page_limits(
    max_events: Option<usize>,
    max_bytes: Option<usize>,
    operation: &'static str,
) -> Result<(usize, usize), ClientError> {
    let max_events = max_events.unwrap_or(OUTPUT_REPLAY_PAGE_EVENT_LIMIT);
    if max_events == 0 || max_events > OUTPUT_REPLAY_PAGE_EVENT_LIMIT {
        return Err(replay_limit_error(
            operation,
            "output_replay_page_events",
            max_events as u64,
            OUTPUT_REPLAY_PAGE_EVENT_LIMIT as u64,
            "maxEvents",
        ));
    }
    let max_bytes = max_bytes.unwrap_or(OUTPUT_REPLAY_PAGE_BYTE_LIMIT);
    if max_bytes == 0 || max_bytes > OUTPUT_REPLAY_PAGE_BYTE_LIMIT {
        return Err(replay_limit_error(
            operation,
            "output_replay_page_bytes",
            max_bytes as u64,
            OUTPUT_REPLAY_PAGE_BYTE_LIMIT as u64,
            "maxBytes",
        ));
    }
    Ok((max_events, max_bytes))
}

pub(crate) fn require_page_progress(
    page: &OutputReplayPage,
    operation: &'static str,
    max_bytes: usize,
) -> Result<(), ClientError> {
    if page.events.is_empty() && page.has_more {
        return Err(replay_limit_error(
            operation,
            "output_replay_page_bytes",
            max_bytes as u64,
            OUTPUT_REPLAY_PAGE_BYTE_LIMIT as u64,
            "maxBytes",
        ));
    }
    Ok(())
}

fn replay_limit_error(
    operation: &'static str,
    limit_name: &'static str,
    requested: u64,
    configured_limit: u64,
    field: &'static str,
) -> ClientError {
    ClientError::ResourceLimit {
        code: String::from("ERR_AGENTOS_RESOURCE_LIMIT"),
        message: format!(
            "{operation} {field} requested {requested}; supported range is 1..={configured_limit}"
        ),
        details: Box::new(ResourceLimitDetails {
            limit_name: Some(limit_name.to_owned()),
            configured_limit: Some(configured_limit),
            requested: Some(requested),
            unit: Some(
                if field == "maxBytes" {
                    "bytes"
                } else {
                    "events"
                }
                .to_owned(),
            ),
            scope: Some(String::from("vm")),
            operation: Some(operation.to_owned()),
            configuration_path: Some(field.to_owned()),
            retryable: Some(false),
            ..ResourceLimitDetails::default()
        }),
    }
}

fn epoch_ms_now() -> i64 {
    use std::time::{SystemTime, UNIX_EPOCH};
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_millis().min(i64::MAX as u128) as i64)
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn replay_preserves_order_cursor_and_page_bounds() {
        let mut replay = OutputReplayBuffer::new();
        replay.push(ProcessStream::Stdout, b"one");
        replay.push(ProcessStream::Stderr, b"two");

        let page = replay.read(Some(0), 1, 100);
        assert_eq!(page.events.len(), 1);
        assert_eq!(page.events[0].sequence, 1);
        assert_eq!(page.events[0].stream, ProcessStream::Stderr);
        assert_eq!(page.next_cursor, Some(1));
        assert!(!page.has_more);
        assert!(!page.truncated);
    }

    #[test]
    fn replay_bounds_empty_and_oversized_events() {
        let mut replay = OutputReplayBuffer::new();
        for _ in 0..=OUTPUT_REPLAY_EVENT_LIMIT {
            replay.push(ProcessStream::Stdout, b"");
        }
        let page = replay.read(None, OUTPUT_REPLAY_PAGE_EVENT_LIMIT, 1);
        assert_eq!(page.events.len(), OUTPUT_REPLAY_PAGE_EVENT_LIMIT);
        assert!(page.has_more);
        assert!(page.truncated);

        replay.push(
            ProcessStream::Stdout,
            &vec![0; OUTPUT_REPLAY_BYTE_LIMIT + 1],
        );
        assert!(
            replay
                .read(
                    Some(OUTPUT_REPLAY_EVENT_LIMIT as u64),
                    1,
                    OUTPUT_REPLAY_PAGE_BYTE_LIMIT
                )
                .truncated
        );
    }

    #[test]
    fn page_limit_failures_are_typed() {
        let error = page_limits(Some(0), None, "process.output.read").unwrap_err();
        let ClientError::ResourceLimit { details, .. } = error else {
            panic!("expected resource limit");
        };
        assert_eq!(details.configuration_path.as_deref(), Some("maxEvents"));
        assert_eq!(
            details.configured_limit,
            Some(OUTPUT_REPLAY_PAGE_EVENT_LIMIT as u64)
        );
    }
}
