# std patch: implement env::split_paths for wasm32-wasip1
std's wasi os.rs has `split_paths` → panic!("unsupported"); codex parses PATH via
env::split_paths during session setup and aborts ("RuntimeError: unreachable").
wasi uses ':'-separated lists like Unix — implement SplitPaths to split on ':' and
yield PathBufs (see library/std/src/sys/pal/wasi/os.rs). Apply to the -Z build-std sysroot.
