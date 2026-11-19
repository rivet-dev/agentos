//! The sidecar's shared queue of public process events waiting for the host.
//!
//! Every VM on the sidecar shares this queue's bounds, but each VM has its own
//! FIFO. The host drain takes VMs round-robin, so a quiet VM never waits
//! behind another VM's backlog, and per-VM scans (an exit's trailing output)
//! touch only that VM's events. A producer the queue cannot take now is
//! paused (its guest blocks like a writer on a full pipe); the next consumer
//! release of queue capacity wakes the process-event pump once.

use std::collections::{HashMap, VecDeque};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use crate::state::{ActiveExecutionEvent, ProcessEventEnvelope};

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub(crate) struct VmPendingProcessEvents {
    pub(crate) events: usize,
    pub(crate) bytes: usize,
}

#[derive(Debug, Default)]
struct VmQueue {
    events: VecDeque<ProcessEventEnvelope>,
    counts: VmPendingProcessEvents,
}

#[derive(Debug)]
pub(crate) struct PendingProcessEvents {
    vms: HashMap<String, VmQueue>,
    /// VMs with queued events, in drain order.
    ring: VecDeque<String>,
    len: usize,
    output_events: usize,
    bytes: usize,
    producer_paused: AtomicBool,
    release_wake: Arc<tokio::sync::Notify>,
}

pub(crate) fn is_output_event(event: &ActiveExecutionEvent) -> bool {
    matches!(
        event,
        ActiveExecutionEvent::Stdout(_) | ActiveExecutionEvent::Stderr(_)
    )
}

impl PendingProcessEvents {
    pub(crate) fn new(release_wake: Arc<tokio::sync::Notify>) -> Self {
        Self {
            vms: HashMap::new(),
            ring: VecDeque::new(),
            len: 0,
            output_events: 0,
            bytes: 0,
            producer_paused: AtomicBool::new(false),
            release_wake,
        }
    }

    /// Records that a producer is waiting for capacity.
    pub(crate) fn pause_producer(&self) {
        self.producer_paused.store(true, Ordering::Release);
    }

    /// Called after a consumer released capacity. Wakes the pump once if a
    /// producer is paused. Scans that remove and reinsert events must not
    /// call it: a paused pump would wake itself in a loop.
    pub(crate) fn wake_paused_producer(&self) {
        if self.producer_paused.swap(false, Ordering::AcqRel) {
            self.release_wake.notify_one();
        }
    }

    pub(crate) fn len(&self) -> usize {
        self.len
    }

    pub(crate) fn output_events(&self) -> usize {
        self.output_events
    }

    pub(crate) fn bytes(&self) -> usize {
        self.bytes
    }

    pub(crate) fn vm(&self, vm_id: &str) -> VmPendingProcessEvents {
        self.vms
            .get(vm_id)
            .map(|queue| queue.counts)
            .unwrap_or_default()
    }

    /// All queued events, VM by VM in drain order.
    #[cfg(test)]
    pub(crate) fn iter(&self) -> impl Iterator<Item = &ProcessEventEnvelope> {
        self.ring
            .iter()
            .filter_map(|vm_id| self.vms.get(vm_id))
            .flat_map(|queue| queue.events.iter())
    }

    /// One VM's queued events, in order.
    pub(crate) fn vm_iter(&self, vm_id: &str) -> impl Iterator<Item = &ProcessEventEnvelope> {
        self.vms
            .get(vm_id)
            .into_iter()
            .flat_map(|queue| queue.events.iter())
    }

    /// VMs with queued events, in drain order.
    pub(crate) fn vm_ids(&self) -> Vec<String> {
        self.ring.iter().cloned().collect()
    }

    pub(crate) fn vm_get(&self, vm_id: &str, index: usize) -> Option<&ProcessEventEnvelope> {
        self.vms.get(vm_id)?.events.get(index)
    }

    #[cfg(test)]
    pub(crate) fn front(&self) -> Option<&ProcessEventEnvelope> {
        self.iter().next()
    }

    fn queue_for(&mut self, envelope: &ProcessEventEnvelope) -> &mut VmQueue {
        if !self.vms.contains_key(&envelope.vm_id) {
            self.ring.push_back(envelope.vm_id.clone());
            self.vms.insert(envelope.vm_id.clone(), VmQueue::default());
        }
        self.vms
            .get_mut(&envelope.vm_id)
            .expect("the VM queue was just ensured")
    }

    fn account_add(&mut self, envelope: &ProcessEventEnvelope) {
        let bytes = envelope.retained_bytes();
        let output = is_output_event(&envelope.event);
        self.len += 1;
        self.bytes = self.bytes.saturating_add(bytes);
        if output {
            self.output_events += 1;
        }
        let counts = &mut self.queue_for(envelope).counts;
        counts.events += 1;
        counts.bytes = counts.bytes.saturating_add(bytes);
    }

    /// Updates totals for an envelope already taken out of its VM queue.
    fn account_removed(&mut self, envelope: &ProcessEventEnvelope) {
        let bytes = envelope.retained_bytes();
        let output = is_output_event(&envelope.event);
        self.len -= 1;
        self.bytes = self.bytes.saturating_sub(bytes);
        if output {
            self.output_events -= 1;
        }
        let Some(queue) = self.vms.get_mut(&envelope.vm_id) else {
            debug_assert!(
                false,
                "pending process event accounting lost VM {}",
                envelope.vm_id
            );
            return;
        };
        queue.counts.events -= 1;
        queue.counts.bytes = queue.counts.bytes.saturating_sub(bytes);
    }

    /// Drops a VM from the drain order once its queue is empty.
    fn forget_vm_if_empty(&mut self, vm_id: &str) {
        if self
            .vms
            .get(vm_id)
            .is_some_and(|queue| queue.events.is_empty())
        {
            self.vms.remove(vm_id);
            if let Some(position) = self.ring.iter().position(|queued| queued == vm_id) {
                self.ring.remove(position);
            }
        }
    }

    pub(crate) fn push_back(&mut self, envelope: ProcessEventEnvelope) {
        self.account_add(&envelope);
        self.queue_for(&envelope).events.push_back(envelope);
    }

    pub(crate) fn push_front(&mut self, envelope: ProcessEventEnvelope) {
        self.account_add(&envelope);
        self.queue_for(&envelope).events.push_front(envelope);
    }

    pub(crate) fn append(&mut self, other: &mut VecDeque<ProcessEventEnvelope>) {
        for envelope in other.drain(..) {
            self.push_back(envelope);
        }
    }

    pub(crate) fn vm_insert(&mut self, index: usize, envelope: ProcessEventEnvelope) {
        self.account_add(&envelope);
        self.queue_for(&envelope).events.insert(index, envelope);
    }

    pub(crate) fn vm_remove(&mut self, vm_id: &str, index: usize) -> Option<ProcessEventEnvelope> {
        let envelope = self.vms.get_mut(vm_id)?.events.remove(index)?;
        self.account_removed(&envelope);
        self.forget_vm_if_empty(vm_id);
        Some(envelope)
    }

    /// Takes the first event of the next VM, round-robin, that matches. The
    /// VM it came from moves to the back of the drain order.
    pub(crate) fn take_next(
        &mut self,
        mut matches: impl FnMut(&ProcessEventEnvelope) -> bool,
    ) -> Option<ProcessEventEnvelope> {
        for _ in 0..self.ring.len() {
            let vm_id = self.ring.pop_front()?;
            let index = self
                .vms
                .get(&vm_id)
                .and_then(|queue| queue.events.iter().position(&mut matches));
            self.ring.push_back(vm_id);
            if let Some(index) = index {
                let vm_id = self.ring.back().expect("the VM was just rotated").clone();
                return self.vm_remove(&vm_id, index);
            }
        }
        None
    }

    /// Takes the first event of one VM that matches.
    pub(crate) fn take_from_vm(
        &mut self,
        vm_id: &str,
        matches: impl FnMut(&ProcessEventEnvelope) -> bool,
    ) -> Option<ProcessEventEnvelope> {
        let index = self.vms.get(vm_id)?.events.iter().position(matches)?;
        self.vm_remove(vm_id, index)
    }

    /// Takes, in order, every event of one VM process that matches. Costs
    /// O(that VM's queued events).
    pub(crate) fn take_process_events(
        &mut self,
        vm_id: &str,
        process_id: &str,
        mut matches: impl FnMut(&ProcessEventEnvelope) -> bool,
    ) -> Vec<ProcessEventEnvelope> {
        let Some(queue) = self.vms.get_mut(vm_id) else {
            return Vec::new();
        };
        let mut taken = Vec::new();
        let mut kept = VecDeque::with_capacity(queue.events.len());
        for envelope in queue.events.drain(..) {
            if envelope.process_id == process_id && matches(&envelope) {
                taken.push(envelope);
            } else {
                kept.push_back(envelope);
            }
        }
        queue.events = kept;
        for envelope in &taken {
            self.account_removed(envelope);
        }
        self.forget_vm_if_empty(vm_id);
        taken
    }

    // Used by the service integration fixture, not the lib unit tests.
    #[cfg(test)]
    #[allow(dead_code)]
    pub(crate) fn clear(&mut self) {
        while self.take_next(|_| true).is_some() {}
    }
}
