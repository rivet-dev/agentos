mod support;

use agentos_vm::wire::{EventPayload, GuestRuntimeKind, RequestPayload, WriteStdinRequest};
use agentos_vm::VmManager;
use nix::libc;
use std::collections::HashMap;
use std::path::Path;
use std::time::{Duration, Instant};
use support::{
    assert_node_available, authenticate_wire, create_vm_wire_with_metadata, execute_wire,
    new_sidecar, open_session_wire, temp_dir, wire_request, wire_vm, write_fixture,
    RecordingBridge,
};

struct Guest {
    connection_id: String,
    session_id: String,
    vm_id: String,
    process_id: String,
    stdout: String,
}

fn start_guest(
    sidecar: &mut VmManager<RecordingBridge>,
    index: usize,
    entry: &Path,
    cwd: &Path,
) -> Guest {
    let connection_id = authenticate_wire(sidecar, &format!("conn-idle-guest-{index}"));
    let session_id = open_session_wire(sidecar, 2, &connection_id);
    let (vm_id, _) = create_vm_wire_with_metadata(
        sidecar,
        3,
        &connection_id,
        &session_id,
        GuestRuntimeKind::JavaScript,
        cwd,
        HashMap::new(),
    );
    let process_id = format!("idle-guest-{index}");
    execute_wire(
        sidecar,
        4,
        &connection_id,
        &session_id,
        &vm_id,
        &process_id,
        GuestRuntimeKind::JavaScript,
        entry,
        Vec::new(),
    );
    let mut guest = Guest {
        connection_id,
        session_id,
        vm_id,
        process_id,
        stdout: String::new(),
    };
    wait_for_stdout(sidecar, &mut guest, "ready");
    guest
}

/// Waits for a stdout line that starts with `prefix` and returns the rest of it.
fn wait_for_stdout(
    sidecar: &mut VmManager<RecordingBridge>,
    guest: &mut Guest,
    prefix: &str,
) -> String {
    let ownership = wire_vm(&guest.connection_id, &guest.session_id, &guest.vm_id);
    let deadline = Instant::now() + Duration::from_secs(20);
    loop {
        let stdout = &guest.stdout;
        if let Some(line) = stdout.lines().find(|line| line.starts_with(prefix)) {
            return line[prefix.len()..].to_owned();
        }
        assert!(
            Instant::now() < deadline,
            "timed out waiting for {prefix:?} from {}; stdout: {stdout:?}",
            guest.process_id
        );
        let event = sidecar
            .poll_event_wire_blocking(&ownership, Duration::from_millis(50))
            .expect("poll sidecar event");
        match event.map(|event| event.payload) {
            Some(EventPayload::ProcessOutputEvent(output))
                if output.process_id == guest.process_id =>
            {
                guest
                    .stdout
                    .push_str(&String::from_utf8_lossy(&output.chunk));
            }
            Some(EventPayload::ProcessExitedEvent(exited))
                if exited.process_id == guest.process_id =>
            {
                panic!(
                    "{} exited with {} while waiting for {prefix:?}; stdout: {:?}",
                    guest.process_id, exited.exit_code, guest.stdout
                );
            }
            _ => {}
        }
    }
}

fn write_stdin(sidecar: &mut VmManager<RecordingBridge>, guest: &Guest, chunk: &str) {
    sidecar
        .dispatch_wire_blocking(wire_request(
            5,
            wire_vm(&guest.connection_id, &guest.session_id, &guest.vm_id),
            RequestPayload::WriteStdinRequest(WriteStdinRequest {
                process_id: guest.process_id.clone(),
                chunk: chunk.as_bytes().to_vec(),
            }),
        ))
        .expect("write guest stdin");
}

fn process_cpu_time() -> Duration {
    let mut usage = std::mem::MaybeUninit::<libc::rusage>::zeroed();
    // SAFETY: getrusage writes a complete rusage into the provided pointer.
    let status = unsafe { libc::getrusage(libc::RUSAGE_SELF, usage.as_mut_ptr()) };
    assert_eq!(status, 0, "getrusage failed");
    // SAFETY: getrusage returned success, so the struct is initialized.
    let usage = unsafe { usage.assume_init() };
    let micros = |time: libc::timeval| time.tv_sec as u64 * 1_000_000 + time.tv_usec as u64;
    Duration::from_micros(micros(usage.ru_utime) + micros(usage.ru_stime))
}

const IDLE_GUEST_COUNT: usize = 16;
const IDLE_WINDOW: Duration = Duration::from_secs(5);
/// Total CPU the whole process may burn while every guest is idle. A fixed
/// 1 ms platform tick costs several percent of a core per idle guest.
const MAX_IDLE_CPU_FRACTION: f64 = 0.05;

#[test]
#[ignore = "timing: measures process CPU over a multi-second idle window; run with --ignored"]
fn idle_javascript_guests_consume_almost_no_cpu_and_stay_responsive() {
    assert_node_available();
    let mut sidecar = new_sidecar("idle-guest-cpu");
    let cwd = temp_dir("idle-guest-cpu-cwd");
    let entry = cwd.join("idle.mjs");
    write_fixture(
        &entry,
        "process.stdin.on('data', (chunk) => console.log(`echo:${chunk}`));\nconsole.log('ready');\n",
    );
    let mut guests: Vec<Guest> = (0..IDLE_GUEST_COUNT)
        .map(|index| start_guest(&mut sidecar, index, &entry, &cwd))
        .collect();

    // Let startup work (GC, compile caches) settle before measuring.
    std::thread::sleep(Duration::from_secs(1));
    let cpu_before = process_cpu_time();
    let wall_before = Instant::now();
    std::thread::sleep(IDLE_WINDOW);
    let cpu_used = process_cpu_time() - cpu_before;
    let cpu_fraction = cpu_used.as_secs_f64() / wall_before.elapsed().as_secs_f64();
    assert!(
        cpu_fraction < MAX_IDLE_CPU_FRACTION,
        "{IDLE_GUEST_COUNT} idle guests used {:.1}% of a core over {IDLE_WINDOW:?} (limit {:.1}%)",
        cpu_fraction * 100.0,
        MAX_IDLE_CPU_FRACTION * 100.0
    );

    // Idle must mean parked, not stalled: every guest still answers input.
    for guest in &mut guests {
        write_stdin(&mut sidecar, guest, "ping");
        assert_eq!(wait_for_stdout(&mut sidecar, guest, "echo:"), "ping");
    }
}

/// Starts a guest that has been idle long enough for any idle backoff to reach
/// its longest wait, then sends it `command` on stdin.
fn idle_guest_then_send(
    name: &str,
    script: &str,
    command: &str,
) -> (VmManager<RecordingBridge>, Guest) {
    assert_node_available();
    let mut sidecar = new_sidecar(name);
    let cwd = temp_dir(&format!("{name}-cwd"));
    let entry = cwd.join("guest.mjs");
    write_fixture(&entry, script);
    let guest = start_guest(&mut sidecar, 0, &entry, &cwd);
    std::thread::sleep(Duration::from_secs(1));
    write_stdin(&mut sidecar, &guest, command);
    (sidecar, guest)
}

#[test]
fn idle_javascript_guest_gets_async_wasm_compile_result_promptly() {
    // Async Wasm compile finishes through a V8 foreground task posted from a
    // background compile thread, which no session channel signals. The module
    // is large enough that compilation outlives the pumps that run right
    // after the stdin dispatch.
    let script = [
        "const leb = (n, out) => { do { let b = n & 0x7f; n >>>= 7; if (n) b |= 0x80; out.push(b); } while (n); };",
        "const section = (id, body, out) => { out.push(id); leb(body.length, out); for (const b of body) out.push(b); };",
        "const FUNCTIONS = 20000;",
        "const fnBody = [0];",
        "for (let i = 0; i < 40; i++) fnBody.push(0x41, i, 0x1a);",
        "fnBody.push(0x0b);",
        "const funcs = []; leb(FUNCTIONS, funcs); for (let i = 0; i < FUNCTIONS; i++) funcs.push(0);",
        "const code = []; leb(FUNCTIONS, code);",
        "for (let i = 0; i < FUNCTIONS; i++) { leb(fnBody.length, code); for (const b of fnBody) code.push(b); }",
        "const bytes = [0, 97, 115, 109, 1, 0, 0, 0];",
        "section(1, [1, 0x60, 0, 0], bytes); section(3, funcs, bytes); section(10, code, bytes);",
        "const wasm = new Uint8Array(bytes);",
        "process.stdin.on('data', () => {",
        "  const started = performance.now();",
        "  WebAssembly.compile(wasm).then(",
        "    () => console.log(`wasm-ms:${performance.now() - started}`),",
        "    (error) => console.log(`wasm-ms:error ${error}`),",
        "  );",
        "});",
        "console.log('ready');",
    ]
    .join("\n");
    let (mut sidecar, mut guest) = idle_guest_then_send("idle-guest-wasm", &script, "go");
    let wasm_ms: f64 = wait_for_stdout(&mut sidecar, &mut guest, "wasm-ms:")
        .parse()
        .expect("async Wasm compile must succeed");
    assert!(
        wasm_ms < 150.0,
        "async Wasm compile in a previously idle guest took {wasm_ms:.1} ms"
    );
}

#[test]
fn idle_javascript_guest_timer_fires_on_time() {
    let script = [
        "process.stdin.on('data', () => {",
        "  const started = performance.now();",
        "  setTimeout(() => console.log(`timer-late:${performance.now() - started - 20}`), 20);",
        "});",
        "console.log('ready');",
    ]
    .join("\n");
    let (mut sidecar, mut guest) = idle_guest_then_send("idle-guest-timer", &script, "go");
    let timer_late_ms: f64 = wait_for_stdout(&mut sidecar, &mut guest, "timer-late:")
        .parse()
        .expect("parse timer lateness");
    assert!(
        timer_late_ms < 100.0,
        "a 20 ms timer in a previously idle guest fired {timer_late_ms:.1} ms late"
    );
}
