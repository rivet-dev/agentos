//! Product metadata supplied to the shared agentOS contract generator.

use agentos_actor_contract::schema::{
    self, ActorContract, ContractMetadata, EventContract, TypeCollector, TypeScriptShape,
};

const API_VERSION: &str = "https://rivet.dev/agentos/v1alpha1";
const CONTRACT_MAJOR: u32 = 1;
const CAPABILITIES: &[&str] = &[
    "config.merge-patch",
    "network.pull-streaming",
    "output.replay",
    "inspector.filesystem",
    "inspector.processes",
    "inspector.software",
    "inspector.terminals",
    "inspector.vm",
    "language.javascript",
    "language.python",
    "language.typescript",
];

pub fn export() -> ActorContract {
    let mut types = TypeCollector::default();
    types.collect::<crate::AgentOsActorCreateInput>();
    crate::action_set::collect_contract_types(&mut types);
    collect_event_types(&mut types);

    schema::build(
        ContractMetadata {
            schema_version: 1,
            api_version: API_VERSION,
            contract_major: CONTRACT_MAJOR,
            capabilities: CAPABILITIES,
            actor_name: crate::ACTOR_NAME,
        },
        schema::shape::<crate::AgentOsActorCreateInput>(),
        crate::action_set::contract(),
        event_contract(),
        types.finish(),
        TypeScriptShape {
            input: "never".to_owned(),
            output: "{ group: string; code: string; message: string; metadata: JsonValue | null }"
                .to_owned(),
        },
    )
}

fn event_contract() -> Vec<EventContract> {
    use rivetkit::Event;

    macro_rules! events {
        ($($event:ty),+ $(,)?) => {
            vec![$(
                EventContract {
                    name: <$event as Event>::NAME,
                    payload: schema::output::<$event>(),
                }
            ),+]
        };
    }

    events!(
        crate::VmBooted,
        crate::VmShutdown,
        crate::VmLimitWarning,
        crate::ProcessOutputEvent,
        crate::ProcessExitEvent,
        crate::TerminalOutputEvent,
        crate::TerminalExitEvent,
        crate::CronFiredEvent,
    )
}

fn collect_event_types(types: &mut TypeCollector) {
    macro_rules! events {
        ($($event:ty),+ $(,)?) => {
            $(types.collect::<$event>();)+
        };
    }

    events!(
        crate::VmBooted,
        crate::VmShutdown,
        crate::VmLimitWarning,
        crate::ProcessOutputEvent,
        crate::ProcessExitEvent,
        crate::TerminalOutputEvent,
        crate::TerminalExitEvent,
        crate::CronFiredEvent,
    );
}

#[cfg(test)]
mod tests {
    use rivetkit::ActionSet;

    use super::*;

    #[test]
    fn contract_actions_match_registered_actions() {
        let registered = crate::action_set::AgentOsActionSet::entries()
            .into_iter()
            .map(|entry| entry.name)
            .collect::<Vec<_>>();
        let contract = export();
        let exported = contract
            .actions
            .iter()
            .map(|action| action.name)
            .collect::<Vec<_>>();
        assert_eq!(exported, registered);
        assert!(contract
            .actions
            .iter()
            .any(|action| !action.public && action.name == "__agentos.cron.invoke"));
    }

    #[test]
    fn contract_export_is_deterministic() {
        let first = export();
        let second = export();
        assert_eq!(first, second);
        assert!(first
            .actions
            .windows(2)
            .all(|actions| actions[0].name < actions[1].name));
        assert!(first
            .types
            .windows(2)
            .all(|types| types[0].name < types[1].name));
    }

    #[test]
    fn contract_covers_prototype_wire_shapes() {
        let contract = export();
        assert_eq!(contract.actor_name, "agentOS");
        assert_eq!(contract.api_version, "https://rivet.dev/agentos/v1alpha1");
        assert_eq!(contract.contract_major, 1);
        assert_eq!(contract.contract_hash.len(), "sha256:".len() + 64);
        assert!(contract.capabilities.contains(&"config.merge-patch"));
        assert!(contract
            .types
            .iter()
            .any(|item| item.name == "FileBytes" && item.declaration.contains("Uint8Array")));
        assert!(contract.types.iter().any(|item| {
            item.name == "FileContentInput" && item.declaration.contains("string | Uint8Array")
        }));
        assert!(contract.types.iter().any(|item| item.name == "JsonValue"));
        assert!(contract
            .events
            .iter()
            .any(|event| event.name == "process.output"));
    }
}
