//! Hierarchical count/byte admission with exact RAII ownership.

use std::collections::BTreeMap;
use std::fmt;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

use crate::metrics::{BufferMetricClass, ResourceMetricClass, RuntimeMetrics};

/// Low-cardinality resource classes used by the sidecar admission policy.
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub enum ResourceClass {
    Capabilities,
    ReadyHandles,
    Sockets,
    Connections,
    BufferedBytes,
    Datagrams,
    HandleCommands,
    HandleCommandBytes,
    BridgeCalls,
    BridgeRequestBytes,
    BridgeResponseBytes,
    AsyncCompletions,
    AsyncCompletionBytes,
    UdpDatagrams,
    UdpBytes,
    TlsBytes,
    Timers,
    Tasks,
    ExecutorSlots,
    ExecutorBytes,
    Http2Connections,
    Http2Streams,
    Http2BufferedBytes,
    Http2HeaderBytes,
    Http2DataBytes,
    Http2Commands,
    Http2CommandBytes,
    Http2Events,
    Http2EventBytes,
}

impl ResourceClass {
    pub const ALL: [Self; 29] = [
        Self::Capabilities,
        Self::ReadyHandles,
        Self::Sockets,
        Self::Connections,
        Self::BufferedBytes,
        Self::Datagrams,
        Self::HandleCommands,
        Self::HandleCommandBytes,
        Self::BridgeCalls,
        Self::BridgeRequestBytes,
        Self::BridgeResponseBytes,
        Self::AsyncCompletions,
        Self::AsyncCompletionBytes,
        Self::UdpDatagrams,
        Self::UdpBytes,
        Self::TlsBytes,
        Self::Timers,
        Self::Tasks,
        Self::ExecutorSlots,
        Self::ExecutorBytes,
        Self::Http2Connections,
        Self::Http2Streams,
        Self::Http2BufferedBytes,
        Self::Http2HeaderBytes,
        Self::Http2DataBytes,
        Self::Http2Commands,
        Self::Http2CommandBytes,
        Self::Http2Events,
        Self::Http2EventBytes,
    ];

    pub const fn name(self) -> &'static str {
        match self {
            Self::Capabilities => "capabilities",
            Self::ReadyHandles => "readyHandles",
            Self::Sockets => "sockets",
            Self::Connections => "connections",
            Self::BufferedBytes => "bufferedBytes",
            Self::Datagrams => "datagrams",
            Self::HandleCommands => "handleCommands",
            Self::HandleCommandBytes => "handleCommandBytes",
            Self::BridgeCalls => "bridgeCalls",
            Self::BridgeRequestBytes => "bridgeRequestBytes",
            Self::BridgeResponseBytes => "bridgeResponseBytes",
            Self::AsyncCompletions => "asyncCompletions",
            Self::AsyncCompletionBytes => "asyncCompletionBytes",
            Self::UdpDatagrams => "udpDatagrams",
            Self::UdpBytes => "udpBytes",
            Self::TlsBytes => "tlsBytes",
            Self::Timers => "timers",
            Self::Tasks => "tasks",
            Self::ExecutorSlots => "executorSlots",
            Self::ExecutorBytes => "executorBytes",
            Self::Http2Connections => "http2Connections",
            Self::Http2Streams => "http2Streams",
            Self::Http2BufferedBytes => "http2BufferedBytes",
            Self::Http2HeaderBytes => "http2HeaderBytes",
            Self::Http2DataBytes => "http2DataBytes",
            Self::Http2Commands => "http2Commands",
            Self::Http2CommandBytes => "http2CommandBytes",
            Self::Http2Events => "http2Events",
            Self::Http2EventBytes => "http2EventBytes",
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ResourceLimit {
    pub maximum: usize,
    pub config_path: String,
}

impl ResourceLimit {
    pub fn new(maximum: usize, config_path: impl Into<String>) -> Self {
        Self {
            maximum,
            config_path: config_path.into(),
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct LimitError {
    pub scope: String,
    pub resource: ResourceClass,
    pub used: usize,
    pub requested: usize,
    pub limit: usize,
    pub config_path: String,
}

impl fmt::Display for LimitError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            formatter,
            "ERR_AGENTOS_RESOURCE_LIMIT: scope={} resource={} used={} requested={} limit={}; raise {}",
            self.scope,
            self.resource.name(),
            self.used,
            self.requested,
            self.limit,
            self.config_path
        )
    }
}

impl std::error::Error for LimitError {}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ResourceUsage {
    pub used: usize,
    pub limit: Option<usize>,
}

#[derive(Debug, Default)]
struct CounterState {
    used: usize,
    warning_active: bool,
}

#[derive(Debug)]
struct LedgerState {
    counters: BTreeMap<ResourceClass, CounterState>,
}

#[derive(Debug)]
struct LedgerInner {
    scope: String,
    limits: BTreeMap<ResourceClass, ResourceLimit>,
    state: Mutex<LedgerState>,
    capacity_changed: tokio::sync::Notify,
    buffered_bytes_released: tokio::sync::Notify,
    buffered_bytes_release_generation: AtomicU64,
    integrity_failed: AtomicBool,
    metrics: Option<RuntimeMetrics>,
}

/// One process or VM accounting scope. A child ledger reserves its parent first,
/// so aggregate process policy and tenant policy cover the same allocation.
#[derive(Clone, Debug)]
pub struct ResourceLedger {
    inner: Arc<LedgerInner>,
    parent: Option<Arc<ResourceLedger>>,
}

impl ResourceLedger {
    pub fn root(
        scope: impl Into<String>,
        limits: impl IntoIterator<Item = (ResourceClass, ResourceLimit)>,
    ) -> Self {
        Self::new(scope, limits, None, None)
    }

    pub fn root_with_metrics(
        scope: impl Into<String>,
        limits: impl IntoIterator<Item = (ResourceClass, ResourceLimit)>,
        metrics: RuntimeMetrics,
    ) -> Self {
        Self::new(scope, limits, None, Some(metrics))
    }

    pub fn child(
        scope: impl Into<String>,
        limits: impl IntoIterator<Item = (ResourceClass, ResourceLimit)>,
        parent: Arc<ResourceLedger>,
    ) -> Self {
        Self::new(scope, limits, Some(parent), None)
    }

    fn new(
        scope: impl Into<String>,
        limits: impl IntoIterator<Item = (ResourceClass, ResourceLimit)>,
        parent: Option<Arc<ResourceLedger>>,
        metrics: Option<RuntimeMetrics>,
    ) -> Self {
        Self {
            inner: Arc::new(LedgerInner {
                scope: scope.into(),
                limits: limits.into_iter().collect(),
                state: Mutex::new(LedgerState {
                    counters: BTreeMap::new(),
                }),
                capacity_changed: tokio::sync::Notify::new(),
                buffered_bytes_released: tokio::sync::Notify::new(),
                buffered_bytes_release_generation: AtomicU64::new(0),
                integrity_failed: AtomicBool::new(false),
                metrics,
            }),
            parent,
        }
    }

    pub fn scope(&self) -> &str {
        &self.inner.scope
    }

    /// Reserve before allocation. A zero-sized reservation is valid and owns no
    /// counters, which lets callers keep one unconditional cleanup path.
    pub fn reserve(
        &self,
        resource: ResourceClass,
        amount: usize,
    ) -> Result<Reservation, LimitError> {
        let mut reservations = self.reserve_all(&[(resource, amount)])?;
        Ok(reservations
            .pop()
            .expect("one admission request yields one reservation"))
    }

    /// Reserve several resources in every scope as one admission. Either every
    /// request fits in every scope and all are charged together, or nothing
    /// changes. A rejected admission never charges, rolls back, or notifies, so
    /// a waiter cannot wake itself through its own failed attempt.
    pub fn reserve_all(
        &self,
        requests: &[(ResourceClass, usize)],
    ) -> Result<Vec<Reservation>, LimitError> {
        // Lock root first, so every admission takes ledger locks in one order.
        let mut scopes = Vec::with_capacity(2);
        let mut ledger = Some(self);
        while let Some(current) = ledger {
            scopes.push(&current.inner);
            ledger = current.parent.as_deref();
        }
        scopes.reverse();
        let mut states: Vec<_> = scopes
            .iter()
            .map(|inner| lock_state(inner, "admission"))
            .collect();

        // A resource can appear more than once in one admission; check its sum.
        let mut requested = BTreeMap::<ResourceClass, usize>::new();
        for &(resource, amount) in requests {
            let total = requested.entry(resource).or_default();
            *total = total.saturating_add(amount);
        }
        for (inner, state) in scopes.iter().zip(&states) {
            for (&resource, &amount) in &requested {
                let Some(limit) = inner.limits.get(&resource) else {
                    continue;
                };
                let used = state
                    .counters
                    .get(&resource)
                    .map_or(0, |counter| counter.used);
                if used
                    .checked_add(amount)
                    .is_none_or(|total| total > limit.maximum)
                {
                    return Err(LimitError {
                        scope: inner.scope.clone(),
                        resource,
                        used,
                        requested: amount,
                        limit: limit.maximum,
                        config_path: limit.config_path.clone(),
                    });
                }
            }
        }

        for (inner, state) in scopes.iter().zip(&mut states) {
            for &(resource, amount) in requests {
                if amount == 0 {
                    continue;
                }
                let counter = state.counters.entry(resource).or_default();
                counter.used = counter.used.saturating_add(amount);
                maybe_warn(inner, resource, counter);
                observe_usage(inner, resource, counter.used);
            }
        }
        drop(states);

        Ok(requests
            .iter()
            .map(|&(resource, amount)| Reservation {
                resource,
                amount,
                allocations: if amount == 0 {
                    Vec::new()
                } else {
                    scopes
                        .iter()
                        .map(|inner| Allocation {
                            ledger: Arc::clone(inner),
                            resource,
                            amount,
                        })
                        .collect()
                },
            })
            .collect())
    }

    pub fn usage(&self, resource: ResourceClass) -> ResourceUsage {
        let state = self.inner.state.lock().unwrap_or_else(|poisoned| {
            eprintln!(
                "ERR_AGENTOS_RESOURCE_LEDGER_POISONED: recovering scope={} resource={}",
                self.inner.scope,
                resource.name()
            );
            poisoned.into_inner()
        });
        ResourceUsage {
            used: state
                .counters
                .get(&resource)
                .map_or(0, |counter| counter.used),
            limit: self.inner.limits.get(&resource).map(|limit| limit.maximum),
        }
    }

    pub fn configured_limit(&self, resource: ResourceClass) -> Option<ResourceLimit> {
        self.inner.limits.get(&resource).cloned()
    }

    /// Best-effort readiness probe that does not acquire and immediately drop
    /// a reservation. Admission must still call [`Self::reserve`]; this method
    /// exists for POLLOUT-style hints where a transient reservation would emit
    /// a false capacity-change notification and churn blocked producers.
    pub fn capacity_available(&self, resource: ResourceClass, amount: usize) -> bool {
        if let Some(parent) = &self.parent {
            if !parent.capacity_available(resource, amount) {
                return false;
            }
        }
        if amount == 0 {
            return true;
        }
        let state = self.inner.state.lock().unwrap_or_else(|poisoned| {
            eprintln!(
                "ERR_AGENTOS_RESOURCE_LEDGER_POISONED: recovering capacity probe scope={} resource={}",
                self.inner.scope,
                resource.name()
            );
            poisoned.into_inner()
        });
        let used = state
            .counters
            .get(&resource)
            .map_or(0, |counter| counter.used);
        self.inner.limits.get(&resource).is_none_or(|limit| {
            used.checked_add(amount)
                .is_some_and(|total| total <= limit.maximum)
        })
    }

    pub fn is_zero(&self) -> bool {
        let state = self.inner.state.lock().unwrap_or_else(|poisoned| {
            eprintln!(
                "ERR_AGENTOS_RESOURCE_LEDGER_POISONED: recovering scope={}",
                self.inner.scope
            );
            poisoned.into_inner()
        });
        state.counters.values().all(|counter| counter.used == 0)
    }

    pub fn integrity_ok(&self) -> bool {
        !self.inner.integrity_failed.load(Ordering::Acquire)
    }

    /// Wait without polling until any owner releases capacity. Callers must
    /// retry their full multi-resource admission after this notification.
    pub async fn capacity_changed(&self) {
        let local_changed = self.inner.capacity_changed.notified();
        if let Some(parent) = &self.parent {
            let parent_changed = parent.inner.capacity_changed.notified();
            tokio::select! {
                _ = local_changed => {}
                _ = parent_changed => {}
            }
        } else {
            local_changed.await;
        }
    }

    /// Count of buffered-byte releases in this scope and its parent. Take it
    /// before an admission attempt and pass it to
    /// [`Self::buffered_bytes_released`].
    pub fn buffered_bytes_release_generation(&self) -> u64 {
        let local = self
            .inner
            .buffered_bytes_release_generation
            .load(Ordering::Acquire);
        self.parent.as_ref().map_or(local, |parent| {
            local.wrapping_add(parent.buffered_bytes_release_generation())
        })
    }

    /// Wait without polling until buffered-byte capacity is released after
    /// `observed`. Unlike [`Self::capacity_changed`], this wakes every waiter,
    /// so it cannot steal the single wakeup an admission waiter depends on.
    pub async fn buffered_bytes_released(&self, observed: u64) {
        let local = self.inner.buffered_bytes_released.notified();
        let parent = self
            .parent
            .as_ref()
            .map(|parent| parent.inner.buffered_bytes_released.notified());
        tokio::pin!(local, parent);
        // Arm before checking so a release between the check and the await
        // is not missed.
        local.as_mut().enable();
        if let Some(parent) = parent.as_mut().as_pin_mut() {
            parent.enable();
        }
        if self.buffered_bytes_release_generation() != observed {
            return;
        }
        match parent.as_pin_mut() {
            Some(parent) => tokio::select! {
                _ = local => {}
                _ = parent => {}
            },
            None => local.await,
        }
    }

    /// Pause an async source until the requested capacity is available. The
    /// notification is armed before each retry so a concurrent release cannot
    /// be missed between observing the limit and awaiting capacity.
    pub async fn reserve_when_available(
        &self,
        resource: ResourceClass,
        amount: usize,
    ) -> Result<Reservation, LimitError> {
        loop {
            // Arm both accounting scopes before admission so a concurrent
            // release cannot be missed. Ledgers are currently process -> VM;
            // flatten this wait set if another hierarchy level is introduced.
            let local_changed = self.inner.capacity_changed.notified();
            let parent_changed = self
                .parent
                .as_ref()
                .map(|parent| parent.inner.capacity_changed.notified());
            match self.reserve(resource, amount) {
                Ok(reservation) => return Ok(reservation),
                Err(error) if amount > error.limit => return Err(error),
                Err(_) => {
                    if let Some(parent_changed) = parent_changed {
                        tokio::select! {
                            _ = local_changed => {}
                            _ = parent_changed => {}
                        }
                    } else {
                        local_changed.await;
                    }
                }
            }
        }
    }
}

fn maybe_warn(inner: &LedgerInner, resource: ResourceClass, counter: &mut CounterState) {
    let Some(limit) = inner.limits.get(&resource) else {
        return;
    };
    // Integer comparisons avoid rounding and make zero impossible to treat as
    // near-limit. The warning rearms only after usage falls below 70%.
    let near =
        counter.used != 0 && counter.used.saturating_mul(100) >= limit.maximum.saturating_mul(80);
    if near && !counter.warning_active {
        counter.warning_active = true;
        eprintln!(
            "WARN_AGENTOS_RESOURCE_NEAR_LIMIT: scope={} resource={} used={} limit={} config={}",
            inner.scope,
            resource.name(),
            counter.used,
            limit.maximum,
            limit.config_path
        );
    }
}

#[derive(Debug)]
struct Allocation {
    ledger: Arc<LedgerInner>,
    resource: ResourceClass,
    amount: usize,
}

fn lock_state<'a>(inner: &'a LedgerInner, context: &str) -> std::sync::MutexGuard<'a, LedgerState> {
    inner.state.lock().unwrap_or_else(|poisoned| {
        eprintln!(
            "ERR_AGENTOS_RESOURCE_LEDGER_POISONED: recovering {context} scope={}",
            inner.scope
        );
        poisoned.into_inner()
    })
}

fn release_allocation(allocation: &Allocation) {
    let mut state = allocation.ledger.state.lock().unwrap_or_else(|poisoned| {
        eprintln!(
            "ERR_AGENTOS_RESOURCE_LEDGER_POISONED: recovering release scope={} resource={}",
            allocation.ledger.scope,
            allocation.resource.name()
        );
        poisoned.into_inner()
    });
    let counter = state.counters.entry(allocation.resource).or_default();
    if allocation.amount > counter.used {
        eprintln!(
            "ERR_AGENTOS_RESOURCE_ACCOUNTING_UNDERFLOW: scope={} resource={} used={} release={}",
            allocation.ledger.scope,
            allocation.resource.name(),
            counter.used,
            allocation.amount
        );
        allocation
            .ledger
            .integrity_failed
            .store(true, Ordering::Release);
        counter.used = 0;
    } else {
        counter.used -= allocation.amount;
    }
    if let Some(limit) = allocation.ledger.limits.get(&allocation.resource) {
        if counter.used.saturating_mul(100) < limit.maximum.saturating_mul(70) {
            counter.warning_active = false;
        }
    }
    observe_usage(&allocation.ledger, allocation.resource, counter.used);
    // `notify_one` retains a permit when no waiter is currently polled, which
    // closes the release-between-retry-and-await race.
    allocation.ledger.capacity_changed.notify_one();
    if allocation.resource == ResourceClass::BufferedBytes {
        allocation
            .ledger
            .buffered_bytes_release_generation
            .fetch_add(1, Ordering::Release);
        allocation.ledger.buffered_bytes_released.notify_waiters();
    }
}

fn observe_usage(inner: &LedgerInner, resource: ResourceClass, used: usize) {
    let Some(metrics) = &inner.metrics else {
        return;
    };
    match resource {
        ResourceClass::Capabilities => {
            metrics.observe_resource(ResourceMetricClass::Capabilities, used)
        }
        ResourceClass::ReadyHandles => {
            metrics.observe_resource(ResourceMetricClass::ReadyHandles, used)
        }
        ResourceClass::Sockets => metrics.observe_resource(ResourceMetricClass::Sockets, used),
        ResourceClass::Connections => {
            metrics.observe_resource(ResourceMetricClass::Connections, used)
        }
        ResourceClass::BufferedBytes => metrics.observe_buffer(BufferMetricClass::Native, used),
        ResourceClass::Datagrams => metrics.observe_resource(ResourceMetricClass::Datagrams, used),
        ResourceClass::HandleCommands => {
            metrics.observe_resource(ResourceMetricClass::HandleCommands, used)
        }
        ResourceClass::HandleCommandBytes => {
            metrics.observe_buffer(BufferMetricClass::Native, used)
        }
        ResourceClass::BridgeCalls => {
            metrics.observe_resource(ResourceMetricClass::BridgeCalls, used)
        }
        ResourceClass::BridgeRequestBytes | ResourceClass::BridgeResponseBytes => {
            metrics.observe_buffer(BufferMetricClass::Bridge, used)
        }
        ResourceClass::AsyncCompletions => {
            metrics.observe_resource(ResourceMetricClass::AsyncCompletions, used)
        }
        ResourceClass::AsyncCompletionBytes => {
            metrics.observe_buffer(BufferMetricClass::Bridge, used)
        }
        ResourceClass::UdpDatagrams => {
            metrics.observe_resource(ResourceMetricClass::Datagrams, used)
        }
        ResourceClass::UdpBytes => metrics.observe_buffer(BufferMetricClass::Datagram, used),
        ResourceClass::TlsBytes => metrics.observe_buffer(BufferMetricClass::Tls, used),
        ResourceClass::Timers => metrics.observe_resource(ResourceMetricClass::Timers, used),
        ResourceClass::Tasks => metrics.observe_resource(ResourceMetricClass::Tasks, used),
        ResourceClass::ExecutorSlots => {}
        ResourceClass::ExecutorBytes => metrics.observe_buffer(BufferMetricClass::Executor, used),
        ResourceClass::Http2BufferedBytes => metrics.observe_buffer(BufferMetricClass::Http2, used),
        ResourceClass::Http2Connections => {
            metrics.observe_resource(ResourceMetricClass::Http2Connections, used)
        }
        ResourceClass::Http2Streams => {
            metrics.observe_resource(ResourceMetricClass::Http2Streams, used)
        }
        ResourceClass::Http2HeaderBytes
        | ResourceClass::Http2DataBytes
        | ResourceClass::Http2Commands
        | ResourceClass::Http2CommandBytes
        | ResourceClass::Http2Events
        | ResourceClass::Http2EventBytes => {}
    }
}

fn release_allocations(allocations: &mut Vec<Allocation>) {
    for allocation in allocations.drain(..).rev() {
        release_allocation(&allocation);
    }
}

/// Exact ownership of one admitted amount. Moving the value transfers ownership;
/// dropping it releases every scope exactly once.
#[derive(Debug)]
pub struct Reservation {
    resource: ResourceClass,
    amount: usize,
    allocations: Vec<Allocation>,
}

/// Cloneable ownership used when a charged payload crosses an in-process
/// response router. The underlying reservation releases only after the final
/// envelope/consumer drops it.
#[derive(Clone)]
pub struct SharedReservation(Arc<Reservation>);

impl SharedReservation {
    pub fn new(reservation: Reservation) -> Self {
        Self(Arc::new(reservation))
    }

    pub fn resource(&self) -> ResourceClass {
        self.0.resource()
    }

    pub fn amount(&self) -> usize {
        self.0.amount()
    }
}

impl fmt::Debug for SharedReservation {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("SharedReservation")
            .field("resource", &self.resource())
            .field("amount", &self.amount())
            .finish_non_exhaustive()
    }
}

impl PartialEq for SharedReservation {
    fn eq(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.0, &other.0)
    }
}

impl Eq for SharedReservation {}

impl Reservation {
    pub fn resource(&self) -> ResourceClass {
        self.resource
    }

    pub fn amount(&self) -> usize {
        self.amount
    }

    /// Split ownership without changing any ledger counter.
    pub fn split(&mut self, amount: usize) -> Option<Self> {
        if amount > self.amount {
            return None;
        }
        self.amount -= amount;
        let mut allocations = Vec::with_capacity(self.allocations.len());
        for allocation in &mut self.allocations {
            allocation.amount -= amount;
            allocations.push(Allocation {
                ledger: Arc::clone(&allocation.ledger),
                resource: allocation.resource,
                amount,
            });
        }
        Some(Self {
            resource: self.resource,
            amount,
            allocations,
        })
    }

    /// Merge two reservations only when they refer to the same scopes and class.
    pub fn merge(&mut self, mut other: Self) -> Result<(), Self> {
        if self.resource != other.resource || self.allocations.len() != other.allocations.len() {
            return Err(other);
        }
        if self
            .allocations
            .iter()
            .zip(&other.allocations)
            .any(|(left, right)| !Arc::ptr_eq(&left.ledger, &right.ledger))
        {
            return Err(other);
        }
        let Some(total) = self.amount.checked_add(other.amount) else {
            return Err(other);
        };
        for (left, right) in self.allocations.iter_mut().zip(&other.allocations) {
            left.amount += right.amount;
        }
        self.amount = total;
        other.amount = 0;
        other.allocations.clear();
        Ok(())
    }
}

impl Drop for Reservation {
    fn drop(&mut self) {
        release_allocations(&mut self.allocations);
        self.amount = 0;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn limit(maximum: usize) -> [(ResourceClass, ResourceLimit); 1] {
        [(
            ResourceClass::BufferedBytes,
            ResourceLimit::new(maximum, "runtime.resources.maxSocketBufferedBytes"),
        )]
    }

    #[test]
    fn waiter_on_full_child_scope_parks_until_capacity_is_released() {
        let process = Arc::new(ResourceLedger::root("process", limit(10)));
        let vm = Arc::new(ResourceLedger::child(
            "vm-1",
            limit(2),
            Arc::clone(&process),
        ));
        let held = vm
            .reserve(ResourceClass::BufferedBytes, 2)
            .expect("fill child");

        let waiting_vm = Arc::clone(&vm);
        let admitted = crate::test_support::assert_parks_until_released(
            async move {
                waiting_vm
                    .reserve_when_available(ResourceClass::BufferedBytes, 1)
                    .await
            },
            move || drop(held),
        )
        .expect("released capacity admits the waiter");

        assert_eq!(process.usage(ResourceClass::BufferedBytes).used, 1);
        assert_eq!(vm.usage(ResourceClass::BufferedBytes).used, 1);
        drop(admitted);
        assert!(process.is_zero() && vm.is_zero());
    }

    #[test]
    fn rejected_admission_charges_no_scope() {
        let limits = [
            (
                ResourceClass::BufferedBytes,
                ResourceLimit::new(10, "runtime.resources.maxSocketBufferedBytes"),
            ),
            (
                ResourceClass::TlsBytes,
                ResourceLimit::new(10, "runtime.resources.maxTlsBytes"),
            ),
        ];
        let process = Arc::new(ResourceLedger::root("process", limits.clone()));
        let vm = ResourceLedger::child(
            "vm-1",
            [
                limits[0].clone(),
                (
                    ResourceClass::TlsBytes,
                    ResourceLimit::new(1, "limits.tls.maxBufferedBytes"),
                ),
            ],
            Arc::clone(&process),
        );

        let error = vm
            .reserve_all(&[
                (ResourceClass::BufferedBytes, 4),
                (ResourceClass::TlsBytes, 4),
            ])
            .expect_err("the VM TLS limit rejects the admission");

        assert_eq!(error.scope, "vm-1");
        assert_eq!(error.resource, ResourceClass::TlsBytes);
        assert!(
            process.is_zero(),
            "the process scope keeps no partial charge"
        );
        assert!(vm.is_zero(), "the VM scope keeps no partial charge");
    }

    #[test]
    fn child_reservation_charges_and_releases_both_scopes() {
        let process = Arc::new(ResourceLedger::root("process", limit(10)));
        let vm = ResourceLedger::child("vm-1", limit(6), Arc::clone(&process));
        let reservation = vm.reserve(ResourceClass::BufferedBytes, 6).unwrap();
        assert_eq!(process.usage(ResourceClass::BufferedBytes).used, 6);
        assert_eq!(vm.usage(ResourceClass::BufferedBytes).used, 6);
        let error = vm.reserve(ResourceClass::BufferedBytes, 1).unwrap_err();
        assert_eq!(error.scope, "vm-1");
        assert_eq!(process.usage(ResourceClass::BufferedBytes).used, 6);
        drop(reservation);
        assert!(process.is_zero());
        assert!(vm.is_zero());
    }

    #[test]
    fn failed_child_admission_rolls_back_parent() {
        let process = Arc::new(ResourceLedger::root("process", limit(10)));
        let vm = ResourceLedger::child("vm-1", limit(2), Arc::clone(&process));
        let error = vm.reserve(ResourceClass::BufferedBytes, 3).unwrap_err();
        assert_eq!(error.scope, "vm-1");
        assert!(process.is_zero());
        assert!(vm.is_zero());
    }

    #[test]
    fn capacity_probe_checks_parent_and_child_without_changing_usage() {
        let process = Arc::new(ResourceLedger::root("process", limit(3)));
        let vm = ResourceLedger::child("vm-1", limit(5), Arc::clone(&process));
        let held = process
            .reserve(ResourceClass::BufferedBytes, 2)
            .expect("reserve process capacity");

        assert!(vm.capacity_available(ResourceClass::BufferedBytes, 1));
        assert!(!vm.capacity_available(ResourceClass::BufferedBytes, 2));
        assert_eq!(process.usage(ResourceClass::BufferedBytes).used, 2);
        assert_eq!(vm.usage(ResourceClass::BufferedBytes).used, 0);

        drop(held);
        assert!(vm.capacity_available(ResourceClass::BufferedBytes, 2));
    }

    #[test]
    fn split_and_merge_transfer_without_counter_drift() {
        let ledger = ResourceLedger::root("vm-1", limit(10));
        let mut source = ledger.reserve(ResourceClass::BufferedBytes, 8).unwrap();
        let transferred = source.split(3).unwrap();
        assert_eq!(source.amount(), 5);
        assert_eq!(transferred.amount(), 3);
        assert_eq!(ledger.usage(ResourceClass::BufferedBytes).used, 8);
        source.merge(transferred).unwrap();
        assert_eq!(source.amount(), 8);
        assert_eq!(ledger.usage(ResourceClass::BufferedBytes).used, 8);
        drop(source);
        assert!(ledger.is_zero());
    }

    #[tokio::test]
    async fn impossible_async_reservation_returns_typed_error() {
        let ledger = ResourceLedger::root("vm-1", limit(4));
        let error = ledger
            .reserve_when_available(ResourceClass::BufferedBytes, 5)
            .await
            .unwrap_err();
        assert_eq!(error.resource, ResourceClass::BufferedBytes);
        assert_eq!(error.requested, 5);
        assert_eq!(error.limit, 4);
        assert_eq!(
            error.config_path,
            "runtime.resources.maxSocketBufferedBytes"
        );
    }

    #[tokio::test]
    async fn child_waiter_wakes_when_only_parent_capacity_changes() {
        let process = Arc::new(ResourceLedger::root("process", limit(1)));
        let vm = Arc::new(ResourceLedger::child(
            "vm-1",
            limit(2),
            Arc::clone(&process),
        ));
        let held = process
            .reserve(ResourceClass::BufferedBytes, 1)
            .expect("fill parent");
        let waiting_vm = Arc::clone(&vm);
        let waiter = tokio::spawn(async move {
            waiting_vm
                .reserve_when_available(ResourceClass::BufferedBytes, 1)
                .await
        });
        tokio::task::yield_now().await;
        assert!(!waiter.is_finished());
        drop(held);
        let reservation = tokio::time::timeout(std::time::Duration::from_secs(1), waiter)
            .await
            .expect("parent release must wake child waiter")
            .expect("waiter task")
            .expect("reservation");
        drop(reservation);
        assert!(process.is_zero());
        assert!(vm.is_zero());
    }

    #[test]
    fn accounting_underflow_latches_integrity_failure() {
        let ledger = ResourceLedger::root("vm-1", limit(1));
        let inner = Arc::clone(&ledger.inner);
        let malformed = Reservation {
            resource: ResourceClass::BufferedBytes,
            amount: 1,
            allocations: vec![Allocation {
                ledger: inner,
                resource: ResourceClass::BufferedBytes,
                amount: 1,
            }],
        };
        drop(malformed);
        assert!(!ledger.integrity_ok());
        assert!(ledger.is_zero());
    }
}
