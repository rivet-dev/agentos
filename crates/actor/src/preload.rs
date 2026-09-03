//! Thin adapter between the hosted actor lifecycle and the process-level
//! preload subsystem. The coordinator and cache policy live outside the VM
//! actor so actor instances do not each own a copy of that machinery.

use rivetkit::Ctx;

use crate::AgentOsActor;

pub use agentos_preload::{
    configure_process_preload, shutdown_process_preload, PreloadArtifact, PreloadBaselineReplaced,
    PreloadCoordinatorActor, PreloadCoordinatorConfig, PreloadCoordinatorConfigInput,
    PreloadCoordinatorCreateInput, PreloadCoordinatorStatus, PreloadGetPlan, PreloadPlan,
    PreloadProcessOptions, PreloadRecordUsage, PreloadReplaceBaseline, PreloadStatus,
    PreloadUsageAccepted, PreloadUsageObservation, ProcessPreloadReport,
    PRELOAD_COORDINATOR_ACTOR_KEY, PRELOAD_COORDINATOR_ACTOR_NAME, PRELOAD_PROTOCOL_VERSION,
};

pub(crate) use agentos_preload::{acquire_required_package, observe_software_usage};

pub(crate) async fn warm_process_once(ctx: &Ctx<AgentOsActor>) -> ProcessPreloadReport {
    agentos_preload::warm_process_once(ctx.client()).await
}
