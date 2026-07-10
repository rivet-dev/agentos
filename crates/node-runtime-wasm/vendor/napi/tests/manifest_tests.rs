use std::env;
use std::path::PathBuf;
use std::process::Command;
use std::sync::OnceLock;

use serde::Deserialize;

fn crate_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

fn path_from_env_or(name: &str, default: PathBuf) -> PathBuf {
    env::var_os(name)
        .map(PathBuf::from)
        .map(|path| {
            if path.is_absolute() {
                path
            } else {
                crate_root().join(path)
            }
        })
        .unwrap_or(default)
}

fn cargo_target_dir() -> PathBuf {
    path_from_env_or("CARGO_TARGET_DIR", crate_root().join("target"))
}

fn native_test_dir() -> PathBuf {
    path_from_env_or(
        "NAPI_NATIVE_TEST_OUT_DIR",
        cargo_target_dir().join("native"),
    )
}

fn wasix_test_dir() -> PathBuf {
    path_from_env_or(
        "NAPI_WASIX_TEST_OUT_DIR",
        cargo_target_dir().join("wasm32-wasix/release"),
    )
}

#[derive(Deserialize)]
struct TestManifest {
    tests: Vec<TestCase>,
}

#[derive(Deserialize)]
struct TestCase {
    name: String,
    description: String,
    expected_stdout: Option<String>,
    expected_stdout_prefix: Option<String>,
}

fn build_native_test(name: &str) -> PathBuf {
    let root = crate_root();
    let status = Command::new("./tests/build-test-native.sh")
        .arg(name)
        .current_dir(&root)
        .status()
        .expect("failed to execute tests/build-test-native.sh");
    assert!(status.success(), "failed to build native test: {name}");
    native_test_dir().join(name)
}

fn build_wasix_test(name: &str) -> PathBuf {
    let root = crate_root();
    let status = Command::new("./tests/build-test-wasix.sh")
        .arg(name)
        .current_dir(&root)
        .status()
        .expect("failed to execute tests/build-test-wasix.sh");
    assert!(status.success(), "failed to build WASIX test: {name}");
    wasix_test_dir().join(format!("{name}.wasm"))
}

fn build_cli_binary() -> PathBuf {
    static CLI_PATH: OnceLock<PathBuf> = OnceLock::new();
    CLI_PATH
        .get_or_init(|| {
            let root = crate_root();
            let status = Command::new("./cargo-standalone.sh")
                .args(["build", "--features", "cli", "--bin", "napi_wasmer"])
                .current_dir(&root)
                .status()
                .expect("failed to build napi_wasmer CLI binary for tests");
            assert!(status.success(), "failed to build napi_wasmer CLI binary");
            cargo_target_dir().join("debug/napi_wasmer")
        })
        .clone()
}

#[test]
fn manifest_tests_match_native_and_wasix() {
    let root = crate_root();
    let cli = build_cli_binary();
    let manifest_path = root.join("tests/programs/manifest.json");
    let manifest_content = std::fs::read_to_string(&manifest_path)
        .expect("failed to read test/programs/manifest.json");
    let manifest: TestManifest =
        serde_json::from_str(&manifest_content).expect("failed to parse test manifest json");

    assert!(
        !manifest.tests.is_empty(),
        "manifest must contain at least one test"
    );

    for case in &manifest.tests {
        eprintln!("running test: {} ({})", case.name, case.description);

        // Build and run native test
        let native = build_native_test(&case.name);
        let native_out = Command::new(&native)
            .output()
            .expect("failed to run native test");
        assert_eq!(
            native_out.status.code().unwrap_or(-1),
            0,
            "native test exited non-zero: {}",
            case.name
        );
        let native_stdout = String::from_utf8(native_out.stdout).expect("native output not utf-8");

        if let Some(expected) = &case.expected_stdout {
            assert_eq!(
                &native_stdout, expected,
                "native stdout mismatch for {}",
                case.name
            );
        }
        if let Some(prefix) = &case.expected_stdout_prefix {
            assert!(
                native_stdout.starts_with(prefix),
                "native stdout prefix mismatch for {}",
                case.name
            );
        }

        // Build and run WASIX test
        let wasix_path = build_wasix_test(&case.name);
        let wasix_out = Command::new(&cli)
            .arg(&wasix_path)
            .output()
            .expect("failed to run WASIX test via napi_wasmer CLI");
        assert_eq!(
            wasix_out.status.code().unwrap_or(-1),
            0,
            "wasix test exited non-zero: {}",
            case.name
        );
        let wasix_stdout = String::from_utf8(wasix_out.stdout).expect("wasix output not utf-8");
        if let Some(expected) = &case.expected_stdout {
            assert_eq!(
                &wasix_stdout, expected,
                "wasix stdout mismatch for {}",
                case.name
            );
        }
        if let Some(prefix) = &case.expected_stdout_prefix {
            assert!(
                wasix_stdout.starts_with(prefix),
                "wasix stdout prefix mismatch for {}",
                case.name
            );
        }
    }
}
