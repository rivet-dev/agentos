//! Pinned, verbatim Node.js standard-library sources and AgentOS binding assets.

include!(concat!(env!("OUT_DIR"), "/builtin_sources.rs"));
include!(concat!(env!("OUT_DIR"), "/binding_assets.rs"));

pub const VENDOR_MANIFEST_JSON: &str = include_str!("../vendor/manifest.json");
pub const BINDING_INVENTORY_JSON: &str = include_str!("../bindings/inventory.json");
pub const NODE_VERSION: &str = "24.15.0";
pub const NODE_COMMIT: &str = "848430679556aed0bd073f2bc263331ad84fa119";
pub const OPENSSL_VERSION: &str = "3.5.5";

pub fn vendor_manifest_json() -> &'static str {
    VENDOR_MANIFEST_JSON
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pinned_sources_include_the_eager_bootstrap_set() {
        for id in [
            "internal/bootstrap/realm",
            "buffer",
            "util",
            "events",
            "stream",
            "fs",
            "path",
            "timers",
        ] {
            assert!(builtin_source(id).is_some(), "missing Node builtin {id}");
        }
        assert!(BUILTIN_IDS.len() > 300, "expected the full Node lib tree");
    }

    #[test]
    fn manifest_records_the_pinned_node_and_openssl_versions() {
        assert!(VENDOR_MANIFEST_JSON.contains(NODE_COMMIT));
        assert!(VENDOR_MANIFEST_JSON.contains("\"version\": \"3.5.5\""));
    }

    #[test]
    fn every_reconciled_v24_binding_has_an_inert_bootstrap_entry() {
        assert_eq!(BINDING_IDS.len(), 69);
        for required in ["buffer", "fs", "crypto", "worker", "mksnapshot"] {
            assert!(BINDING_IDS.contains(&required));
        }
        assert!(INERT_BINDING_BOOTSTRAP_SOURCE.contains("ERR_UNKNOWN_INTERNAL_BINDING"));
        assert!(PROCESS_BOOTSTRAP_SOURCE.contains("/opt/agentos/bin/node"));
    }

    #[test]
    fn real_bootstrap_contains_every_public_builtin_and_the_realm_loader() {
        assert!(PUBLIC_BUILTIN_IDS.len() > 50);
        assert!(PUBLIC_BUILTIN_IDS.contains(&"fs"));
        assert!(PUBLIC_BUILTIN_IDS.contains(&"http"));
        assert!(!PUBLIC_BUILTIN_IDS
            .iter()
            .any(|id| id.starts_with("internal/")));
        assert!(NODE_BUILTIN_SOURCE_BYTES > 3_000_000);
        assert!(REAL_STDLIB_BOOTSTRAP_SOURCE.contains("internal/bootstrap/realm"));
        assert!(REAL_STDLIB_BOOTSTRAP_SOURCE.contains("ERR_NODE_STDLIB_INERT_LOAD"));
    }
}
