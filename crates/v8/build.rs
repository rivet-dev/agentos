use std::{env, fs, path::PathBuf, process::Command};

fn main() {
  println!("cargo:rerun-if-changed=provider");
  println!("cargo:rerun-if-env-changed=AGENTOS_RUSTY_V8_SOURCE");
  assert!(
    env::var_os("CARGO_FEATURE_USE_CUSTOM_LIBCXX").is_some(),
    "agentOS V8 archives require the bundled C++ runtime"
  );
  for feature in [
    "SIMDUTF",
    "V8_ENABLE_POINTER_COMPRESSION",
    "V8_ENABLE_MULTI_CAGE",
    "V8_ENABLE_SANDBOX",
    "V8_ENABLE_V8_CHECKS",
  ] {
    assert!(
      env::var_os(format!("CARGO_FEATURE_{feature}")).is_none(),
      "feature {feature} does not match the fixed agentOS V8 archive ABI"
    );
  }
  let target = env::var("TARGET").expect("Cargo target");
  let status = Command::new("python3")
    .args(["-B", "provider/prepare.py", &target])
    .status()
    .expect("Python 3 is required to verify agentOS V8 archives");
  assert!(status.success(), "agentOS V8 archive verification failed");
  let out =
    PathBuf::from(env::var_os("OUT_DIR").expect("Cargo output directory"));
  let root = PathBuf::from(
    fs::read_to_string(out.join("bundle-root")).expect("verified bundle root"),
  );
  let library = root.join(format!("librusty_v8_release_{target}.a"));
  println!("cargo:rerun-if-changed={}", library.display());
  fs::copy(library, out.join("librusty_v8.a"))
    .expect("stage matched V8 library");
  println!("cargo::metadata=bundle_root={}", root.display());
  println!(
    "cargo:rustc-env=RUSTY_V8_SRC_BINDING_PATH={}/src_binding_release_{target}.rs",
    root.display()
  );
  println!("cargo:rustc-link-search=native={}", out.display());
  println!("cargo:rustc-link-lib=static=rusty_v8");
}
