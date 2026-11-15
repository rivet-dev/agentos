//! Sidecar-owned request queue.
//!
//! The sidecar admits a bounded number of requests per lane at once. A request
//! that finds its lane full waits here, in arrival order, instead of being
//! rejected. Clients send every request directly and never queue: one
//! implementation owns the lanes, the bounds, and the deadline.
//!
//! Each lane has its own queue, so a flood of ordinary work never delays a
//! priority request (teardown, stdin, PTY resize), and a burst of VM creates
//! never queues ahead of other VMs' ordinary calls.
//! A full queue or an expired deadline rejects the request with a typed,
//! retryable error that names the limit to raise.

use std::collections::VecDeque;
use std::time::Duration;

use tokio::time::Instant;

use crate::request_operations::AdmissionLane;
use crate::wire::{self, RequestFrame};

const REQUEST_QUEUE_LIMIT_CODE: &str = "ERR_AGENTOS_REQUEST_QUEUE_LIMIT";
const REQUEST_QUEUE_TIMEOUT_CODE: &str = "ERR_AGENTOS_REQUEST_QUEUE_TIMEOUT";
const QUEUE_LIMIT_PATH: &str = "runtime.protocol.requestQueueLimit";
const QUEUE_BYTES_PATH: &str = "runtime.protocol.requestQueueBytes";
const QUEUE_DEADLINE_PATH: &str = "runtime.protocol.requestQueueDeadlineMs";

pub(super) struct QueuedRequest {
    pub(super) request: RequestFrame,
    pub(super) bytes: usize,
    pub(super) deadline: Instant,
}

#[derive(Default)]
struct LaneQueue {
    entries: VecDeque<QueuedRequest>,
    bytes: usize,
    warned: bool,
}

pub(super) struct RequestQueue {
    limit: usize,
    max_bytes: usize,
    deadline: Duration,
    ordinary: LaneQueue,
    create: LaneQueue,
    priority: LaneQueue,
}

impl RequestQueue {
    pub(super) fn new(protocol: &agentos_runtime::RuntimeProtocolConfig) -> Self {
        Self {
            limit: protocol.request_queue_limit,
            max_bytes: protocol.request_queue_bytes,
            deadline: Duration::from_millis(protocol.request_queue_deadline_ms),
            ordinary: LaneQueue::default(),
            create: LaneQueue::default(),
            priority: LaneQueue::default(),
        }
    }

    fn lane(&self, lane: AdmissionLane) -> &LaneQueue {
        match lane {
            AdmissionLane::Ordinary => &self.ordinary,
            AdmissionLane::Create => &self.create,
            AdmissionLane::Priority => &self.priority,
        }
    }

    fn lane_mut(&mut self, lane: AdmissionLane) -> &mut LaneQueue {
        match lane {
            AdmissionLane::Ordinary => &mut self.ordinary,
            AdmissionLane::Create => &mut self.create,
            AdmissionLane::Priority => &mut self.priority,
        }
    }

    pub(super) fn is_empty(&self) -> bool {
        self.ordinary.entries.is_empty()
            && self.create.entries.is_empty()
            && self.priority.entries.is_empty()
    }

    /// Whether earlier requests in `lane` still wait. A new request must then
    /// queue behind them, even if a slot opened, to keep arrival order.
    pub(super) fn has_waiting(&self, lane: AdmissionLane) -> bool {
        !self.lane(lane).entries.is_empty()
    }

    /// Queue `request` at the back of its lane, or return the typed rejection
    /// to send when the lane is full.
    #[allow(clippy::result_large_err)] // a full lane hands the request back with its rejection
    pub(super) fn push_back(
        &mut self,
        lane: AdmissionLane,
        request: RequestFrame,
        bytes: usize,
    ) -> Result<(), (RequestFrame, wire::RejectedResponse)> {
        let limit = self.limit;
        let max_bytes = self.max_bytes;
        let deadline = Instant::now() + self.deadline;
        let queue = self.lane_mut(lane);
        let queued = queue.entries.len();
        if queued >= limit {
            let rejection = queue_rejection(
                lane,
                REQUEST_QUEUE_LIMIT_CODE,
                format!(
                    "{REQUEST_QUEUE_LIMIT_CODE}: {queued} {} requests already wait for in-flight capacity, limit {limit}; retry later (the shipped sidecar fixes this limit)",
                    lane.name()
                ),
                "queued requests",
                limit,
                queued,
                1,
                "requests",
                QUEUE_LIMIT_PATH,
            );
            return Err((request, rejection));
        }
        let next_bytes = queue.bytes.saturating_add(bytes);
        if next_bytes > max_bytes {
            let rejection = queue_rejection(
                lane,
                REQUEST_QUEUE_LIMIT_CODE,
                format!(
                    "{REQUEST_QUEUE_LIMIT_CODE}: {} queued {} request bytes plus {bytes} exceed {max_bytes}; retry later (the shipped sidecar fixes this limit)",
                    queue.bytes,
                    lane.name()
                ),
                "queued request bytes",
                max_bytes,
                queue.bytes,
                bytes,
                "bytes",
                QUEUE_BYTES_PATH,
            );
            return Err((request, rejection));
        }
        queue.bytes = next_bytes;
        queue.entries.push_back(QueuedRequest {
            request,
            bytes,
            deadline,
        });
        let near_limit = queue.entries.len().saturating_mul(10) >= limit.saturating_mul(8);
        if near_limit && !queue.warned {
            queue.warned = true;
            tracing::warn!(
                lane = lane.name(),
                queued = queue.entries.len(),
                limit,
                "agentOS sidecar request queue is near {QUEUE_LIMIT_PATH}"
            );
        } else if !near_limit {
            queue.warned = false;
        }
        Ok(())
    }

    /// Byte size of the oldest request waiting in `lane`.
    pub(super) fn front_bytes(&self, lane: AdmissionLane) -> Option<usize> {
        self.lane(lane).entries.front().map(|entry| entry.bytes)
    }

    /// Requests waiting in `lane`.
    pub(super) fn waiting(&self, lane: AdmissionLane) -> usize {
        self.lane(lane).entries.len()
    }

    /// Put a request taken with [`Self::pop_front`] back at the head of its
    /// lane, with its original deadline, because a slot it expected was gone.
    pub(super) fn push_front(
        &mut self,
        lane: AdmissionLane,
        request: RequestFrame,
        bytes: usize,
        deadline: Instant,
    ) {
        let queue = self.lane_mut(lane);
        queue.bytes = queue.bytes.saturating_add(bytes);
        queue.entries.push_front(QueuedRequest {
            request,
            bytes,
            deadline,
        });
    }

    /// The oldest request in `lane`, if `admissible` accepts its byte size.
    pub(super) fn pop_front(
        &mut self,
        lane: AdmissionLane,
        admissible: impl FnOnce(usize) -> bool,
    ) -> Option<QueuedRequest> {
        let queue = self.lane_mut(lane);
        let bytes = queue.entries.front()?.bytes;
        if !admissible(bytes) {
            return None;
        }
        let entry = queue.entries.pop_front()?;
        queue.bytes = queue.bytes.saturating_sub(entry.bytes);
        Some(entry)
    }

    /// The oldest request whose deadline passed, with the typed rejection to
    /// send.
    pub(super) fn pop_expired(
        &mut self,
        now: Instant,
    ) -> Option<(RequestFrame, wire::RejectedResponse)> {
        let deadline_ms = u64::try_from(self.deadline.as_millis()).unwrap_or(u64::MAX);
        let lane = AdmissionLane::DRAIN_ORDER.into_iter().find(|lane| {
            self.lane(*lane)
                .entries
                .front()
                .is_some_and(|entry| entry.deadline <= now)
        })?;
        let queue = self.lane_mut(lane);
        let entry = queue.entries.pop_front()?;
        queue.bytes = queue.bytes.saturating_sub(entry.bytes);
        let waited = usize::try_from(deadline_ms).unwrap_or(usize::MAX);
        let rejection = queue_rejection(
            lane,
            REQUEST_QUEUE_TIMEOUT_CODE,
            format!(
                "{REQUEST_QUEUE_TIMEOUT_CODE}: {} request waited {deadline_ms} ms for in-flight capacity; retry later (the shipped sidecar fixes this deadline)",
                lane.name()
            ),
            "queued request wait",
            waited,
            waited,
            1,
            "milliseconds",
            QUEUE_DEADLINE_PATH,
        );
        Some((entry.request, rejection))
    }

    /// When the oldest queued request expires. Lanes are FIFO with one
    /// deadline, so only lane heads matter.
    pub(super) fn next_deadline(&self) -> Option<Instant> {
        [&self.priority, &self.ordinary, &self.create]
            .into_iter()
            .filter_map(|queue| queue.entries.front().map(|entry| entry.deadline))
            .min()
    }
}

#[allow(clippy::too_many_arguments)]
fn queue_rejection(
    lane: AdmissionLane,
    code: &str,
    message: String,
    limit_name: &str,
    configured_limit: usize,
    current_usage: usize,
    requested: usize,
    unit: &str,
    configuration_path: &str,
) -> wire::RejectedResponse {
    let as_u64 = |value: usize| u64::try_from(value).unwrap_or(u64::MAX);
    wire::RejectedResponse {
        code: code.to_owned(),
        message,
        limit_name: Some(limit_name.to_owned()),
        configured_limit: Some(as_u64(configured_limit)),
        current_usage: Some(as_u64(current_usage)),
        requested: Some(as_u64(requested)),
        unit: Some(unit.to_owned()),
        scope: Some(String::from("connection")),
        vm_id: None,
        session_generation: None,
        capability_id: None,
        operation: Some(format!("stdio.requestQueue.{}", lane.name())),
        configuration_path: Some(configuration_path.to_owned()),
        retryable: Some(true),
        errno: Some(String::from("EAGAIN")),
    }
}
