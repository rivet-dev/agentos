//! URL-only hosted software mutation contract.

pub use agentos_client::InstalledSoftware as ActorInstalledSoftware;
use serde::{Deserialize, Serialize};

use crate::config::{RemotePackageSource, RemotePackageSourceInput};
use crate::lifecycle::ConfigSnapshot;

macro_rules! action {
    ($name:ident => $output:ty, $wire_name:literal) => {
        impl rivetkit::Action for $name {
            type Output = $output;
            const NAME: &'static str = $wire_name;
        }
    };
}

#[cfg_attr(feature = "contract", derive(ts_rs::TS))]
#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SoftwareInstall {
    pub source: RemotePackageSourceInput,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub expected_revision: Option<u64>,
}

action!(SoftwareInstall => SoftwareMutationResult, "software.install");

#[cfg_attr(feature = "contract", derive(ts_rs::TS))]
#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SoftwareUninstall {
    pub package_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub expected_revision: Option<u64>,
}

action!(SoftwareUninstall => SoftwareMutationResult, "software.uninstall");

#[cfg_attr(feature = "contract", derive(ts_rs::TS))]
#[derive(Debug, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SoftwareList {}

action!(SoftwareList => Vec<ActorInstalledSoftware>, "software.list");

#[cfg_attr(feature = "contract", derive(ts_rs::TS))]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SoftwareMutationResult {
    pub software: Option<ActorInstalledSoftware>,
    pub source: RemotePackageSource,
    pub config: ConfigSnapshot,
}
