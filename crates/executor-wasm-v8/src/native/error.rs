//! Stable agentOS outcomes for V8 WebAssembly exceptions.
//!
//! The codes and trap kinds match the Wasmtime executor's classification so
//! both engines report the same failure the same way. V8's exception text is
//! a private diagnostic used only for classification.

use agentos_executor_contract::backend::HostServiceError;

/// Classify an exception thrown out of `_start` or a signal handler.
pub(super) fn classify_trap(diagnostic: &str) -> HostServiceError {
    // Ordered most specific first: V8 prefixes every trap with its class.
    const TRAPS: &[(&str, &str, &str)] = &[
        (
            "Maximum call stack size exceeded",
            "stack-overflow",
            "WebAssembly exhausted its call stack",
        ),
        (
            "unreachable",
            "unreachable",
            "WebAssembly executed an unreachable instruction",
        ),
        (
            "memory access out of bounds",
            "memory-out-of-bounds",
            "WebAssembly accessed memory outside its current bounds",
        ),
        (
            "operation does not support unaligned accesses",
            "misaligned-atomic",
            "WebAssembly attempted a misaligned atomic memory operation",
        ),
        (
            "table index is out of bounds",
            "table-out-of-bounds",
            "WebAssembly accessed a table outside its current bounds",
        ),
        (
            "invalid index into function table",
            "table-out-of-bounds",
            "WebAssembly accessed a table outside its current bounds",
        ),
        (
            "null function or function signature mismatch",
            "indirect-call-type-mismatch",
            "WebAssembly made an indirect call with the wrong function type",
        ),
        (
            "divide result unrepresentable",
            "integer-overflow",
            "WebAssembly integer arithmetic overflowed",
        ),
        (
            "divide by zero",
            "integer-division-by-zero",
            "WebAssembly attempted integer division by zero",
        ),
        (
            "remainder by zero",
            "integer-division-by-zero",
            "WebAssembly attempted integer division by zero",
        ),
        (
            "float unrepresentable in integer range",
            "invalid-float-to-integer",
            "WebAssembly attempted an invalid float-to-integer conversion",
        ),
        (
            "dereferencing a null pointer",
            "null-reference",
            "WebAssembly dereferenced a null reference",
        ),
        (
            "requested new array is too large",
            "allocation-too-large",
            "WebAssembly attempted an allocation that is too large",
        ),
    ];
    let (kind, message) = TRAPS
        .iter()
        .find(|(needle, _, _)| diagnostic.contains(needle))
        .map_or(("other", "WebAssembly trapped"), |(_, kind, message)| {
            (*kind, *message)
        });
    HostServiceError::new("ERR_AGENTOS_WASM_TRAP", message)
        .with_details(serde_json::json!({ "trapKind": kind }))
}

/// Instantiation failures (start function or data/element segment traps).
pub(super) fn instantiation_error() -> HostServiceError {
    HostServiceError::new(
        "ERR_AGENTOS_WASM_INSTANTIATION",
        "WebAssembly host imports do not match module requirements",
    )
}

pub(super) fn heap_limit_error() -> HostServiceError {
    HostServiceError::new(
        "ERR_AGENTOS_V8_HEAP_LIMIT",
        "the V8 isolate running this WebAssembly guest reached its heap limit",
    )
    .with_details(serde_json::json!({ "limitName": "limits.wasm.runnerHeapLimitMb" }))
}

pub(super) fn wall_clock_error(limit_ms: u64) -> HostServiceError {
    HostServiceError::new(
        "ERR_AGENTOS_WASMTIME_WALL_CLOCK_LIMIT",
        "WebAssembly execution exceeded its wall-clock budget",
    )
    .with_details(serde_json::json!({
        "limitName": "limits.resources.maxWasmWallClockTimeMs",
        "limit": limit_ms,
    }))
}

#[cfg(test)]
mod tests {
    use super::classify_trap;

    #[test]
    fn v8_trap_messages_map_to_shared_trap_kinds() {
        for (diagnostic, kind) in [
            ("RuntimeError: unreachable", "unreachable"),
            ("RuntimeError: memory access out of bounds", "memory-out-of-bounds"),
            ("RuntimeError: divide by zero", "integer-division-by-zero"),
            ("RuntimeError: divide result unrepresentable", "integer-overflow"),
            ("RangeError: Maximum call stack size exceeded", "stack-overflow"),
            ("Error: something else", "other"),
        ] {
            let error = classify_trap(diagnostic);
            assert_eq!(error.code, "ERR_AGENTOS_WASM_TRAP");
            assert_eq!(error.details.expect("details")["trapKind"], kind);
        }
    }
}
