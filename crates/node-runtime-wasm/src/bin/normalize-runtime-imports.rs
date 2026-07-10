use std::{env, error::Error, fs, io, path::PathBuf};

use agentos_node_runtime_wasm::{ENGINE_IMPORT_MODULE, NAPI_IMPORT_MODULE, POSIX_IMPORT_MODULE};
use wasm_encoder::{reencode::Reencode, ImportSection, Module};
use wasmparser::{Imports, Parser, TypeRef};

struct AgentOsImportNormalizer;

impl AgentOsImportNormalizer {
    fn module<'a>(module: &'a str, name: &str, ty: TypeRef) -> Result<&'a str, io::Error> {
        match module {
            "wasi_snapshot_preview1" | "wasi_unstable" | "wasi" => Ok(POSIX_IMPORT_MODULE),
            NAPI_IMPORT_MODULE | ENGINE_IMPORT_MODULE | POSIX_IMPORT_MODULE => Ok(module),
            "env" if name == "memory" && matches!(ty, TypeRef::Memory(_)) => Ok(module),
            _ => Err(io::Error::other(format!(
                "forbidden Node runtime import {module}.{name}"
            ))),
        }
    }
}

impl Reencode for AgentOsImportNormalizer {
    type Error = io::Error;

    fn parse_imports(
        &mut self,
        section: &mut ImportSection,
        imports: Imports<'_>,
    ) -> Result<(), wasm_encoder::reencode::Error<Self::Error>> {
        match imports {
            Imports::Single(_, import) => {
                let module = Self::module(import.module, import.name, import.ty)
                    .map_err(wasm_encoder::reencode::Error::UserError)?;
                section.import(module, import.name, self.entity_type(import.ty)?);
            }
            Imports::Compact1 { module, items } => {
                for item in items {
                    let item = item?;
                    let normalized = Self::module(module, item.name, item.ty)
                        .map_err(wasm_encoder::reencode::Error::UserError)?;
                    section.import(normalized, item.name, self.entity_type(item.ty)?);
                }
            }
            Imports::Compact2 { module, ty, names } => {
                for name in names {
                    let name = name?;
                    let normalized = Self::module(module, name, ty)
                        .map_err(wasm_encoder::reencode::Error::UserError)?;
                    section.import(normalized, name, self.entity_type(ty)?);
                }
            }
        }
        Ok(())
    }
}

fn main() -> Result<(), Box<dyn Error>> {
    let mut args = env::args_os().skip(1);
    let input = PathBuf::from(
        args.next()
            .ok_or("usage: normalize-runtime-imports INPUT OUTPUT")?,
    );
    let output = PathBuf::from(
        args.next()
            .ok_or("usage: normalize-runtime-imports INPUT OUTPUT")?,
    );
    if args.next().is_some() {
        return Err("usage: normalize-runtime-imports INPUT OUTPUT".into());
    }

    let bytes = fs::read(input)?;
    let mut module = Module::new();
    AgentOsImportNormalizer.parse_core_module(&mut module, Parser::new(0), &bytes)?;
    fs::write(output, module.finish())?;
    Ok(())
}
