#![forbid(unsafe_code)]

//! agentOS sidecar composition root.

pub mod transport;

pub fn extensions() -> Vec<Box<dyn agentos_vm::Extension>> {
    Vec::new()
}

pub fn executor_registry() -> agentos_vm::ExecutorRegistry {
    let registry = agentos_vm::ExecutorRegistry::empty();
    #[cfg(feature = "node-v8")]
    let registry = registry.with(agentos_vm::ExecutorKind::NodeV8);
    #[cfg(feature = "python-v8-pyodide")]
    let registry = registry.with(agentos_vm::ExecutorKind::PythonV8Pyodide);
    #[cfg(feature = "wasm-v8")]
    let registry = registry.with(agentos_vm::ExecutorKind::WasmV8);
    #[cfg(feature = "wasm-wasmtime")]
    let registry = registry.with(agentos_vm::ExecutorKind::WasmWasmtime);
    #[cfg(feature = "wasm-wasmtime-threads")]
    let registry = registry.with(agentos_vm::ExecutorKind::WasmWasmtimeThreads);
    registry
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn executor_registry_matches_enabled_sidecar_features() {
        let registry = executor_registry();
        assert_eq!(
            registry.contains(agentos_vm::ExecutorKind::NodeV8),
            cfg!(feature = "node-v8")
        );
        assert_eq!(
            registry.contains(agentos_vm::ExecutorKind::PythonV8Pyodide),
            cfg!(feature = "python-v8-pyodide")
        );
        assert_eq!(
            registry.contains(agentos_vm::ExecutorKind::WasmV8),
            cfg!(feature = "wasm-v8")
        );
        assert_eq!(
            registry.contains(agentos_vm::ExecutorKind::WasmWasmtime),
            cfg!(feature = "wasm-wasmtime")
        );
    }
}
