//! Isolate-local Node-API capability primitives.
//!
//! Guest `napi_value` and related handles are opaque nonzero wasm32 integers,
//! never host pointers or V8 representations. IDs are allocated process-wide
//! and are never reused, so a stale or cross-environment handle cannot alias a
//! later live value even when guest code forges arbitrary `u32` values.

use std::collections::{HashMap, HashSet};
use std::fmt;
use std::num::{NonZeroU32, NonZeroU64};
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering};

pub const MAX_NAPI_VALUES_FIELD: &str = "limits.nodeRuntime.maxNapiValues";
pub const DEFAULT_MAX_NAPI_VALUES: usize = 65_536;

static NEXT_ENVIRONMENT_ID: AtomicU64 = AtomicU64::new(1);
static NEXT_HANDLE_ID: AtomicU32 = AtomicU32::new(1);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct EnvironmentId(NonZeroU64);

impl EnvironmentId {
    pub fn allocate() -> Result<Self, CapabilityError> {
        let id = NEXT_ENVIRONMENT_ID
            .fetch_update(Ordering::AcqRel, Ordering::Acquire, |current| {
                current.checked_add(1)
            })
            .map_err(|_| CapabilityError::IdentifierSpaceExhausted {
                kind: "Node-API environment",
            })?;
        if id == 0 {
            return Err(CapabilityError::IdentifierSpaceExhausted {
                kind: "Node-API environment",
            });
        }
        NonZeroU64::new(id)
            .map(Self)
            .ok_or(CapabilityError::IdentifierSpaceExhausted {
                kind: "Node-API environment",
            })
    }

    pub const fn get(self) -> u64 {
        self.0.get()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct HandleId(NonZeroU32);

impl HandleId {
    pub const fn get(self) -> u32 {
        self.0.get()
    }

    pub fn from_guest(value: u32) -> Result<Self, CapabilityError> {
        NonZeroU32::new(value)
            .map(Self)
            .ok_or(CapabilityError::InvalidHandle { handle: value })
    }

    fn allocate() -> Result<Self, CapabilityError> {
        let id = NEXT_HANDLE_ID
            .fetch_update(Ordering::AcqRel, Ordering::Acquire, |current| {
                current.checked_add(1)
            })
            .map_err(|_| CapabilityError::IdentifierSpaceExhausted {
                kind: "Node-API handle",
            })?;
        NonZeroU32::new(id)
            .map(Self)
            .ok_or(CapabilityError::IdentifierSpaceExhausted {
                kind: "Node-API handle",
            })
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum CapabilityKind {
    Value,
    Reference,
    Deferred,
    HandleScope,
    CallbackInfo,
    AsyncContext,
    AsyncWork,
    ThreadsafeFunction,
    Wrap,
    TypeTag,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CapabilityError {
    LimitExceeded {
        field: &'static str,
        configured: usize,
    },
    IdentifierSpaceExhausted {
        kind: &'static str,
    },
    InvalidHandle {
        handle: u32,
    },
    WrongEnvironment {
        handle: u32,
        expected: u64,
        actual: u64,
    },
    WrongKind {
        handle: u32,
        expected: CapabilityKind,
        actual: CapabilityKind,
    },
}

impl fmt::Display for CapabilityError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::LimitExceeded { field, configured } => write!(
                formatter,
                "{field}={configured} exhausted; raise the typed VM limit to admit more handles"
            ),
            Self::IdentifierSpaceExhausted { kind } => {
                write!(formatter, "process-wide {kind} identifier space exhausted")
            }
            Self::InvalidHandle { handle } => write!(formatter, "invalid Node-API handle {handle}"),
            Self::WrongEnvironment {
                handle,
                expected,
                actual,
            } => write!(
                formatter,
                "Node-API handle {handle} belongs to environment {actual}, not {expected}"
            ),
            Self::WrongKind {
                handle,
                expected,
                actual,
            } => write!(
                formatter,
                "Node-API handle {handle} has kind {actual:?}, expected {expected:?}"
            ),
        }
    }
}

impl std::error::Error for CapabilityError {}

struct Entry<T> {
    environment: EnvironmentId,
    kind: CapabilityKind,
    scope: Option<HandleId>,
    value: T,
}

pub struct CapabilityTable<T> {
    environment: EnvironmentId,
    limit_field: &'static str,
    max_live: usize,
    entries: HashMap<HandleId, Entry<T>>,
    warning_emitted: AtomicBool,
}

impl<T> CapabilityTable<T> {
    pub fn new(environment: EnvironmentId, max_live: usize) -> Result<Self, CapabilityError> {
        Self::new_with_field(environment, MAX_NAPI_VALUES_FIELD, max_live)
    }

    pub fn new_with_field(
        environment: EnvironmentId,
        limit_field: &'static str,
        max_live: usize,
    ) -> Result<Self, CapabilityError> {
        if max_live == 0 {
            return Err(CapabilityError::LimitExceeded {
                field: limit_field,
                configured: 0,
            });
        }
        Ok(Self {
            environment,
            limit_field,
            max_live,
            entries: HashMap::new(),
            warning_emitted: AtomicBool::new(false),
        })
    }

    pub fn insert(
        &mut self,
        kind: CapabilityKind,
        scope: Option<HandleId>,
        value: T,
    ) -> Result<HandleId, CapabilityError> {
        if self.entries.len() >= self.max_live {
            return Err(CapabilityError::LimitExceeded {
                field: self.limit_field,
                configured: self.max_live,
            });
        }
        if let Some(scope) = scope {
            self.validate(scope, CapabilityKind::HandleScope)?;
        }
        let handle = HandleId::allocate()?;
        self.entries.insert(
            handle,
            Entry {
                environment: self.environment,
                kind,
                scope,
                value,
            },
        );
        let warning_at = self.max_live.saturating_mul(4).div_ceil(5);
        if self.entries.len() >= warning_at && !self.warning_emitted.swap(true, Ordering::AcqRel) {
            eprintln!(
                "agentos-node-api-v8: {} nearing limit: live={} configured={}",
                self.limit_field,
                self.entries.len(),
                self.max_live
            );
        }
        Ok(handle)
    }

    pub fn get(&self, handle: HandleId, expected: CapabilityKind) -> Result<&T, CapabilityError> {
        let entry = self.validate(handle, expected)?;
        Ok(&entry.value)
    }

    pub fn get_mut(
        &mut self,
        handle: HandleId,
        expected: CapabilityKind,
    ) -> Result<&mut T, CapabilityError> {
        self.validate(handle, expected)?;
        Ok(&mut self
            .entries
            .get_mut(&handle)
            .expect("validated entry")
            .value)
    }

    pub fn remove(
        &mut self,
        handle: HandleId,
        expected: CapabilityKind,
    ) -> Result<T, CapabilityError> {
        self.validate(handle, expected)?;
        Ok(self.entries.remove(&handle).expect("validated entry").value)
    }

    pub fn close_scope(&mut self, scope: HandleId) -> Result<T, CapabilityError> {
        self.validate(scope, CapabilityKind::HandleScope)?;
        let mut doomed = HashSet::from([scope]);
        loop {
            let previous = doomed.len();
            for (handle, entry) in &self.entries {
                if entry.scope.is_some_and(|parent| doomed.contains(&parent)) {
                    doomed.insert(*handle);
                }
            }
            if doomed.len() == previous {
                break;
            }
        }
        self.entries
            .retain(|handle, _| *handle == scope || !doomed.contains(handle));
        self.remove(scope, CapabilityKind::HandleScope)
    }

    pub fn clear(&mut self) {
        self.entries.clear();
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    fn validate(
        &self,
        handle: HandleId,
        expected: CapabilityKind,
    ) -> Result<&Entry<T>, CapabilityError> {
        let entry = self
            .entries
            .get(&handle)
            .ok_or(CapabilityError::InvalidHandle {
                handle: handle.get(),
            })?;
        if entry.environment != self.environment {
            return Err(CapabilityError::WrongEnvironment {
                handle: handle.get(),
                expected: self.environment.get(),
                actual: entry.environment.get(),
            });
        }
        if entry.kind != expected {
            return Err(CapabilityError::WrongKind {
                handle: handle.get(),
                expected,
                actual: entry.kind,
            });
        }
        Ok(entry)
    }
}

enum V8Capability {
    Value(v8::Global<v8::Value>),
    HandleScope,
}

/// Isolate-local owner for guest-visible `napi_value` handles and handle
/// scopes. All V8 globals are dropped by `teardown` while the isolate is live.
pub struct V8NodeApiEnvironment {
    id: EnvironmentId,
    capabilities: CapabilityTable<V8Capability>,
}

impl V8NodeApiEnvironment {
    pub fn new(max_values_and_scopes: usize) -> Result<Self, CapabilityError> {
        let id = EnvironmentId::allocate()?;
        Ok(Self {
            id,
            capabilities: CapabilityTable::new(id, max_values_and_scopes)?,
        })
    }

    pub const fn id(&self) -> EnvironmentId {
        self.id
    }

    pub fn open_handle_scope(&mut self) -> Result<HandleId, CapabilityError> {
        self.capabilities
            .insert(CapabilityKind::HandleScope, None, V8Capability::HandleScope)
    }

    pub fn add_value<'s>(
        &mut self,
        scope: &mut v8::HandleScope<'s>,
        handle_scope: HandleId,
        value: v8::Local<'s, v8::Value>,
    ) -> Result<HandleId, CapabilityError> {
        self.capabilities.insert(
            CapabilityKind::Value,
            Some(handle_scope),
            V8Capability::Value(v8::Global::new(scope, value)),
        )
    }

    pub fn value<'s>(
        &self,
        scope: &mut v8::HandleScope<'s>,
        handle: HandleId,
    ) -> Result<v8::Local<'s, v8::Value>, CapabilityError> {
        match self.capabilities.get(handle, CapabilityKind::Value)? {
            V8Capability::Value(value) => Ok(v8::Local::new(scope, value)),
            V8Capability::HandleScope => unreachable!("kind-checked capability payload"),
        }
    }

    pub fn close_handle_scope(&mut self, scope: HandleId) -> Result<(), CapabilityError> {
        let _ = self.capabilities.close_scope(scope)?;
        Ok(())
    }

    pub fn live_capabilities(&self) -> usize {
        self.capabilities.len()
    }

    /// Drop all V8 globals before isolate teardown. Calling code must keep the
    /// isolate alive until this returns.
    pub fn teardown(&mut self) {
        self.capabilities.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_zero_stale_wrong_kind_and_cross_environment_handles() {
        assert!(matches!(
            HandleId::from_guest(0),
            Err(CapabilityError::InvalidHandle { handle: 0 })
        ));
        let env_a = EnvironmentId::allocate().unwrap();
        let env_b = EnvironmentId::allocate().unwrap();
        let mut table_a = CapabilityTable::new(env_a, 4).unwrap();
        let table_b = CapabilityTable::<u32>::new(env_b, 4).unwrap();
        let handle = table_a.insert(CapabilityKind::Value, None, 41_u32).unwrap();
        assert!(matches!(
            table_a.get(handle, CapabilityKind::Reference),
            Err(CapabilityError::WrongKind { .. })
        ));
        assert!(matches!(
            table_b.get(handle, CapabilityKind::Value),
            Err(CapabilityError::InvalidHandle { .. })
        ));
        assert_eq!(table_a.remove(handle, CapabilityKind::Value).unwrap(), 41);
        assert!(matches!(
            table_a.get(handle, CapabilityKind::Value),
            Err(CapabilityError::InvalidHandle { .. })
        ));
        let replacement = table_a.insert(CapabilityKind::Value, None, 42).unwrap();
        assert_ne!(replacement, handle);
    }

    #[test]
    fn scope_close_drops_children_and_scope_without_reusing_ids() {
        let environment = EnvironmentId::allocate().unwrap();
        let mut table = CapabilityTable::new(environment, 8).unwrap();
        let scope = table
            .insert(CapabilityKind::HandleScope, None, "scope")
            .unwrap();
        let child = table
            .insert(CapabilityKind::Value, Some(scope), "child")
            .unwrap();
        let nested_scope = table
            .insert(CapabilityKind::HandleScope, Some(scope), "nested-scope")
            .unwrap();
        let nested_child = table
            .insert(CapabilityKind::Value, Some(nested_scope), "nested-child")
            .unwrap();
        let persistent = table
            .insert(CapabilityKind::Reference, None, "persistent")
            .unwrap();
        assert_eq!(table.close_scope(scope).unwrap(), "scope");
        assert!(matches!(
            table.get(child, CapabilityKind::Value),
            Err(CapabilityError::InvalidHandle { .. })
        ));
        assert!(matches!(
            table.get(nested_child, CapabilityKind::Value),
            Err(CapabilityError::InvalidHandle { .. })
        ));
        assert_eq!(
            table.get(persistent, CapabilityKind::Reference),
            Ok(&"persistent")
        );
    }

    #[test]
    fn live_handle_limit_fails_before_allocation_and_recovers_after_remove() {
        let environment = EnvironmentId::allocate().unwrap();
        let mut table = CapabilityTable::new(environment, 2).unwrap();
        let first = table.insert(CapabilityKind::Value, None, 1).unwrap();
        table.insert(CapabilityKind::Value, None, 2).unwrap();
        assert_eq!(
            table.insert(CapabilityKind::Value, None, 3),
            Err(CapabilityError::LimitExceeded {
                field: MAX_NAPI_VALUES_FIELD,
                configured: 2,
            })
        );
        assert_eq!(table.remove(first, CapabilityKind::Value).unwrap(), 1);
        assert!(table.insert(CapabilityKind::Value, None, 3).is_ok());
    }
}
