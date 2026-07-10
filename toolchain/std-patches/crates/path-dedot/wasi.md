# path-dedot wasip1 fix
path-dedot 3.1.1 gates its unix `parse_dot`/`parse_dot_from` impl on
`any(unix, all(target_family = "wasm", feature = "use_unix_paths_on_wasm"))`
(src/lib.rs:290, src/macros.rs:1). On wasm32-wasip1 without that feature the
methods vanish → codex-core fails. Fix: route all `target_family = "wasm"` to the
unix-paths impl (the VM uses unix-style absolute paths). Apply by either enabling
path-dedot's `use_unix_paths_on_wasm` feature in the vendored build, or replacing
`all(target_family = "wasm", feature = "use_unix_paths_on_wasm")` with
`target_family = "wasm"` in src/lib.rs + src/macros.rs.
Same fix applies to path-absolutize (same author/pattern).
