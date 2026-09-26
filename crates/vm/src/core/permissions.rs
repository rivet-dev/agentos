use crate::core::root_fs::SidecarCoreError;
use agentos_vm_config as vm_config;
use agentos_vm_host_interface::FilesystemAccess;
use agentos_vm_kernel::permissions::{
    permission_glob_matches, CommandAccessRequest, EnvAccessRequest, EnvironmentOperation,
    FsAccessRequest, FsOperation, NetworkAccessRequest, NetworkOperation, PermissionDecision,
    PermissionEvaluator, Permissions,
};
use std::sync::Arc;

pub fn deny_all_policy() -> vm_config::PermissionsPolicy {
    vm_config::PermissionsPolicy {
        fs: Some(vm_config::FsPermissionScope::Mode(
            vm_config::PermissionMode::Deny,
        )),
        network: Some(vm_config::PatternPermissionScope::Mode(
            vm_config::PermissionMode::Deny,
        )),
        child_process: Some(vm_config::PatternPermissionScope::Mode(
            vm_config::PermissionMode::Deny,
        )),
        process: Some(vm_config::PatternPermissionScope::Mode(
            vm_config::PermissionMode::Deny,
        )),
        env: Some(vm_config::PatternPermissionScope::Mode(
            vm_config::PermissionMode::Deny,
        )),
        host_function: Some(vm_config::PatternPermissionScope::Mode(
            vm_config::PermissionMode::Deny,
        )),
    }
}

pub fn allow_all_policy() -> vm_config::PermissionsPolicy {
    vm_config::PermissionsPolicy {
        fs: Some(vm_config::FsPermissionScope::Mode(
            vm_config::PermissionMode::Allow,
        )),
        network: Some(vm_config::PatternPermissionScope::Mode(
            vm_config::PermissionMode::Allow,
        )),
        child_process: Some(vm_config::PatternPermissionScope::Mode(
            vm_config::PermissionMode::Allow,
        )),
        process: Some(vm_config::PatternPermissionScope::Mode(
            vm_config::PermissionMode::Allow,
        )),
        env: Some(vm_config::PatternPermissionScope::Mode(
            vm_config::PermissionMode::Allow,
        )),
        host_function: Some(vm_config::PatternPermissionScope::Mode(
            vm_config::PermissionMode::Allow,
        )),
    }
}

/// The policy for every scope a client leaves out. The guest behaves like a
/// sandboxed machine: its virtual filesystem, processes, environment, host
/// functions, listeners, and loopback networking work. External network access
/// is denied until the client grants it explicitly.
/// The host filesystem is reachable only through mounts the client configures.
pub fn default_permissions_policy() -> vm_config::PermissionsPolicy {
    let allow = || {
        Some(vm_config::PatternPermissionScope::Mode(
            vm_config::PermissionMode::Allow,
        ))
    };
    vm_config::PermissionsPolicy {
        fs: Some(vm_config::FsPermissionScope::Mode(
            vm_config::PermissionMode::Allow,
        )),
        network: Some(vm_config::PatternPermissionScope::Rules(
            vm_config::PatternPermissionRuleSet {
                default: Some(vm_config::PermissionMode::Deny),
                rules: vec![
                    vm_config::PatternPermissionRule {
                        mode: vm_config::PermissionMode::Allow,
                        operations: vec![String::from("listen")],
                        patterns: vec![String::from("tcp://**"), String::from("unix:**")],
                    },
                    vm_config::PatternPermissionRule {
                        mode: vm_config::PermissionMode::Allow,
                        operations: vec![String::from("http")],
                        patterns: vec![
                            String::from("tcp://127.0.0.1:*"),
                            String::from("tcp://localhost:*"),
                            String::from("tcp://::1:*"),
                            String::from("unix:**"),
                        ],
                    },
                ],
            },
        )),
        child_process: allow(),
        process: allow(),
        env: allow(),
        host_function: allow(),
    }
}

/// Resolve a client's policy: each scope it sets replaces the default for that
/// scope, and every scope it leaves out keeps the default. No policy at all
/// means the default policy.
pub fn resolve_permissions_policy(
    requested: Option<&vm_config::PermissionsPolicy>,
) -> vm_config::PermissionsPolicy {
    let defaults = default_permissions_policy();
    let Some(requested) = requested else {
        return defaults;
    };
    vm_config::PermissionsPolicy {
        fs: requested.fs.clone().or(defaults.fs),
        network: requested.network.clone().or(defaults.network),
        child_process: requested.child_process.clone().or(defaults.child_process),
        process: requested.process.clone().or(defaults.process),
        env: requested.env.clone().or(defaults.env),
        host_function: requested.host_function.clone().or(defaults.host_function),
    }
}

/// Canonical agentOS VM policy used whenever an embedded or hosted client
/// omits permission fields. All local VM capabilities are enabled, while
/// outbound networking starts with the hosted LLM endpoint allowlist.
pub fn agentos_default_permissions_policy() -> vm_config::PermissionsPolicy {
    const EGRESS_HOSTS: &[&str] = &[
        "api.anthropic.com",
        "api.openai.com",
        "generativelanguage.googleapis.com",
        "openrouter.ai",
    ];
    let patterns = EGRESS_HOSTS
        .iter()
        .flat_map(|host| [format!("dns://{host}"), format!("tcp://{host}:*")])
        .collect();
    vm_config::PermissionsPolicy {
        fs: Some(vm_config::FsPermissionScope::Mode(
            vm_config::PermissionMode::Allow,
        )),
        network: Some(vm_config::PatternPermissionScope::Rules(
            vm_config::PatternPermissionRuleSet {
                default: Some(vm_config::PermissionMode::Deny),
                rules: vec![vm_config::PatternPermissionRule {
                    mode: vm_config::PermissionMode::Allow,
                    operations: vec![String::from("*")],
                    patterns,
                }],
            },
        )),
        child_process: Some(vm_config::PatternPermissionScope::Mode(
            vm_config::PermissionMode::Allow,
        )),
        process: Some(vm_config::PatternPermissionScope::Mode(
            vm_config::PermissionMode::Allow,
        )),
        env: Some(vm_config::PatternPermissionScope::Mode(
            vm_config::PermissionMode::Allow,
        )),
        host_function: Some(vm_config::PatternPermissionScope::Mode(
            vm_config::PermissionMode::Allow,
        )),
    }
}

/// Applies an optional field-by-field policy over the selected profile. This
/// gives explicit partial permission objects the same defaults in every client.
pub fn resolve_profile_permissions_policy(
    profile: vm_config::VmDefaultsProfile,
    overrides: Option<vm_config::PermissionsPolicy>,
) -> vm_config::PermissionsPolicy {
    let mut resolved = match profile {
        vm_config::VmDefaultsProfile::Secure => default_permissions_policy(),
        vm_config::VmDefaultsProfile::AgentOs => agentos_default_permissions_policy(),
    };
    if let Some(overrides) = overrides {
        if overrides.fs.is_some() {
            resolved.fs = overrides.fs;
        }
        if overrides.network.is_some() {
            resolved.network = overrides.network;
        }
        if overrides.child_process.is_some() {
            resolved.child_process = overrides.child_process;
        }
        if overrides.process.is_some() {
            resolved.process = overrides.process;
        }
        if overrides.env.is_some() {
            resolved.env = overrides.env;
        }
        if overrides.host_function.is_some() {
            resolved.host_function = overrides.host_function;
        }
    }
    resolved
}

pub fn evaluate_permissions_policy(
    permissions: &vm_config::PermissionsPolicy,
    domain: &str,
    capability: &str,
    resource: Option<&str>,
) -> vm_config::PermissionMode {
    match domain {
        "fs" => evaluate_fs_permission_scope(
            permissions.fs.as_ref(),
            capability_operation(capability, domain),
            resource,
        ),
        "network" => evaluate_pattern_permission_scope(
            permissions.network.as_ref(),
            capability_operation(capability, domain),
            resource,
        ),
        "child_process" => evaluate_pattern_permission_scope(
            permissions.child_process.as_ref(),
            capability_operation(capability, domain),
            resource,
        ),
        "process" => evaluate_pattern_permission_scope(
            permissions.process.as_ref(),
            capability_operation(capability, domain),
            resource,
        ),
        "env" => evaluate_pattern_permission_scope(
            permissions.env.as_ref(),
            capability_operation(capability, domain),
            resource,
        ),
        "hostFunction" => evaluate_pattern_permission_scope(
            permissions.host_function.as_ref(),
            capability_operation(capability, domain),
            resource,
        ),
        _ => vm_config::PermissionMode::Deny,
    }
}

/// Evaluate only rules that explicitly match `resource` in a pattern scope.
///
/// This is used for post-resolution network checks. The requested hostname is
/// evaluated with the ordinary policy (including the rule-set default); each
/// resolved address then adds a restriction only when a rule explicitly
/// names that address. A scope-wide mode still applies to every resource.
pub fn evaluate_matching_pattern_permission_policy(
    permissions: &vm_config::PermissionsPolicy,
    domain: &str,
    capability: &str,
    resource: Option<&str>,
) -> Option<vm_config::PermissionMode> {
    let scope = match domain {
        "network" => permissions.network.as_ref(),
        "child_process" => permissions.child_process.as_ref(),
        "process" => permissions.process.as_ref(),
        "env" => permissions.env.as_ref(),
        "hostFunction" => permissions.host_function.as_ref(),
        _ => return None,
    }?;
    let operation = capability_operation(capability, domain);
    match scope {
        vm_config::PatternPermissionScope::Mode(mode) => Some(*mode),
        vm_config::PatternPermissionScope::Rules(rules) => rules
            .rules
            .iter()
            .filter(|rule| pattern_rule_matches(rule, operation, resource))
            .map(|rule| rule.mode)
            .next_back(),
    }
}

fn evaluate_fs_permission_scope(
    scope: Option<&vm_config::FsPermissionScope>,
    operation: &str,
    resource: Option<&str>,
) -> vm_config::PermissionMode {
    match scope {
        Some(vm_config::FsPermissionScope::Mode(mode)) => *mode,
        Some(vm_config::FsPermissionScope::Rules(rules)) => {
            let mut mode = rules.default.unwrap_or(vm_config::PermissionMode::Deny);
            for rule in &rules.rules {
                if fs_rule_matches(rule, operation, resource) {
                    mode = rule.mode;
                }
            }
            mode
        }
        None => vm_config::PermissionMode::Deny,
    }
}

fn evaluate_pattern_permission_scope(
    scope: Option<&vm_config::PatternPermissionScope>,
    operation: &str,
    resource: Option<&str>,
) -> vm_config::PermissionMode {
    match scope {
        Some(vm_config::PatternPermissionScope::Mode(mode)) => *mode,
        Some(vm_config::PatternPermissionScope::Rules(rules)) => {
            let mut mode = rules.default.unwrap_or(vm_config::PermissionMode::Deny);
            for rule in &rules.rules {
                if pattern_rule_matches(rule, operation, resource) {
                    mode = rule.mode;
                }
            }
            mode
        }
        None => vm_config::PermissionMode::Deny,
    }
}

fn fs_rule_matches(
    rule: &vm_config::FsPermissionRule,
    operation: &str,
    resource: Option<&str>,
) -> bool {
    let operations_match = permission_operation_matches(&rule.operations, operation);
    let paths_match = permission_resource_matches(&rule.paths, resource);
    operations_match && paths_match
}

fn pattern_rule_matches(
    rule: &vm_config::PatternPermissionRule,
    operation: &str,
    resource: Option<&str>,
) -> bool {
    let operations_match = permission_operation_matches(&rule.operations, operation);
    let patterns_match = permission_resource_matches(&rule.patterns, resource);
    operations_match && patterns_match
}

fn permission_operation_matches(candidates: &[String], operation: &str) -> bool {
    candidates
        .iter()
        .any(|candidate| candidate == "*" || candidate == operation)
}

fn permission_resource_matches(patterns: &[String], resource: Option<&str>) -> bool {
    resource.is_some_and(|value| {
        patterns
            .iter()
            .any(|pattern| permission_glob_matches(pattern, value))
    })
}

pub fn validate_permissions_policy(
    permissions: &vm_config::PermissionsPolicy,
) -> Result<(), SidecarCoreError> {
    if let Some(scope) = permissions.fs.as_ref() {
        validate_fs_permission_scope("fs", scope)?;
    }
    if let Some(scope) = permissions.network.as_ref() {
        validate_pattern_permission_scope("network", scope)?;
    }
    if let Some(scope) = permissions.child_process.as_ref() {
        validate_pattern_permission_scope("child_process", scope)?;
    }
    if let Some(scope) = permissions.process.as_ref() {
        validate_pattern_permission_scope("process", scope)?;
    }
    if let Some(scope) = permissions.env.as_ref() {
        validate_pattern_permission_scope("env", scope)?;
    }
    if let Some(scope) = permissions.host_function.as_ref() {
        validate_pattern_permission_scope("hostFunction", scope)?;
    }
    Ok(())
}

fn validate_fs_permission_scope(
    domain: &str,
    scope: &vm_config::FsPermissionScope,
) -> Result<(), SidecarCoreError> {
    let vm_config::FsPermissionScope::Rules(rule_set) = scope else {
        return Ok(());
    };

    for (index, rule) in rule_set.rules.iter().enumerate() {
        validate_permission_rule_field(
            &rule.operations,
            &format!("{domain}.rules[{index}].operations"),
        )?;
        validate_permission_rule_field(&rule.paths, &format!("{domain}.rules[{index}].paths"))?;
    }

    Ok(())
}

fn validate_pattern_permission_scope(
    domain: &str,
    scope: &vm_config::PatternPermissionScope,
) -> Result<(), SidecarCoreError> {
    let vm_config::PatternPermissionScope::Rules(rule_set) = scope else {
        return Ok(());
    };

    for (index, rule) in rule_set.rules.iter().enumerate() {
        validate_permission_rule_field(
            &rule.operations,
            &format!("{domain}.rules[{index}].operations"),
        )?;
        validate_permission_rule_field(
            &rule.patterns,
            &format!("{domain}.rules[{index}].patterns"),
        )?;
    }

    Ok(())
}

fn validate_permission_rule_field(values: &[String], field: &str) -> Result<(), SidecarCoreError> {
    if values.is_empty() {
        return Err(SidecarCoreError::new(format!(
            "invalid permissions policy: {field} must not be empty; use [\"*\"] for wildcard"
        )));
    }
    Ok(())
}

fn capability_operation<'a>(capability: &'a str, domain: &str) -> &'a str {
    capability
        .strip_prefix(domain)
        .and_then(|value| value.strip_prefix('.'))
        .unwrap_or("")
}

pub fn permission_mode_to_kernel_decision(
    mode: vm_config::PermissionMode,
    capability: &str,
) -> PermissionDecision {
    match mode {
        vm_config::PermissionMode::Allow => PermissionDecision::allow(),
        vm_config::PermissionMode::Ask => {
            PermissionDecision::deny(format!("permission prompt required for {capability}"))
        }
        vm_config::PermissionMode::Deny => {
            PermissionDecision::deny(format!("blocked by {capability} policy"))
        }
    }
}

pub fn permissions_from_policy(policy: vm_config::PermissionsPolicy) -> Permissions {
    let filesystem_unrestricted = matches!(
        policy.fs.as_ref(),
        Some(vm_config::FsPermissionScope::Mode(
            vm_config::PermissionMode::Allow
        ))
    );
    let fs_policy = Arc::new(policy.clone());
    let network_policy = Arc::new(policy.clone());
    let child_process_policy = Arc::new(policy.clone());
    let env_policy = Arc::new(policy);

    Permissions {
        filesystem: if filesystem_unrestricted {
            PermissionEvaluator::Allow
        } else {
            PermissionEvaluator::dynamic(move |request: &FsAccessRequest| {
                let capability = fs_permission_capability(request.op);
                permission_mode_to_kernel_decision(
                    evaluate_permissions_policy(&fs_policy, "fs", capability, Some(&request.path)),
                    capability,
                )
            })
        },
        filesystem_unrestricted,
        network: PermissionEvaluator::dynamic(move |request: &NetworkAccessRequest| {
            let capability = network_permission_capability(request.op);
            permission_mode_to_kernel_decision(
                evaluate_permissions_policy(
                    &network_policy,
                    "network",
                    capability,
                    Some(&request.resource),
                ),
                capability,
            )
        }),
        child_process: PermissionEvaluator::dynamic(move |request: &CommandAccessRequest| {
            let capability = "child_process.spawn";
            permission_mode_to_kernel_decision(
                evaluate_permissions_policy(
                    &child_process_policy,
                    "child_process",
                    capability,
                    Some(&request.command),
                ),
                capability,
            )
        }),
        environment: PermissionEvaluator::dynamic(move |request: &EnvAccessRequest| {
            let capability = environment_permission_capability(request.op);
            permission_mode_to_kernel_decision(
                evaluate_permissions_policy(&env_policy, "env", capability, Some(&request.key)),
                capability,
            )
        }),
    }
}

pub fn fs_permission_capability(operation: FsOperation) -> &'static str {
    match operation {
        FsOperation::Read => filesystem_permission_capability(FilesystemAccess::Read),
        FsOperation::Write => filesystem_permission_capability(FilesystemAccess::Write),
        FsOperation::Mkdir | FsOperation::CreateDir => {
            filesystem_permission_capability(FilesystemAccess::CreateDir)
        }
        FsOperation::ReadDir => filesystem_permission_capability(FilesystemAccess::ReadDir),
        FsOperation::Stat | FsOperation::Exists => {
            filesystem_permission_capability(FilesystemAccess::Stat)
        }
        FsOperation::Remove => filesystem_permission_capability(FilesystemAccess::Remove),
        FsOperation::Rename => filesystem_permission_capability(FilesystemAccess::Rename),
        FsOperation::Symlink => filesystem_permission_capability(FilesystemAccess::Symlink),
        FsOperation::ReadLink => filesystem_permission_capability(FilesystemAccess::ReadLink),
        FsOperation::Link | FsOperation::Chmod | FsOperation::Chown | FsOperation::Utimes => {
            filesystem_permission_capability(FilesystemAccess::Write)
        }
        FsOperation::Truncate => filesystem_permission_capability(FilesystemAccess::Truncate),
        FsOperation::MountSensitive => "fs.mount_sensitive",
    }
}

pub fn filesystem_permission_capability(access: FilesystemAccess) -> &'static str {
    match access {
        FilesystemAccess::Read => "fs.read",
        FilesystemAccess::Write => "fs.write",
        FilesystemAccess::Stat => "fs.stat",
        FilesystemAccess::ReadDir => "fs.readdir",
        FilesystemAccess::CreateDir => "fs.create_dir",
        FilesystemAccess::Remove => "fs.rm",
        FilesystemAccess::Rename => "fs.rename",
        FilesystemAccess::Symlink => "fs.symlink",
        FilesystemAccess::ReadLink => "fs.readlink",
        FilesystemAccess::Chmod => "fs.chmod",
        FilesystemAccess::Truncate => "fs.truncate",
    }
}

pub fn network_permission_capability(operation: NetworkOperation) -> &'static str {
    match operation {
        NetworkOperation::Fetch => "network.fetch",
        NetworkOperation::Http => "network.http",
        NetworkOperation::Dns => "network.dns",
        NetworkOperation::Listen => "network.listen",
    }
}

pub fn environment_permission_capability(operation: EnvironmentOperation) -> &'static str {
    match operation {
        EnvironmentOperation::Read => "env.read",
        EnvironmentOperation::Write => "env.write",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn allowed(
        policy: &vm_config::PermissionsPolicy,
        domain: &str,
        capability: &str,
        resource: &str,
    ) -> bool {
        evaluate_permissions_policy(policy, domain, capability, Some(resource))
            == vm_config::PermissionMode::Allow
    }

    #[test]
    fn default_creation_profile_allows_local_vm_operations_and_denies_external_network() {
        let profile = vm_config::CreateVmConfig::default().defaults_profile();
        let policy = resolve_profile_permissions_policy(profile, None);

        assert!(allowed(&policy, "fs", "fs.read", "/workspace/a"));
        assert!(allowed(
            &policy,
            "child_process",
            "child_process.spawn",
            "node"
        ));
        assert!(allowed(&policy, "env", "env.read", "HOME"));
        assert!(allowed(
            &policy,
            "hostFunction",
            "hostFunction.invoke",
            "tools:weather"
        ));
        assert!(!allowed(
            &policy,
            "network",
            "network.http",
            "tcp://example.com:443"
        ));
        assert!(allowed(
            &policy,
            "network",
            "network.listen",
            "tcp://127.0.0.1:3000"
        ));
        assert!(allowed(
            &policy,
            "network",
            "network.http",
            "tcp://127.0.0.1:3000"
        ));
        assert!(!allowed(
            &policy,
            "network",
            "network.dns",
            "dns://api.anthropic.com"
        ));
        assert!(!allowed(
            &policy,
            "network",
            "network.http",
            "tcp://api.anthropic.com:443"
        ));
    }

    #[test]
    fn requested_scopes_replace_defaults_and_omitted_scopes_keep_them() {
        let requested = vm_config::PermissionsPolicy {
            fs: None,
            network: Some(vm_config::PatternPermissionScope::Mode(
                vm_config::PermissionMode::Allow,
            )),
            child_process: Some(vm_config::PatternPermissionScope::Mode(
                vm_config::PermissionMode::Deny,
            )),
            process: None,
            env: None,
            host_function: None,
        };
        let policy = resolve_permissions_policy(Some(&requested));

        assert!(allowed(
            &policy,
            "network",
            "network.http",
            "tcp://example.com:443"
        ));
        assert!(!allowed(
            &policy,
            "child_process",
            "child_process.spawn",
            "node"
        ));
        assert!(allowed(&policy, "fs", "fs.write", "/workspace/a"));
        assert!(allowed(&policy, "env", "env.read", "HOME"));
    }

    #[test]
    fn agentos_profile_materializes_defaults_and_merges_partial_overrides() {
        let defaults =
            resolve_profile_permissions_policy(vm_config::VmDefaultsProfile::AgentOs, None);
        assert_eq!(
            defaults.child_process,
            Some(vm_config::PatternPermissionScope::Mode(
                vm_config::PermissionMode::Allow
            ))
        );
        let Some(vm_config::PatternPermissionScope::Rules(network)) = &defaults.network else {
            panic!("agentOS network default must be an allowlist");
        };
        assert_eq!(network.default, Some(vm_config::PermissionMode::Deny));
        assert_eq!(network.rules.len(), 1);
        for expected in [
            "dns://api.anthropic.com",
            "tcp://api.anthropic.com:*",
            "dns://api.openai.com",
            "dns://generativelanguage.googleapis.com",
            "dns://openrouter.ai",
        ] {
            assert!(network.rules[0]
                .patterns
                .iter()
                .any(|value| value == expected));
        }

        let overridden = resolve_profile_permissions_policy(
            vm_config::VmDefaultsProfile::AgentOs,
            Some(vm_config::PermissionsPolicy {
                fs: None,
                network: Some(vm_config::PatternPermissionScope::Mode(
                    vm_config::PermissionMode::Deny,
                )),
                child_process: None,
                process: None,
                env: None,
                host_function: None,
            }),
        );
        assert_eq!(overridden.fs, defaults.fs);
        assert_eq!(overridden.child_process, defaults.child_process);
        assert_eq!(
            overridden.network,
            Some(vm_config::PatternPermissionScope::Mode(
                vm_config::PermissionMode::Deny
            ))
        );
    }

    #[test]
    fn permissions_default_to_deny() {
        let policy = vm_config::PermissionsPolicy {
            fs: None,
            network: None,
            child_process: None,
            process: None,
            env: None,
            host_function: None,
        };

        assert_eq!(
            evaluate_permissions_policy(&policy, "fs", "fs.read", Some("/tmp/a")),
            vm_config::PermissionMode::Deny
        );
    }

    #[test]
    fn permissions_default_to_deny_for_every_domain() {
        let policy = vm_config::PermissionsPolicy {
            fs: None,
            network: None,
            child_process: None,
            process: None,
            env: None,
            host_function: None,
        };

        for (domain, capability, resource) in [
            ("fs", "fs.read", "/workspace/file.txt"),
            ("network", "network.http", "example.com:443"),
            ("child_process", "child_process.spawn", "sh"),
            ("process", "process.kill", "123"),
            ("env", "env.read", "TOKEN"),
            ("hostFunction", "hostFunction.invoke", "shell"),
        ] {
            assert_eq!(
                evaluate_permissions_policy(&policy, domain, capability, Some(resource)),
                vm_config::PermissionMode::Deny,
                "{domain} should default to deny",
            );
        }
    }

    #[test]
    fn matching_pattern_evaluation_ignores_rule_set_default() {
        let policy = vm_config::PermissionsPolicy {
            fs: None,
            network: Some(vm_config::PatternPermissionScope::Rules(
                vm_config::PatternPermissionRuleSet {
                    default: Some(vm_config::PermissionMode::Deny),
                    rules: vec![vm_config::PatternPermissionRule {
                        mode: vm_config::PermissionMode::Allow,
                        operations: vec![String::from("http")],
                        patterns: vec![String::from("203.0.113.*:443")],
                    }],
                },
            )),
            child_process: None,
            process: None,
            env: None,
            host_function: None,
        };

        assert_eq!(
            evaluate_matching_pattern_permission_policy(
                &policy,
                "network",
                "network.http",
                Some("198.51.100.7:443"),
            ),
            None,
        );
        assert_eq!(
            evaluate_matching_pattern_permission_policy(
                &policy,
                "network",
                "network.http",
                Some("203.0.113.9:443"),
            ),
            Some(vm_config::PermissionMode::Allow),
        );
    }

    #[test]
    fn ask_permission_modes_become_kernel_denials() {
        let decision =
            permission_mode_to_kernel_decision(vm_config::PermissionMode::Ask, "network.http");

        assert!(!decision.allow);
        assert_eq!(
            decision.reason.as_deref(),
            Some("permission prompt required for network.http")
        );
    }

    #[test]
    fn ask_permission_modes_deny_every_policy_domain() {
        let policy = vm_config::PermissionsPolicy {
            fs: Some(vm_config::FsPermissionScope::Mode(
                vm_config::PermissionMode::Ask,
            )),
            network: Some(vm_config::PatternPermissionScope::Mode(
                vm_config::PermissionMode::Ask,
            )),
            child_process: Some(vm_config::PatternPermissionScope::Mode(
                vm_config::PermissionMode::Ask,
            )),
            process: Some(vm_config::PatternPermissionScope::Mode(
                vm_config::PermissionMode::Ask,
            )),
            env: Some(vm_config::PatternPermissionScope::Mode(
                vm_config::PermissionMode::Ask,
            )),
            host_function: Some(vm_config::PatternPermissionScope::Mode(
                vm_config::PermissionMode::Ask,
            )),
        };

        for (domain, capability, resource) in [
            ("fs", "fs.read", "/workspace/file.txt"),
            ("network", "network.http", "example.com:443"),
            ("child_process", "child_process.spawn", "sh"),
            ("process", "process.kill", "123"),
            ("env", "env.read", "TOKEN"),
            ("hostFunction", "hostFunction.invoke", "shell"),
        ] {
            let mode = evaluate_permissions_policy(&policy, domain, capability, Some(resource));
            assert_eq!(
                mode,
                vm_config::PermissionMode::Ask,
                "{domain} should preserve Ask until kernel-decision mapping",
            );
            let decision = permission_mode_to_kernel_decision(mode, capability);
            assert!(
                !decision.allow,
                "{domain} Ask should map to a kernel denial",
            );
            assert_eq!(
                decision.reason.as_deref(),
                Some(format!("permission prompt required for {capability}").as_str()),
                "{domain} Ask denial should explain the denied prompt",
            );
        }
    }

    #[test]
    fn ask_permission_modes_deny_every_kernel_callback_domain() {
        let permissions = permissions_from_policy(vm_config::PermissionsPolicy {
            fs: Some(vm_config::FsPermissionScope::Mode(
                vm_config::PermissionMode::Ask,
            )),
            network: Some(vm_config::PatternPermissionScope::Mode(
                vm_config::PermissionMode::Ask,
            )),
            child_process: Some(vm_config::PatternPermissionScope::Mode(
                vm_config::PermissionMode::Ask,
            )),
            process: None,
            env: Some(vm_config::PatternPermissionScope::Mode(
                vm_config::PermissionMode::Ask,
            )),
            host_function: None,
        });

        assert!(
            !permissions
                .filesystem
                .evaluate(&FsAccessRequest {
                    vm_id: String::from("vm"),
                    op: FsOperation::Read,
                    path: String::from("/workspace/file.txt"),
                })
                .allow
        );
        assert!(
            !permissions
                .network
                .evaluate(&NetworkAccessRequest {
                    vm_id: String::from("vm"),
                    op: NetworkOperation::Http,
                    resource: String::from("example.com:443"),
                })
                .allow
        );
        assert!(
            !permissions
                .child_process
                .evaluate(&CommandAccessRequest {
                    vm_id: String::from("vm"),
                    command: String::from("sh"),
                    args: Vec::new(),
                    cwd: None,
                    env: Default::default(),
                })
                .allow
        );
        assert!(
            !permissions
                .environment
                .evaluate(&EnvAccessRequest {
                    vm_id: String::from("vm"),
                    op: EnvironmentOperation::Read,
                    key: String::from("TOKEN"),
                    value: None,
                })
                .allow
        );
    }

    #[test]
    fn permissions_from_policy_builds_kernel_callbacks() {
        let policy = vm_config::PermissionsPolicy {
            fs: Some(vm_config::FsPermissionScope::Rules(
                vm_config::FsPermissionRuleSet {
                    default: Some(vm_config::PermissionMode::Deny),
                    rules: vec![vm_config::FsPermissionRule {
                        mode: vm_config::PermissionMode::Allow,
                        operations: vec![String::from("read")],
                        paths: vec![String::from("/workspace/**")],
                    }],
                },
            )),
            network: None,
            child_process: None,
            process: None,
            env: None,
            host_function: None,
        };

        let permissions = permissions_from_policy(policy);
        let check = &permissions.filesystem;

        assert!(
            check
                .evaluate(&FsAccessRequest {
                    vm_id: String::from("vm"),
                    op: FsOperation::Read,
                    path: String::from("/workspace/file.txt"),
                })
                .allow
        );
        assert!(
            !check
                .evaluate(&FsAccessRequest {
                    vm_id: String::from("vm"),
                    op: FsOperation::Read,
                    path: String::from("/secrets/file.txt"),
                })
                .allow
        );
        assert!(!permissions.filesystem_unrestricted);
    }

    #[test]
    fn only_unconditional_allow_uses_static_evaluator() {
        let unrestricted = permissions_from_policy(vm_config::PermissionsPolicy {
            fs: Some(vm_config::FsPermissionScope::Mode(
                vm_config::PermissionMode::Allow,
            )),
            network: None,
            child_process: None,
            process: None,
            env: None,
            host_function: None,
        });
        assert!(unrestricted.filesystem.is_allow());
        assert!(unrestricted.filesystem_unrestricted);

        let rule_based = permissions_from_policy(vm_config::PermissionsPolicy {
            fs: Some(vm_config::FsPermissionScope::Rules(
                vm_config::FsPermissionRuleSet {
                    default: Some(vm_config::PermissionMode::Allow),
                    rules: Vec::new(),
                },
            )),
            network: None,
            child_process: None,
            process: None,
            env: None,
            host_function: None,
        });
        assert!(!rule_based.filesystem.is_allow());
        assert!(!rule_based.filesystem_unrestricted);
    }

    #[test]
    fn last_matching_rule_wins() {
        let policy = vm_config::PermissionsPolicy {
            fs: Some(vm_config::FsPermissionScope::Rules(
                vm_config::FsPermissionRuleSet {
                    default: Some(vm_config::PermissionMode::Deny),
                    rules: vec![
                        vm_config::FsPermissionRule {
                            mode: vm_config::PermissionMode::Allow,
                            operations: vec![String::from("read")],
                            paths: vec![String::from("/workspace/**")],
                        },
                        vm_config::FsPermissionRule {
                            mode: vm_config::PermissionMode::Deny,
                            operations: vec![String::from("read")],
                            paths: vec![String::from("/workspace/secrets/**")],
                        },
                    ],
                },
            )),
            network: None,
            child_process: None,
            process: None,
            env: None,
            host_function: None,
        };

        assert_eq!(
            evaluate_permissions_policy(&policy, "fs", "fs.read", Some("/workspace/secrets/key")),
            vm_config::PermissionMode::Deny
        );
    }

    #[test]
    fn empty_rule_fields_are_rejected() {
        let policy = vm_config::PermissionsPolicy {
            fs: Some(vm_config::FsPermissionScope::Rules(
                vm_config::FsPermissionRuleSet {
                    default: None,
                    rules: vec![vm_config::FsPermissionRule {
                        mode: vm_config::PermissionMode::Allow,
                        operations: Vec::new(),
                        paths: vec![String::from("*")],
                    }],
                },
            )),
            network: None,
            child_process: None,
            process: None,
            env: None,
            host_function: None,
        };

        let error = validate_permissions_policy(&policy).expect_err("policy should be invalid");
        assert!(error
            .to_string()
            .contains("fs.rules[0].operations must not be empty"));
    }
}
