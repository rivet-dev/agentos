use std::sync::Arc;

pub(crate) use agentos_actor_contract::classify_public_error;
#[cfg(test)]
use agentos_actor_contract::{
    decode_action_input as decode_contract_input, normalize_client_tags, MAX_ACTION_INPUT_DEPTH,
    MAX_ACTION_INPUT_NODES,
};
use agentos_actor_contract::{
    dispatch_typed_with_limits as dispatch_contract_action, ActionCodecLimits, DispatchFuture,
};
#[cfg(test)]
use anyhow::Result;
#[cfg(test)]
use rivet_error::{RivetError, RivetErrorKind};
use rivetkit::{Action, ActionEntry, ActionSet, Ctx, Handler};

use crate::AgentOsActor;

pub(crate) fn actor_codec_limits(actor: &AgentOsActor) -> ActionCodecLimits {
    actor.budgets.codec
}

/// Product-owned typed action registry.
///
/// This is intentionally independent of RivetKit's tuple implementations so
/// the agentOS contract can grow without turning API groups into raw router
/// actions. The TypeScript contract generator consumes this same registry.
pub struct AgentOsActionSet;

pub(crate) fn dispatch_typed<A>(
    actor: Arc<AgentOsActor>,
    ctx: Ctx<AgentOsActor>,
    args: &[u8],
) -> DispatchFuture
where
    A: Action,
    AgentOsActor: Handler<A>,
{
    let limits = actor_codec_limits(&actor);
    dispatch_contract_action::<AgentOsActor, A>(actor, ctx, args, limits)
}

pub(crate) struct RegisteredAction {
    pub(crate) name: &'static str,
    pub(crate) dispatch: fn(Arc<AgentOsActor>, Ctx<AgentOsActor>, &[u8]) -> DispatchFuture,
    #[cfg(feature = "contract")]
    pub(crate) contract: fn() -> agentos_actor_contract::schema::ActionContract,
    #[cfg(feature = "contract")]
    pub(crate) collect_types: fn(&mut agentos_actor_contract::schema::TypeCollector),
}

inventory::collect!(RegisteredAction);

#[cfg(feature = "contract")]
pub(crate) fn contract_for<A>() -> agentos_actor_contract::schema::ActionContract
where
    A: Action + ts_rs::TS,
    A::Output: ts_rs::TS,
{
    agentos_actor_contract::schema::ActionContract {
        name: A::NAME,
        public: !A::NAME.starts_with("__"),
        input: agentos_actor_contract::schema::input::<A>(),
        output: agentos_actor_contract::schema::output::<A::Output>(),
    }
}

#[cfg(feature = "contract")]
pub(crate) fn collect_types_for<A>(types: &mut agentos_actor_contract::schema::TypeCollector)
where
    A: Action + ts_rs::TS + 'static,
    A::Output: ts_rs::TS + 'static,
{
    if !A::NAME.starts_with("__") {
        types.collect::<A>();
        types.collect::<A::Output>();
    }
}

#[macro_export]
macro_rules! register_action {
    ($action:ty => $output:ty, $name:literal) => {
        impl rivetkit::Action for $action {
            type Output = $output;
            const NAME: &'static str = $name;
        }

        inventory::submit! {
            $crate::action_set::RegisteredAction {
                name: $name,
                dispatch: $crate::action_set::dispatch_typed::<$action>,
                #[cfg(feature = "contract")]
                contract: $crate::action_set::contract_for::<$action>,
                #[cfg(feature = "contract")]
                collect_types: $crate::action_set::collect_types_for::<$action>,
            }
        }
    };
}

#[cfg(test)]
fn decode_action_input<A: serde::de::DeserializeOwned>(args: &[u8]) -> Result<A> {
    decode_contract_input(args, crate::ACTOR_MESSAGE_SIZE_LIMIT as usize)
}

#[cfg(test)]
pub(crate) fn encode_action_output(output: &impl serde::Serialize) -> Result<Vec<u8>> {
    agentos_actor_contract::encode_action_output(output, crate::ACTOR_MESSAGE_SIZE_LIMIT as usize)
}

#[cfg(feature = "contract")]
pub(crate) fn contract() -> Vec<agentos_actor_contract::schema::ActionContract> {
    let mut actions = inventory::iter::<RegisteredAction>
        .into_iter()
        .map(|action| (action.contract)())
        .collect::<Vec<_>>();
    actions.sort_by_key(|action| action.name);
    actions
}

#[cfg(feature = "contract")]
pub(crate) fn collect_contract_types(types: &mut agentos_actor_contract::schema::TypeCollector) {
    for action in inventory::iter::<RegisteredAction> {
        (action.collect_types)(types);
    }
}

impl ActionSet<AgentOsActor> for AgentOsActionSet {
    fn entries() -> Vec<ActionEntry<AgentOsActor>> {
        let mut actions = inventory::iter::<RegisteredAction>
            .into_iter()
            .collect::<Vec<_>>();
        actions.sort_by_key(|action| action.name);
        actions
            .into_iter()
            .map(|action| ActionEntry::new(action.name))
            .collect()
    }

    fn dispatch(
        actor: Arc<AgentOsActor>,
        ctx: Ctx<AgentOsActor>,
        name: &str,
        args: &[u8],
    ) -> Option<DispatchFuture> {
        inventory::iter::<RegisteredAction>
            .into_iter()
            .find(|action| action.name == name)
            .map(|action| (action.dispatch)(actor, ctx, args))
    }
}

#[cfg(test)]
#[path = "../tests/unit/action_set.rs"]
mod tests;
