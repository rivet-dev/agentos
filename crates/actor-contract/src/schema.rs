use std::any::TypeId;
use std::collections::{BTreeMap, BTreeSet};

use serde::Serialize;
use sha2::{Digest, Sha256};
use ts_rs::TS;

#[derive(Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ActorContract {
    pub schema_version: u32,
    pub api_version: &'static str,
    pub contract_major: u32,
    pub contract_hash: String,
    pub capabilities: &'static [&'static str],
    pub actor_name: &'static str,
    pub create_input: TypeScriptShape,
    pub actions: Vec<ActionContract>,
    pub events: Vec<EventContract>,
    pub types: Vec<TypeContract>,
    pub error: TypeScriptShape,
}

#[derive(Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TypeScriptShape {
    pub input: String,
    pub output: String,
}

#[derive(Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ActionContract {
    pub name: &'static str,
    pub public: bool,
    pub input: String,
    pub output: String,
}

#[derive(Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct EventContract {
    pub name: &'static str,
    pub payload: String,
}

#[derive(Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TypeContract {
    pub name: String,
    pub declaration: String,
}

pub struct ContractMetadata {
    pub schema_version: u32,
    pub api_version: &'static str,
    pub contract_major: u32,
    pub capabilities: &'static [&'static str],
    pub actor_name: &'static str,
}

pub fn build(
    metadata: ContractMetadata,
    create_input: TypeScriptShape,
    actions: Vec<ActionContract>,
    events: Vec<EventContract>,
    types: Vec<TypeContract>,
    error: TypeScriptShape,
) -> ActorContract {
    let contract_hash = contract_hash(&metadata, &create_input, &actions, &events, &types, &error);
    ActorContract {
        schema_version: metadata.schema_version,
        api_version: metadata.api_version,
        contract_major: metadata.contract_major,
        contract_hash,
        capabilities: metadata.capabilities,
        actor_name: metadata.actor_name,
        create_input,
        actions,
        events,
        types,
        error,
    }
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct PublicContractHashInput<'a> {
    api_version: &'static str,
    contract_major: u32,
    capabilities: &'static [&'static str],
    actor_name: &'static str,
    create_input: &'a TypeScriptShape,
    actions: Vec<&'a ActionContract>,
    events: &'a [EventContract],
    types: &'a [TypeContract],
    error: &'a TypeScriptShape,
}

fn contract_hash(
    metadata: &ContractMetadata,
    create_input: &TypeScriptShape,
    actions: &[ActionContract],
    events: &[EventContract],
    types: &[TypeContract],
    error: &TypeScriptShape,
) -> String {
    let public = PublicContractHashInput {
        api_version: metadata.api_version,
        contract_major: metadata.contract_major,
        capabilities: metadata.capabilities,
        actor_name: metadata.actor_name,
        create_input,
        actions: actions.iter().filter(|action| action.public).collect(),
        events,
        types,
        error,
    };
    let canonical = serde_json::to_vec(&public).expect("serialize public agentOS contract");
    format!("sha256:{}", hex::encode(Sha256::digest(canonical)))
}

#[derive(Default)]
pub struct TypeCollector {
    seen: BTreeSet<TypeId>,
    declarations: BTreeMap<String, String>,
}

impl TypeCollector {
    pub fn collect<T: TS + 'static + ?Sized>(&mut self) {
        <Self as ts_rs::TypeVisitor>::visit::<T>(self);
    }

    pub fn finish(self) -> Vec<TypeContract> {
        self.declarations
            .into_iter()
            .map(|(name, declaration)| TypeContract { name, declaration })
            .collect()
    }
}

impl ts_rs::TypeVisitor for TypeCollector {
    fn visit<T: TS + 'static + ?Sized>(&mut self) {
        if !self.seen.insert(TypeId::of::<T>()) {
            return;
        }
        if T::output_path().is_some() {
            let name = T::ident();
            let declaration = T::decl();
            if let Some(previous) = self.declarations.insert(name.clone(), declaration.clone()) {
                assert_eq!(
                    previous, declaration,
                    "two Rust DTOs export the conflicting TypeScript name {name}"
                );
            }
        }
        T::visit_dependencies(self);
    }
}

pub fn input<T: TS>() -> String {
    normalize(T::inline())
}

pub fn output<T: TS>() -> String {
    normalize(T::inline())
}

pub fn shape<T: TS>() -> TypeScriptShape {
    let inline = T::inline();
    TypeScriptShape {
        input: normalize(inline.clone()),
        output: normalize(inline),
    }
}

fn normalize(value: String) -> String {
    value.replace("bigint", "number | bigint")
}
