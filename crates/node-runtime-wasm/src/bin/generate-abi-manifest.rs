use agentos_node_runtime_wasm::{
    ENGINE_IMPORT_MODULE, NAPI_IMPORT_MODULE, POSIX_IMPORT_MODULE, REQUIRED_REACTOR_EXPORTS,
};
use serde::Serialize;
use sha2::{Digest, Sha256};
use std::{collections::HashSet, env, error::Error, fs, path::PathBuf};
use wasmparser::{ExternalKind, FuncType, Parser, Payload, TypeRef};

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct Manifest {
    format_version: u32,
    wasm_sha256: String,
    wasm_bytes: usize,
    memories: Vec<MemoryLimit>,
    tables: Vec<TableLimit>,
    imports: Vec<AbiEntry>,
    exports: Vec<AbiEntry>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct ImportSource {
    module: String,
    name: String,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct MemoryLimit {
    index: u32,
    imported_from: Option<ImportSource>,
    initial_pages: u64,
    maximum_pages: Option<u64>,
    shared: bool,
    memory64: bool,
    page_size_log2: Option<u32>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct TableLimit {
    index: u32,
    imported_from: Option<ImportSource>,
    initial_elements: u64,
    maximum_elements: Option<u64>,
    shared: bool,
    table64: bool,
    element_type: String,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct AbiEntry {
    module: Option<String>,
    name: String,
    kind: String,
    signature: String,
    authority: String,
    result_classification: String,
    test_id: String,
}

fn import_result_classification(module: &str, name: &str, kind: &str) -> &'static str {
    if kind != "function" {
        return "imported-resource";
    }
    match module {
        NAPI_IMPORT_MODULE | ENGINE_IMPORT_MODULE => {
            if matches!(
                name,
                "unofficial_napi_free_buffer" | "unofficial_napi_release_serialized_value"
            ) {
                "void"
            } else {
                "napi-status"
            }
        }
        POSIX_IMPORT_MODULE => match name {
            "proc_exit" => "noreturn",
            "thread-spawn" => "positive-thread-id-or-negative-error",
            "fd_size" | "path_size" => "u64-size-or-max-sentinel",
            _ => "wasi-errno",
        },
        "env" => "imported-resource",
        _ => "forbidden",
    }
}

fn export_result_classification(name: &str, kind: &str) -> &'static str {
    if kind != "function" {
        return "exported-resource";
    }
    match name {
        "_initialize" => "void",
        "agentos_node_runtime_alloc" => "guest-pointer-or-zero",
        "agentos_node_runtime_free"
        | "agentos_node_runtime_create"
        | "agentos_node_runtime_bootstrap"
        | "agentos_node_runtime_run"
        | "agentos_node_runtime_interrupt"
        | "agentos_node_runtime_teardown" => "reactor-status",
        "agentos_node_runtime_quiescence" => "boolean-i32",
        "agentos_node_runtime_last_error" => "length-or-negative-status",
        "agentos_node_runtime_allocated_bytes" | "agentos_node_runtime_allocation_count" => {
            "nonnegative-counter"
        }
        _ => "direct-value",
    }
}

fn import_authority(module: &str) -> Result<&'static str, Box<dyn Error>> {
    match module {
        NAPI_IMPORT_MODULE | ENGINE_IMPORT_MODULE => Ok("isolate-local-v8"),
        POSIX_IMPORT_MODULE => Ok("vm-kernel-linux-posix"),
        "env" => Ok("wasm-linear-memory"),
        _ => Err(format!("forbidden import module in normalized runtime: {module}").into()),
    }
}

fn function_signature(types: &[FuncType], index: u32) -> Result<String, Box<dyn Error>> {
    let ty = types
        .get(index as usize)
        .ok_or_else(|| format!("function type index {index} is out of range"))?;
    Ok(ty.to_string())
}

fn external_kind(kind: ExternalKind) -> &'static str {
    match kind {
        ExternalKind::Func | ExternalKind::FuncExact => "function",
        ExternalKind::Table => "table",
        ExternalKind::Memory => "memory",
        ExternalKind::Global => "global",
        ExternalKind::Tag => "tag",
    }
}

fn validate_runtime_shape(
    imports: &[AbiEntry],
    exports: &[AbiEntry],
    memories: &[MemoryLimit],
    tables: &[TableLimit],
) -> Result<(), Box<dyn Error>> {
    if memories.len() != 1 {
        return Err(format!(
            "Node reactor must declare exactly one memory, saw {}",
            memories.len()
        )
        .into());
    }
    let memory = &memories[0];
    let source = memory
        .imported_from
        .as_ref()
        .ok_or("Node reactor memory must be imported")?;
    if source.module != "env"
        || source.name != "memory"
        || memory.initial_pages != 1024
        || memory.maximum_pages != Some(4096)
        || !memory.shared
        || memory.memory64
    {
        return Err(
            "Node reactor memory must be env.memory, shared wasm32, 1024..4096 pages".into(),
        );
    }

    if tables.len() != 1 {
        return Err(format!(
            "Node reactor must declare exactly one table, saw {}",
            tables.len()
        )
        .into());
    }
    let table = &tables[0];
    if table.imported_from.is_some()
        || table.shared
        || table.table64
        || table.maximum_elements.is_none()
        || table.maximum_elements != Some(table.initial_elements)
        || table
            .maximum_elements
            .is_some_and(|maximum| maximum > 16_384)
    {
        return Err(
            "Node reactor callback table must be local, fixed, wasm32, and at most 16384 elements"
                .into(),
        );
    }

    let mut imported_names = HashSet::new();
    for import in imports {
        let key = (
            import.module.as_deref().unwrap_or_default(),
            import.name.as_str(),
        );
        if !imported_names.insert(key) {
            return Err(format!("duplicate Node reactor import {}.{}", key.0, key.1).into());
        }
    }

    let export_names = exports
        .iter()
        .map(|entry| entry.name.as_str())
        .collect::<HashSet<_>>();
    for required in REQUIRED_REACTOR_EXPORTS {
        if !export_names.contains(required) {
            return Err(format!("Node reactor is missing required export {required}").into());
        }
    }
    if export_names.contains("_start") {
        return Err("command-style _start is forbidden in the Node reactor".into());
    }
    Ok(())
}

fn main() -> Result<(), Box<dyn Error>> {
    let mut args = env::args_os().skip(1);
    let input = PathBuf::from(
        args.next()
            .ok_or("usage: generate-abi-manifest WASM [OUTPUT]")?,
    );
    let output = args.next().map(PathBuf::from);
    if args.next().is_some() {
        return Err("usage: generate-abi-manifest WASM [OUTPUT]".into());
    }

    let bytes = fs::read(&input)?;
    let mut types = Vec::new();
    let mut function_type_indices = Vec::new();
    let mut memories = Vec::new();
    let mut tables = Vec::new();
    let mut imports = Vec::new();
    let mut exports = Vec::new();

    for payload in Parser::new(0).parse_all(&bytes) {
        match payload? {
            Payload::TypeSection(section) => {
                for ty in section.into_iter_err_on_gc_types() {
                    types.push(ty?);
                }
            }
            Payload::ImportSection(section) => {
                for import in section.into_imports() {
                    let import = import?;
                    let (kind, signature) = match import.ty {
                        TypeRef::Func(index) | TypeRef::FuncExact(index) => {
                            function_type_indices.push(index);
                            ("function", function_signature(&types, index)?)
                        }
                        TypeRef::Memory(memory) => (
                            "memory",
                            format!(
                                "memory(min={},max={},shared={},memory64={})",
                                memory.initial,
                                memory
                                    .maximum
                                    .map_or_else(|| "none".to_owned(), |value| value.to_string()),
                                memory.shared,
                                memory.memory64
                            ),
                        ),
                        TypeRef::Table(table) => ("table", format!("{table:?}")),
                        TypeRef::Global(global) => ("global", format!("{global:?}")),
                        TypeRef::Tag(tag) => ("tag", format!("{tag:?}")),
                    };
                    match import.ty {
                        TypeRef::Memory(memory) => memories.push(MemoryLimit {
                            index: memories.len() as u32,
                            imported_from: Some(ImportSource {
                                module: import.module.to_owned(),
                                name: import.name.to_owned(),
                            }),
                            initial_pages: memory.initial,
                            maximum_pages: memory.maximum,
                            shared: memory.shared,
                            memory64: memory.memory64,
                            page_size_log2: memory.page_size_log2,
                        }),
                        TypeRef::Table(table) => tables.push(TableLimit {
                            index: tables.len() as u32,
                            imported_from: Some(ImportSource {
                                module: import.module.to_owned(),
                                name: import.name.to_owned(),
                            }),
                            initial_elements: table.initial,
                            maximum_elements: table.maximum,
                            shared: table.shared,
                            table64: table.table64,
                            element_type: format!("{:?}", table.element_type),
                        }),
                        _ => {}
                    }
                    let result_classification =
                        import_result_classification(import.module, import.name, kind).to_owned();
                    imports.push(AbiEntry {
                        module: Some(import.module.to_owned()),
                        name: import.name.to_owned(),
                        kind: kind.to_owned(),
                        signature,
                        authority: import_authority(import.module)?.to_owned(),
                        result_classification,
                        test_id: format!("abi:import:{}:{}", import.module, import.name),
                    });
                }
            }
            Payload::FunctionSection(section) => {
                for index in section {
                    function_type_indices.push(index?);
                }
            }
            Payload::MemorySection(section) => {
                for memory in section {
                    let memory = memory?;
                    memories.push(MemoryLimit {
                        index: memories.len() as u32,
                        imported_from: None,
                        initial_pages: memory.initial,
                        maximum_pages: memory.maximum,
                        shared: memory.shared,
                        memory64: memory.memory64,
                        page_size_log2: memory.page_size_log2,
                    });
                }
            }
            Payload::TableSection(section) => {
                for table in section {
                    let table = table?.ty;
                    tables.push(TableLimit {
                        index: tables.len() as u32,
                        imported_from: None,
                        initial_elements: table.initial,
                        maximum_elements: table.maximum,
                        shared: table.shared,
                        table64: table.table64,
                        element_type: format!("{:?}", table.element_type),
                    });
                }
            }
            Payload::ExportSection(section) => {
                for export in section {
                    let export = export?;
                    let signature = match export.kind {
                        ExternalKind::Func | ExternalKind::FuncExact => {
                            let type_index = function_type_indices
                                .get(export.index as usize)
                                .ok_or_else(|| {
                                    format!(
                                        "exported function index {} is out of range",
                                        export.index
                                    )
                                })?;
                            function_signature(&types, *type_index)?
                        }
                        ExternalKind::Memory => memories
                            .get(export.index as usize)
                            .map(|memory| {
                                format!(
                                    "memory(min={},max={},shared={},memory64={})",
                                    memory.initial_pages,
                                    memory.maximum_pages.map_or_else(
                                        || "none".to_owned(),
                                        |value| value.to_string()
                                    ),
                                    memory.shared,
                                    memory.memory64
                                )
                            })
                            .ok_or_else(|| {
                                format!("exported memory {} is out of range", export.index)
                            })?,
                        ExternalKind::Table => tables
                            .get(export.index as usize)
                            .map(|table| {
                                format!(
                                    "table(min={},max={},shared={},table64={},element={})",
                                    table.initial_elements,
                                    table.maximum_elements.map_or_else(
                                        || "none".to_owned(),
                                        |value| value.to_string()
                                    ),
                                    table.shared,
                                    table.table64,
                                    table.element_type
                                )
                            })
                            .ok_or_else(|| {
                                format!("exported table {} is out of range", export.index)
                            })?,
                        _ => format!("index({})", export.index),
                    };
                    let kind = external_kind(export.kind);
                    exports.push(AbiEntry {
                        module: None,
                        name: export.name.to_owned(),
                        kind: kind.to_owned(),
                        signature,
                        authority: "wasm-reactor".to_owned(),
                        result_classification: export_result_classification(export.name, kind)
                            .to_owned(),
                        test_id: format!("abi:export:{}", export.name),
                    });
                }
            }
            _ => {}
        }
    }

    imports.sort_by(|left, right| {
        left.module
            .cmp(&right.module)
            .then_with(|| left.name.cmp(&right.name))
    });
    exports.sort_by(|left, right| left.name.cmp(&right.name));
    validate_runtime_shape(&imports, &exports, &memories, &tables)?;
    let manifest = Manifest {
        format_version: 3,
        wasm_sha256: format!("{:x}", Sha256::digest(&bytes)),
        wasm_bytes: bytes.len(),
        memories,
        tables,
        imports,
        exports,
    };
    let json = serde_json::to_string_pretty(&manifest)? + "\n";
    if let Some(output) = output {
        fs::write(output, json)?;
    } else {
        print!("{json}");
    }
    Ok(())
}
