//! Validates, against REAL package-manager layouts, the store signatures the
//! sidecar's `symlinked_node_modules_hint` detector keys on. Each layout is a
//! clean, isolated monorepo generated in Docker by
//! `crates/sidecar/tests/fixtures/gen-pm-layouts.sh` (one node_modules per
//! package manager). Point `PM_LAYOUTS_DIR` at that output and run:
//!   PM_LAYOUTS_DIR=/path cargo test -p agentos-client --test module_layout_e2e -- --nocapture
//!
//! This is a pure filesystem inspection (no VM): it proves that the failing
//! package managers lay a transitive/direct dep into a store dir that escapes the
//! mounted project (so the runtime ENOENT carries that store path — see the
//! detector unit tests in crates/sidecar/src/service.rs for the matching error
//! strings, including the real pi-coding-agent pnpm failure), and that flat
//! managers do not.
//!
//! Skips honestly when `PM_LAYOUTS_DIR` is unset.

use std::path::Path;

/// What the direct dep's entry under `app/node_modules` should look like.
enum Expect {
    /// A symlink whose target contains this store marker (escapes the mount).
    StoreSymlink(&'static str),
    /// No `node_modules` at all; a Plug'n'Play runtime file is present at root.
    PlugNPlay,
    /// Flat: deps hoisted to the workspace root, no per-PM store dir.
    Flat,
}

struct Case {
    name: &'static str,
    expect: Expect,
}

const CASES: &[Case] = &[
    Case {
        name: "pnpm-isolated",
        expect: Expect::StoreSymlink("node_modules/.pnpm/"),
    },
    Case {
        name: "bun",
        expect: Expect::StoreSymlink("node_modules/.bun/"),
    },
    Case {
        name: "yarn-pnpm",
        expect: Expect::StoreSymlink("node_modules/.store/"),
    },
    Case {
        name: "yarn-pnp",
        expect: Expect::PlugNPlay,
    },
    Case {
        name: "npm",
        expect: Expect::Flat,
    },
    Case {
        name: "yarn-nm",
        expect: Expect::Flat,
    },
];

#[test]
fn package_manager_layouts_carry_expected_store_signatures() {
    let Ok(base) = std::env::var("PM_LAYOUTS_DIR") else {
        eprintln!("skipping module_layout_e2e: set PM_LAYOUTS_DIR (run crates/sidecar/tests/fixtures/gen-pm-layouts.sh)");
        return;
    };
    let base = Path::new(&base);
    let mut checked = 0;

    for case in CASES {
        let layout = base.join(case.name);
        if !layout.exists() {
            eprintln!("[{}] layout missing — skipping", case.name);
            continue;
        }
        checked += 1;
        let app_nm = layout.join("app/node_modules");
        let is_odd = app_nm.join("is-odd");

        match case.expect {
            Expect::StoreSymlink(marker) => {
                let target = std::fs::read_link(&is_odd).unwrap_or_else(|e| {
                    panic!(
                        "[{}] app/node_modules/is-odd should be a symlink: {e}",
                        case.name
                    )
                });
                let target = target.to_string_lossy();
                eprintln!(
                    "[{}] is-odd -> {target}  (expect store marker {marker:?})",
                    case.name
                );
                assert!(
                    target.contains(marker),
                    "[{}] symlink target {target:?} should contain store marker {marker:?}",
                    case.name
                );
            }
            Expect::PlugNPlay => {
                eprintln!("[{}] expect .pnp.cjs + no node_modules", case.name);
                assert!(
                    layout.join(".pnp.cjs").exists(),
                    "[{}] expected .pnp.cjs",
                    case.name
                );
                assert!(
                    !app_nm.exists(),
                    "[{}] PnP should have no app/node_modules",
                    case.name
                );
            }
            Expect::Flat => {
                eprintln!("[{}] expect flat (hoisted to root, no store)", case.name);
                // Direct dep resolves to a real dir at the workspace root, no store.
                let root_isodd = layout.join("node_modules/is-odd");
                assert!(
                    root_isodd.exists(),
                    "[{}] expected hoisted node_modules/is-odd",
                    case.name
                );
                assert!(
                    !root_isodd
                        .symlink_metadata()
                        .unwrap()
                        .file_type()
                        .is_symlink(),
                    "[{}] flat layout's is-odd should be a real dir, not a symlink",
                    case.name
                );
                for store in [
                    "node_modules/.pnpm",
                    "node_modules/.bun",
                    "node_modules/.store",
                ] {
                    assert!(
                        !layout.join(store).exists(),
                        "[{}] flat layout must not have {store}",
                        case.name
                    );
                }
            }
        }
    }

    assert!(
        checked > 0,
        "no layouts found under PM_LAYOUTS_DIR — generate them first"
    );
    eprintln!("\nvalidated {checked} package-manager layouts");
}
