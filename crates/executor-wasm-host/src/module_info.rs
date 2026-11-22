//! Import and entrypoint facts read from module bytes.
//!
//! An engine that cannot reflect import signatures (V8's public API reports
//! only names and kinds) reads them here, so import validation and exec
//! preflight run the same registry checks as an engine that can.

use crate::abi::CoreValueType;
use crate::backend::HostServiceError;
use crate::imports::{ImportDesc, ImportKind};
use wasmparser::{ExternalKind, Parser, Payload, TypeRef, ValType};

/// The module facts the host imports need before instantiation.
#[derive(Debug)]
pub struct ModuleInfo<'a> {
    pub imports: Vec<ImportDesc<'a>>,
    /// Whether `_start` is exported as a function of type `() -> ()`.
    pub has_valid_start: bool,
    /// Whether `__wasi_signal_trampoline` is exported as `(i32) -> ()`.
    pub has_signal_trampoline: bool,
    /// Whether `__agentos_set_initial_sigmask` is exported as
    /// `(i32, i32) -> i32`.
    pub has_initial_sigmask_setter: bool,
    /// Whether the module imports the WASI threads runtime (`wasi`
    /// `thread-spawn` or a shared `env.memory`).
    pub uses_thread_runtime: bool,
}

/// Read imports and the `_start` signature from a module that has already
/// passed profile validation.
pub fn inspect_module(bytes: &[u8]) -> Result<ModuleInfo<'_>, HostServiceError> {
    let mut types = Vec::new();
    let mut function_types = Vec::new();
    let mut imports = Vec::new();
    let mut exports = Vec::new();
    for payload in Parser::new(0).parse_all(bytes) {
        match payload.map_err(invalid_module)? {
            Payload::TypeSection(reader) => {
                for ty in reader.into_iter_err_on_gc_types() {
                    let ty = ty.map_err(invalid_module)?;
                    types.push((
                        ty.params().iter().copied().map(core_type).collect::<Option<Vec<_>>>(),
                        ty.results().iter().copied().map(core_type).collect::<Option<Vec<_>>>(),
                    ));
                }
            }
            Payload::ImportSection(reader) => {
                for import in reader.into_imports() {
                    let import = import.map_err(invalid_module)?;
                    let kind = match import.ty {
                        TypeRef::Func(index) => {
                            function_types.push(index);
                            match types.get(index as usize) {
                                Some((Some(params), Some(results))) => ImportKind::Func {
                                    params: params.clone(),
                                    results: results.clone(),
                                },
                                _ => ImportKind::Other,
                            }
                        }
                        TypeRef::Memory(memory) => ImportKind::Memory {
                            shared: memory.shared,
                            memory64: memory.memory64,
                        },
                        _ => ImportKind::Other,
                    };
                    imports.push(ImportDesc {
                        module: import.module,
                        name: import.name,
                        kind,
                    });
                }
            }
            Payload::FunctionSection(reader) => {
                for index in reader {
                    function_types.push(index.map_err(invalid_module)?);
                }
            }
            Payload::ExportSection(reader) => {
                for export in reader {
                    let export = export.map_err(invalid_module)?;
                    if export.kind == ExternalKind::Func {
                        exports.push((export.name, export.index));
                    }
                }
            }
            _ => {}
        }
    }
    let export_is = |name: &str, params: &[CoreValueType], results: &[CoreValueType]| {
        exports
            .iter()
            .find(|(export, _)| *export == name)
            .and_then(|(_, function)| function_types.get(*function as usize))
            .and_then(|ty| types.get(*ty as usize))
            .is_some_and(|(actual_params, actual_results)| {
                actual_params.as_deref() == Some(params) && actual_results.as_deref() == Some(results)
            })
    };
    use CoreValueType::I32;
    let has_valid_start = export_is("_start", &[], &[]);
    let has_signal_trampoline = export_is("__wasi_signal_trampoline", &[I32], &[]);
    let has_initial_sigmask_setter = export_is("__agentos_set_initial_sigmask", &[I32, I32], &[I32]);
    let uses_thread_runtime = imports.iter().any(|import| {
        matches!(
            (import.module, import.name, &import.kind),
            ("wasi", "thread-spawn", _) | ("env", "memory", ImportKind::Memory { shared: true, .. })
        )
    });
    Ok(ModuleInfo {
        imports,
        has_valid_start,
        has_signal_trampoline,
        has_initial_sigmask_setter,
        uses_thread_runtime,
    })
}

fn core_type(value: ValType) -> Option<CoreValueType> {
    match value {
        ValType::I32 => Some(CoreValueType::I32),
        ValType::I64 => Some(CoreValueType::I64),
        _ => None,
    }
}

fn invalid_module(error: wasmparser::BinaryReaderError) -> HostServiceError {
    HostServiceError::new(
        "ERR_AGENTOS_WASM_INVALID_MODULE",
        format!("WebAssembly module could not be read: {}", error.message()),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_import_signatures_and_start() {
        let bytes = wat::parse_str(
            r#"(module
                (import "wasi_snapshot_preview1" "fd_write" (func (param i32 i32 i32 i32) (result i32)))
                (import "env" "memory" (memory 1))
                (func (export "_start")))"#,
        )
        .expect("module");
        let info = inspect_module(&bytes).expect("inspect");
        assert!(info.has_valid_start);
        assert!(!info.has_signal_trampoline);
        assert!(!info.uses_thread_runtime);
        assert_eq!(info.imports.len(), 2);
        assert!(matches!(
            &info.imports[0].kind,
            ImportKind::Func { params, results } if params.len() == 4 && results.len() == 1
        ));
        assert!(matches!(
            info.imports[1].kind,
            ImportKind::Memory { shared: false, memory64: false }
        ));
    }

    #[test]
    fn reads_runtime_export_signatures() {
        let bytes = wat::parse_str(
            r#"(module
                (func (export "__wasi_signal_trampoline") (param i32))
                (func (export "__agentos_set_initial_sigmask") (param i32 i32) (result i32) i32.const 0))"#,
        )
        .expect("module");
        let info = inspect_module(&bytes).expect("inspect");
        assert!(info.has_signal_trampoline);
        assert!(info.has_initial_sigmask_setter);
        assert!(!info.has_valid_start);
    }

    #[test]
    fn start_with_parameters_is_not_an_entrypoint() {
        let bytes = wat::parse_str(r#"(module (func (export "_start") (param i32)))"#)
            .expect("module");
        assert!(!inspect_module(&bytes).expect("inspect").has_valid_start);
    }
}
