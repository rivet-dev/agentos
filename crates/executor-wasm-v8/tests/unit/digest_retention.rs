use super::*;
use std::cell::Cell;

#[derive(Clone)]
struct Capture(Arc<Mutex<Vec<String>>>);
impl tracing::Subscriber for Capture {
    fn enabled(&self, _: &tracing::Metadata<'_>) -> bool {
        true
    }
    fn new_span(&self, _: &tracing::span::Attributes<'_>) -> tracing::span::Id {
        tracing::span::Id::from_u64(1)
    }
    fn record(&self, _: &tracing::span::Id, _: &tracing::span::Record<'_>) {}
    fn record_follows_from(&self, _: &tracing::span::Id, _: &tracing::span::Id) {}
    fn event(&self, event: &tracing::Event<'_>) {
        struct Fields(String);
        impl tracing::field::Visit for Fields {
            fn record_debug(&mut self, field: &tracing::field::Field, value: &dyn std::fmt::Debug) {
                use std::fmt::Write;
                write!(&mut self.0, "{}={value:?};", field.name()).unwrap();
            }
        }
        let mut fields = Fields(String::new());
        event.record(&mut fields);
        self.0.lock().unwrap().push(fields.0);
    }
    fn enter(&self, _: &tracing::span::Id) {}
    fn exit(&self, _: &tracing::span::Id) {}
}

#[test]
fn digest_fifo_capacity_revalidates_evicted_images_in_isolated_process() {
    const CHILD: &str = "AGENTOS_DIGEST_RETENTION_TEST_CHILD";
    if std::env::var_os(CHILD).is_none() {
        struct OwnedChild(Option<std::process::Child>);
        impl OwnedChild {
            fn cleanup(&mut self) -> Vec<String> {
                let Some(child) = self.0.as_mut() else {
                    return Vec::new();
                };
                let mut failures = Vec::new();
                // An observation failure must not skip termination or a reap attempt.
                let exited = match child.try_wait() {
                    Ok(Some(_)) => true,
                    Ok(None) => false,
                    Err(error) => {
                        failures.push(format!("cleanup try_wait: {error}"));
                        false
                    }
                };
                if !exited {
                    let kill_error = child.kill().err();
                    // Always attempt wait, including when kill races natural exit.
                    match child.wait() {
                        Ok(_) => {
                            if let Some(error) = kill_error {
                                eprintln!("isolated owner kill returned {error}; child nevertheless reaped");
                            }
                        }
                        Err(error) => {
                            if let Some(error) = kill_error {
                                failures.push(format!("cleanup kill: {error}"));
                            }
                            failures.push(format!("cleanup wait: {error}"));
                            // Retain ownership so Drop makes one further cleanup attempt.
                            return failures;
                        }
                    }
                }
                self.0 = None;
                failures
            }
        }
        impl Drop for OwnedChild {
            fn drop(&mut self) {
                for failure in self.cleanup() {
                    eprintln!("isolated cache-owner cleanup failure: {failure}");
                }
            }
        }
        let child = std::process::Command::new(std::env::current_exe().unwrap())
            .args(["--exact", "digest_retention_tests::digest_fifo_capacity_revalidates_evicted_images_in_isolated_process", "--nocapture", "--test-threads=1"])
            .env(CHILD, "1").spawn().expect("spawn isolated cache owner");
        let mut owner = OwnedChild(Some(child));
        let deadline = std::time::Instant::now() + Duration::from_secs(20);
        let mut failures = Vec::new();
        loop {
            match owner.0.as_mut().unwrap().try_wait() {
                Ok(Some(status)) => {
                    if !status.success() {
                        failures.push(format!("isolated cache owner failed: {status}"));
                    }
                    break;
                }
                Ok(None) => {}
                Err(error) => {
                    failures.push(format!("observe isolated owner: {error}"));
                    break;
                }
            }
            if std::time::Instant::now() >= deadline {
                failures.push("isolated cache owner exceeded 20 seconds".into());
                break;
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        failures.extend(owner.cleanup());
        drop(owner);
        assert!(failures.is_empty(), "{}", failures.join("; "));
        return;
    }
    assert!(validated_profile_digests().lock().unwrap().is_empty());
    let events = Arc::new(Mutex::new(Vec::new()));
    let _capture = tracing::subscriber::set_default(Capture(Arc::clone(&events)));
    let root = tempfile::tempdir().unwrap();
    let calls = Cell::new(0usize);
    let validate = |bytes: &[u8]| {
        let result = profile::validate_locked_profile(bytes);
        calls.set(calls.get() + 1);
        result
    };
    let load = |name: &str, bytes: &[u8]| {
        let owner = root.path().join(name);
        fs::create_dir(&owner).unwrap();
        let path = owner.join("guest.wasm");
        fs::write(&path, bytes).unwrap();
        validate_module_profile_with(
            &ResolvedWasmModule {
                specifier: "guest".into(),
                resolved_path: path,
            },
            &owner,
            validate,
        )
    };
    let images: Vec<_> = (0..=WASM_VALIDATED_PROFILE_DIGEST_CAPACITY)
        .map(|n| wat::parse_str(format!("(module (func (export \"image-{n}\")))")).unwrap())
        .collect();
    for (n, bytes) in images.iter().enumerate() {
        load(&format!("image-{n}"), bytes).unwrap();
        assert!(
            validated_profile_digests().lock().unwrap().len()
                <= WASM_VALIDATED_PROFILE_DIGEST_CAPACITY
        );
        if n == 1 {
            let before = validated_profile_digests().lock().unwrap().clone();
            load("duplicate-before-eviction", bytes).unwrap();
            assert_eq!(
                *validated_profile_digests().lock().unwrap(),
                before,
                "duplicate must not refresh FIFO order"
            );
        }
    }
    assert_eq!(calls.get(), 65);
    let first: [u8; 32] = Sha256::digest(&images[0]).into();
    let second: [u8; 32] = Sha256::digest(&images[1]).into();
    assert!(!validated_profile_digests().lock().unwrap().contains(&first));
    load("retained-fresh-owner", &images[1]).unwrap();
    assert_eq!(calls.get(), 65, "fresh path uses retained digest");
    load("evicted-fresh-owner", &images[0]).unwrap();
    assert_eq!(
        calls.get(),
        66,
        "fresh owner must invoke real validator after eviction"
    );
    assert!(
        !validated_profile_digests()
            .lock()
            .unwrap()
            .contains(&second),
        "retained duplicate must not become newest"
    );
    let invalid = wat::parse_str("(module (func (return_call 1)) (func))").unwrap();
    let before = validated_profile_digests().lock().unwrap().clone();
    for name in ["invalid-first", "invalid-fresh-owner"] {
        assert!(load(name, &invalid).is_err());
        assert_eq!(*validated_profile_digests().lock().unwrap(), before);
    }
    assert_eq!(
        calls.get(),
        68,
        "failed profiles must invoke real validator again"
    );
    let events = events.lock().unwrap();
    let cache_events: Vec<_> = events
        .iter()
        .filter(|s| s.contains("wasm.validatedProfileDigests"))
        .collect();
    assert_eq!(
        cache_events.len(),
        3,
        "one near-capacity warning and two evictions"
    );
    for event in cache_events {
        assert!(event.contains("capacity=64"));
        assert!(event.contains("module admission is unaffected"));
        assert!(event.contains("validated again"));
        assert!(!event.contains("limit="));
        assert!(!event.contains("validatedProfileDigestEntries"));
    }
}
