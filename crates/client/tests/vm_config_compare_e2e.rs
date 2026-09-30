//! Config comparison against a running VM through a real `agentos-native-sidecar`.
#![cfg(feature = "service-internals")]

mod common;

use std::collections::BTreeMap;

use agentos_client::config::AgentOsConfig;
use agentos_client::service_internals::vm_config_equivalent;
use agentos_client::AgentOs;

fn config(environment: Option<BTreeMap<String, String>>) -> AgentOsConfig {
    AgentOsConfig {
        environment,
        ..Default::default()
    }
}

#[tokio::test]
async fn running_vm_compares_configs_through_the_sidecar() {
    if !common::require_sidecar("running_vm_compares_configs_through_the_sidecar") {
        return;
    }
    let vm = AgentOs::create(config(None))
        .await
        .expect("create VM against real sidecar");

    let same = vm_config_equivalent(&vm, &config(None), &config(None), Vec::new(), Vec::new())
        .await
        .expect("compare an unchanged config");
    let changed_environment = Some(BTreeMap::from([(String::from("DEBUG"), String::from("1"))]));
    let changed = vm_config_equivalent(
        &vm,
        &config(None),
        &config(changed_environment),
        Vec::new(),
        Vec::new(),
    )
    .await
    .expect("compare a config with a changed environment");
    vm.shutdown().await.expect("shutdown VM");

    assert!(same);
    assert!(!changed);
}
