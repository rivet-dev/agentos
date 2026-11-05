mod support;

use agentos_native_sidecar::wire::{
    CloseStdinRequest, CreateVmRequest, DisposeReason, DisposeVmRequest, EventPayload,
    GuestRuntimeKind, PatternPermissionScope, PermissionMode, PermissionsPolicy, RequestPayload,
    ResponsePayload, RootFilesystemDescriptor, RootFilesystemMode, StreamChannel,
    WriteStdinRequest,
};
use hickory_resolver::proto::op::{Message, Query};
use hickory_resolver::proto::rr::domain::Name;
use hickory_resolver::proto::rr::rdata::{A, AAAA, CAA, CNAME, MX, NAPTR, NS, PTR, SOA, SRV, TXT};
use hickory_resolver::proto::rr::{RData, Record, RecordType};
use serde_json::{json, Value};
use std::collections::HashMap;
use std::io::{Read, Write};
use std::net::{Shutdown, SocketAddr, TcpListener, TcpStream, UdpSocket};
use std::path::Path;
use std::process::{Command, Stdio};
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc,
};
use std::thread;
use std::time::{Duration, Instant};
use support::{
    assert_node_available, authenticate_wire, dispose_vm_and_close_session_wire, execute_wire,
    new_sidecar, open_session_wire, temp_dir, wire_permissions_allow_all, wire_request,
    wire_session, wire_vm, write_fixture,
};

// Timing-sensitive assertions flake under the CPU contention of a parallel test
// run (see CLAUDE.md > Testing). Gated off by default; the nightly timing lane
// sets AGENTOS_RUN_TIMING_TESTS=1 to enforce them.
fn run_timing_sensitive_tests() -> bool {
    std::env::var_os("AGENTOS_RUN_TIMING_TESTS").is_some()
}

const ALLOWED_NODE_BUILTINS: &[&str] = &[
    "assert",
    "buffer",
    "child_process",
    "console",
    "constants",
    "crypto",
    "events",
    "fs",
    "module",
    "os",
    "path",
    "perf_hooks",
    "punycode",
    "querystring",
    "stream",
    "string_decoder",
    "timers",
    "tty",
    "url",
    "util",
    "zlib",
];

const BUILTIN_CONFORMANCE_CASES: &[&str] = &[
    "fs",
    "console",
    "child_process",
    "path",
    "crypto",
    "dns",
    "events",
    "stream",
    "buffer",
    "global_base64",
    "url",
    "stdlib_polyfill",
    "web_streams",
    "regexp",
    "extended_builtin_polyfills",
];

const PROBE_OUTPUT_BYTE_LIMIT: usize = 1024 * 1024;

fn run_host_probe(cwd: &Path, entrypoint: &Path) -> Value {
    run_host_probe_with_env(cwd, entrypoint, &[])
}

fn run_host_probe_with_env(cwd: &Path, entrypoint: &Path, env: &[(&str, &str)]) -> Value {
    let mut command = Command::new("node");
    command.arg(entrypoint).current_dir(cwd);
    for (key, value) in env {
        command.env(key, value);
    }

    let mut child = command
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawn host node probe");
    let stdout = child.stdout.take().expect("host probe stdout pipe");
    let stderr = child.stderr.take().expect("host probe stderr pipe");
    let stdout_reader = thread::spawn(move || read_probe_pipe(stdout, "stdout"));
    let stderr_reader = thread::spawn(move || read_probe_pipe(stderr, "stderr"));
    let status = child.wait().expect("wait host node probe");
    let stdout = stdout_reader
        .join()
        .expect("join host probe stdout reader")
        .expect("read bounded host probe stdout");
    let stderr = stderr_reader
        .join()
        .expect("join host probe stderr reader")
        .expect("read bounded host probe stderr");

    assert!(
        status.success(),
        "host probe failed with status {:?}\nstdout:\n{}\nstderr:\n{}",
        status.code(),
        String::from_utf8_lossy(&stdout),
        String::from_utf8_lossy(&stderr)
    );

    serde_json::from_slice(&stdout).expect("parse host probe JSON")
}

fn read_probe_pipe(mut pipe: impl Read, channel: &str) -> Result<Vec<u8>, String> {
    let mut output = Vec::new();
    let mut chunk = [0_u8; 8192];
    loop {
        let read = pipe
            .read(&mut chunk)
            .map_err(|err| format!("read host probe {channel}: {err}"))?;
        if read == 0 {
            return Ok(output);
        }
        if output.len().saturating_add(read) > PROBE_OUTPUT_BYTE_LIMIT {
            return Err(format!(
                "host probe exceeded {PROBE_OUTPUT_BYTE_LIMIT} bytes on {channel}"
            ));
        }
        output.extend_from_slice(&chunk[..read]);
    }
}

fn run_guest_probe(case_name: &str, cwd: &Path, entrypoint: &Path) -> Value {
    run_guest_probe_with_config(
        case_name,
        cwd,
        entrypoint,
        HashMap::new(),
        wire_permissions_allow_all(),
        ALLOWED_NODE_BUILTINS,
    )
}

#[allow(clippy::too_many_arguments)]
fn create_vm_with_metadata_and_permissions(
    sidecar: &mut agentos_native_sidecar::NativeSidecar<support::RecordingBridge>,
    request_id: i64,
    connection_id: &str,
    session_id: &str,
    runtime: GuestRuntimeKind,
    cwd: &Path,
    mut metadata: HashMap<String, String>,
    permissions: PermissionsPolicy,
) -> String {
    metadata
        .entry(String::from("cwd"))
        .or_insert_with(|| cwd.to_string_lossy().into_owned());

    let result = sidecar
        .dispatch_wire_blocking(wire_request(
            request_id,
            wire_session(connection_id, session_id),
            RequestPayload::CreateVmRequest(CreateVmRequest::legacy_test_config(
                runtime,
                metadata,
                RootFilesystemDescriptor {
                    mode: RootFilesystemMode::Ephemeral,
                    disable_default_base_layer: false,
                    lowers: Vec::new(),
                    bootstrap_entries: Vec::new(),
                },
                Some(permissions),
            )),
        ))
        .expect("create sidecar VM through wire");

    match result.response.payload {
        ResponsePayload::VmCreatedResponse(response) => response.vm_id,
        other => panic!("unexpected wire vm create response: {other:?}"),
    }
}

fn collect_builtin_process_output(
    sidecar: &mut agentos_native_sidecar::NativeSidecar<support::RecordingBridge>,
    connection_id: &str,
    session_id: &str,
    vm_id: &str,
    process_id: &str,
) -> (String, String, i32) {
    collect_builtin_process_output_with_timeout(
        sidecar,
        connection_id,
        session_id,
        vm_id,
        process_id,
        Duration::from_secs(10),
    )
}

fn collect_builtin_process_output_with_timeout(
    sidecar: &mut agentos_native_sidecar::NativeSidecar<support::RecordingBridge>,
    connection_id: &str,
    session_id: &str,
    vm_id: &str,
    process_id: &str,
    timeout: Duration,
) -> (String, String, i32) {
    let ownership = wire_session(connection_id, session_id);
    let deadline = Instant::now() + timeout;
    let mut stdout = String::new();
    let mut stderr = String::new();
    let mut exit = None;

    loop {
        let event = sidecar
            .poll_event_wire_blocking(&ownership, Duration::from_millis(100))
            .expect("poll builtin conformance wire event");
        if let Some(event) = event {
            assert_eq!(event.ownership, wire_vm(connection_id, session_id, vm_id));

            match event.payload {
                EventPayload::ProcessOutputEvent(output) if output.process_id == process_id => {
                    match output.channel {
                        StreamChannel::Stdout => {
                            append_probe_output(&mut stdout, &output.chunk, process_id, "stdout")
                        }
                        StreamChannel::Stderr => {
                            append_probe_output(&mut stderr, &output.chunk, process_id, "stderr")
                        }
                    }
                }
                EventPayload::ProcessExitedEvent(exited) if exited.process_id == process_id => {
                    exit = Some((exited.exit_code, Instant::now()));
                }
                _ => {}
            }
        }

        if let Some((exit_code, seen_at)) = exit {
            if Instant::now().duration_since(seen_at) >= Duration::from_millis(200) {
                return (stdout, stderr, exit_code);
            }
        }

        assert!(
            Instant::now() < deadline,
            "timed out waiting for builtin conformance process {process_id}\nstdout:\n{stdout}\nstderr:\n{stderr}"
        );
    }
}

fn append_probe_output(buffer: &mut String, chunk: &[u8], process_id: &str, channel: &str) {
    let text = String::from_utf8_lossy(chunk);
    assert!(
        buffer.len().saturating_add(text.len()) <= PROBE_OUTPUT_BYTE_LIMIT,
        "builtin conformance process {process_id} exceeded {PROBE_OUTPUT_BYTE_LIMIT} bytes on {channel}"
    );
    buffer.push_str(&text);
}

fn run_guest_probe_with_config(
    case_name: &str,
    cwd: &Path,
    entrypoint: &Path,
    mut metadata: HashMap<String, String>,
    permissions: PermissionsPolicy,
    allowed_builtins: &[&str],
) -> Value {
    let mut sidecar = new_sidecar(case_name);
    let connection_id = authenticate_wire(&mut sidecar, &format!("conn-{case_name}"));
    let session_id = open_session_wire(&mut sidecar, 2, &connection_id);
    let allowed_builtins =
        serde_json::to_string(allowed_builtins).expect("serialize builtin allowlist");
    metadata.insert(
        String::from("env.AGENTOS_ALLOWED_NODE_BUILTINS"),
        allowed_builtins,
    );
    let vm_id = create_vm_with_metadata_and_permissions(
        &mut sidecar,
        3,
        &connection_id,
        &session_id,
        GuestRuntimeKind::JavaScript,
        cwd,
        metadata,
        permissions,
    );

    execute_wire(
        &mut sidecar,
        4,
        &connection_id,
        &session_id,
        &vm_id,
        &format!("proc-{case_name}"),
        GuestRuntimeKind::JavaScript,
        entrypoint,
        Vec::new(),
    );

    let (stdout, stderr, exit_code) = collect_builtin_process_output(
        &mut sidecar,
        &connection_id,
        &session_id,
        &vm_id,
        &format!("proc-{case_name}"),
    );
    dispose_vm_and_close_session_wire(&mut sidecar, &connection_id, &session_id, &vm_id);

    assert_eq!(
        exit_code, 0,
        "guest probe failed for {case_name}\nstdout:\n{stdout}\nstderr:\n{stderr}"
    );
    assert!(
        stderr.trim().is_empty(),
        "guest probe stderr for {case_name}:\n{stderr}"
    );

    serde_json::from_str(stdout.trim()).expect("parse guest probe JSON")
}

#[allow(clippy::too_many_arguments)]
fn run_guest_probe_in_existing_session(
    sidecar: &mut agentos_native_sidecar::NativeSidecar<support::RecordingBridge>,
    request_id_base: i64,
    connection_id: &str,
    session_id: &str,
    case_name: &str,
    cwd: &Path,
    entrypoint: &Path,
    mut metadata: HashMap<String, String>,
) -> Value {
    let allowed_builtins =
        serde_json::to_string(ALLOWED_NODE_BUILTINS).expect("serialize builtin allowlist");
    metadata.insert(
        String::from("env.AGENTOS_ALLOWED_NODE_BUILTINS"),
        allowed_builtins,
    );

    let vm_id = create_vm_with_metadata_and_permissions(
        sidecar,
        request_id_base,
        connection_id,
        session_id,
        GuestRuntimeKind::JavaScript,
        cwd,
        metadata,
        wire_permissions_allow_all(),
    );

    let process_id = format!("proc-{case_name}");
    execute_wire(
        sidecar,
        request_id_base + 1,
        connection_id,
        session_id,
        &vm_id,
        &process_id,
        GuestRuntimeKind::JavaScript,
        entrypoint,
        Vec::new(),
    );

    let (stdout, stderr, exit_code) =
        collect_builtin_process_output(sidecar, connection_id, session_id, &vm_id, &process_id);

    let result = sidecar
        .dispatch_wire_blocking(wire_request(
            request_id_base + 2,
            wire_vm(connection_id, session_id, &vm_id),
            RequestPayload::DisposeVmRequest(DisposeVmRequest {
                reason: DisposeReason::Requested,
            }),
        ))
        .expect("dispose sidecar VM through wire");

    match result.response.payload {
        ResponsePayload::VmDisposedResponse(response) => {
            assert_eq!(response.vm_id, vm_id);
        }
        other => panic!("unexpected wire vm dispose response: {other:?}"),
    }

    assert_eq!(
        exit_code, 0,
        "guest probe failed for {case_name}\nstdout:\n{stdout}\nstderr:\n{stderr}"
    );
    assert!(
        stderr.trim().is_empty(),
        "guest probe stderr for {case_name}:\n{stderr}"
    );

    serde_json::from_str(stdout.trim()).expect("parse guest probe JSON")
}

fn assert_conformance(case_name: &str, script: &str) {
    assert_node_available();

    let cwd = temp_dir(&format!("builtin-conformance-{case_name}"));
    let entrypoint = cwd.join("entry.mjs");
    write_fixture(&entrypoint, script);

    let host = run_host_probe(&cwd, &entrypoint);
    let guest = run_guest_probe(case_name, &cwd, &entrypoint);

    assert_eq!(
        guest,
        host,
        "guest V8 result diverged from host Node for {case_name}\nhost: {}\nguest: {}",
        serde_json::to_string_pretty(&host).expect("pretty host JSON"),
        serde_json::to_string_pretty(&guest).expect("pretty guest JSON")
    );
}

fn run_isolated_builtin_conformance_test(test_name: &str) {
    let current_exe = std::env::current_exe().expect("current test binary path");
    let status = Command::new(&current_exe)
        .arg("--exact")
        .arg("__builtin_conformance_extra_test_runner")
        .arg("--nocapture")
        .env("AGENTOS_BUILTIN_CONFORMANCE_EXTRA_TEST", test_name)
        .status()
        .unwrap_or_else(|error| {
            panic!("spawn builtin conformance extra runner for {test_name}: {error}")
        });

    assert!(
        status.success(),
        "builtin conformance extra test {test_name} failed with status {status}"
    );
}

fn write_process_stdin(
    sidecar: &mut agentos_native_sidecar::NativeSidecar<support::RecordingBridge>,
    request_id: i64,
    connection_id: &str,
    session_id: &str,
    vm_id: &str,
    process_id: &str,
    chunk: &str,
) {
    let result = sidecar
        .dispatch_wire_blocking(wire_request(
            request_id,
            wire_vm(connection_id, session_id, vm_id),
            RequestPayload::WriteStdinRequest(WriteStdinRequest {
                process_id: process_id.to_owned(),
                chunk: chunk.as_bytes().to_vec(),
            }),
        ))
        .expect("write builtin conformance stdin through wire");

    match result.response.payload {
        ResponsePayload::StdinWrittenResponse(response) => {
            assert_eq!(response.process_id, process_id);
            assert_eq!(response.accepted_bytes, chunk.len() as u64);
        }
        other => panic!("unexpected wire stdin-written response: {other:?}"),
    }
}

fn close_process_stdin(
    sidecar: &mut agentos_native_sidecar::NativeSidecar<support::RecordingBridge>,
    request_id: i64,
    connection_id: &str,
    session_id: &str,
    vm_id: &str,
    process_id: &str,
) {
    let result = sidecar
        .dispatch_wire_blocking(wire_request(
            request_id,
            wire_vm(connection_id, session_id, vm_id),
            RequestPayload::CloseStdinRequest(CloseStdinRequest {
                process_id: process_id.to_owned(),
            }),
        ))
        .expect("close builtin conformance stdin through wire");

    match result.response.payload {
        ResponsePayload::StdinClosedResponse(response) => {
            assert_eq!(response.process_id, process_id);
        }
        other => panic!("unexpected wire stdin-closed response: {other:?}"),
    }
}

struct FixtureDnsServer {
    addr: SocketAddr,
    running: Arc<AtomicBool>,
    thread: Option<thread::JoinHandle<()>>,
}

impl FixtureDnsServer {
    fn start() -> Self {
        let socket = UdpSocket::bind("127.0.0.1:0").expect("bind fixture DNS server");
        socket
            .set_read_timeout(Some(Duration::from_millis(100)))
            .expect("set fixture DNS timeout");
        let addr = socket.local_addr().expect("fixture DNS local addr");
        let running = Arc::new(AtomicBool::new(true));
        let thread_running = Arc::clone(&running);
        let thread = thread::spawn(move || {
            let mut buffer = [0_u8; 2048];
            while thread_running.load(Ordering::SeqCst) {
                let Ok((len, peer)) = socket.recv_from(&mut buffer) else {
                    continue;
                };
                let Ok(request) = Message::from_vec(&buffer[..len]) else {
                    continue;
                };
                let response = fixture_dns_response(&request);
                let bytes = response.to_vec().expect("encode fixture DNS response");
                let _ = socket.send_to(&bytes, peer);
            }
        });
        Self {
            addr,
            running,
            thread: Some(thread),
        }
    }
}

impl Drop for FixtureDnsServer {
    fn drop(&mut self) {
        self.running.store(false, Ordering::SeqCst);
        if let Ok(socket) = UdpSocket::bind("127.0.0.1:0") {
            let _ = socket.send_to(&[0], self.addr);
        }
        if let Some(thread) = self.thread.take() {
            thread.join().expect("join fixture DNS thread");
        }
    }
}

fn fixture_dns_response(request: &Message) -> Message {
    let mut response = Message::response(request.metadata.id, request.metadata.op_code);
    response.metadata.authoritative = true;
    response.metadata.recursion_available = true;
    response.add_queries(request.queries.iter().cloned());
    if let Some(query) = request.queries.first() {
        response.add_answers(fixture_dns_answers(query));
    }
    response
}

fn fixture_dns_answers(query: &Query) -> Vec<Record> {
    let name = query.name().to_ascii();
    match (name.as_str(), query.query_type()) {
        ("bundle.example.test.", RecordType::A) => vec![
            fixture_dns_record("bundle.example.test.", RData::A(A::new(203, 0, 113, 10))),
            fixture_dns_record("bundle.example.test.", RData::A(A::new(203, 0, 113, 11))),
        ],
        ("bundle.example.test.", RecordType::AAAA) => vec![
            fixture_dns_record(
                "bundle.example.test.",
                RData::AAAA(AAAA::new(0x2001, 0x0db8, 0, 0, 0, 0, 0, 0x0010)),
            ),
            fixture_dns_record(
                "bundle.example.test.",
                RData::AAAA(AAAA::new(0x2001, 0x0db8, 0, 0, 0, 0, 0, 0x0011)),
            ),
        ],
        ("bundle.example.test.", RecordType::MX) => vec![fixture_dns_record(
            "bundle.example.test.",
            RData::MX(MX::new(10, fixture_dns_name("mail.example.test."))),
        )],
        ("bundle.example.test.", RecordType::TXT) => vec![
            fixture_dns_record(
                "bundle.example.test.",
                RData::TXT(TXT::new(vec![String::from("v=spf1"), String::from("-all")])),
            ),
            fixture_dns_record(
                "bundle.example.test.",
                RData::TXT(TXT::new(vec![String::from("agentos")])),
            ),
        ],
        ("bundle.example.test.", RecordType::ANY) => vec![
            fixture_dns_record("bundle.example.test.", RData::A(A::new(203, 0, 113, 10))),
            fixture_dns_record("bundle.example.test.", RData::A(A::new(203, 0, 113, 11))),
            fixture_dns_record(
                "bundle.example.test.",
                RData::AAAA(AAAA::new(0x2001, 0x0db8, 0, 0, 0, 0, 0, 0x0010)),
            ),
            fixture_dns_record(
                "bundle.example.test.",
                RData::AAAA(AAAA::new(0x2001, 0x0db8, 0, 0, 0, 0, 0, 0x0011)),
            ),
            fixture_dns_record(
                "bundle.example.test.",
                RData::MX(MX::new(10, fixture_dns_name("mail.example.test."))),
            ),
            fixture_dns_record(
                "bundle.example.test.",
                RData::TXT(TXT::new(vec![String::from("v=spf1"), String::from("-all")])),
            ),
        ],
        ("alias.example.test.", RecordType::CNAME) => vec![fixture_dns_record(
            "alias.example.test.",
            RData::CNAME(CNAME(fixture_dns_name("bundle.example.test."))),
        )],
        ("ptr.example.test.", RecordType::PTR) => vec![fixture_dns_record(
            "ptr.example.test.",
            RData::PTR(PTR(fixture_dns_name("host.example.test."))),
        )],
        ("zone.example.test.", RecordType::NS) => vec![fixture_dns_record(
            "zone.example.test.",
            RData::NS(NS(fixture_dns_name("ns1.example.test."))),
        )],
        ("zone.example.test.", RecordType::SOA) => vec![fixture_dns_record(
            "zone.example.test.",
            RData::SOA(SOA::new(
                fixture_dns_name("ns1.example.test."),
                fixture_dns_name("hostmaster.example.test."),
                2026041601,
                3600,
                600,
                86400,
                60,
            )),
        )],
        ("_svc._tcp.example.test.", RecordType::SRV) => vec![fixture_dns_record(
            "_svc._tcp.example.test.",
            RData::SRV(SRV::new(
                1,
                5,
                8443,
                fixture_dns_name("svc-target.example.test."),
            )),
        )],
        ("naptr.example.test.", RecordType::NAPTR) => vec![fixture_dns_record(
            "naptr.example.test.",
            RData::NAPTR(NAPTR::new(
                10,
                20,
                b"s".to_vec().into_boxed_slice(),
                b"SIP+D2U".to_vec().into_boxed_slice(),
                b"!^.*$!sip:service@example.test!"
                    .to_vec()
                    .into_boxed_slice(),
                fixture_dns_name("_sip._udp.example.test."),
            )),
        )],
        ("caa.example.test.", RecordType::CAA) => vec![
            fixture_dns_record(
                "caa.example.test.",
                RData::CAA(CAA::new_issue(
                    false,
                    Some(fixture_dns_name("letsencrypt.org.")),
                    vec![],
                )),
            ),
            fixture_dns_record(
                "caa.example.test.",
                RData::CAA(CAA::new_iodef(
                    false,
                    url::Url::parse("https://iodef.example.test/report")
                        .expect("fixture CAA iodef URL"),
                )),
            ),
        ],
        _ => Vec::new(),
    }
}

fn fixture_dns_record(name: &str, data: RData) -> Record {
    Record::from_rdata(fixture_dns_name(name), 60, data)
}

fn fixture_dns_name(name: &str) -> Name {
    name.parse().expect("valid fixture DNS name")
}

fn read_http_request(stream: &mut TcpStream) -> String {
    let mut request = Vec::new();
    let mut buffer = [0_u8; 1024];

    loop {
        let bytes_read = stream.read(&mut buffer).expect("read http request");
        assert!(
            bytes_read > 0,
            "connection closed before full HTTP request arrived"
        );
        request.extend_from_slice(&buffer[..bytes_read]);
        if request.windows(4).any(|window| window == b"\r\n\r\n") {
            break;
        }
    }

    String::from_utf8(request).expect("request utf8")
}

fn http_request_custom_agent_reuses_keepalive_socket_impl() {
    assert_node_available();

    let listener = TcpListener::bind("127.0.0.1:0").expect("bind host http listener");
    let port = listener.local_addr().expect("listener addr").port();
    let cwd = temp_dir("builtin-http-agent-keepalive");
    let entrypoint = cwd.join("entry.mjs");
    write_fixture(
        &entrypoint,
        format!(
            r#"
import http from "node:http";

const agent = new http.Agent({{
  keepAlive: true,
  maxSockets: 1,
}});

function request(path) {{
  return new Promise((resolve, reject) => {{
    const req = http.request({{
      host: "127.0.0.1",
      port: {port},
      path,
      method: "GET",
      agent,
    }}, (res) => {{
      res.setEncoding("utf8");
      let body = "";
      res.on("data", (chunk) => {{
        body += chunk;
      }});
      res.on("end", () => {{
        resolve({{
          body,
          reusedSocket: req.reusedSocket,
          socketLocalPort: req.socket?.localPort ?? null,
          statusCode: res.statusCode ?? null,
        }});
      }});
    }});
    req.on("error", reject);
    req.end();
  }});
}}

const first = await request("/first");
const second = await request("/second");
await new Promise((resolve) => setTimeout(resolve, 0));

const freeSockets = Object.values(agent.freeSockets).reduce(
  (total, sockets) => total + sockets.length,
  0,
);

console.log(JSON.stringify({{
  first,
  second,
  freeSockets,
  totalSocketCount: agent.totalSocketCount,
}}));

agent.destroy();
"#,
        ),
    );

    let case_name = "builtin-http-agent-keepalive";
    let mut sidecar = new_sidecar(case_name);
    let connection_id = authenticate_wire(&mut sidecar, &format!("conn-{case_name}"));
    let session_id = open_session_wire(&mut sidecar, 2, &connection_id);
    let allowed_builtins = serde_json::to_string(&["http"]).expect("serialize builtin allowlist");
    let guest_env = HashMap::from([
        (
            String::from("env.AGENTOS_ALLOWED_NODE_BUILTINS"),
            allowed_builtins,
        ),
        (
            String::from("env.AGENTOS_LOOPBACK_EXEMPT_PORTS"),
            format!("[{port}]"),
        ),
    ]);
    let vm_id = create_vm_with_metadata_and_permissions(
        &mut sidecar,
        3,
        &connection_id,
        &session_id,
        GuestRuntimeKind::JavaScript,
        &cwd,
        guest_env,
        wire_permissions_allow_all(),
    );

    let server = thread::spawn(move || {
        listener
            .set_nonblocking(true)
            .expect("configure nonblocking listener");
        let deadline = Instant::now() + Duration::from_secs(10);
        let (mut stream, _) = loop {
            match listener.accept() {
                Ok(accepted) => break accepted,
                Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                    assert!(
                        Instant::now() < deadline,
                        "timed out waiting for guest keep-alive connection"
                    );
                    thread::sleep(Duration::from_millis(10));
                }
                Err(error) => panic!("accept keep-alive connection: {error}"),
            }
        };

        stream
            .set_read_timeout(Some(Duration::from_secs(5)))
            .expect("set read timeout");

        let first_request = read_http_request(&mut stream);
        assert!(
            first_request.contains("GET /first HTTP/1.1"),
            "unexpected first request: {first_request}"
        );
        stream
            .write_all(
                b"HTTP/1.1 200 OK\r\nContent-Length: 5\r\nConnection: keep-alive\r\n\r\nfirst",
            )
            .expect("write first keep-alive response");
        stream.flush().expect("flush first keep-alive response");

        let second_request = read_http_request(&mut stream);
        assert!(
            second_request.contains("GET /second HTTP/1.1"),
            "unexpected second request: {second_request}"
        );
        stream
            .write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 6\r\nConnection: close\r\n\r\nsecond")
            .expect("write second keep-alive response");
        stream.flush().expect("flush second keep-alive response");
    });

    execute_wire(
        &mut sidecar,
        4,
        &connection_id,
        &session_id,
        &vm_id,
        &format!("proc-{case_name}"),
        GuestRuntimeKind::JavaScript,
        &entrypoint,
        Vec::new(),
    );
    let (stdout, stderr, exit_code) = collect_builtin_process_output(
        &mut sidecar,
        &connection_id,
        &session_id,
        &vm_id,
        "proc-builtin-http-agent-keepalive",
    );
    dispose_vm_and_close_session_wire(&mut sidecar, &connection_id, &session_id, &vm_id);

    server.join().expect("join keep-alive server");

    assert_eq!(
        exit_code, 0,
        "guest probe failed for {case_name}\nstdout:\n{stdout}\nstderr:\n{stderr}"
    );
    assert!(
        stderr.trim().is_empty(),
        "guest probe stderr for {case_name}:\n{stderr}"
    );
    let guest: Value = serde_json::from_str(stdout.trim()).expect("parse guest probe JSON");

    assert_eq!(guest["first"]["statusCode"], 200);
    assert_eq!(guest["first"]["body"], "first");
    assert_eq!(guest["first"]["reusedSocket"], false);
    assert_eq!(guest["second"]["statusCode"], 200);
    assert_eq!(guest["second"]["body"], "second");
    assert_eq!(guest["second"]["reusedSocket"], true);
    assert_eq!(
        guest["first"]["socketLocalPort"], guest["second"]["socketLocalPort"],
        "expected second request to reuse the first socket"
    );
    // The second response explicitly closes the connection. Node removes that
    // reused socket from both the active and free pools by the next timer turn.
    assert_eq!(guest["freeSockets"], 0);
    assert_eq!(guest["totalSocketCount"], 0);
}

fn http_request_denied_egress_returns_permission_error_impl() {
    assert_node_available();

    let cwd = temp_dir("builtin-http-agent-denied");
    let entrypoint = cwd.join("entry.mjs");
    write_fixture(
        &entrypoint,
        r#"
import http from "node:http";

const result = await new Promise((resolve) => {
  // A scalar network denial keeps VM-local loopback, so probe an external
  // address that crosses the VM boundary.
  const req = http.get("http://203.0.113.10:9/denied", (res) => {
    res.resume();
    resolve({
      statusCode: res.statusCode ?? null,
      unexpected: true,
    });
  });
  req.on("error", (error) => {
    resolve({
      code: error?.code ?? null,
      message: String(error?.message ?? ""),
      name: error?.name ?? null,
    });
  });
});

console.log(JSON.stringify(result));
"#,
    );

    let allow_all = wire_permissions_allow_all();
    let guest = run_guest_probe_with_config(
        "builtin-http-agent-denied",
        &cwd,
        &entrypoint,
        HashMap::new(),
        PermissionsPolicy {
            fs: allow_all.fs,
            network: Some(PatternPermissionScope::PermissionMode(PermissionMode::Deny)),
            child_process: allow_all.child_process,
            process: allow_all.process,
            env: allow_all.env,
            host_function: allow_all.host_function,
        },
        &["http"],
    );

    assert_eq!(guest["code"], "EACCES");
    assert_eq!(guest["unexpected"], Value::Null);
    assert!(
        guest["message"]
            .as_str()
            .is_some_and(|message| message.contains("permission denied")),
        "unexpected denied-egress payload: {guest}"
    );
}

fn loopback_connect_without_vm_listener_is_refused_impl() {
    assert_node_available();

    // A host service listens on a port the VM does not own and does not exempt.
    let host_service = TcpListener::bind("127.0.0.1:0").expect("bind host loopback service");
    host_service
        .set_nonblocking(true)
        .expect("set host loopback service nonblocking");
    let port = host_service
        .local_addr()
        .expect("host loopback service address")
        .port();

    let cwd = temp_dir("builtin-loopback-connect-refused");
    let entrypoint = cwd.join("entry.mjs");
    write_fixture(
        &entrypoint,
        format!(
            r#"
import net from "node:net";

const netConnect = await new Promise((resolve) => {{
  const socket = net.connect({port}, "127.0.0.1");
  socket.on("connect", () => {{
    socket.destroy();
    resolve({{ connected: true }});
  }});
  socket.on("error", (error) => resolve({{ code: error.code ?? null }}));
}});
const fetched = await fetch("http://127.0.0.1:{port}/").then(
  (response) => ({{ status: response.status }}),
  (error) => ({{ code: (error.cause ?? error).code ?? null }}),
);

console.log(JSON.stringify({{ netConnect, fetched }}));
"#
        ),
    );

    let guest = run_guest_probe_with_config(
        "builtin-loopback-connect-refused",
        &cwd,
        &entrypoint,
        HashMap::new(),
        wire_permissions_allow_all(),
        &["net"],
    );
    match host_service.accept() {
        Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {}
        other => panic!("guest loopback reached the non-exempt host service: {other:?}"),
    }
    assert_eq!(
        guest["netConnect"]["code"], "ECONNREFUSED",
        "guest: {guest}"
    );

    // With the host service closed, host Node sees the same closed loopback port.
    drop(host_service);
    let host = run_host_probe(&cwd, &entrypoint);
    assert_eq!(
        guest, host,
        "guest V8 result diverged from host Node\nhost: {host}\nguest: {guest}"
    );
}

#[test]
fn loopback_connect_without_vm_listener_is_refused() {
    run_isolated_builtin_conformance_test("loopback-connect-refused");
}

#[test]
fn http_request_custom_agent_reuses_keepalive_socket() {
    run_isolated_builtin_conformance_test("http-request-keepalive");
}

#[test]
fn http_request_denied_egress_returns_permission_error() {
    run_isolated_builtin_conformance_test("http-request-denied");
}

fn http_socket_writes_do_not_silently_drop_data_impl() {
    assert_node_available();

    let request_socket_listener =
        TcpListener::bind("127.0.0.1:0").expect("bind host request-socket listener");
    let request_socket_port = request_socket_listener
        .local_addr()
        .expect("request-socket listener addr")
        .port();
    let request_socket_payload = "agent-socket-payload";

    let request_socket_server = thread::spawn(move || {
        let (mut stream, _) = request_socket_listener
            .accept()
            .expect("accept request-socket stream");
        stream
            .set_read_timeout(Some(Duration::from_secs(5)))
            .expect("set request-socket read timeout");

        let request = read_http_request(&mut stream);
        assert!(
            request.contains("GET /socket-write HTTP/1.1"),
            "unexpected keep-alive request: {request}"
        );

        stream
            .write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 2\r\nConnection: keep-alive\r\n\r\nok")
            .expect("write keep-alive response");
        stream.flush().expect("flush keep-alive response");

        let mut payload = vec![0; request_socket_payload.len()];
        match stream.read(&mut payload) {
            Ok(0) => {}
            Ok(bytes_read) => {
                let payload = payload[..bytes_read].to_vec();
                assert_eq!(
                    String::from_utf8(payload.clone()).expect("utf8 tunneled payload"),
                    request_socket_payload
                );
                stream
                    .shutdown(Shutdown::Write)
                    .expect("shutdown request-socket write half");
            }
            Err(error)
                if matches!(
                    error.kind(),
                    std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut
                ) => {}
            Err(error) => panic!("read request-socket payload: {error}"),
        }
    });

    let cwd = temp_dir("builtin-http-socket-writes");
    let entrypoint = cwd.join("entry.mjs");
    write_fixture(
        &entrypoint,
        format!(
            r#"
import http from "node:http";

const requestSocketResult = await new Promise((resolve, reject) => {{
  const agent = new http.Agent({{ keepAlive: true, maxSockets: 1 }});
  const req = http.request({{
    host: "127.0.0.1",
    port: {request_socket_port},
    path: "/socket-write",
    method: "GET",
    agent,
    headers: {{ Connection: "keep-alive" }},
  }}, (res) => {{
    res.resume();
    res.on("end", () => {{
      const payload = "{request_socket_payload}";
      const finish = (result) => {{
        agent.destroy();
        resolve(result);
      }};

      req.socket.once("error", (error) => {{
        finish({{
          outcome: error?.code ?? error?.name ?? String(error),
          sameSocket: !!req.socket,
          statusCode: res.statusCode ?? null,
        }});
      }});

      try {{
        let writeReturn = null;
        writeReturn = req.socket.write(payload, () => {{
          finish({{
            outcome: "forwarded",
            writeReturn,
            sameSocket: !!req.socket,
            statusCode: res.statusCode ?? null,
          }});
        }});
      }} catch (error) {{
        finish({{
          outcome: error?.code ?? error?.name ?? String(error),
          sameSocket: !!req.socket,
          statusCode: res.statusCode ?? null,
        }});
      }}
    }});
  }});
  req.on("error", reject);
  req.end();
}});

const responseResult = (() => {{
  const res = new http.ServerResponse({{ method: "GET" }});
  const result = {{
    hasConnectionAlias: res.connection === res.socket,
    socketPresent: !!res.socket,
  }};
  try {{
    result.outcome = "forwarded";
    result.returnValue = res.socket.write("socket-body:");
  }} catch (error) {{
    result.outcome = error?.code ?? error?.name ?? String(error);
  }}
  res.end("tail");
  result.body = Buffer.concat(res._chunks ?? []).toString("utf8");
  result.headersSent = res.headersSent;
  result.writableFinished = res.writableFinished;
  return result;
}})();

console.log(JSON.stringify({{ requestSocketResult, responseResult }}));
await new Promise((resolve) => setTimeout(resolve, 0));
process.exit(0);
"#,
        ),
    );

    let guest = run_guest_probe_with_config(
        "builtin-http-socket-writes",
        &cwd,
        &entrypoint,
        HashMap::from([(
            String::from("env.AGENTOS_LOOPBACK_EXEMPT_PORTS"),
            format!("[{request_socket_port}]"),
        )]),
        wire_permissions_allow_all(),
        &["http"],
    );

    request_socket_server
        .join()
        .expect("join request-socket fixture server");

    assert_eq!(guest["requestSocketResult"]["statusCode"], 200);
    assert_eq!(
        guest["requestSocketResult"]["sameSocket"],
        Value::Bool(true)
    );
    let connect_outcome = guest["requestSocketResult"]["outcome"]
        .as_str()
        .expect("req.socket outcome");
    assert!(
        connect_outcome == "forwarded" || connect_outcome == "ERR_NOT_IMPLEMENTED",
        "unexpected req.socket.write outcome: {guest}"
    );
    if connect_outcome == "forwarded" {
        assert_eq!(guest["requestSocketResult"]["statusCode"], 200);
    }

    assert_eq!(guest["responseResult"]["socketPresent"], Value::Bool(true));
    assert_eq!(
        guest["responseResult"]["hasConnectionAlias"],
        Value::Bool(true)
    );
    let response_outcome = guest["responseResult"]["outcome"]
        .as_str()
        .expect("ServerResponse.socket outcome");
    assert!(
        response_outcome == "forwarded" || response_outcome == "ERR_NOT_IMPLEMENTED",
        "unexpected res.socket.write outcome: {guest}"
    );
    if response_outcome == "forwarded" {
        assert_eq!(
            guest["responseResult"]["returnValue"],
            Value::Bool(true),
            "expected res.socket.write to mirror ServerResponse.write return value"
        );
        assert_eq!(guest["responseResult"]["headersSent"], Value::Bool(true));
        assert_eq!(
            guest["responseResult"]["writableFinished"],
            Value::Bool(true)
        );
        assert_eq!(
            guest["responseResult"]["body"],
            Value::String(String::from("socket-body:tail"))
        );
    } else {
        assert_eq!(
            guest["responseResult"]["body"],
            Value::String(String::from("tail"))
        );
    }
}

#[test]
fn http_socket_writes_do_not_silently_drop_data() {
    run_isolated_builtin_conformance_test("http-socket-writes");
}

fn net_socket_readable_state_tracks_ssh2_writable_shape_impl() {
    assert_node_available();

    let cwd = temp_dir("builtin-net-socket-readable-state");
    let entrypoint = cwd.join("entry.mjs");
    write_fixture(
        &entrypoint,
        r#"
import net from "node:net";

const isWritable = (stream) =>
  Boolean(stream?.writable && stream?._readableState?.ended === false);

const socket = new net.Socket();
const open = {
  ended: socket._readableState?.ended ?? null,
  endEmitted: socket._readableState?.endEmitted ?? null,
  writable: socket.writable ?? null,
  isWritable: isWritable(socket),
};

socket.destroy();

const closed = {
  ended: socket._readableState?.ended ?? null,
  endEmitted: socket._readableState?.endEmitted ?? null,
  writable: socket.writable ?? null,
  isWritable: isWritable(socket),
  destroyed: socket.destroyed ?? null,
};

console.log(JSON.stringify({ open, closed }));
"#,
    );

    let guest = run_guest_probe_with_config(
        "net-socket-readable-state",
        &cwd,
        &entrypoint,
        HashMap::new(),
        wire_permissions_allow_all(),
        &["net"],
    );

    assert_eq!(guest["open"]["ended"], Value::Bool(false));
    assert_eq!(guest["open"]["endEmitted"], Value::Bool(false));
    assert_eq!(guest["open"]["isWritable"], Value::Bool(true));
    assert_eq!(guest["closed"]["ended"], Value::Bool(true));
    assert_eq!(guest["closed"]["endEmitted"], Value::Bool(true));
    assert_eq!(guest["closed"]["isWritable"], Value::Bool(false));
}

#[test]
fn net_socket_readable_state_tracks_ssh2_writable_shape() {
    run_isolated_builtin_conformance_test("net-socket-readable-state");
}

fn net_socket_writes_larger_than_socket_buffer_match_host_node_impl() {
    assert_node_available();

    let cwd = temp_dir("builtin-net-large-socket-write");
    let entrypoint = cwd.join("entry.mjs");
    // 6 MiB exceeds the default 4 MiB VM socket buffer budget, so the writer
    // must wait for the in-VM reader to drain instead of failing.
    write_fixture(
        &entrypoint,
        r#"
import net from "node:net";

const payload = Buffer.alloc(6 * 1024 * 1024);
for (let index = 0; index < payload.length; index += 1) payload[index] = index % 251;
const first = payload.subarray(0, 5 * 1024 * 1024);
const second = payload.subarray(5 * 1024 * 1024);

const server = net.createServer();
await new Promise((resolve) => server.listen(0, "127.0.0.1", resolve));
const received = new Promise((resolve) => {
  server.once("connection", (socket) => {
    const chunks = [];
    socket.on("data", (chunk) => chunks.push(chunk));
    socket.on("close", () => resolve(Buffer.concat(chunks)));
  });
});

const callbacks = [];
let firstWriteReturned = null;
let clientError = null;
const client = net.connect(server.address().port, "127.0.0.1");
const clientClosed = new Promise((resolve) => client.on("close", resolve));
client.on("error", (error) => {
  clientError = error.code ?? error.message;
});
client.on("connect", () => {
  firstWriteReturned = client.write(first, () => callbacks.push("first"));
  client.write(second, () => callbacks.push("second"));
  client.end(() => callbacks.push("end"));
});

const [bytes] = await Promise.all([received, clientClosed]);
server.close();
console.log(JSON.stringify({
  receivedBytes: bytes.length,
  intact: bytes.equals(payload),
  callbacks,
  firstWriteReturned,
  clientError,
}));
"#,
    );

    let host = run_host_probe(&cwd, &entrypoint);
    let guest = run_guest_probe_with_config(
        "net-large-socket-write",
        &cwd,
        &entrypoint,
        HashMap::new(),
        wire_permissions_allow_all(),
        &["net", "buffer"],
    );

    assert_eq!(
        guest, host,
        "guest V8 result diverged from host Node for net-large-socket-write"
    );
}

#[test]
fn net_socket_writes_larger_than_socket_buffer_match_host_node() {
    run_isolated_builtin_conformance_test("net-large-socket-write");
}

fn readable_on_data_respects_explicit_pause_matches_host_node_impl() {
    assert_conformance(
        "readable-on-data-explicit-pause",
        r#"
import fs from "node:fs";

const fixturePath = new URL("./fixture.txt", import.meta.url);
fs.writeFileSync(fixturePath, "abcdef");

const delay = (ms) => new Promise((resolve) => setTimeout(resolve, ms));

async function pauseThenOnDataThenResume() {
  const stream = fs.createReadStream(fixturePath, { encoding: "utf8", highWaterMark: 2 });
  const chunks = [];
  stream.pause();
  const afterPause = stream.readableFlowing;
  stream.on("data", (chunk) => chunks.push(chunk));
  const afterOnData = stream.readableFlowing;
  await delay(20);
  const beforeResumeChunkCount = chunks.length;
  stream.resume();
  const afterResume = stream.readableFlowing;
  await new Promise((resolve, reject) => {
    stream.on("end", resolve);
    stream.on("error", reject);
  });
  return {
    afterPause,
    afterOnData,
    beforeResumeChunkCount,
    afterResume,
    chunks,
  };
}

async function onDataAlone() {
  const stream = fs.createReadStream(fixturePath, { encoding: "utf8", highWaterMark: 2 });
  const chunks = [];
  const initialFlowing = stream.readableFlowing;
  stream.on("data", (chunk) => chunks.push(chunk));
  const afterOnData = stream.readableFlowing;
  await new Promise((resolve, reject) => {
    stream.on("end", resolve);
    stream.on("error", reject);
  });
  return {
    initialFlowing,
    afterOnData,
    chunks,
  };
}

async function multiplePauseResumeCycles() {
  const stream = fs.createReadStream(fixturePath, { encoding: "utf8", highWaterMark: 2 });
  const chunks = [];
  const checkpoints = [];
  let firstChunkSeen = false;

  stream.pause();
  checkpoints.push(["afterInitialPause", stream.readableFlowing]);
  stream.on("data", (chunk) => {
    chunks.push(chunk);
    if (!firstChunkSeen) {
      firstChunkSeen = true;
      stream.pause();
      checkpoints.push(["afterMidStreamPause", stream.readableFlowing]);
      setTimeout(() => {
        checkpoints.push(["beforeSecondResumeChunkCount", chunks.length]);
        stream.resume();
        checkpoints.push(["afterSecondResume", stream.readableFlowing]);
      }, 20);
    }
  });
  checkpoints.push(["afterOnData", stream.readableFlowing]);
  await delay(20);
  checkpoints.push(["beforeFirstResumeChunkCount", chunks.length]);
  stream.resume();
  checkpoints.push(["afterFirstResume", stream.readableFlowing]);
  await new Promise((resolve, reject) => {
    stream.on("end", resolve);
    stream.on("error", reject);
  });
  return { checkpoints, chunks };
}

console.log(JSON.stringify({
  pauseThenOnDataThenResume: await pauseThenOnDataThenResume(),
  onDataAlone: await onDataAlone(),
  multiplePauseResumeCycles: await multiplePauseResumeCycles(),
}));
"#,
    );
}

#[test]
fn readable_on_data_respects_explicit_pause_matches_host_node() {
    run_isolated_builtin_conformance_test("readable-on-data-explicit-pause");
}

fn readline_question_reads_real_stdin_impl() {
    assert_node_available();

    let cwd = temp_dir("builtin-readline-question");
    let entrypoint = cwd.join("entry.mjs");
    write_fixture(
        &entrypoint,
        r#"
import readline from "node:readline";

const output = { write() {} };
const rl = readline.createInterface({ input: process.stdin, output });
process.stdout.write("__READY__\n");

const callbackAnswer = await new Promise((resolve, reject) => {
  const timeout = setTimeout(() => reject(new Error("callback question timed out")), 2000);
  rl.question("callback> ", (answer) => {
    clearTimeout(timeout);
    resolve(answer);
  });
});

const promiseAnswer = await rl.question("promise> ");
rl.close();

console.log(JSON.stringify({ callbackAnswer, promiseAnswer }));
"#,
    );

    let mut sidecar = new_sidecar("builtin-readline-question");
    let connection_id = authenticate_wire(&mut sidecar, "conn-readline-question");
    let session_id = open_session_wire(&mut sidecar, 2, &connection_id);
    let vm_id = create_vm_with_metadata_and_permissions(
        &mut sidecar,
        3,
        &connection_id,
        &session_id,
        GuestRuntimeKind::JavaScript,
        &cwd,
        HashMap::from([
            (
                String::from("env.AGENTOS_ALLOWED_NODE_BUILTINS"),
                serde_json::to_string(&["readline"]).expect("serialize builtin allowlist"),
            ),
            (
                String::from("env.AGENTOS_KEEP_STDIN_OPEN"),
                String::from("1"),
            ),
        ]),
        wire_permissions_allow_all(),
    );

    execute_wire(
        &mut sidecar,
        4,
        &connection_id,
        &session_id,
        &vm_id,
        "proc-readline-question",
        GuestRuntimeKind::JavaScript,
        &entrypoint,
        Vec::new(),
    );
    let ownership = wire_session(&connection_id, &session_id);
    let deadline = Instant::now() + Duration::from_secs(10);
    let mut stdout = String::new();
    let mut stderr = String::new();
    let mut exit = None;
    let mut stdin_sent = false;

    loop {
        let event = sidecar
            .poll_event_wire_blocking(&ownership, Duration::from_millis(100))
            .expect("poll readline question wire event");

        if let Some(event) = event {
            assert_eq!(
                event.ownership,
                wire_vm(&connection_id, &session_id, &vm_id)
            );

            match event.payload {
                EventPayload::ProcessOutputEvent(output)
                    if output.process_id == "proc-readline-question" =>
                {
                    match output.channel {
                        StreamChannel::Stdout => append_probe_output(
                            &mut stdout,
                            &output.chunk,
                            &output.process_id,
                            "stdout",
                        ),
                        StreamChannel::Stderr => append_probe_output(
                            &mut stderr,
                            &output.chunk,
                            &output.process_id,
                            "stderr",
                        ),
                    }
                }
                EventPayload::ProcessExitedEvent(exited)
                    if exited.process_id == "proc-readline-question" =>
                {
                    exit = Some((exited.exit_code, Instant::now()));
                }
                _ => {}
            }
        }

        if !stdin_sent && stdout.contains("__READY__\n") {
            write_process_stdin(
                &mut sidecar,
                5,
                &connection_id,
                &session_id,
                &vm_id,
                "proc-readline-question",
                "hello\nworld\n",
            );
            close_process_stdin(
                &mut sidecar,
                6,
                &connection_id,
                &session_id,
                &vm_id,
                "proc-readline-question",
            );
            stdin_sent = true;
        }

        if let Some((exit_code, seen_at)) = exit {
            if Instant::now().duration_since(seen_at) >= Duration::from_millis(200) {
                let stdout = stdout.replace("__READY__\n", "");
                dispose_vm_and_close_session_wire(
                    &mut sidecar,
                    &connection_id,
                    &session_id,
                    &vm_id,
                );

                assert_eq!(
                    exit_code, 0,
                    "readline question probe failed\nstdout:\n{stdout}\nstderr:\n{stderr}"
                );
                assert!(stderr.trim().is_empty(), "unexpected stderr:\n{stderr}");

                let payload: Value =
                    serde_json::from_str(stdout.trim()).expect("parse readline JSON");
                assert_eq!(payload["callbackAnswer"], "hello");
                assert_eq!(payload["promiseAnswer"], "world");
                return;
            }
        }

        assert!(
            Instant::now() < deadline,
            "timed out waiting for readline question probe\nstdout:\n{stdout}\nstderr:\n{stderr}"
        );
    }
}

#[test]
fn readline_question_reads_real_stdin() {
    run_isolated_builtin_conformance_test("readline-question");
}

fn vm_is_context_only_accepts_create_context_tagged_sandboxes_impl() {
    assert_conformance(
        "vm-is-context",
        r#"
import { createRequire } from "node:module";

const require = createRequire(import.meta.url);
const vm = require("node:vm");

const safeIsContext = (value) => {
  try {
    return vm.isContext(value);
  } catch {
    return false;
  }
};

const sandbox = {};
const tagged = vm.createContext(sandbox);
const taggedArray = vm.createContext([]);

console.log(JSON.stringify({
  sameReference: tagged === sandbox,
  matrix: {
    plainObject: safeIsContext({}),
    taggedObject: safeIsContext(tagged),
    plainArray: safeIsContext([]),
    taggedArray: safeIsContext(taggedArray),
    functionValue: safeIsContext(function demo() {}),
    nullValue: safeIsContext(null),
    numberValue: safeIsContext(1),
    stringValue: safeIsContext("text"),
  },
}));
"#,
    );
}

#[test]
fn vm_is_context_only_accepts_create_context_tagged_sandboxes() {
    run_isolated_builtin_conformance_test("vm-is-context");
}

fn vm_context_isolation_and_script_options_match_host_node_impl() {
    assert_conformance(
        "vm-context-isolation",
        r#"
import { createRequire } from "node:module";

const require = createRequire(import.meta.url);
const vm = require("node:vm");

const sandbox = { answer: 41 };
const context = vm.createContext(sandbox);
const runResult = vm.runInContext("answer += 1; typeof globalThis.require", context);

let filenameLine = false;
try {
  new vm.Script("throw new Error('boom')", {
    filename: "named-vm.js",
    lineOffset: 2,
    columnOffset: 4,
  }).runInNewContext({});
} catch (error) {
  filenameLine = String(error?.stack ?? error).includes("named-vm.js:3");
}

let invalidContextType = null;
try {
  vm.runInContext("1 + 1", {});
} catch (error) {
  invalidContextType = error?.name ?? null;
}

console.log(JSON.stringify({
  sameReference: context === sandbox,
  sandboxAnswer: sandbox.answer,
  newContextRequire: vm.runInNewContext("typeof globalThis.require"),
  newContextBuffer: vm.runInNewContext("typeof Buffer"),
  contextRequire: runResult,
  filenameLine,
  invalidContextType,
}));
"#,
    );
}

#[test]
fn vm_context_isolation_and_script_options_match_host_node() {
    run_isolated_builtin_conformance_test("vm-context-isolation");
}

fn vm_timeout_terminates_within_deadline_impl() {
    let cwd = temp_dir("builtin-conformance-vm-timeout");
    let entrypoint = cwd.join("entry.mjs");
    write_fixture(
        &entrypoint,
        r#"
import { createRequire } from "node:module";

const require = createRequire(import.meta.url);
const vm = require("node:vm");

const started = Date.now();
let timeoutCode = null;
let timeoutMessage = null;
try {
  vm.runInNewContext("while (true) {}", {}, { timeout: 100 });
} catch (error) {
  timeoutCode = error?.code ?? null;
  timeoutMessage = String(error?.message ?? error);
}

console.log(JSON.stringify({
  elapsedMs: Date.now() - started,
  timeoutCode,
  timeoutMessage,
}));
"#,
    );

    let result = run_guest_probe("vm-timeout", &cwd, &entrypoint);
    let elapsed_ms = result["elapsedMs"]
        .as_u64()
        .expect("vm timeout elapsed milliseconds");
    if run_timing_sensitive_tests() {
        assert!(
            elapsed_ms <= 200,
            "vm timeout exceeded 200ms: {elapsed_ms}ms ({result})"
        );
    }
    assert_eq!(
        result["timeoutCode"],
        Value::String(String::from("ERR_SCRIPT_EXECUTION_TIMEOUT"))
    );
    assert!(
        result["timeoutMessage"]
            .as_str()
            .is_some_and(|message| message.contains("timed out")),
        "vm timeout message missing timeout marker: {result}"
    );
}

#[test]
fn vm_timeout_terminates_within_deadline() {
    run_isolated_builtin_conformance_test("vm-timeout");
}

fn vm_optional_surface_is_implemented_or_explicitly_not_implemented_impl() {
    let cwd = temp_dir("builtin-conformance-vm-optional-surface");
    let entrypoint = cwd.join("entry.mjs");
    write_fixture(
        &entrypoint,
        r#"
import { createRequire } from "node:module";

const require = createRequire(import.meta.url);
const vm = require("node:vm");

function capture(label, fn) {
  try {
    const value = fn();
    return typeof value?.then === "function" ? "ok" : "ok";
  } catch (error) {
    return error?.code ?? `${label}-error`;
  }
}

console.log(JSON.stringify({
  compileFunction: capture("compileFunction", () => vm.compileFunction("return value;", ["value"])),
  measureMemory: capture("measureMemory", () => vm.measureMemory()),
}));
"#,
    );

    let result = run_guest_probe("vm-optional-surface", &cwd, &entrypoint);
    for key in ["compileFunction", "measureMemory"] {
        let outcome = result[key].as_str().unwrap_or_default();
        assert!(
            outcome == "ok" || outcome == "ERR_NOT_IMPLEMENTED",
            "vm optional surface {key} returned unexpected outcome: {result}"
        );
    }
}

#[test]
fn vm_optional_surface_is_implemented_or_explicitly_not_implemented() {
    run_isolated_builtin_conformance_test("vm-optional-surface");
}

fn perf_hooks_observer_and_histogram_match_host_node_impl() {
    assert_conformance(
        "perf-hooks-observer",
        r#"
import { createRequire } from "node:module";

const require = createRequire(import.meta.url);
const { PerformanceObserver, createHistogram, performance } = require("node:perf_hooks");

function sortEntries(entries) {
  return [...entries].sort((left, right) => left.localeCompare(right));
}

function toEntryNames(entries) {
  return entries.map((entry) => `${entry.entryType}:${entry.name}`);
}

performance.clearMarks?.();
performance.clearMeasures?.();

const callbackEntries = [];
const observer = new PerformanceObserver((list) => {
  callbackEntries.push(...toEntryNames(list.getEntries()));
});
observer.observe({ entryTypes: ["mark", "measure"] });
performance.mark("start");
performance.mark("end");
performance.measure("delta", "start", "end");
await new Promise((resolve) => setImmediate(resolve));
const callbackObserved = sortEntries(callbackEntries);
const afterFlush = sortEntries(toEntryNames(observer.takeRecords()));
observer.disconnect();

performance.clearMarks?.();
performance.clearMeasures?.();

const takeRecordsObserver = new PerformanceObserver(() => {});
takeRecordsObserver.observe({ entryTypes: ["mark", "measure"] });
performance.mark("alpha");
performance.mark("omega");
performance.measure("window", "alpha", "omega");
const takeRecordsBeforeFlush = sortEntries(
  toEntryNames(takeRecordsObserver.takeRecords()),
);
await new Promise((resolve) => setImmediate(resolve));
const takeRecordsAfterFlush = sortEntries(
  toEntryNames(takeRecordsObserver.takeRecords()),
);
takeRecordsObserver.disconnect();

const histogram = createHistogram();
histogram.record(10);
histogram.record(20);
histogram.record(30);

console.log(JSON.stringify({
  callbackObserved,
  afterFlush,
  takeRecordsBeforeFlush,
  takeRecordsAfterFlush,
  histogram: {
    emptyP50: createHistogram().percentile(50),
    p50: histogram.percentile(50),
    p90: histogram.percentile(90),
  },
}));
"#,
    );
}

#[test]
fn perf_hooks_observer_and_histogram_match_host_node() {
    run_isolated_builtin_conformance_test("perf-hooks-observer");
}

fn run_guest_script(case_name: &str, script: &str) -> Value {
    assert_node_available();

    let cwd = temp_dir(&format!("builtin-guest-{case_name}"));
    let entrypoint = cwd.join("entry.mjs");
    write_fixture(&entrypoint, script);

    run_guest_probe(case_name, &cwd, &entrypoint)
}

fn process_runtime_stats_are_live_impl() {
    let cwd = temp_dir("process-runtime-stats");
    let entrypoint = cwd.join("entry.mjs");
    write_fixture(
        &entrypoint,
        r#"
const before = process.memoryUsage();
const beforeCpu = process.cpuUsage();
const retained = [];
for (let i = 0; i < 25000; i += 1) {
  retained.push({
    index: i,
    text: `${i}-`.padEnd(256, String(i % 10)),
  });
}
let cpuAccumulator = 0;
for (let i = 0; i < 500000; i += 1) {
  cpuAccumulator += Math.sqrt(i % 1000);
}
globalThis.__retainedProcessStatsFixture = retained;
const after = process.memoryUsage();
const deltaCpu = process.cpuUsage(beforeCpu);
const resource = process.resourceUsage();

console.log(JSON.stringify({
  before,
  after,
  deltaCpu,
  resource,
  versions: {
    node: process.versions.node,
    v8: process.versions.v8,
    openssl: process.versions.openssl,
  },
  retainedCount: retained.length,
  cpuAccumulator,
}));
"#,
    );
    let guest = run_guest_probe_with_config(
        "process-runtime-stats",
        &cwd,
        &entrypoint,
        HashMap::new(),
        wire_permissions_allow_all(),
        &[],
    );

    let before_heap_used = guest["before"]["heapUsed"]
        .as_u64()
        .expect("before heapUsed should be a number");
    let after_heap_used = guest["after"]["heapUsed"]
        .as_u64()
        .expect("after heapUsed should be a number");
    assert!(
        after_heap_used > before_heap_used + 512_000,
        "expected heapUsed to grow by at least 512KiB after allocation, before={before_heap_used}, after={after_heap_used}, guest={guest}",
    );

    let user_cpu = guest["deltaCpu"]["user"]
        .as_u64()
        .expect("cpuUsage.user should be a number");
    let system_cpu = guest["deltaCpu"]["system"]
        .as_u64()
        .expect("cpuUsage.system should be a number");
    assert!(
        user_cpu + system_cpu > 0,
        "expected cpuUsage delta to report live CPU time, guest={guest}",
    );

    for field in [
        "userCPUTime",
        "systemCPUTime",
        "maxRSS",
        "minorPageFault",
        "majorPageFault",
        "voluntaryContextSwitches",
        "involuntaryContextSwitches",
    ] {
        assert!(
            guest["resource"][field].is_number(),
            "expected resourceUsage.{field} to be numeric, guest={guest}",
        );
    }

    assert_eq!(
        guest["versions"]["v8"],
        Value::String(v8::V8::get_version().to_string())
    );
    assert_eq!(
        guest["versions"]["openssl"],
        Value::String(agentos_execution::EMULATED_OPENSSL_VERSION.to_owned())
    );
}

#[test]
fn process_runtime_stats_are_live() {
    run_isolated_builtin_conformance_test("process-runtime-stats");
}

fn os_resource_limits_are_vm_scoped_impl() {
    let cwd = temp_dir("builtin-conformance-os-resource-limits");
    let entrypoint = cwd.join("entry.mjs");
    write_fixture(
        &entrypoint,
        r#"
import os from "node:os";

console.log(JSON.stringify({
  availableParallelism: os.availableParallelism(),
  cpusLength: os.cpus().length,
  freemem: os.freemem(),
  totalmem: os.totalmem(),
  username: os.userInfo().username,
  homedir: os.homedir(),
  userInfoHomedir: os.userInfo().homedir,
  envUser: process.env.USER,
  envHome: process.env.HOME,
}));
"#,
    );

    let mut sidecar = new_sidecar("os-resource-limits");
    let connection_id = authenticate_wire(&mut sidecar, "conn-os-resource-limits");
    let session_id = open_session_wire(&mut sidecar, 2, &connection_id);

    let constrained = run_guest_probe_in_existing_session(
        &mut sidecar,
        3,
        &connection_id,
        &session_id,
        "os-resource-limits-constrained",
        &cwd,
        &entrypoint,
        HashMap::from([
            (String::from("resource.cpu_count"), String::from("2")),
            (
                String::from("env.HOME"),
                String::from("/tmp/constrained-home"),
            ),
            (
                String::from("resource.max_wasm_memory_bytes"),
                (64_u64 * 1024 * 1024).to_string(),
            ),
        ]),
    );
    let expanded = run_guest_probe_in_existing_session(
        &mut sidecar,
        5,
        &connection_id,
        &session_id,
        "os-resource-limits-expanded",
        &cwd,
        &entrypoint,
        HashMap::from([
            (String::from("resource.cpu_count"), String::from("5")),
            (String::from("env.HOME"), String::from("/tmp/expanded-home")),
            (
                String::from("resource.max_wasm_memory_bytes"),
                (256_u64 * 1024 * 1024).to_string(),
            ),
        ]),
    );

    sidecar
        .close_session_blocking(&connection_id, &session_id)
        .expect("close sidecar session");
    sidecar
        .remove_connection_blocking(&connection_id)
        .expect("remove sidecar connection");

    // Virtual identity must reflect the configured VM (distinct from the loader's
    // hardcoded root/"/root" defaults) — this is the sidecar-level coverage that
    // exercises the guestOs/`import os` path, which an engine-only test does not.
    assert_eq!(constrained["username"], "agentos");
    assert_eq!(constrained["homedir"], "/tmp/constrained-home");
    assert_eq!(constrained["envHome"], "/tmp/constrained-home");
    assert_eq!(constrained["userInfoHomedir"], "/home/agentos");
    assert_eq!(expanded["username"], "agentos");
    assert_eq!(expanded["homedir"], "/tmp/expanded-home");
    assert_eq!(expanded["envHome"], "/tmp/expanded-home");
    assert_eq!(expanded["userInfoHomedir"], "/home/agentos");

    assert_eq!(constrained["availableParallelism"], 2);
    assert_eq!(constrained["cpusLength"], 2);
    assert_eq!(constrained["totalmem"], 64_u64 * 1024 * 1024);
    assert_eq!(constrained["freemem"], 64_u64 * 1024 * 1024);

    assert_eq!(expanded["availableParallelism"], 5);
    assert_eq!(expanded["cpusLength"], 5);
    assert_eq!(expanded["totalmem"], 256_u64 * 1024 * 1024);
    assert_eq!(expanded["freemem"], 256_u64 * 1024 * 1024);

    assert_ne!(constrained, expanded);
}

#[test]
fn os_resource_limits_are_vm_scoped() {
    run_isolated_builtin_conformance_test("os-resource-limits");
}

fn dns_conformance_matches_host_node() {
    assert_node_available();

    let dns_server = FixtureDnsServer::start();
    let dns_server_addr = dns_server.addr.to_string();
    let cwd = temp_dir("builtin-conformance-dns");
    let entrypoint = cwd.join("entry.mjs");
    write_fixture(
        &entrypoint,
        r#"
import dns from "node:dns";

if (process.env.AGENTOS_TEST_DNS_SERVER) {
  dns.setServers([process.env.AGENTOS_TEST_DNS_SERVER]);
}

function sortStrings(values) {
  return [...values].sort((left, right) => left.localeCompare(right));
}

function sortObjects(values) {
  return [...values].sort((left, right) =>
    JSON.stringify(left).localeCompare(JSON.stringify(right)),
  );
}

function resolveWithCallback(hostname, rrtype) {
  return new Promise((resolve, reject) => {
    dns.resolve(hostname, rrtype, (error, records) => {
      if (error) {
        reject(error);
        return;
      }
      resolve(records);
    });
  });
}

const resolveAny = sortObjects(await dns.promises.resolveAny("bundle.example.test"));
const results = {
  resolveCallbackA: sortStrings(await resolveWithCallback("bundle.example.test", "A")),
  resolve4: sortStrings(await dns.promises.resolve4("bundle.example.test")),
  resolve6: sortStrings(await dns.promises.resolve6("bundle.example.test")),
  resolveCallbackMx: sortObjects(await resolveWithCallback("bundle.example.test", "MX")),
  resolveTxt: sortObjects(await dns.promises.resolveTxt("bundle.example.test")),
  resolveSrv: sortObjects(await dns.promises.resolveSrv("_svc._tcp.example.test")),
  resolveCname: sortStrings(await dns.promises.resolveCname("alias.example.test")),
  resolvePtr: sortStrings(await dns.promises.resolvePtr("ptr.example.test")),
  resolveNs: sortStrings(await dns.promises.resolveNs("zone.example.test")),
  resolveSoa: await dns.promises.resolveSoa("zone.example.test"),
  resolveNaptr: sortObjects(await dns.promises.resolveNaptr("naptr.example.test")),
  resolveCaa: sortObjects(await dns.promises.resolveCaa("caa.example.test")),
  resolveAny,
};

console.log(JSON.stringify(results));
"#,
    );

    let host = run_host_probe_with_env(
        &cwd,
        &entrypoint,
        &[("AGENTOS_TEST_DNS_SERVER", dns_server_addr.as_str())],
    );
    let guest = run_guest_probe_with_config(
        "dns",
        &cwd,
        &entrypoint,
        HashMap::from([(String::from("network.dns.servers"), dns_server_addr.clone())]),
        wire_permissions_allow_all(),
        &["dns"],
    );

    // `dns.resolveSrv`/`resolveCaa`/`resolve(..., "MX")` record shapes differ
    // by host Node *version*, not by guest correctness: Node >= 24 attaches a
    // `type` discriminator (e.g. `"type":"SRV"`) to these records, while Node
    // <= 22 omits it. The guest shim always emits the modern shape with `type`.
    // CI runs on Node 22 and the dev machines run Node 24, so a raw
    // `guest == host` comparison flaps on the runner's Node version even though
    // every actual resolved value is identical. We therefore:
    //   1. strip the version-dependent `type` discriminator from both sides
    //      before comparing the resolved data, so the guest-vs-host conformance
    //      check still covers all version-stable fields (addresses, ttls,
    //      priorities, exchanges, ports, soa fields, ...); and
    //   2. assert the guest's own `type` discriminators directly, so we keep
    //      testing the guest's record-shape correctness independent of the
    //      host Node version.
    fn strip_record_type(value: &Value) -> Value {
        match value {
            Value::Object(map) => Value::Object(
                map.iter()
                    .filter(|(key, _)| key.as_str() != "type")
                    .map(|(key, val)| (key.clone(), strip_record_type(val)))
                    .collect(),
            ),
            Value::Array(items) => Value::Array(items.iter().map(strip_record_type).collect()),
            other => other.clone(),
        }
    }

    assert_eq!(
        strip_record_type(&guest),
        strip_record_type(&host),
        "guest V8 result diverged from host Node for dns (ignoring host-Node-version-dependent record `type` field)\nhost: {}\nguest: {}",
        serde_json::to_string_pretty(&host).expect("pretty host JSON"),
        serde_json::to_string_pretty(&guest).expect("pretty guest JSON")
    );

    // Guest record-shape correctness, asserted directly so it does not depend on
    // the host Node version. The modern Node shape attaches these discriminators.
    let type_at = |key: &str, index: usize| -> Option<String> {
        guest[key]
            .get(index)
            .and_then(|record| record.get("type"))
            .and_then(Value::as_str)
            .map(str::to_owned)
    };
    assert_eq!(type_at("resolveSrv", 0).as_deref(), Some("SRV"));
    assert_eq!(type_at("resolveCallbackMx", 0).as_deref(), Some("MX"));
    assert!(
        guest["resolveCaa"].as_array().is_some_and(|records| records
            .iter()
            .all(|record| record.get("type").and_then(Value::as_str) == Some("CAA"))),
        "guest resolveCaa records missing CAA type discriminator: {}",
        guest["resolveCaa"]
    );
    assert!(
        guest["resolveAny"].as_array().is_some_and(|records| records
            .iter()
            .all(|record| record.get("type").and_then(Value::as_str).is_some())),
        "guest resolveAny records missing type discriminator: {}",
        guest["resolveAny"]
    );

    let unsupported_cwd = temp_dir("builtin-conformance-dns-unsupported");
    let unsupported_entrypoint = unsupported_cwd.join("entry.mjs");
    write_fixture(
        &unsupported_entrypoint,
        r#"
import dns from "node:dns";

try {
  await dns.promises.resolve("bundle.example.test", "TLSA");
  console.log(JSON.stringify({ unexpected: true }));
} catch (error) {
  console.log(JSON.stringify({
    code: error?.code ?? null,
    message: String(error?.message ?? ""),
  }));
}
"#,
    );
    let unsupported = run_guest_probe_with_config(
        "dns-unsupported",
        &unsupported_cwd,
        &unsupported_entrypoint,
        HashMap::new(),
        wire_permissions_allow_all(),
        &["dns"],
    );

    assert_eq!(unsupported["code"], "ERR_NOT_IMPLEMENTED");
    assert!(
        unsupported["message"]
            .as_str()
            .is_some_and(|message| message.contains("TLSA")),
        "unexpected unsupported rrtype payload: {unsupported}"
    );

    let resolver_cwd = temp_dir("builtin-conformance-dns-resolver");
    let resolver_entrypoint = resolver_cwd.join("entry.mjs");
    write_fixture(
        &resolver_entrypoint,
        r#"
import dns, { Resolver as CallbackResolver } from "node:dns";
import dnsPromises, { Resolver as PromisesResolver } from "node:dns/promises";

const callbackResolver = new CallbackResolver();
callbackResolver.setServers(["203.0.113.53:5353"]);
const callbackResult = await new Promise((resolve, reject) => {
  callbackResolver.resolve4("bundle.example.test", (error, records) => {
    if (error) {
      reject(error);
      return;
    }
    resolve(records);
  });
});

const promisesResolver = new PromisesResolver();
promisesResolver.setServers(["203.0.113.54", "203.0.113.55:5353"]);

console.log(JSON.stringify({
  callbackResolverIsConstructor: typeof CallbackResolver === "function",
  promisesResolverIsConstructor: typeof PromisesResolver === "function",
  sameCallbackResolverExport: dns.Resolver === CallbackResolver,
  samePromisesResolverExport: dnsPromises.Resolver === PromisesResolver,
  callbackServers: callbackResolver.getServers(),
  promisesServers: promisesResolver.getServers(),
  callbackResult: [...callbackResult].sort(),
  promisesResult: [...(await promisesResolver.resolve4("bundle.example.test"))].sort(),
}));
"#,
    );
    let resolver_probe = run_guest_probe_with_config(
        "dns-resolver",
        &resolver_cwd,
        &resolver_entrypoint,
        HashMap::from([(String::from("network.dns.servers"), dns_server_addr.clone())]),
        wire_permissions_allow_all(),
        &["dns"],
    );

    assert_eq!(
        resolver_probe["callbackResolverIsConstructor"],
        Value::Bool(true)
    );
    assert_eq!(
        resolver_probe["promisesResolverIsConstructor"],
        Value::Bool(true)
    );
    assert_eq!(
        resolver_probe["sameCallbackResolverExport"],
        Value::Bool(true)
    );
    assert_eq!(
        resolver_probe["samePromisesResolverExport"],
        Value::Bool(true)
    );
    assert_eq!(
        resolver_probe["callbackServers"],
        json!([String::from("203.0.113.53:5353")])
    );
    assert_eq!(
        resolver_probe["promisesServers"],
        json!([
            String::from("203.0.113.54"),
            String::from("203.0.113.55:5353"),
        ])
    );
    assert_eq!(
        resolver_probe["callbackResult"],
        json!([String::from("203.0.113.10"), String::from("203.0.113.11"),])
    );
    assert_eq!(
        resolver_probe["promisesResult"],
        json!([String::from("203.0.113.10"), String::from("203.0.113.11"),])
    );
}

fn fs_conformance_matches_host_node() {
    assert_conformance(
        "fs",
        r#"
import fs from "node:fs";

fs.mkdirSync("scratchdir");
fs.mkdirSync("scratchdir/nested");
fs.writeFileSync("scratchdir/nested/alpha.txt", Buffer.from("alpha-sync", "utf8"));
await new Promise((resolve, reject) => {
  fs.writeFile("scratchdir/beta.txt", Buffer.from("beta-async", "utf8"), (error) => {
    if (error) {
      reject(error);
      return;
    }
    resolve();
  });
});

let missingStatCode = null;
try {
  fs.statSync("scratchdir/missing.txt");
} catch (error) {
  missingStatCode = error?.code ?? null;
}

let missingReadCode = null;
try {
  await new Promise((resolve, reject) => {
    fs.readFile("scratchdir/missing.txt", "utf8", (error, value) => {
      if (error) {
        reject(error);
        return;
      }
      resolve(value);
    });
  });
} catch (error) {
  missingReadCode = error?.code ?? null;
}

const asyncRead = await new Promise((resolve, reject) => {
  fs.readFile("scratchdir/beta.txt", "utf8", (error, value) => {
    if (error) {
      reject(error);
      return;
    }
    resolve(value);
  });
});

console.log(JSON.stringify({
  syncRead: fs.readFileSync("scratchdir/nested/alpha.txt", "utf8"),
  asyncRead,
  entries: fs.readdirSync("scratchdir").sort(),
  statSize: fs.statSync("scratchdir/nested/alpha.txt").size,
  existsAlpha: fs.existsSync("scratchdir/nested/alpha.txt"),
  existsBeta: fs.existsSync("scratchdir/beta.txt"),
  existsOverlongPath: fs.existsSync("x".repeat(5000)),
  missingStatCode,
  missingReadCode,
}));
"#,
    );
}

fn write_file_sync_numeric_fd_matches_host_node_impl() {
    assert_conformance(
        "fs-write-file-numeric-fd",
        r#"
import fs from "node:fs";

const path = "numeric-fd.txt";
const fd = fs.openSync(path, "wx");
fs.writeFileSync(fd, "first");
fs.closeSync(fd);
fs.appendFileSync(path, "-second");
console.log(JSON.stringify({ content: fs.readFileSync(path, "utf8") }));
"#,
    );
}

#[test]
fn write_file_sync_numeric_fd_matches_host_node() {
    run_isolated_builtin_conformance_test("fs-write-file-numeric-fd");
}

fn rm_does_not_follow_symlinks_matches_host_node_impl() {
    assert_conformance(
        "fs-rm-symlinks",
        r#"
import fs from "node:fs";

const setup = (root) => {
  fs.mkdirSync(`${root}/keep`, { recursive: true });
  fs.writeFileSync(`${root}/keep/data.txt`, "keep");
  fs.mkdirSync(`${root}/doomed/sub`, { recursive: true });
  fs.writeFileSync(`${root}/doomed/sub/file.txt`, "doomed");
  fs.symlinkSync("../keep", `${root}/doomed/link-dir`);
};
const describe = async (root, remove) => {
  setup(root);
  let error = null;
  try {
    await remove(`${root}/doomed`);
  } catch (caught) {
    error = caught.code ?? caught.message;
  }
  return {
    error,
    doomedExists: fs.existsSync(`${root}/doomed`),
    keep: fs.readdirSync(`${root}/keep`),
  };
};

const sync = await describe("sync", (path) => fs.rmSync(path, { recursive: true }));
const callback = await describe(
  "callback",
  (path) =>
    new Promise((resolve, reject) => {
      fs.rm(path, { recursive: true }, (error) => (error ? reject(error) : resolve()));
    })
);
const promises = await describe("promises", (path) => fs.promises.rm(path, { recursive: true }));

fs.symlinkSync("missing-target", "dangling");
let danglingError = null;
try {
  fs.rmSync("dangling");
} catch (caught) {
  danglingError = caught.code ?? caught.message;
}
let danglingRemoved = true;
try {
  fs.lstatSync("dangling");
  danglingRemoved = false;
} catch {}

console.log(JSON.stringify({ sync, callback, promises, danglingError, danglingRemoved }));
"#,
    );
}

#[test]
fn rm_does_not_follow_symlinks_matches_host_node() {
    run_isolated_builtin_conformance_test("fs-rm-symlinks");
}

// The probe chdirs into a VM-local directory, so relative paths cannot be
// rescued by the host-mapped harness cwd.
fn process_chdir_moves_relative_paths_and_children_matches_host_node_impl() {
    assert_conformance(
        "process-chdir-cwd",
        r#"
import fs from "node:fs";
import path from "node:path";
import { execFileSync } from "node:child_process";

const base = fs.realpathSync(fs.mkdtempSync("/tmp/agentos-cwd-"));
const rel = (target) => path.relative(base, target);
const childCwd = (options) =>
  rel(execFileSync("node", ["-e", "process.stdout.write(process.cwd())"], options).toString());
fs.mkdirSync(path.join(base, "p/sub"), { recursive: true });
process.chdir(path.join(base, "p"));

fs.mkdirSync("made");
fs.symlinkSync("made", "link");
const relativeOps = {
  entries: fs.readdirSync(process.cwd()).sort(),
  linkIsSymlink: fs.lstatSync("link").isSymbolicLink(),
  strayAtRoot: ["made", "link"].filter((name) => fs.existsSync(`/${name}`)),
};
fs.unlinkSync("link");
fs.rmdirSync("made");

process.chdir("sub");
const cwd = { process: rel(process.cwd()), proc: rel(fs.readlinkSync("/proc/self/cwd")) };
const child = { inherited: childCwd(), relativeOption: childCwd({ cwd: ".." }) };

const chdirError = (target) => {
  try {
    process.chdir(target);
    return null;
  } catch (error) {
    return {
      code: error.code,
      syscall: error.syscall,
      path: rel(error.path),
      dest: error.dest,
      message: error.message.replaceAll(base, "<base>"),
    };
  }
};
fs.writeFileSync("file.txt", "x");
const errors = {
  missing: chdirError("missing"),
  notDirectory: chdirError("file.txt"),
  cwdAfterFailures: rel(process.cwd()),
};

process.chdir("/");
fs.rmSync(base, { recursive: true });
console.log(JSON.stringify({ relativeOps, cwd, child, errors }));
"#,
    );
}

#[test]
fn process_chdir_moves_relative_paths_and_children_matches_host_node() {
    run_isolated_builtin_conformance_test("process-chdir-cwd");
}

// Each walker case runs under the host-mapped cwd and under a /tmp directory,
// which the guest serves from the kernel VFS. The host probe shares the cwd,
// so each run uses fresh directories and removes them.
const FS_TWO_ROOTS_PRELUDE: &str = r#"
import fs from "node:fs";
import path from "node:path";
const roots = [
  fs.mkdtempSync(path.join(process.cwd(), "case-")),
  fs.mkdtempSync("/tmp/agentos-conformance-"),
];
const attempt = async (fn) => {
  try {
    await fn();
    return null;
  } catch (error) {
    return error.code ?? error.message;
  }
};
const runInRoots = async (run) => {
  const results = [];
  for (const root of roots) {
    results.push(await run(root));
    fs.rmSync(root, { recursive: true });
  }
  console.log(JSON.stringify(results));
};
"#;

fn cp_copies_symlinks_as_links_matches_host_node_impl() {
    let script = format!(
        "{FS_TWO_ROOTS_PRELUDE}{}",
        r#"
await runInRoots(async (root) => {
  const shown = (target) => (target.startsWith(root) ? `<root>${target.slice(root.length)}` : target);
  const entry = (path) => {
    const stats = fs.lstatSync(path);
    if (stats.isSymbolicLink()) return { link: shown(fs.readlinkSync(path)) };
    if (stats.isDirectory()) return { dir: fs.readdirSync(path).sort() };
    return { file: fs.readFileSync(path, "utf8") };
  };
  const tree = (dir) =>
    Object.fromEntries(fs.readdirSync(dir).sort().map((name) => [name, entry(`${dir}/${name}`)]));

  fs.mkdirSync(`${root}/keep`);
  fs.writeFileSync(`${root}/keep/data.txt`, "keep");
  fs.mkdirSync(`${root}/src`);
  fs.writeFileSync(`${root}/src/file.txt`, "file");
  fs.symlinkSync("../keep", `${root}/src/rel-dir`);
  fs.symlinkSync(`${root}/keep/data.txt`, `${root}/src/abs-file`);
  fs.symlinkSync("missing", `${root}/src/dangling`);
  fs.mkdirSync(`${root}/existing`);

  const result = {
    intoExisting: await attempt(() => fs.cpSync(`${root}/src`, `${root}/existing`, { recursive: true })),
    existing: tree(`${root}/existing`),
    verbatim: await attempt(() =>
      fs.promises.cp(`${root}/src`, `${root}/verbatim`, { recursive: true, verbatimSymlinks: true })
    ),
    verbatimTree: tree(`${root}/verbatim`),
    topLevelLinkWithoutRecursive: await attempt(() => fs.cpSync(`${root}/src/rel-dir`, `${root}/top-link`)),
    topLevelLink: await attempt(() =>
      fs.cpSync(`${root}/src/rel-dir`, `${root}/top-link`, { recursive: true })
    ),
    topLevel: entry(`${root}/top-link`),
    topLevelDangling: await attempt(() => fs.cpSync(`${root}/src/dangling`, `${root}/top-dangling`)),
  };
  result.dereference = await attempt(() =>
    fs.cpSync(`${root}/src/rel-dir`, `${root}/deref`, { recursive: true, dereference: true })
  );
  result.dereferenced = entry(`${root}/deref`);
  result.keep = fs.readdirSync(`${root}/keep`);
  return result;
});
"#
    );
    assert_conformance("fs-cp-symlinks", &script);
}

#[test]
fn cp_copies_symlinks_as_links_matches_host_node() {
    run_isolated_builtin_conformance_test("fs-cp-symlinks");
}

fn readdir_dirent_types_match_host_node_impl() {
    let script = format!(
        "{FS_TWO_ROOTS_PRELUDE}{}",
        r#"
await runInRoots(async (root) => {
  fs.mkdirSync(`${root}/dir/sub`, { recursive: true });
  fs.writeFileSync(`${root}/dir/file.txt`, "file");
  fs.symlinkSync("sub", `${root}/dir/link-dir`);
  fs.symlinkSync("file.txt", `${root}/dir/link-file`);
  fs.symlinkSync("missing", `${root}/dir/dangling`);
  const types = (entries) =>
    entries
      .map((dirent) => [
        dirent.name,
        dirent.isFile(),
        dirent.isDirectory(),
        dirent.isSymbolicLink(),
        dirent.isFIFO(),
        dirent.isSocket(),
        dirent.isCharacterDevice(),
        dirent.isBlockDevice(),
      ])
      .sort();
  return {
    sync: types(fs.readdirSync(`${root}/dir`, { withFileTypes: true })),
    promises: types(await fs.promises.readdir(`${root}/dir`, { withFileTypes: true })),
  };
});
"#
    );
    assert_conformance("fs-dirent-types", &script);
}

#[test]
fn readdir_dirent_types_match_host_node() {
    run_isolated_builtin_conformance_test("fs-dirent-types");
}

fn rm_directory_without_recursive_matches_host_node_impl() {
    assert_conformance(
        "fs-rm-directory-without-recursive",
        r#"
import fs from "node:fs";

fs.mkdirSync("full");
fs.writeFileSync("full/file.txt", "full");
fs.mkdirSync("empty");
const describe = async (remove) => {
  try {
    await remove();
    return null;
  } catch (error) {
    const { code, name, errno, syscall, message } = error;
    return { code, name, errno, syscall, message };
  }
};

console.log(
  JSON.stringify({
    full: await describe(() => fs.rmSync("full")),
    empty: await describe(() => fs.rmSync("empty")),
    force: await describe(() => fs.rmSync("empty", { force: true })),
    callback: await describe(
      () => new Promise((resolve, reject) => fs.rm("empty", (error) => (error ? reject(error) : resolve())))
    ),
    promises: await describe(() => fs.promises.rm("empty")),
    remaining: ["full", "full/file.txt", "empty"].filter((path) => fs.existsSync(path)),
  })
);
"#,
    );
}

#[test]
fn rm_directory_without_recursive_matches_host_node() {
    run_isolated_builtin_conformance_test("fs-rm-directory-without-recursive");
}

fn console_conformance_matches_host_node() {
    assert_conformance(
        "console",
        r#"
import * as consoleModule from "node:console";
import { Writable } from "node:stream";
const consoleInstance = new consoleModule.Console(process.stdout, process.stderr);
const task = consoleModule.createTask("demo-task");
const detachedChunks = [];
const detachedErrors = [];
const createSink = (target) =>
  new Writable({
    write(chunk, _encoding, callback) {
      target.push(String(chunk));
      callback();
    },
  });
const detachedConsole = new consoleModule.Console(
  createSink(detachedChunks),
  createSink(detachedErrors),
);
const detachedLog = detachedConsole.log;
const detachedError = detachedConsole.error;
detachedLog("detached-log");
detachedError("detached-error");

console.log(JSON.stringify({
  types: {
    Console: typeof consoleModule.Console,
    context: typeof consoleModule.context,
    createTask: typeof consoleModule.createTask,
    log: typeof consoleModule.log,
    table: typeof consoleModule.table,
  },
  taskRunType: typeof task.run,
  consoleMethods: {
    assert: typeof consoleInstance.assert,
    clear: typeof consoleInstance.clear,
    count: typeof consoleInstance.count,
    countReset: typeof consoleInstance.countReset,
    debug: typeof consoleInstance.debug,
    dir: typeof consoleInstance.dir,
    dirxml: typeof consoleInstance.dirxml,
    error: typeof consoleInstance.error,
    group: typeof consoleInstance.group,
    groupCollapsed: typeof consoleInstance.groupCollapsed,
    groupEnd: typeof consoleInstance.groupEnd,
    info: typeof consoleInstance.info,
    log: typeof consoleInstance.log,
    profile: typeof consoleInstance.profile,
    profileEnd: typeof consoleInstance.profileEnd,
    table: typeof consoleInstance.table,
    time: typeof consoleInstance.time,
    timeEnd: typeof consoleInstance.timeEnd,
    timeLog: typeof consoleInstance.timeLog,
    timeStamp: typeof consoleInstance.timeStamp,
    trace: typeof consoleInstance.trace,
    warn: typeof consoleInstance.warn,
  },
  detachedOutput: detachedChunks.join(""),
  detachedErrorOutput: detachedErrors.join(""),
}));
"#,
    );
}

fn child_process_conformance_matches_host_node() {
    assert_conformance(
        "child-process",
        r#"
import childProcess from "node:child_process";
const syncStdout = childProcess.spawnSync(
  "node",
  ["-e", "process.stdout.write(process.argv[1] ?? '')", "alpha-sync"],
);
const syncError = childProcess.spawnSync(
  "node",
  ["-e", "process.stderr.write('sync-error'); throw new Error('sync-fail');"],
);

const asyncEchoResult = await new Promise((resolve, reject) => {
  const child = childProcess.spawn(
    "node",
    [
      "-e",
      "let data=''; let settled = false; const fallback = setTimeout(() => { if (!settled) process.exit(19); }, 50); process.stdin.on('data', (chunk) => { data += chunk; }); process.stdin.on('end', () => { settled = true; clearTimeout(fallback); process.exit(data === 'beta-async' ? 0 : 17); });",
    ],
  );
  const timer = setTimeout(() => {
    reject(new Error("spawn(node async echo) did not close within 2s"));
  }, 2000);
  const stdout = [];
  const stderr = [];
  child.stdout.on("data", (chunk) => {
    stdout.push(Buffer.from(chunk));
  });
  child.stderr.on("data", (chunk) => {
    stderr.push(Buffer.from(chunk));
  });
  child.stdin.write(Buffer.from("beta-async"));
  child.stdin.end();
  child.on("error", reject);
  child.on("close", (code, signal) => {
    clearTimeout(timer);
    resolve({
      code,
      signal,
      stdoutBase64: Buffer.concat(stdout).toString("base64"),
      stderrBase64: Buffer.concat(stderr).toString("base64"),
    });
  });
});

const asyncErrorResult = await new Promise((resolve, reject) => {
  const child = childProcess.spawn(
    "node",
    [
      "-e",
      "setTimeout(() => { process.stderr.write('async-error'); throw new Error('async-fail'); }, 10);",
    ],
  );
  const timer = setTimeout(() => {
    reject(new Error("spawn(node async failure) did not close within 2s"));
  }, 2000);
  const stdout = [];
  const stderr = [];
  child.stdout.on("data", (chunk) => {
    stdout.push(Buffer.from(chunk));
  });
  child.stderr.on("data", (chunk) => {
    stderr.push(Buffer.from(chunk));
  });
  child.on("error", reject);
  child.on("close", (code, signal) => {
    clearTimeout(timer);
    resolve({
      code,
      signal,
      stdoutBase64: Buffer.concat(stdout).toString("base64"),
      stderrBase64: Buffer.concat(stderr).toString("base64"),
    });
  });
});

console.log(JSON.stringify({
  syncStdoutStatus: syncStdout.status,
  syncStdoutTrimmed: Buffer.from(syncStdout.stdout ?? []).toString("utf8").trim(),
  syncStdoutStderrBase64: Buffer.from(syncStdout.stderr ?? []).toString("base64"),
  syncErrorStatus: syncError.status,
  syncErrorStdoutBase64: Buffer.from(syncError.stdout ?? []).toString("base64"),
  syncErrorHasMarker: Buffer.from(syncError.stderr ?? []).toString("utf8").includes("sync-error"),
  syncErrorHasNonZeroStatus: (syncError.status ?? 0) !== 0,
  asyncEchoCode: asyncEchoResult.code,
  asyncEchoSignal: asyncEchoResult.signal,
  asyncEchoStdoutBase64: asyncEchoResult.stdoutBase64,
  asyncEchoStderrBase64: asyncEchoResult.stderrBase64,
  asyncErrorCode: asyncErrorResult.code,
  asyncErrorSignal: asyncErrorResult.signal,
  asyncErrorStdoutBase64: asyncErrorResult.stdoutBase64,
  asyncErrorHasNonZeroStatus: (asyncErrorResult.code ?? 0) !== 0,
}));
"#,
    );
}

fn child_process_fork_supports_basic_ipc_impl() {
    let cwd = temp_dir("builtin-child-process-fork-ipc");
    let entrypoint = cwd.join("entry.mjs");
    let worker = cwd.join("worker.mjs");
    write_fixture(
        &worker,
        r#"
process.send({
  type: "ready",
  connected: process.connected,
  argv: process.argv.slice(-1),
});

process.on("message", (message) => {
  process.send({
    type: "pong",
    value: message.value + 1,
    connected: process.connected,
  });
  process.exit(0);
});
"#,
    );
    write_fixture(
        &entrypoint,
        r#"
import childProcess from "node:child_process";
import { Buffer } from "node:buffer";

const child = childProcess.fork("./worker.mjs", ["worker-arg"]);
const stdout = [];
const messages = [];
const errors = [];
let sendReturn = null;

child.stdout.on("data", (chunk) => stdout.push(Buffer.from(chunk)));
child.on("error", (error) => errors.push({
  name: error?.name ?? null,
  message: error?.message ?? null,
  code: error?.code ?? null,
}));
child.on("message", (message) => {
  messages.push(message);
  if (message.type === "ready") {
    sendReturn = child.send({ type: "ping", value: 41 });
  }
});

const exit = await new Promise((resolve) => {
  child.on("close", (code, signal) => resolve({ code, signal }));
});

console.log(JSON.stringify({
  connectedAfterFork: child.connected,
  sendReturn,
  messages,
  errors,
  stdoutBase64: Buffer.concat(stdout).toString("base64"),
  exit,
}));
"#,
    );

    let guest = run_guest_probe_with_config(
        "child-process-fork-ipc",
        &cwd,
        &entrypoint,
        HashMap::new(),
        wire_permissions_allow_all(),
        &["child_process"],
    );

    let pretty_guest = serde_json::to_string_pretty(&guest).expect("pretty guest JSON");
    assert_eq!(
        guest["sendReturn"],
        Value::Bool(true),
        "guest result:\n{pretty_guest}"
    );
    assert_eq!(
        guest["errors"],
        Value::Array(Vec::new()),
        "guest result:\n{pretty_guest}"
    );
    assert_eq!(guest["stdoutBase64"], Value::String(String::new()));
    assert_eq!(guest["exit"]["code"], Value::from(0));
    assert_eq!(guest["exit"]["signal"], Value::Null);
    assert_eq!(
        guest["messages"][0]["type"],
        Value::String(String::from("ready"))
    );
    assert_eq!(guest["messages"][0]["connected"], Value::Bool(true));
    assert_eq!(
        guest["messages"][0]["argv"][0],
        Value::String(String::from("worker-arg"))
    );
    assert_eq!(
        guest["messages"][1]["type"],
        Value::String(String::from("pong"))
    );
    assert_eq!(guest["messages"][1]["value"], Value::from(42));
    assert_eq!(guest["messages"][1]["connected"], Value::Bool(true));
}

#[test]
fn child_process_fork_supports_basic_ipc() {
    run_isolated_builtin_conformance_test("child-process-fork-ipc");
}

fn child_process_exec_preserves_spawn_error_codes_impl() {
    assert_node_available();

    let cwd = temp_dir("builtin-child-process-exec-spawn-error-code");
    let entrypoint = cwd.join("entry.mjs");
    write_fixture(
        &entrypoint,
        r#"
import childProcess from "node:child_process";

const result = await new Promise((resolve) => {
  const callbacks = [];
  const closeEvents = [];
  const child = childProcess.exec(
    "/definitely/not/a/binary",
    (err, stdout, stderr) => {
      callbacks.push({
        code: err?.code ?? null,
        errno: typeof err?.errno === "number" ? err.errno : null,
        syscall: err?.syscall ?? null,
        path: err?.path ?? null,
        stdout,
        stderr,
      });
      setTimeout(() => resolve({ callbacks, closeEvents }), 0);
    },
  );
  child.on("close", (code, signal) => {
    closeEvents.push({
      code: code ?? null,
      signal: signal ?? null,
    });
  });
  child.on("error", () => {});
});

console.log(JSON.stringify(result));
"#,
    );

    let guest = run_guest_probe_with_config(
        "child-process-exec-spawn-error-code",
        &cwd,
        &entrypoint,
        HashMap::new(),
        wire_permissions_allow_all(),
        &["child_process"],
    );

    assert_eq!(
        guest["callbacks"][0]["code"],
        Value::String(String::from("ENOENT")),
        "guest exec() callback should preserve the original spawn error code",
    );
    assert_eq!(
        guest["callbacks"].as_array().map(Vec::len),
        Some(1),
        "guest exec() callback should not be re-fired after a spawn error"
    );
    assert_eq!(
        guest["callbacks"][0]["stdout"],
        Value::String(String::new())
    );
    assert_eq!(
        guest["callbacks"][0]["stderr"],
        Value::String(String::new())
    );
}

#[test]
fn child_process_exec_preserves_spawn_error_codes() {
    run_isolated_builtin_conformance_test("child-process-exec-spawn-error-code");
}

fn child_process_rejects_native_elf_binaries_before_wasm_compile_impl() {
    let cwd = temp_dir("builtin-child-process-native-elf-reject");
    let entrypoint = cwd.join("entry.mjs");
    write_fixture(
        &entrypoint,
        r#"
import childProcess from "node:child_process";
import fs from "node:fs";

const fakeRgPath = "/tmp/fake-rg";
fs.writeFileSync(
  fakeRgPath,
  Buffer.from([0x7f, 0x45, 0x4c, 0x46, 0x02, 0x01, 0x01, 0x00]),
);
fs.chmodSync(fakeRgPath, 0o755);

const syncResult = childProcess.spawnSync(fakeRgPath, ["--version"]);

const asyncResult = await new Promise((resolve) => {
  const child = childProcess.spawn(fakeRgPath, ["--version"]);
  child.once("error", (error) => {
    resolve({
      code: error?.code ?? null,
      message: error?.message ?? null,
    });
  });
});

console.log(JSON.stringify({
  sync: {
    status: syncResult.status,
    errorCode: syncResult.error?.code ?? null,
    errorMessage: syncResult.error?.message ?? null,
    stderr: Buffer.isBuffer(syncResult.stderr)
      ? syncResult.stderr.toString("utf8")
      : String(syncResult.stderr ?? ""),
  },
  async: asyncResult,
}));
"#,
    );

    let guest = run_guest_probe_with_config(
        "child-process-native-elf-reject",
        &cwd,
        &entrypoint,
        HashMap::new(),
        wire_permissions_allow_all(),
        &["child_process", "fs"],
    );

    assert_eq!(guest["sync"]["status"], Value::Number(1.into()));
    assert_eq!(
        guest["sync"]["errorCode"],
        Value::String(String::from("ERR_NATIVE_BINARY_NOT_SUPPORTED"))
    );
    let sync_stderr = guest["sync"]["stderr"]
        .as_str()
        .expect("sync stderr string");
    assert!(
        sync_stderr.contains("ERR_NATIVE_BINARY_NOT_SUPPORTED"),
        "sync stderr should expose the explicit native-binary rejection: {sync_stderr}"
    );
    assert!(
        sync_stderr.contains("ELF"),
        "sync stderr should name the detected ELF format: {sync_stderr}"
    );
    assert!(
        !sync_stderr.contains("CompileError"),
        "sync stderr must not fall back to the WASM compile error: {sync_stderr}"
    );
    assert_eq!(
        guest["async"]["code"],
        Value::String(String::from("ERR_NATIVE_BINARY_NOT_SUPPORTED"))
    );
    let async_message = guest["async"]["message"]
        .as_str()
        .expect("async error message string");
    assert!(
        async_message.contains("ERR_NATIVE_BINARY_NOT_SUPPORTED"),
        "async spawn error should preserve the explicit native-binary code: {async_message}"
    );
    assert!(
        async_message.contains("ELF"),
        "async spawn error should name the detected ELF format: {async_message}"
    );
    assert!(
        !async_message.contains("CompileError"),
        "async spawn error must not fall back to the WASM compile error: {async_message}"
    );
}

#[test]
fn child_process_rejects_native_elf_binaries_before_wasm_compile() {
    run_isolated_builtin_conformance_test("child-process-native-elf-reject");
}

fn child_process_kill_numeric_signals_match_host_node_impl() {
    assert_node_available();

    let cwd = temp_dir("builtin-child-process-kill-numeric-signal");
    let entrypoint = cwd.join("entry.mjs");
    write_fixture(
        &entrypoint,
        r#"
import childProcess from "node:child_process";

async function captureKill(signal) {
  const child = childProcess.spawn("node", ["-e", "setInterval(() => {}, 1000)"]);
  const killResult = child.kill(signal);
  const signalCodeAfterKill = child.signalCode ?? null;
  return await new Promise((resolve, reject) => {
    const timer = setTimeout(() => {
      reject(new Error(`spawn(node interval) kill(${String(signal)}) did not exit within 2s`));
    }, 2000);
    child.on("error", reject);
    child.on("exit", (code, exitSignal) => {
      clearTimeout(timer);
      resolve({
        killResult,
        signalCodeAfterKill,
        code: code ?? null,
        signal: exitSignal ?? null,
        signalCodeAfterExit: child.signalCode ?? null,
        killed: child.killed,
      });
    });
  });
}

console.log(JSON.stringify({
  numeric: await captureKill(11),
  alias: await captureKill("SIGIOT"),
}));
"#,
    );

    let host = run_host_probe(&cwd, &entrypoint);
    let guest = run_guest_probe_with_config(
        "child-process-kill-numeric-signal",
        &cwd,
        &entrypoint,
        HashMap::new(),
        wire_permissions_allow_all(),
        &["child_process"],
    );

    assert_eq!(
        guest,
        host,
        "guest child_process.kill signal mapping diverged from host Node\nhost: {}\nguest: {}",
        serde_json::to_string_pretty(&host).expect("pretty host JSON"),
        serde_json::to_string_pretty(&guest).expect("pretty guest JSON")
    );
    assert_eq!(
        guest["numeric"]["signalCodeAfterExit"],
        Value::String(String::from("SIGSEGV"))
    );
}

#[test]
fn child_process_kill_numeric_signals_match_host_node() {
    run_isolated_builtin_conformance_test("child-process-kill-numeric-signal");
}

fn child_process_abort_reports_sigabrt_impl() {
    assert_node_available();

    let cwd = temp_dir("builtin-child-process-abort-signal");
    let entrypoint = cwd.join("entry.mjs");
    // Use an inline `node -e` child rather than spawning a child *file*. The
    // previous version wrote the child script to a hardcoded host `/tmp` path
    // and spawned it with `cwd: "/tmp"`; that path is written into the guest
    // VFS by the parent, but the spawned guest child resolves the module
    // against the runner's filesystem, so it fails with `Cannot find module`
    // (exiting with code 1) before ever calling `process.abort()`. Whether the
    // file happened to be visible depended on the CI runner's `/tmp` layout,
    // which made this case flaky. The inline form mirrors the sibling
    // `child-process-kill-numeric-signal` case and exercises the exact same
    // guest behavior — `process.abort()` mapping to a SIGABRT-shaped exit —
    // without any cross-runtime filesystem dependency.
    write_fixture(
        &entrypoint,
        r#"
import childProcess from "node:child_process";

const child = childProcess.spawn("node", ["-e", "process.abort();"]);
const result = await new Promise((resolve, reject) => {
  const timer = setTimeout(() => {
    reject(new Error("spawn(node abort child) did not exit within 2s"));
  }, 2000);
  child.on("error", reject);
  child.on("exit", (code, signal) => {
    clearTimeout(timer);
    resolve({
      code: code ?? null,
      signal: signal ?? null,
      signalCodeAfterExit: child.signalCode ?? null,
      killed: child.killed,
    });
  });
});

console.log(JSON.stringify(result));
"#,
    );

    let host = run_host_probe(&cwd, &entrypoint);
    let guest = run_guest_probe_with_config(
        "child-process-abort-signal",
        &cwd,
        &entrypoint,
        HashMap::new(),
        wire_permissions_allow_all(),
        &["child_process"],
    );

    assert_eq!(guest["code"], host["code"]);
    assert_eq!(guest["signal"], host["signal"]);
    assert_eq!(guest["signalCodeAfterExit"], host["signalCodeAfterExit"]);
    assert_eq!(guest["killed"], host["killed"]);
    assert_eq!(guest["signal"], Value::String(String::from("SIGABRT")));
    assert_eq!(
        guest["signalCodeAfterExit"],
        Value::String(String::from("SIGABRT"))
    );
}

#[test]
fn child_process_abort_reports_sigabrt() {
    run_isolated_builtin_conformance_test("child-process-abort-signal");
}

fn path_conformance_matches_host_node() {
    assert_conformance(
        "path",
        r#"
import * as pathNs from "node:path";

const path = pathNs.default ?? pathNs;

console.log(JSON.stringify({
  join: path.join("/virtual", "project", "file.txt"),
  resolve: path.resolve("/virtual/root", "alpha", "..", "beta", "file.txt"),
  dirname: path.dirname("/virtual/root/beta/file.txt"),
  basename: path.basename("/virtual/root/beta/file.txt"),
  extname: path.extname("/virtual/root/beta/file.txt"),
  isAbsoluteFile: path.isAbsolute("/virtual/root/beta/file.txt"),
  isAbsoluteRelative: path.isAbsolute("virtual/root/beta/file.txt"),
  relative: path.relative("/virtual/root/alpha", "/virtual/root/beta/file.txt"),
  normalize: path.normalize("/virtual//root/alpha/../beta//file.txt"),
}));
"#,
    );
}

fn crypto_conformance_matches_host_node() {
    assert_conformance(
        "crypto",
        r#"
import crypto from "node:crypto";

const random = crypto.randomBytes(16);
const uuid = crypto.randomUUID();
const ciphers = crypto.getCiphers();
const curves = crypto.getCurves();

console.log(JSON.stringify({
  hashesIncludeSha256: crypto.getHashes().includes("sha256"),
  ciphersIncludeAes256Cbc: ciphers.includes("aes-256-cbc"),
  ciphersIncludeAes256Gcm: ciphers.includes("aes-256-gcm"),
  ciphersSorted: ciphers.join(",") === [...ciphers].sort().join(","),
  curvesIncludePrime256v1: curves.includes("prime256v1"),
  curvesIncludeSecp384r1: curves.includes("secp384r1"),
  curvesSorted: curves.join(",") === [...curves].sort().join(","),
  sha256: crypto.createHash("sha256").update("agentos").digest("hex"),
  hmacSha256: crypto.createHmac("sha256", "shared-secret").update("agentos").digest("hex"),
  randomBytesLength: random.length,
  randomBytesHexLength: random.toString("hex").length,
  randomBytesAllZero: Array.from(random).every((value) => value === 0),
  randomUuidValid: /^[0-9a-f]{8}-[0-9a-f]{4}-4[0-9a-f]{3}-[89ab][0-9a-f]{3}-[0-9a-f]{12}$/i.test(uuid),
}));
"#,
    );
}

fn crypto_extended_surface_matches_host_node() {
    assert_conformance(
        "crypto-extended",
        r#"
import crypto from "node:crypto";

const cipherKey = Buffer.alloc(32, 7);
const cipherIv = Buffer.alloc(16, 9);
const cipherPlaintext = Buffer.from("agentos-crypto-surface", "utf8");
const cipher = crypto.createCipheriv("aes-256-cbc", cipherKey, cipherIv);
const encrypted = Buffer.concat([cipher.update(cipherPlaintext), cipher.final()]);
const decipher = crypto.createDecipheriv("aes-256-cbc", cipherKey, cipherIv);
const decrypted = Buffer.concat([decipher.update(encrypted), decipher.final()]).toString("utf8");

const pbkdf2Hex = await new Promise((resolve, reject) => {
  crypto.pbkdf2("password", "salt", 10, 32, "sha256", (error, result) => {
    if (error) {
      reject(error);
      return;
    }
    resolve(result.toString("hex"));
  });
});

const scryptHex = await new Promise((resolve, reject) => {
  crypto.scrypt("password", "salt", 32, { N: 1024, r: 8, p: 1 }, (error, result) => {
    if (error) {
      reject(error);
      return;
    }
    resolve(result.toString("hex"));
  });
});

const { publicKey, privateKey } = crypto.generateKeyPairSync("rsa", { modulusLength: 1024 });
const privatePem = privateKey.export({ format: "pem", type: "pkcs8" });
const publicPem = publicKey.export({ format: "pem", type: "spki" });
const importedPrivateKey = crypto.createPrivateKey(privatePem);
const importedPublicKey = crypto.createPublicKey(publicPem);

const signer = crypto.createSign("sha256");
signer.update("agentos-signature");
const signature = signer.sign(importedPrivateKey);

const verifier = crypto.createVerify("sha256");
verifier.update("agentos-signature");
const signatureVerified = verifier.verify(importedPublicKey, signature);

const oneShotSignature = crypto.sign("sha256", Buffer.from("agentos-signature"), importedPrivateKey);
const oneShotVerified = crypto.verify(
  "sha256",
  Buffer.from("agentos-signature"),
  importedPublicKey,
  oneShotSignature,
);

const rsaCiphertext = crypto.publicEncrypt(
  { key: importedPublicKey, padding: crypto.constants.RSA_PKCS1_PADDING },
  Buffer.from("agentos-rsa", "utf8"),
);
const rsaPlaintext = crypto.privateDecrypt(
  { key: importedPrivateKey, padding: crypto.constants.RSA_PKCS1_PADDING },
  rsaCiphertext,
).toString("utf8");

const secretKey = crypto.createSecretKey(Buffer.from("abcd", "utf8"));
const generatedHmacKey = crypto.generateKeySync("hmac", { length: 256 });
const generatedAesKey = crypto.generateKeySync("aes", { length: 128 });
const generatedPrime = crypto.generatePrimeSync(64, { bigint: true });

const groupAlice = crypto.getDiffieHellman("modp14");
const groupBob = crypto.getDiffieHellman("modp14");
groupAlice.generateKeys();
groupBob.generateKeys();
const groupSecretA = groupAlice.computeSecret(groupBob.getPublicKey());
const groupSecretB = groupBob.computeSecret(groupAlice.getPublicKey());

const ecdhAlice = crypto.createECDH("prime256v1");
const ecdhBob = crypto.createECDH("prime256v1");
ecdhAlice.generateKeys();
ecdhBob.generateKeys();
const ecdhSecretA = ecdhAlice.computeSecret(ecdhBob.getPublicKey());
const ecdhSecretB = ecdhBob.computeSecret(ecdhAlice.getPublicKey());

const x25519Alice = crypto.generateKeyPairSync("x25519");
const x25519Bob = crypto.generateKeyPairSync("x25519");
const x25519SecretA = crypto.diffieHellman({
  privateKey: x25519Alice.privateKey,
  publicKey: x25519Bob.publicKey,
});
const x25519SecretB = crypto.diffieHellman({
  privateKey: x25519Bob.privateKey,
  publicKey: x25519Alice.publicKey,
});

const generatedAsyncPair = await new Promise((resolve, reject) => {
  crypto.generateKeyPair("rsa", { modulusLength: 1024 }, (error, publicKeyValue, privateKeyValue) => {
    if (error) {
      reject(error);
      return;
    }
    resolve({
      publicType: publicKeyValue.type,
      privateType: privateKeyValue.type,
      publicAsymmetricKeyType: publicKeyValue.asymmetricKeyType,
      privateAsymmetricKeyType: privateKeyValue.asymmetricKeyType,
    });
  });
});

console.log(JSON.stringify({
  cipherHex: encrypted.toString("hex"),
  decipheredText: decrypted,
  pbkdf2SyncHex: crypto.pbkdf2Sync("password", "salt", 10, 32, "sha256").toString("hex"),
  pbkdf2Hex,
  scryptSyncHex: crypto.scryptSync("password", "salt", 32, { N: 1024, r: 8, p: 1 }).toString("hex"),
  scryptHex,
  importedPrivateType: importedPrivateKey.type,
  importedPrivateAsymmetricKeyType: importedPrivateKey.asymmetricKeyType,
  importedPublicType: importedPublicKey.type,
  importedPublicAsymmetricKeyType: importedPublicKey.asymmetricKeyType,
  importedPrivateEquals: importedPrivateKey.equals(crypto.createPrivateKey(privatePem)),
  importedPublicEquals: importedPublicKey.equals(crypto.createPublicKey(publicPem)),
  signatureLength: signature.length,
  signatureVerified,
  oneShotSignatureLength: oneShotSignature.length,
  oneShotVerified,
  rsaCiphertextLength: rsaCiphertext.length,
  rsaPlaintext,
  secretKeyType: secretKey.type,
  secretKeyExportHex: secretKey.export().toString("hex"),
  generatedHmacKeyType: generatedHmacKey.type,
  generatedHmacKeyLength: generatedHmacKey.export().length,
  generatedAesKeyType: generatedAesKey.type,
  generatedAesKeyLength: generatedAesKey.export().length,
  generatedPrimeType: typeof generatedPrime,
  generatedPrimePositive: generatedPrime > 0n,
  groupVerifyError: groupAlice.verifyError,
  groupSecretMatches: groupSecretA.equals(groupSecretB),
  groupPrimeLength: groupAlice.getPrime().length,
  ecdhSecretMatches: ecdhSecretA.equals(ecdhSecretB),
  ecdhPublicKeyLength: ecdhAlice.getPublicKey().length,
  x25519SecretMatches: x25519SecretA.equals(x25519SecretB),
  x25519SecretLength: x25519SecretA.length,
  generatedAsyncPair,
}));
"#,
    );
}

fn crypto_basic_fixture_matches_shared_expected_impl() {
    assert_node_available();

    let fixture_json = include_str!("../../../tests/fixtures/crypto-basic-conformance.json");
    let fixture: Value = serde_json::from_str(fixture_json).expect("parse crypto fixture");
    let script = r#"
import crypto from "node:crypto";

const fixture = __CRYPTO_FIXTURE__;
const pbkdf2Hex = await new Promise((resolve, reject) => {
  crypto.pbkdf2(fixture.password, fixture.salt, fixture.iterations, fixture.keyLength, "sha256", (error, value) => {
    if (error) reject(error);
    else resolve(value.toString("hex"));
  });
});
const pbkdf2Sha384Hex = await new Promise((resolve, reject) => {
  crypto.pbkdf2(fixture.password, fixture.salt, fixture.iterations, fixture.keyLength, "sha384", (error, value) => {
    if (error) reject(error);
    else resolve(value.toString("hex"));
  });
});
const scryptHex = await new Promise((resolve, reject) => {
  crypto.scrypt(fixture.password, fixture.salt, fixture.keyLength, fixture.scrypt, (error, value) => {
    if (error) reject(error);
    else resolve(value.toString("hex"));
  });
});
const generatedPrime = crypto.generatePrimeSync(fixture.expected.primes.bits, { bigint: true });
const generatedSafePrime = crypto.generatePrimeSync(fixture.expected.primes.safeBits, {
  bigint: true,
  safe: true,
});
const generatedPrimeBuffer = crypto.generatePrimeSync(fixture.expected.primes.bufferBits);
const bytesFromHex = (hex) => Uint8Array.from(hex.match(/../g).map((byte) => parseInt(byte, 16)));
const bytesToHex = (bytes) => Array.from(bytes, (byte) => byte.toString(16).padStart(2, "0")).join("");
const aesCbcKey = Buffer.from(fixture.aesCbc.keyHex, "hex");
const aesCbcIv = Buffer.from(fixture.aesCbc.ivHex, "hex");
const cipher = crypto.createCipheriv(fixture.aesCbc.algorithm, aesCbcKey, aesCbcIv);
const aes256CbcCiphertext = cipher.update(fixture.aesCbc.plaintext, "utf8", "hex") + cipher.final("hex");
const decipher = crypto.createDecipheriv(fixture.aesCbc.algorithm, aesCbcKey, aesCbcIv);
const aes256CbcPlaintext = decipher.update(aes256CbcCiphertext, "hex", "utf8") + decipher.final("utf8");
const aesGcmKey = Buffer.from(fixture.aesGcm.keyHex, "hex");
const aesGcmIv = Buffer.from(fixture.aesGcm.ivHex, "hex");
const aesGcmAad = Buffer.from(fixture.aesGcm.aad);
const gcmCipher = crypto.createCipheriv(fixture.aesGcm.algorithm, aesGcmKey, aesGcmIv, {
  authTagLength: fixture.aesGcm.authTagLength,
});
gcmCipher.setAAD(aesGcmAad);
const aes256GcmCiphertext = gcmCipher.update(fixture.aesGcm.plaintext, "utf8", "hex") + gcmCipher.final("hex");
const aes256GcmAuthTag = gcmCipher.getAuthTag().toString("hex");
const gcmDecipher = crypto.createDecipheriv(fixture.aesGcm.algorithm, aesGcmKey, aesGcmIv, {
  authTagLength: fixture.aesGcm.authTagLength,
});
gcmDecipher.setAAD(aesGcmAad);
gcmDecipher.setAuthTag(bytesFromHex(aes256GcmAuthTag));
const aes256GcmPlaintext = gcmDecipher.update(aes256GcmCiphertext, "hex", "utf8") + gcmDecipher.final("utf8");
const subtleKey = await crypto.subtle.importKey("raw", aesGcmKey, { name: "AES-GCM" }, false, ["encrypt", "decrypt"]);
const subtleAlgorithm = {
  name: "AES-GCM",
  iv: aesGcmIv,
  additionalData: aesGcmAad,
  tagLength: fixture.aesGcm.authTagLength * 8,
};
const aes256GcmWebCryptoBytes = new Uint8Array(await crypto.subtle.encrypt(
  subtleAlgorithm,
  subtleKey,
  Buffer.from(fixture.aesGcm.plaintext),
));
const aes256GcmWebCryptoCiphertext = bytesToHex(aes256GcmWebCryptoBytes);
const aes256GcmWebCryptoPlaintext = Buffer.from(await crypto.subtle.decrypt(
  subtleAlgorithm,
  subtleKey,
  aes256GcmWebCryptoBytes,
)).toString("utf8");
const importedPrivateKey = crypto.createPrivateKey(fixture.rsa.privatePem);
const importedPublicKey = crypto.createPublicKey(fixture.rsa.publicPem);
const rsaSignature = crypto.createSign("sha256").update(fixture.rsa.message).sign(importedPrivateKey);
const rsaVerified = crypto.createVerify("sha256").update(fixture.rsa.message).verify(importedPublicKey, rsaSignature);
const rsaExpectedVerified = crypto.createVerify("sha256")
  .update(fixture.rsa.message)
  .verify(importedPublicKey, Buffer.from(fixture.rsa.sha256SignatureHex, "hex"));
const rsaOneShotSignature = crypto.sign("sha256", Buffer.from(fixture.rsa.message), importedPrivateKey);
const rsaOneShotVerified = crypto.verify(
  "sha256",
  Buffer.from(fixture.rsa.message),
  importedPublicKey,
  rsaOneShotSignature,
);
const dhAlice = crypto.createDiffieHellman(
  Buffer.from(fixture.dh.primeHex, "hex"),
  Buffer.from(fixture.dh.generatorHex, "hex"),
);
const dhBob = crypto.createDiffieHellman(
  Buffer.from(fixture.dh.primeHex, "hex"),
  Buffer.from(fixture.dh.generatorHex, "hex"),
);
dhAlice.setPrivateKey(Buffer.from(fixture.dh.privateAHex, "hex"));
dhAlice.setPublicKey(Buffer.from(fixture.dh.publicAHex, "hex"));
dhBob.setPrivateKey(Buffer.from(fixture.dh.privateBHex, "hex"));
dhBob.setPublicKey(Buffer.from(fixture.dh.publicBHex, "hex"));
const dhSecretA = dhAlice.computeSecret(Buffer.from(fixture.dh.publicBHex, "hex"));
const dhSecretB = dhBob.computeSecret(Buffer.from(fixture.dh.publicAHex, "hex"));
const ecdhAlice = crypto.createECDH(fixture.ecdh.curve);
const ecdhBob = crypto.createECDH(fixture.ecdh.curve);
ecdhAlice.setPrivateKey(Buffer.from(fixture.ecdh.privateAHex, "hex"));
ecdhBob.setPrivateKey(Buffer.from(fixture.ecdh.privateBHex, "hex"));
const ecdhSecretA = ecdhAlice.computeSecret(Buffer.from(fixture.ecdh.publicBHex, "hex"));
const ecdhSecretB = ecdhBob.computeSecret(Buffer.from(fixture.ecdh.publicAHex, "hex"));

console.log(JSON.stringify({
  hashes: crypto.getHashes(),
  ciphers: crypto.getCiphers(),
  curves: crypto.getCurves(),
  md5: crypto.createHash("md5").update(fixture.message).digest("hex"),
  sha224: crypto.createHash("sha224").update(fixture.message).digest("hex"),
  sha256: crypto.createHash("sha256").update(fixture.message).digest("hex"),
  sha384: crypto.createHash("sha384").update(fixture.message).digest("hex"),
  hmacSha256: crypto.createHmac("sha256", fixture.hmacKey).update(fixture.message).digest("hex"),
  hmacSha384: crypto.createHmac("sha384", fixture.hmacKey).update(fixture.message).digest("hex"),
  pbkdf2SyncHex: crypto.pbkdf2Sync(fixture.password, fixture.salt, fixture.iterations, fixture.keyLength, "sha256").toString("hex"),
  pbkdf2Sha384Hex,
  pbkdf2Hex,
  scryptSyncHex: crypto.scryptSync(fixture.password, fixture.salt, fixture.keyLength, fixture.scrypt).toString("hex"),
  scryptHex,
  generatedPrimeType: typeof generatedPrime,
  generatedPrimeBits: generatedPrime.toString(2).length,
  generatedPrimePositive: generatedPrime > 0n,
  generatedSafePrimeBits: generatedSafePrime.toString(2).length,
  generatedSafePrimePositive: generatedSafePrime > 0n,
  generatedPrimeBufferBits: fixture.expected.primes.bufferBits,
  generatedPrimeBufferByteLength: generatedPrimeBuffer.byteLength,
  aes256CbcCiphertext,
  aes256CbcPlaintext,
  aes256GcmCiphertext,
  aes256GcmAuthTag,
  aes256GcmPlaintext,
  aes256GcmWebCryptoCiphertext,
  aes256GcmWebCryptoPlaintext,
  rsaSignatureHex: rsaSignature.toString("hex"),
  rsaVerified,
  rsaExpectedVerified,
  rsaOneShotSignatureHex: rsaOneShotSignature.toString("hex"),
  rsaOneShotVerified,
  dhPublicAHex: dhAlice.getPublicKey("hex"),
  dhPublicBHex: dhBob.getPublicKey("hex"),
  dhSecretAHex: dhSecretA.toString("hex"),
  dhSecretBHex: dhSecretB.toString("hex"),
  ecdhPublicAHex: ecdhAlice.getPublicKey("hex"),
  ecdhPublicBHex: ecdhBob.getPublicKey("hex"),
  ecdhSecretAHex: ecdhSecretA.toString("hex"),
  ecdhSecretBHex: ecdhSecretB.toString("hex"),
}));
"#
    .replace("__CRYPTO_FIXTURE__", fixture_json);

    let cwd = temp_dir("builtin-conformance-crypto-basic-fixture");
    let entrypoint = cwd.join("entry.mjs");
    write_fixture(&entrypoint, &script);
    let guest = run_guest_probe("crypto-basic-fixture", &cwd, &entrypoint);
    let expected = &fixture["expected"];
    let dh_expected_secret = format!(
        "{:0>width$}",
        fixture["dh"]["secretHex"]
            .as_str()
            .expect("dh secret fixture must be a string"),
        width = fixture["dh"]["primeHex"]
            .as_str()
            .expect("dh prime fixture must be a string")
            .len()
    );

    assert_eq!(guest["hashes"], expected["hashes"]);
    assert_eq!(guest["ciphers"], expected["ciphers"]);
    assert_eq!(guest["curves"], expected["curves"]);
    assert_eq!(guest["md5"], expected["md5"]);
    assert_eq!(guest["sha224"], expected["sha224"]);
    assert_eq!(guest["sha256"], expected["sha256"]);
    assert_eq!(guest["sha384"], expected["sha384"]);
    assert_eq!(guest["hmacSha256"], expected["hmacSha256"]);
    assert_eq!(guest["hmacSha384"], expected["hmacSha384"]);
    assert_eq!(guest["pbkdf2SyncHex"], expected["pbkdf2Sha256"]);
    assert_eq!(guest["pbkdf2Hex"], expected["pbkdf2Sha256"]);
    assert_eq!(guest["pbkdf2Sha384Hex"], expected["pbkdf2Sha384"]);
    assert_eq!(guest["scryptSyncHex"], expected["scrypt"]);
    assert_eq!(guest["scryptHex"], expected["scrypt"]);
    assert_eq!(guest["generatedPrimeType"], json!("bigint"));
    assert_eq!(guest["generatedPrimeBits"], expected["primes"]["bits"]);
    assert_eq!(guest["generatedPrimePositive"], json!(true));
    assert_eq!(
        guest["generatedSafePrimeBits"],
        expected["primes"]["safeBits"]
    );
    assert_eq!(guest["generatedSafePrimePositive"], json!(true));
    assert_eq!(
        guest["generatedPrimeBufferBits"],
        expected["primes"]["bufferBits"]
    );
    assert_eq!(
        guest["generatedPrimeBufferByteLength"],
        expected["primes"]["bufferByteLength"]
    );
    assert_eq!(
        guest["aes256CbcCiphertext"],
        expected["aes256CbcCiphertext"]
    );
    assert_eq!(guest["aes256CbcPlaintext"], fixture["aesCbc"]["plaintext"]);
    assert_eq!(
        guest["aes256GcmCiphertext"],
        expected["aes256GcmCiphertext"]
    );
    assert_eq!(guest["aes256GcmAuthTag"], expected["aes256GcmAuthTag"]);
    assert_eq!(guest["aes256GcmPlaintext"], fixture["aesGcm"]["plaintext"]);
    assert_eq!(
        guest["aes256GcmWebCryptoCiphertext"],
        expected["aes256GcmWebCryptoCiphertext"]
    );
    assert_eq!(
        guest["aes256GcmWebCryptoPlaintext"],
        fixture["aesGcm"]["plaintext"]
    );
    assert_eq!(
        guest["rsaSignatureHex"],
        fixture["rsa"]["sha256SignatureHex"]
    );
    assert_eq!(guest["rsaVerified"], json!(true));
    assert_eq!(guest["rsaExpectedVerified"], json!(true));
    assert_eq!(
        guest["rsaOneShotSignatureHex"],
        fixture["rsa"]["sha256SignatureHex"]
    );
    assert_eq!(guest["rsaOneShotVerified"], json!(true));
    assert_eq!(guest["dhPublicAHex"], fixture["dh"]["publicAHex"]);
    assert_eq!(guest["dhPublicBHex"], fixture["dh"]["publicBHex"]);
    assert_eq!(guest["dhSecretAHex"], json!(dh_expected_secret));
    assert_eq!(guest["dhSecretBHex"], json!(dh_expected_secret));
    assert_eq!(guest["ecdhPublicAHex"], fixture["ecdh"]["publicAHex"]);
    assert_eq!(guest["ecdhPublicBHex"], fixture["ecdh"]["publicBHex"]);
    assert_eq!(guest["ecdhSecretAHex"], fixture["ecdh"]["secretHex"]);
    assert_eq!(guest["ecdhSecretBHex"], fixture["ecdh"]["secretHex"]);
}

#[test]
fn crypto_basic_fixture_matches_shared_expected_isolated() {
    run_isolated_builtin_conformance_test("crypto-basic-fixture");
}

#[test]
fn crypto_extended_surface_matches_host_node_isolated() {
    run_isolated_builtin_conformance_test("crypto-extended");
}

fn events_conformance_matches_host_node() {
    assert_conformance(
        "events",
        r#"
import { EventEmitter } from "node:events";
import { createRequire } from "node:module";

const require = createRequire(import.meta.url);
const events = require("events");
const nodeEvents = require("node:events");
const util = require("node:util");

const emitter = new EventEmitter();
class DerivedEmitter extends require("events") {}
const derived = new DerivedEmitter();
const constructed = new (require("events"))();
function LegacyEmitter() {
  events.call(this);
}
util.inherits(LegacyEmitter, events);
const legacy = new LegacyEmitter();
const seen = [];
const metaNew = [];
const metaRemove = [];
const constructedSeen = [];
const derivedSeen = [];
const legacySeen = [];
const warningEvents = [];

function persistent(value) {
  seen.push(`on:${value}`);
}

function onTick() {}
function onceTick() {}
function prependTick() {}
function prependOnceTick() {}
function removeFirst() {}
function removeSecond() {}
function removeThird() {}
function onceVisible() {}

emitter.on("newListener", (eventName, listener) => {
  if (eventName === "newListener") {
    return;
  }
  metaNew.push({
    eventName,
    listenerName: listener.name || "anon",
    tickCountBefore: emitter.listenerCount("tick"),
    tickListenersBefore: emitter.listeners("tick").map((fn) => fn.name || "anon"),
  });
});

const removalEmitter = new EventEmitter();
removalEmitter.on("removeListener", (eventName, listener) => {
  if (eventName === "removeListener") {
    return;
  }
  metaRemove.push({
    eventName,
    listenerName: listener.name || "anon",
    tickCountAfter: removalEmitter.listenerCount("tick"),
    tickListenersAfter: removalEmitter.listeners("tick").map((fn) => fn.name || "anon"),
    eventNamesAfter: removalEmitter.eventNames().sort(),
  });
});

emitter.on("tick", persistent);
emitter.once("tick", (value) => {
  seen.push(`once:${value}`);
});
emitter.on("tick", onTick);
emitter.once("tick", onceTick);
emitter.prependListener("tick", prependTick);
emitter.prependOnceListener("tick", prependOnceTick);
const listenerViewEmitter = new EventEmitter();
listenerViewEmitter.once("visible", onceVisible);
const visibleListeners = listenerViewEmitter.listeners("visible");
const visibleRawListeners = listenerViewEmitter.rawListeners("visible");
emitter.emit("tick", "alpha");
emitter.removeListener("tick", persistent);
emitter.emit("tick", "beta");

removalEmitter.on("tick", removeFirst);
removalEmitter.on("tick", removeSecond);
removalEmitter.on("pong", removeThird);
removalEmitter.removeListener("tick", removeSecond);
removalEmitter.removeAllListeners("tick");
removalEmitter.removeAllListeners();

constructed.on("ready", (value) => {
  constructedSeen.push(`constructed:${value}`);
});
const constructedEmitHandled = constructed.emit("ready", "gamma");

derived.on("tick", (value) => {
  derivedSeen.push(`derived:${value}`);
});
const derivedEmitHandled = derived.emit("tick", "delta");

legacy.on("tick", (value) => {
  legacySeen.push(`legacy:${value}`);
});
const legacyEmitHandled = legacy.emit("tick", "epsilon");

process.on("warning", (warning) => {
  warningEvents.push({
    name: warning.name,
    message: warning.message,
    type: warning.type,
    count: warning.count,
    emitterMatches: warning.emitter === emitter,
  });
});

for (let index = 0; index < 11; index += 1) {
  emitter.on("warning-check", () => {});
}
emitter.once("warning-check", () => {});
emitter.prependListener("warning-check", () => {});
emitter.prependOnceListener("warning-check", () => {});

const zeroMaxListenersEmitter = new EventEmitter();
zeroMaxListenersEmitter.setMaxListeners(0);
for (let index = 0; index < 12; index += 1) {
  zeroMaxListenersEmitter.on("disabled-warning-check", () => {});
}

const domTarget = new EventTarget();
events.setMaxListeners(23, domTarget);
const domMaxListeners = events.getMaxListeners(domTarget);
const domSeen = [];
const coercedType = { toString: () => "coerced" };
domTarget.addEventListener(coercedType, () => domSeen.push("coerced"));
domTarget.dispatchEvent(new Event("coerced"));
domTarget.addEventListener("propagation", (event) => {
  domSeen.push("stop");
  event.stopPropagation();
});
domTarget.addEventListener("propagation", () => domSeen.push("after-stop"));
domTarget.dispatchEvent(new Event("propagation"));
domTarget.addEventListener("immediate", (event) => {
  domSeen.push("immediate");
  event.stopImmediatePropagation();
});
domTarget.addEventListener("immediate", () => domSeen.push("after-immediate"));
domTarget.dispatchEvent(new Event("immediate"));
const onceReentrant = () => {
  domSeen.push("once");
  domTarget.dispatchEvent(new Event("reentrant"));
};
domTarget.addEventListener("reentrant", onceReentrant, { once: true });
domTarget.dispatchEvent(new Event("reentrant"));
const objectListener = {
  handleEvent() {
    domSeen.push(this === objectListener ? "object-this" : "object-wrong-this");
  },
};
domTarget.addEventListener("object", objectListener);
domTarget.dispatchEvent(new Event("object"));
const passiveEvent = new Event("passive", { cancelable: true });
domTarget.addEventListener("passive", (event) => event.preventDefault(), {
  passive: true,
});
const passiveDispatchResult = domTarget.dispatchEvent(passiveEvent);
const canceledEvent = new Event("cancel", { cancelable: true });
domTarget.addEventListener("cancel", (event) => event.preventDefault());
const canceledDispatchResult = domTarget.dispatchEvent(canceledEvent);
const signalController = new AbortController();
domTarget.addEventListener("signaled", () => domSeen.push("signaled"), {
  signal: signalController.signal,
});
signalController.abort();
domTarget.dispatchEvent(new Event("signaled"));
let eventSymbolErrorName = null;
try {
  new Event(Symbol("event"));
} catch (error) {
  eventSymbolErrorName = error?.name ?? null;
}
let listenerSymbolErrorName = null;
try {
  domTarget.addEventListener(Symbol("event"), () => {});
} catch (error) {
  listenerSymbolErrorName = error?.name ?? null;
}
let eventTypeReadonly = false;
const readonlyEvent = new Event("readonly");
try {
  readonlyEvent.type = "changed";
} catch (error) {
  eventTypeReadonly = error instanceof TypeError;
}

const defaultAbortController = new AbortController();
let defaultAbortEvents = 0;
defaultAbortController.signal.addEventListener("abort", () => {
  defaultAbortEvents += 1;
});
defaultAbortController.abort();
defaultAbortController.abort(new Error("ignored"));

const onabortController = new AbortController();
let onabortCount = 0;
onabortController.signal.onabort = () => {
  onabortCount += 1;
};
onabortController.signal.onabort = () => {
  onabortCount += 10;
};
onabortController.abort();

const primitiveAbortController = new AbortController();
primitiveAbortController.abort(17);
let primitiveThrowExact = false;
try {
  primitiveAbortController.signal.throwIfAborted();
} catch (error) {
  primitiveThrowExact = error === 17;
}

const timeoutSignal = AbortSignal.timeout(0);
await new Promise((resolve) => {
  const keepAlive = setTimeout(resolve, 50);
  timeoutSignal.addEventListener("abort", () => {
    clearTimeout(keepAlive);
    resolve();
  }, { once: true });
});

const anyFirst = new AbortController();
const anySecond = new AbortController();
const anySignal = AbortSignal.any([anyFirst.signal, anySecond.signal]);
anySecond.abort("second-reason");
let invalidAnyRejected = false;
try {
  AbortSignal.any([anyFirst.signal, {
    aborted: false,
    addEventListener() {},
    removeEventListener() {},
  }]);
} catch (error) {
  invalidAnyRejected = error instanceof TypeError;
}
const emptyAnySignal = AbortSignal.any([]);
let illegalAbortSignalCode = null;
try {
  new AbortSignal();
} catch (error) {
  illegalAbortSignalCode = error?.code ?? null;
}
let abortStateReadonly = false;
try {
  defaultAbortController.signal.aborted = false;
} catch (error) {
  abortStateReadonly = error instanceof TypeError;
}
let fractionalTimeoutCode = null;
try {
  AbortSignal.timeout(1.5);
} catch (error) {
  fractionalTimeoutCode = error?.code ?? null;
}

await new Promise((resolve) => setTimeout(resolve, 0));

console.log(JSON.stringify({
  abortStateReadonly,
  anyReason: anySignal.reason,
  defaultAbortEvents,
  defaultAbortReasonName: defaultAbortController.signal.reason.name,
  emptyAnyAborted: emptyAnySignal.aborted,
  eventSymbolErrorName,
  eventTypeReadonly,
  fractionalTimeoutCode,
  illegalAbortSignalCode,
  invalidAnyRejected,
  listenerSymbolErrorName,
  onabortCount,
  primitiveThrowExact,
  staticAbortReasonName: AbortSignal.abort().reason.name,
  timeoutReasonName: timeoutSignal.reason.name,
  bareEqualsNode: events === nodeEvents,
  cjsEqualsEventEmitter: events === EventEmitter,
  bareType: typeof events,
  nodeType: typeof nodeEvents,
  eventEmitterPropEqualsSelf: events.EventEmitter === events,
  nodeEventEmitterPropEqualsSelf: nodeEvents.EventEmitter === nodeEvents,
  canceledDefaultPrevented: canceledEvent.defaultPrevented,
  canceledDispatchResult,
  constructedInstanceWorks: constructed instanceof EventEmitter,
  constructedEmitHandled,
  constructedSeen,
  derivedInstanceWorks: derived instanceof EventEmitter,
  derivedEmitHandled,
  derivedSeen,
  domMaxListeners,
  legacyInstanceWorks: legacy instanceof EventEmitter,
  legacyEmitHandled,
  legacySeen,
  domSeen,
  passiveDefaultPrevented: passiveEvent.defaultPrevented,
  passiveDispatchResult,
  visibleListenersIsArray: Array.isArray(visibleListeners),
  visibleRawListenersIsArray: Array.isArray(visibleRawListeners),
  listenersUnwrapOnce: visibleListeners?.[0] === onceVisible,
  rawListenersKeepWrapper: visibleRawListeners?.[0] !== onceVisible,
  rawListenerTargetsOriginal: visibleRawListeners?.[0]?.listener === onceVisible,
  metaNew,
  metaRemove,
  warningEvents,
  seen,
  listenerCount: emitter.listenerCount("tick"),
}));
"#,
    );
}

fn stream_conformance_matches_host_node() {
    assert_conformance(
        "stream",
        r#"
import { createRequire } from "node:module";
import * as streamNs from "node:stream";

const stream = streamNs.default ?? streamNs;
const require = createRequire(import.meta.url);
const cjsStream = require("stream");

class Source extends stream.Readable {
  constructor() {
    super();
    this.sent = false;
  }

  _read() {
    if (this.sent) {
      return;
    }
    this.sent = true;
    this.push("alpha");
    this.push("beta");
    this.push(null);
  }
}

class Sink extends stream.Writable {
  constructor(chunks) {
    super();
    this.chunks = chunks;
  }

  _write(chunk, _encoding, callback) {
    this.chunks.push(Buffer.from(chunk).toString("utf8"));
    callback();
  }
}

class Upper extends stream.Transform {
  _transform(chunk, _encoding, callback) {
    callback(null, Buffer.from(chunk).toString("utf8").toUpperCase());
  }
}

class IterableSource extends stream.Readable {
  constructor(values) {
    super({ objectMode: true });
    this.values = [...values];
  }

  _read() {
    if (this.values.length === 0) {
      this.push(null);
      return;
    }
    this.push(this.values.shift());
  }
}

class RequiredIterableSource extends cjsStream.Readable {
  constructor(values) {
    super({ objectMode: true });
    this.values = [...values];
  }

  _read() {
    if (this.values.length === 0) {
      this.push(null);
      return;
    }
    this.push(this.values.shift());
  }
}

const chunks = [];
const source = new Source();
const sink = new Sink(chunks);
const upper = new Upper();

let pipelineError = null;
const pipelineResult = stream.pipeline(source, upper, sink, (error) => {
  pipelineError = error ? String(error.message || error) : null;
});
source._read();
await new Promise((resolve) => setTimeout(resolve, 0));

const iteratedValues = [];
for await (const chunk of new IterableSource(["gamma", "delta"])) {
  iteratedValues.push(
    Buffer.isBuffer(chunk) ? chunk.toString("utf8") : String(chunk),
  );
}

const requiredIteratedValues = [];
for await (const chunk of new RequiredIterableSource(["theta", "lambda"])) {
  requiredIteratedValues.push(
    Buffer.isBuffer(chunk) ? chunk.toString("utf8") : String(chunk),
  );
}

const selfCheckReadable = new IterableSource(["self-check"]);
const selfCheckIterator = selfCheckReadable[Symbol.asyncIterator]();

console.log(JSON.stringify({
  output: chunks.join("|"),
  pipelineReturnedSink: pipelineResult === sink,
  pipelineError,
  readableIsFunction: typeof stream.Readable === "function",
  writableIsFunction: typeof stream.Writable === "function",
  transformIsFunction: typeof stream.Transform === "function",
  prototypeHasAsyncIterator:
    typeof stream.Readable.prototype[Symbol.asyncIterator] === "function",
  requiredPrototypeHasAsyncIterator:
    typeof cjsStream.Readable.prototype[Symbol.asyncIterator] === "function",
  readableIteratorReturnsSelf:
    selfCheckIterator[Symbol.asyncIterator]() === selfCheckIterator,
  iteratedValues,
  requiredIteratedValues,
}));
"#,
    );
}

fn buffer_conformance_matches_host_node() {
    assert_conformance(
        "buffer",
        r#"
const text = Buffer.from("hello", "utf8");
const filled = Buffer.alloc(4, 0x61);
const combined = Buffer.concat([text, Buffer.from("-world", "utf8")]);

console.log(JSON.stringify({
  fromHex: text.toString("hex"),
  allocUtf8: filled.toString("utf8"),
  concatUtf8: combined.toString("utf8"),
  sliceUtf8: combined.slice(3, 8).toString("utf8"),
}));
"#,
    );
}

fn buffer_concat_truncation_matches_host_node_impl() {
    assert_conformance(
        "buffer-concat-truncation",
        r#"
function describeBuffer(value) {
  return {
    length: value.length,
    hex: value.toString("hex"),
  };
}

function describeError(fn) {
  try {
    fn();
    return { threw: false };
  } catch (error) {
    return {
      threw: true,
      name: error?.name ?? null,
    };
  }
}

const chunks = [Buffer.from("abc"), Buffer.from("def")];

console.log(JSON.stringify({
  smaller: describeBuffer(Buffer.concat(chunks, 4)),
  exact: describeBuffer(Buffer.concat(chunks, 6)),
  larger: describeBuffer(Buffer.concat(chunks, 8)),
  emptyNonZero: describeBuffer(Buffer.concat([], 3)),
  invalidEntry: describeError(() => Buffer.concat([Buffer.from("a"), "x"], 1)),
  invalidList: describeError(() => Buffer.concat("nope", 1)),
}));
"#,
    );
}

#[test]
fn buffer_concat_truncation_matches_host_node() {
    run_isolated_builtin_conformance_test("buffer-concat-truncation");
}

fn global_base64_conformance_matches_host_node() {
    assert_conformance(
        "global_base64",
        r#"
function describeError(callback) {
  try {
    callback();
    return { threw: false };
  } catch (error) {
    return {
      threw: true,
      name: error?.name ?? null,
      code: error?.code ?? null,
      message: error?.message ?? null,
    };
  }
}

console.log(JSON.stringify({
  atobText: atob("aGVsbG8="),
  atobUnpaddedSingleByte: atob("YQ"),
  atobUnpaddedTwoBytes: atob("YWI"),
  atobWhitespace: atob(" YQ== \n"),
  atobNumberCoercion: atob(1234),
  btoaText: btoa("hello"),
  btoaNumberCoercion: btoa(1234),
  invalidAtob: describeError(() => atob("%%%")),
  invalidShortAtob: describeError(() => atob("Y")),
  invalidPaddingAtob: describeError(() => atob("AA=A")),
  invalidPartialPaddingAtob: describeError(() => atob("YQ=")),
  invalidOnlyPaddingAtob: describeError(() => atob("==")),
  invalidUrlSafeDashAtob: describeError(() => atob("AA-A")),
  invalidUrlSafeUnderscoreAtob: describeError(() => atob("AA_A")),
  invalidBtoa: describeError(() => btoa("✓")),
}));
"#,
    );
}

fn guest_sqlite_cannot_reach_host_files_impl() {
    let host_dir = temp_dir("guest-sqlite-host-files");
    let secret_path = host_dir.join("secret.sqlite");
    {
        let secret = rusqlite::Connection::open(&secret_path).expect("create host database");
        secret
            .execute_batch(
                "CREATE TABLE secret(value TEXT); INSERT INTO secret VALUES ('host-only')",
            )
            .expect("seed host database");
    }
    let attach_target = host_dir.join("attach.sqlite");
    let concat_target = host_dir.join("concat.sqlite");
    let vacuum_target = host_dir.join("vacuum.sqlite");
    let concat_vacuum_target = host_dir.join("concat-vacuum.sqlite");
    let secret = secret_path.display().to_string();
    let attach = attach_target.display().to_string();
    let concat = concat_target.display().to_string();
    let vacuum = vacuum_target.display().to_string();
    let concat_vacuum = concat_vacuum_target.display().to_string();

    let script = format!(
        r#"
import {{ DatabaseSync }} from "node:sqlite";
const db = new DatabaseSync(":memory:");
db.exec("CREATE TABLE t(a); INSERT INTO t VALUES (1)");
const outcome = (run) => {{
  try {{
    return {{ value: run() ?? null }};
  }} catch (error) {{
    return /not authorized|authorization denied/.test(error.message) ? {{ denied: true }} : {{ error: error.message }};
  }}
}};
const file = new DatabaseSync("/tmp/in-vm.sqlite");
console.log(JSON.stringify({{
  readHostFile: outcome(() => {{
    db.exec("ATTACH DATABASE '{secret}' AS host");
    return db.prepare("SELECT value FROM host.secret").all();
  }}),
  readHostFileUri: outcome(() => db.exec("ATTACH DATABASE 'file:{secret}?mode=ro' AS uri")),
  createHostFile: outcome(() => db.exec("ATTACH DATABASE '{attach}' AS created; CREATE TABLE created.t(a)")),
  createHostFileWithExpression: outcome(() => db.exec("ATTACH DATABASE ('{concat}' || '') AS created2; CREATE TABLE created2.t(a)")),
  vacuumIntoHostFile: outcome(() => db.exec("VACUUM INTO '{vacuum}'")),
  vacuumIntoHostFileWithExpression: outcome(() => db.exec("VACUUM INTO ('{concat_vacuum}' || '')")),
  relativePath: outcome(() => db.exec("ATTACH DATABASE 'relative.sqlite' AS relative")),
  sharedCache: outcome(() => db.exec("ATTACH DATABASE 'file:shared?mode=memory&cache=shared' AS shared")),
  processWidePragma: outcome(() => db.exec("PRAGMA hard_heap_limit = 1")),
  writableSchema: outcome(() => {{
    db.exec("PRAGMA writable_schema = ON");
    return db.prepare("PRAGMA writable_schema").get().writable_schema;
  }}),
  plainVacuum: outcome(() => db.exec("VACUUM")),
  memoryAttach: outcome(() => {{
    db.exec("ATTACH DATABASE ':memory:' AS mem; CREATE TABLE mem.t(a); INSERT INTO mem.t VALUES (2)");
    return db.prepare("SELECT a FROM mem.t").get().a;
  }}),
  vmFile: outcome(() => {{
    file.exec("CREATE TABLE t(a); INSERT INTO t VALUES (3)");
    return file.prepare("SELECT a FROM t").get().a;
  }}),
}}));
"#
    );

    let result = run_guest_script("guest-sqlite-host-files", &script);
    let denied = serde_json::json!({ "denied": true });

    for case in [
        "readHostFile",
        "readHostFileUri",
        "createHostFile",
        "createHostFileWithExpression",
        "vacuumIntoHostFile",
        "vacuumIntoHostFileWithExpression",
        "relativePath",
        "sharedCache",
        "processWidePragma",
    ] {
        assert_eq!(result[case], denied, "{case}: {result:#}");
    }
    for created in [
        &attach_target,
        &concat_target,
        &vacuum_target,
        &concat_vacuum_target,
    ] {
        assert!(!created.exists(), "guest created host file {created:?}");
    }
    assert_eq!(
        result["writableSchema"]["value"],
        Value::from(0),
        "{result:#}"
    );
    assert_eq!(result["plainVacuum"]["denied"], Value::Null, "{result:#}");
    assert_eq!(
        result["memoryAttach"]["value"],
        Value::from(2),
        "{result:#}"
    );
    assert_eq!(result["vmFile"]["value"], Value::from(3), "{result:#}");
}

#[test]
fn guest_sqlite_cannot_reach_host_files() {
    run_isolated_builtin_conformance_test("guest-sqlite-host-files");
}

fn mkdtemp_sync_collision_safe_matches_host_node_impl() {
    let cwd = temp_dir("mkdtemp-sync-collision-safe");
    let entrypoint = cwd.join("entry.mjs");
    write_fixture(
        &entrypoint,
        r#"
import fs from "node:fs";
import os from "node:os";
import path from "node:path";

const root = fs.mkdtempSync(path.join(os.tmpdir(), "mkdtemp-conformance-"));
const prefix = path.join(root, "x-");
const sampleCount = 32;
const created = await Promise.all(
  Array.from({ length: sampleCount }, () => Promise.resolve().then(() => fs.mkdtempSync(prefix)))
);
const result = {
  createdCount: created.length,
  uniqueCount: new Set(created).size,
  basenameLengths: [...new Set(created.map((value) => path.basename(value).length))].sort(
    (left, right) => left - right
  ),
  prefixesOk: created.every((value) => value.startsWith(prefix)),
};
fs.rmSync(root, { recursive: true, force: true });
console.log(JSON.stringify(result));
"#,
    );

    let guest = run_guest_probe("mkdtemp-sync-collision-safe", &cwd, &entrypoint);

    assert_eq!(guest["createdCount"], Value::from(32));
    assert_eq!(guest["uniqueCount"], Value::from(32));
    assert_eq!(guest["basenameLengths"], json!([8]));
    assert_eq!(guest["prefixesOk"], Value::Bool(true));
}

#[test]
fn mkdtemp_sync_collision_safe_matches_host_node() {
    run_isolated_builtin_conformance_test("mkdtemp-sync-collision-safe");
}

fn url_conformance_matches_host_node() {
    assert_conformance(
        "url",
        r#"
import * as urlNs from "node:url";

const urlModule = urlNs.default ?? urlNs;
const URLSearchParamsCtor = urlNs.URLSearchParams ?? globalThis.URLSearchParams;
const url = new urlModule.URL("https://example.com/a/b?x=1&y=two#frag");
url.searchParams.append("z", "3");
const fileRelative = new urlModule.URL("file:.", "file:///tmp/base/entry.mjs");
const fileRelativeNoBase = new urlModule.URL("file:./child");
const dataUrl = new urlModule.URL("data:image/png;base64,aGVsbG8=");
const dataUrlWithQuery = new urlModule.URL("data:text/plain,hello?x=1#frag");
const mailto = new urlModule.URL("mailto:dev@example.com");
const plusDecoded = new URLSearchParamsCtor("?a=foo+bar");
const invalidPercentDecoded = new URLSearchParamsCtor("?a=%&b=%2&c=%GG&d=%E0%A4%A");
const sortable = new URLSearchParamsCtor([
  ["b", "1"],
  ["a", "first"],
  ["ä", "umlaut"],
  ["a", "second"],
  ["aa", "x"],
]);
sortable.sort();
const setSemantics = new URLSearchParamsCtor("a=1&b=2&a=3&a=4&c=5");
setSemantics.set("a", "z");

const parsed = urlModule.parse("https://example.com/a/b?x=1&y=two#frag", true);
const describeFileUrlError = (callback) => {
  try {
    callback();
    return null;
  } catch (error) {
    return { name: error.name, code: error.code ?? null };
  }
};
const encodedFileUrl = urlModule.pathToFileURL("/tmp/a #?%/ü/");
const decodedFilePath = urlModule.fileURLToPath(encodedFileUrl);
const localhostFilePath = urlModule.fileURLToPath("file://localhost/tmp/a%20b");
const windowsFilePath = urlModule.fileURLToPath(
  "file:///C:/Users/a%20b",
  { windows: true },
);
const windowsUncPath = urlModule.fileURLToPath(
  "file://server/share/a",
  { windows: true },
);
const windowsFileUrl = urlModule.pathToFileURL(
  "C:\\Users\\a #?b",
  { windows: true },
);

console.log(JSON.stringify({
  href: url.href,
  searchParams: Array.from(url.searchParams.entries()),
  plusDecoded: Array.from(plusDecoded.entries()),
  plusDecodedString: plusDecoded.toString(),
  invalidPercentDecoded: Array.from(invalidPercentDecoded.entries()),
  invalidPercentDecodedString: invalidPercentDecoded.toString(),
  sortedSearchParams: Array.from(sortable.entries()),
  setSearchParams: Array.from(setSemantics.entries()),
  setSearchParamsSize: setSemantics.size,
  fileRelativeHref: fileRelative.href,
  fileRelativeNoBaseHref: fileRelativeNoBase.href,
  encodedFileUrl: encodedFileUrl.href,
  decodedFilePath,
  localhostFilePath,
  windowsFilePath,
  windowsFileUrl: windowsFileUrl.href,
  windowsUncPath,
  encodedSlashError: describeFileUrlError(() =>
    urlModule.fileURLToPath("file:///tmp/a%2Fb")
  ),
  invalidFileUrlHostError: describeFileUrlError(() =>
    urlModule.fileURLToPath("file://example.com/tmp/a")
  ),
  invalidFileUrlTypeError: describeFileUrlError(() =>
    urlModule.fileURLToPath({})
  ),
  invalidUrlError: describeFileUrlError(() =>
    urlModule.fileURLToPath("not a url")
  ),
  invalidSchemeError: describeFileUrlError(() =>
    urlModule.fileURLToPath("https://example.com/a")
  ),
  malformedPercentError: describeFileUrlError(() =>
    urlModule.fileURLToPath("file:///tmp/%")
  ),
  dataUrlHref: dataUrl.href,
  dataUrlProtocol: dataUrl.protocol,
  dataUrlPathname: dataUrl.pathname,
  dataUrlOrigin: dataUrl.origin,
  dataUrlToString: dataUrl.toString(),
  dataUrlWithQueryHref: dataUrlWithQuery.href,
  dataUrlWithQueryPathname: dataUrlWithQuery.pathname,
  dataUrlWithQuerySearch: dataUrlWithQuery.search,
  dataUrlWithQueryHash: dataUrlWithQuery.hash,
  mailtoHref: mailto.href,
  mailtoProtocol: mailto.protocol,
  mailtoPathname: mailto.pathname,
  formatted: urlModule.format(parsed),
  parsedPathname: parsed.pathname,
  parsedQuery: parsed.query,
}));
"#,
    );
}

fn stdlib_polyfill_conformance_matches_host_node() {
    assert_conformance(
        "stdlib-polyfills",
        r#"
import { createRequire } from "node:module";
import assertDefault, * as assertModule from "node:assert";
import strictAssertDefault from "node:assert/strict";
import pathDefault, * as pathModule from "node:path";
import pathPosixDefault from "node:path/posix";
import pathWin32Default from "node:path/win32";
import stringDecoderDefault, { StringDecoder } from "node:string_decoder";
import urlDefault, {
  URL as ImportedURL,
  URLSearchParams as ImportedURLSearchParams,
} from "node:url";

const require = createRequire(import.meta.url);
const assert = require("node:assert");
const constants = require("node:constants");
const path = require("node:path");
const punycode = require("node:punycode");
const querystring = require("node:querystring");
const stringDecoder = require("node:string_decoder");
const util = require("node:util");
const utilTypes = require("node:util/types");
const zlib = require("node:zlib");

assert.strictEqual(assertDefault, assert);
assert.strictEqual(assertModule.default, assert);
assert.strictEqual(strictAssertDefault, require("node:assert/strict"));
assert.strictEqual(pathDefault, path);
assert.strictEqual(pathModule.default, path);
assert.strictEqual(pathPosixDefault, require("node:path/posix"));
assert.strictEqual(pathWin32Default, require("node:path/win32"));
assert.strictEqual(stringDecoderDefault, stringDecoder);
assert.strictEqual(StringDecoder, stringDecoder.StringDecoder);
assert.strictEqual(urlDefault, require("node:url"));
assert.strictEqual(ImportedURL, require("node:url").URL);
assert.strictEqual(
  ImportedURLSearchParams,
  require("node:url").URLSearchParams,
);

assert.deepStrictEqual(path.normalize?.("/alpha/../beta"), "/beta");
assert.notStrictEqual(1, 2);
assert.strictEqual(typeof assert.fail, "function");

let throwsCode = null;
assert.throws(
  () => {
    const error = new TypeError("boom");
    error.code = "ERR_BOOM";
    throw error;
  },
  (error) => {
    throwsCode = error?.code ?? null;
    return true;
  },
);

let rejectsCode = null;
await assert.rejects(
  Promise.reject(Object.assign(new Error("reject"), { code: "ERR_REJECT" })),
  (error) => {
    rejectsCode = error?.code ?? null;
    return true;
  },
);

const decoder = new stringDecoder.StringDecoder("utf8");
const textBytes = Buffer.from("Grüße", "utf8");
const decoded =
  decoder.write(textBytes.subarray(0, 4)) +
  decoder.end(textBytes.subarray(4));

const formatted = util.format("value:%s count:%d json:%j", "alpha", 7, { ok: true });
const promisified = await util.promisify((value, callback) => callback(null, value.toUpperCase()))("beta");
const encodedLength = new util.TextEncoder().encode("Grüße").length;
const decodedText = new util.TextDecoder().decode(textBytes);
const malformedSurrogateBytes = Array.from(new TextEncoder().encode("\ud800A"));
const encodeIntoDestination = new Uint8Array(4);
const encodeIntoResult = new TextEncoder().encodeInto(
  "A\u{1f600}",
  encodeIntoDestination,
);
const streamingDecoder = new TextDecoder();
const streamingDecoded =
  streamingDecoder.decode(new Uint8Array([0xe2, 0x82]), { stream: true }) +
  streamingDecoder.decode(new Uint8Array([0xac]));
const maximalSubpartDecoded = new TextDecoder().decode(
  new Uint8Array([0xe1, 0x80, 0x41]),
);
const utf16BeDecoded = new TextDecoder("utf-16be").decode(
  new Uint8Array([0xfe, 0xff, 0x00, 0x41]),
);
const utf16LeStreamDecoder = new TextDecoder("utf-16le");
const utf16LeStreamDecoded =
  utf16LeStreamDecoder.decode(new Uint8Array([0x3d, 0xd8, 0x00]), {
    stream: true,
  }) +
  utf16LeStreamDecoder.decode(new Uint8Array([0xde]));
let fatalDecodeCode = null;
try {
  new TextDecoder("utf-8", { fatal: true }).decode(
    new Uint8Array([0xc0]),
  );
} catch (error) {
  fatalDecodeCode = error?.code ?? null;
}
let unsupportedEncodingCode = null;
try {
  new TextDecoder("not-an-encoding");
} catch (error) {
  unsupportedEncodingCode = error?.code ?? null;
}
let symbolEncodingErrorName = null;
try {
  new TextDecoder(Symbol("utf-8"));
} catch (error) {
  symbolEncodingErrorName = error?.name ?? null;
}
const streamGetterError = new TypeError("stream getter");
let streamGetterPreserved = false;
try {
  new TextDecoder("utf-8", { fatal: true }).decode(new Uint8Array(), {
    get stream() {
      throw streamGetterError;
    },
  });
} catch (error) {
  streamGetterPreserved = error === streamGetterError;
}

const deflated = zlib.deflateSync(Buffer.from("agentos", "utf8"));
const inflated = zlib.inflateSync(deflated).toString("utf8");

console.log(JSON.stringify({
  constants: {
    fOk: constants.F_OK ?? null,
    oRdOnly: constants.O_RDONLY ?? null,
    rOk: constants.R_OK ?? null,
  },
  decoded,
  decodedText,
  deflatedBase64: deflated.toString("base64"),
  encodeIntoBytes: Array.from(encodeIntoDestination),
  encodeIntoResult,
  encodedLength,
  fatalDecodeCode,
  formatted,
  inflated,
  isArrayBufferView: util.types.isArrayBufferView(textBytes),
  isDateViaUtilTypes: utilTypes.isDate(new Date("2024-01-01T00:00:00Z")),
  isMapViaUtilTypes: utilTypes.isMap(new Map([["alpha", 1]])),
  isTextDecoderShared: util.TextDecoder === TextDecoder,
  isTextEncoderShared: util.TextEncoder === TextEncoder,
  isUint8ArrayViaUtilTypes: utilTypes.isUint8Array(textBytes),
  malformedSurrogateBytes,
  maximalSubpartDecoded,
  promisified,
  punycodeAscii: punycode.toASCII("mañana.com"),
  punycodeUnicode: punycode.toUnicode("xn--maana-pta.com"),
  querystringParsed: querystring.parse("a=1&b=x&b=y"),
  querystringStringified: querystring.stringify({ a: 1, b: ["x", "y"] }),
  rejectsCode,
  streamingDecoded,
  streamGetterPreserved,
  symbolEncodingErrorName,
  throwsCode,
  unsupportedEncodingCode,
  utf16BeDecoded,
  utf16LeStreamDecoded,
}));
"#,
    );
}

fn web_streams_conformance_matches_host_node() {
    assert_conformance(
        "web-streams",
        r#"
import {
  ReadableStream as ImportedReadableStream,
  TextDecoderStream as ImportedTextDecoderStream,
  TextEncoderStream as ImportedTextEncoderStream,
  WritableStream as ImportedWritableStream,
  TransformStream as ImportedTransformStream,
} from "node:stream/web";
import {
  Blob as BufferBlob,
  File as BufferFile,
  resolveObjectURL,
} from "node:buffer";

const readAll = async (stream) => {
  const values = [];
  for await (const value of stream) values.push(value);
  return values;
};

let byobPullCount = 0;
const byteStream = new ReadableStream({
  type: "bytes",
  pull(controller) {
    byobPullCount += 1;
    const view = controller.byobRequest.view;
    view[0] = 0x41;
    view[1] = 0x42;
    controller.byobRequest.respond(2);
    controller.close();
  },
});
const byobReader = byteStream.getReader({ mode: "byob" });
const byobResult = await byobReader.read(new Uint8Array(4));
byobReader.releaseLock();

const pipedValues = [];
await new ReadableStream({
  start(controller) {
    controller.enqueue(2);
    controller.enqueue(3);
    controller.close();
  },
})
  .pipeThrough(new TransformStream({
    transform(value, controller) {
      controller.enqueue(value * 10);
    },
  }))
  .pipeTo(new WritableStream({
    write(value) {
      pipedValues.push(value);
    },
  }));

const [teeLeft, teeRight] = new ReadableStream({
  start(controller) {
    controller.enqueue("alpha");
    controller.enqueue("beta");
    controller.close();
  },
}).tee();
const [teeLeftValues, teeRightValues] = await Promise.all([
  readAll(teeLeft),
  readAll(teeRight),
]);

let cancellationReason = null;
const cancellable = new ReadableStream({
  cancel(reason) {
    cancellationReason = reason;
  },
});
await cancellable.cancel("stop");

let releaseWrite;
const pendingWrite = new Promise((resolve) => {
  releaseWrite = resolve;
});
const backpressureStream = new WritableStream({
  write() {
    return pendingWrite;
  },
}, { highWaterMark: 1 });
const writer = backpressureStream.getWriter();
const initialDesiredSize = writer.desiredSize;
const writePromise = writer.write("chunk");
const queuedDesiredSize = writer.desiredSize;
releaseWrite();
await writePromise;
const drainedDesiredSize = writer.desiredSize;
writer.releaseLock();

let pipeError = null;
try {
  await new ReadableStream({
    start(controller) {
      controller.error(new Error("stream-failure"));
    },
  }).pipeTo(new WritableStream());
} catch (error) {
  pipeError = error.message;
}

const lockStream = new ReadableStream();
const lockReader = lockStream.getReader();
const lockedWithReader = lockStream.locked;
lockReader.releaseLock();

const encoderStream = new TextEncoderStream();
const encodedChunksPromise = readAll(encoderStream.readable);
const encoderWriter = encoderStream.writable.getWriter();
await encoderWriter.write("\ud83d");
await encoderWriter.write("\ude00");
await encoderWriter.close();
const encodedStreamBytes = Array.from(
  (await encodedChunksPromise).flatMap((chunk) => Array.from(chunk)),
);

const decoderStream = new TextDecoderStream();
const decodedChunksPromise = readAll(decoderStream.readable);
const decoderWriter = decoderStream.writable.getWriter();
await decoderWriter.write(new Uint8Array([0xe2, 0x82]));
await decoderWriter.write(new Uint8Array([0xac]));
await decoderWriter.close();
const decodedStreamText = (await decodedChunksPromise).join("");

const richBlob = new Blob(
  ["A", new Uint8Array([0, 255]), "\u{1f600}"],
  { type: "IMAGE/PNG" },
);
const richBlobStreamChunks = await readAll(richBlob.stream());
const richBlobSlice = richBlob.slice(0, 2, "IMAGE/PNG");
const richFile = new File(
  [richBlob],
  "pixel-\u{1f600}.png",
  { type: "image/png", lastModified: 1700000000123 },
);
const responseBlob = await new Response(richBlob).blob();
const responseSliceBlob = await new Response(richBlobSlice).blob();
const richFormData = new FormData();
richFormData.append("caption", "pixel");
richFormData.append("asset", richFile);
richFormData.append("slice", richBlobSlice);
const formDataFile = richFormData.get("asset");
const multipartRequest = new Request("https://example.com/upload", {
  method: "POST",
  body: richFormData,
});
const multipartContentType = multipartRequest.headers.get("content-type");
const multipartText = await multipartRequest.text();
const blobUrl = URL.createObjectURL(richBlob);
const resolvedBlob = resolveObjectURL(blobUrl);
const fetchedBlobUrlResponse = await fetch(blobUrl);
const fetchedBlobUrlBytes = Array.from(
  new Uint8Array(await fetchedBlobUrlResponse.arrayBuffer()),
);
const rangedBlobUrlResponse = await fetch(blobUrl, {
  headers: { Range: "bytes=1-3" },
});
const rangedBlobUrlText = await rangedBlobUrlResponse.text();
const resolvedBlobFromUrl = resolveObjectURL(new URL(blobUrl));
let blobHeadRejected = false;
try {
  await fetch(blobUrl, { method: "HEAD" });
} catch {
  blobHeadRejected = true;
}
const abortedBlobFetchController = new AbortController();
abortedBlobFetchController.abort();
let abortedBlobFetchRejected = false;
try {
  await fetch(blobUrl, { signal: abortedBlobFetchController.signal });
} catch {
  abortedBlobFetchRejected = true;
}
const coercionBlobUrl = URL.createObjectURL(richBlob);
URL.revokeObjectURL(new URL(coercionBlobUrl));
URL.revokeObjectURL(blobUrl);
let revokedBlobUrlRejected = false;
try {
  await fetch(blobUrl);
} catch {
  revokedBlobUrlRejected = true;
}

console.log(JSON.stringify({
  bufferBlobShared: BufferBlob === Blob,
  bufferFileShared: BufferFile === File,
  byobBytes: Array.from(byobResult.value),
  byobDone: byobResult.done,
  byobPullCount,
  cancellationReason,
  abortedBlobFetchRejected,
  blobHeadRejected,
  constructorsShared:
    ImportedReadableStream === ReadableStream &&
    ImportedWritableStream === WritableStream &&
    ImportedTransformStream === TransformStream &&
    ImportedTextEncoderStream === TextEncoderStream &&
    ImportedTextDecoderStream === TextDecoderStream,
  decodedStreamText,
  drainedDesiredSize,
  initialDesiredSize,
  lockedAfterRelease: lockStream.locked,
  lockedWithReader,
  pipeError,
  pipedValues,
  queuedDesiredSize,
  richBlobBytes: Array.from(new Uint8Array(await richBlob.arrayBuffer())),
  richBlobSize: richBlob.size,
  richBlobSliceText: await richBlob.slice(1, 3).text(),
  richBlobSliceShared: richBlobSlice instanceof Blob,
  richBlobSliceType: richBlobSlice.type,
  richBlobStreamBytes: richBlobStreamChunks.flatMap((chunk) => Array.from(chunk)),
  richBlobStreamShared: richBlob.stream() instanceof ReadableStream,
  richBlobType: richBlob.type,
  richFileLastModified: richFile.lastModified,
  richFileName: richFile.name,
  richFileSize: richFile.size,
  richFileType: richFile.type,
  resolvedBlobBytes: Array.from(new Uint8Array(await resolvedBlob.arrayBuffer())),
  resolvedBlobFromUrl:
    resolvedBlobFromUrl?.size === richBlob.size &&
    resolvedBlobFromUrl?.type === richBlob.type,
  revokedCoercionBlobUrl:
    resolveObjectURL(coercionBlobUrl) === undefined,
  resolvedBlobType: resolvedBlob.type,
  revokedBlobUrlRejected,
  revokedBlobUrlResolvesUndefined: resolveObjectURL(blobUrl) === undefined,
  fetchedBlobUrlBytes,
  fetchedBlobUrlContentType:
    fetchedBlobUrlResponse.headers.get("content-type"),
  fetchedBlobUrlLength:
    fetchedBlobUrlResponse.headers.get("content-length"),
  fetchedBlobUrlStatusText: fetchedBlobUrlResponse.statusText,
  fetchedBlobUrlType: fetchedBlobUrlResponse.type,
  fetchedBlobUrlUrlMatches: fetchedBlobUrlResponse.url === blobUrl,
  rangedBlobUrlContentRange:
    rangedBlobUrlResponse.headers.get("content-range"),
  rangedBlobUrlStatus: rangedBlobUrlResponse.status,
  rangedBlobUrlText,
  responseBodyShared: new Response("ok").body instanceof ReadableStream,
  responseBlobBytes: Array.from(new Uint8Array(await responseBlob.arrayBuffer())),
  responseBlobInstance: responseBlob instanceof Blob,
  responseBlobType: responseBlob.type,
  responseSliceBlobType: responseSliceBlob.type,
  encodedStreamBytes,
  formDataEntries: Array.from(richFormData, ([name, value]) => [
    name,
    typeof value === "string" ? value : value.name,
  ]),
  formDataFileShared: formDataFile instanceof File,
  multipartHasCaption: multipartText.includes('name="caption"') &&
    multipartText.includes("pixel"),
  multipartHasFile: multipartText.includes('name="asset"') &&
    multipartText.includes('filename="pixel-😀.png"') &&
    multipartText.includes("Content-Type: image/png"),
  multipartHeaderHasBoundary:
    multipartContentType.startsWith("multipart/form-data; boundary="),
  teeLeftValues,
  teeRightValues,
}));
"#,
    );
}

fn regexp_conformance_matches_host_node() {
    assert_conformance(
        "regexp",
        r#"
const rgiEmoji = new RegExp("^\\p{RGI_Emoji}$", "v");
class DerivedRegExp extends RegExp {}
const derived = new DerivedRegExp("^[a-z]$", "v");
const asciiLetter = new RegExp("^[\\p{ASCII}&&\\p{Letter}]+$", "v");

console.log(JSON.stringify({
  asciiLetterMatches: asciiLetter.test("AgentOS"),
  asciiLetterRejectsDigit: !asciiLetter.test("Agent0S"),
  constructorLength: RegExp.length,
  constructorName: RegExp.name,
  derivedMatches: derived.test("a"),
  derivedPrototype: Object.getPrototypeOf(derived) === DerivedRegExp.prototype,
  hasCompatibilityWrapper: "__secureExecRgiEmojiCompat" in RegExp,
  malformedZwjRejected: !rgiEmoji.test("👨‍"),
  plainTextRejected: !rgiEmoji.test("hello"),
  rgiCases: [
    "⚽",
    "👨🏾‍⚕️",
    "🇧🇪",
    "1️⃣",
    "🏴\u{e0067}\u{e0062}\u{e0065}\u{e006e}\u{e0067}\u{e007f}",
  ].map((value) => rgiEmoji.test(value)),
  standaloneModifierRejected: !rgiEmoji.test("🏾"),
}));
"#,
    );
}

fn extended_builtin_polyfills_work_in_guest_v8() {
    let result = run_guest_script(
        "extended-builtins",
        r#"
import os from "node:os";
import { createRequire } from "node:module";

const require = createRequire(import.meta.url);
const moduleBuiltin = require("node:module");
const perfHooks = require("node:perf_hooks");
const streamConsumers = require("node:stream/consumers");
const streamPromises = require("node:stream/promises");
const timersPromises = require("node:timers/promises");
const tty = require("node:tty");
const zlib = require("node:zlib");
const { constants: zlibConstants } = await import("node:zlib");

perfHooks.performance.clearMarks?.();
perfHooks.performance.clearMeasures?.();
perfHooks.performance.mark("start");
await timersPromises.setTimeout(5);
perfHooks.performance.mark("end");
const measure = perfHooks.performance.measure("delta", "start", "end");

const immediateValue = await timersPromises.setImmediate("tick");
const timeoutValue = await timersPromises.setTimeout(1, "done");
const intervalValues = [];
const interval = timersPromises.setInterval(1, "pulse");
intervalValues.push((await interval.next()).value);
intervalValues.push((await interval.next()).value);
await interval.return();

function createSink() {
  const listeners = new Map();
  return {
    chunks: [],
    write(chunk, callback) {
      this.chunks.push(Buffer.from(chunk).toString("utf8"));
      callback?.(null);
    },
    end(callback) {
      queueMicrotask(() => {
        for (const handler of listeners.get("finish") ?? []) handler();
        for (const handler of listeners.get("close") ?? []) handler();
        callback?.(null);
      });
    },
    once(event, handler) {
      const entries = listeners.get(event) ?? [];
      listeners.set(event, [...entries, handler]);
      return this;
    },
    off(event, handler) {
      const entries = listeners.get(event) ?? [];
      listeners.set(
        event,
        entries.filter((candidate) => candidate !== handler),
      );
      return this;
    },
  };
}

const pipelineWritable = createSink();
await streamPromises.pipeline(
  (async function* () {
    yield Buffer.from("left");
    yield Buffer.from("+");
    yield Buffer.from("right");
  })(),
  pipelineWritable,
);

const finishedWritable = createSink();
const finishedResult = streamPromises.finished(finishedWritable).then(() => "resolved");
finishedWritable.end();

function makeAsyncStream(chunks) {
  return (async function* () {
    for (const chunk of chunks) {
      yield chunk;
    }
  })();
}

const textValue = await streamConsumers.text(
  makeAsyncStream([
    Buffer.from("he"),
    Buffer.from("llo"),
  ]),
);
const jsonValue = await streamConsumers.json(
  makeAsyncStream([Buffer.from('{"ok":true,"count":2}')]),
);
const arrayBufferValue = await streamConsumers.arrayBuffer(
  makeAsyncStream([Buffer.from("AB")]),
);
const blobValue = await streamConsumers.blob(
  makeAsyncStream([Buffer.from("blob")]),
);
const bufferValue = await streamConsumers.buffer(
  makeAsyncStream([Buffer.from("buf")]),
);

const deflated = zlib.deflateSync(Buffer.from("agentos", "utf8"));
const inflated = zlib.inflateSync(deflated).toString("utf8");

process.stdout.write(`${JSON.stringify({
  moduleBuiltinHasCreateRequire:
    typeof moduleBuiltin.createRequire === "function",
  moduleBuiltinHasBuiltinModules:
    Array.isArray(moduleBuiltin.builtinModules),
  moduleBuiltinHasStreamPromises:
    moduleBuiltin.builtinModules.includes("stream/promises"),
  os: {
    arch: os.arch(),
    availableParallelism: os.availableParallelism(),
    cpusLength: os.cpus().length,
    eol: os.EOL,
    freemem: os.freemem(),
    hasSignals: typeof os.constants?.signals?.SIGTERM === "number",
    homedir: os.homedir(),
    hostname: os.hostname(),
    networkInterfaceKeys: Object.keys(os.networkInterfaces()),
    platform: os.platform(),
    release: os.release(),
    tmpdir: os.tmpdir(),
    totalmem: os.totalmem(),
    type: os.type(),
    userInfoHomedir: os.userInfo().homedir,
  },
  perf: {
    entriesByType: perfHooks.performance.getEntriesByType?.("measure")?.length ?? 0,
    entriesByName: perfHooks.performance.getEntriesByName?.("delta", "measure")?.length ?? 0,
    hasNow: typeof perfHooks.performance.now === "function",
    hasObserver: typeof perfHooks.PerformanceObserver === "function",
    measureDurationFinite: Number.isFinite(measure.duration),
  },
  streamConsumers: {
    arrayBufferLength: arrayBufferValue.byteLength,
    blobText: await blobValue.text(),
    bufferText: bufferValue.toString("utf8"),
    jsonCount: jsonValue.count,
    jsonOk: jsonValue.ok,
    textValue,
  },
  streamPromises: {
    finishedResult: await finishedResult,
    pipelineText: pipelineWritable.chunks.join(""),
  },
  timersPromises: {
    immediateValue,
    intervalValues,
    timeoutValue,
  },
  tty: {
    isatty0: tty.isatty(0),
    isatty1: tty.isatty(1),
    isatty2: tty.isatty(2),
    readStreamType: typeof tty.ReadStream,
    writeStreamType: typeof tty.WriteStream,
  },
  zlib: {
    constantsHasSyncFlush: typeof zlib.constants?.Z_SYNC_FLUSH === "number",
    importConstantsHasSyncFlush: typeof zlibConstants?.Z_SYNC_FLUSH === "number",
    createDeflateType: typeof zlib.createDeflate,
    createInflateType: typeof zlib.createInflate,
    inflated,
  },
})}\n`);
process.exit(0);
"#,
    );

    assert_eq!(result["moduleBuiltinHasCreateRequire"], true);
    assert_eq!(result["moduleBuiltinHasBuiltinModules"], true);
    assert_eq!(result["moduleBuiltinHasStreamPromises"], true);
    assert_eq!(result["os"]["platform"], "linux");
    assert_eq!(result["os"]["arch"], "x64");
    assert_eq!(result["os"]["type"], "Linux");
    assert!(result["os"]["homedir"]
        .as_str()
        .expect("os.homedir string")
        .starts_with('/'));
    assert_eq!(result["os"]["tmpdir"], "/tmp");
    assert_eq!(result["os"]["userInfoHomedir"], result["os"]["homedir"]);
    assert_eq!(result["os"]["eol"], "\n");
    assert_eq!(result["os"]["availableParallelism"], 1);
    assert_eq!(result["os"]["cpusLength"], 1);
    assert_eq!(result["os"]["totalmem"], 134_217_728u64);
    assert_eq!(result["os"]["freemem"], 134_217_728u64);
    assert_eq!(result["os"]["hasSignals"], true);
    assert!(result["os"]["networkInterfaceKeys"]
        .as_array()
        .expect("network interfaces array")
        .is_empty());
    assert_eq!(result["perf"]["hasNow"], true);
    assert_eq!(result["perf"]["hasObserver"], true);
    assert_eq!(result["perf"]["measureDurationFinite"], true);
    assert_eq!(result["perf"]["entriesByType"], 1);
    assert_eq!(result["perf"]["entriesByName"], 1);
    assert_eq!(result["timersPromises"]["immediateValue"], "tick");
    assert_eq!(result["timersPromises"]["timeoutValue"], "done");
    assert_eq!(
        result["timersPromises"]["intervalValues"]
            .as_array()
            .expect("interval values"),
        &vec![Value::from("pulse"), Value::from("pulse")]
    );
    assert_eq!(result["streamPromises"]["pipelineText"], "left+right");
    assert_eq!(result["streamPromises"]["finishedResult"], "resolved");
    assert_eq!(result["streamConsumers"]["textValue"], "hello");
    assert_eq!(result["streamConsumers"]["jsonOk"], true);
    assert_eq!(result["streamConsumers"]["jsonCount"], 2);
    assert_eq!(result["streamConsumers"]["arrayBufferLength"], 2);
    assert_eq!(result["streamConsumers"]["blobText"], "blob");
    assert_eq!(result["streamConsumers"]["bufferText"], "buf");
    assert_eq!(result["tty"]["readStreamType"], "function");
    assert_eq!(result["tty"]["writeStreamType"], "function");
    assert_eq!(result["tty"]["isatty0"], false);
    assert_eq!(result["tty"]["isatty1"], false);
    assert_eq!(result["tty"]["isatty2"], false);
    assert_eq!(result["zlib"]["constantsHasSyncFlush"], true);
    assert_eq!(result["zlib"]["importConstantsHasSyncFlush"], true);
    assert_eq!(result["zlib"]["createDeflateType"], "function");
    assert_eq!(result["zlib"]["createInflateType"], "function");
    assert_eq!(result["zlib"]["inflated"], "agentos");
}

fn timer_handle_ref_refresh_matches_host_node_impl() {
    assert_node_available();

    let cwd = temp_dir("builtin-timer-handle-ref-refresh");
    let entrypoint = cwd.join("entry.mjs");
    write_fixture(
        &entrypoint,
        r#"
import { performance } from "node:perf_hooks";

const timeout = setTimeout(() => {}, 1_000);
const interval = setInterval(() => {}, 1_000);
const initial = {
  timeout: timeout.hasRef(),
  interval: interval.hasRef(),
};
const unrefReturnSelf = timeout.unref() === timeout && interval.unref() === interval;
const afterUnref = {
  timeout: timeout.hasRef(),
  interval: interval.hasRef(),
};
const refReturnSelf = timeout.ref() === timeout && interval.ref() === interval;
const afterRef = {
  timeout: timeout.hasRef(),
  interval: interval.hasRef(),
};
clearTimeout(timeout);
clearInterval(interval);

const refreshDelay = 80;
const refreshWait = 40;
const refreshTolerance = 20;
const refreshStart = performance.now();
let refreshReturnSelf = false;
let refreshedAt = 0;

await new Promise((resolve) => {
  const refreshedTimeout = setTimeout(() => {
    const elapsed = performance.now() - refreshStart;
    console.log(JSON.stringify({
      initial,
      unrefReturnSelf,
      afterUnref,
      refReturnSelf,
      afterRef,
      refreshReturnSelf,
      refreshHonored: elapsed >= refreshedAt + refreshDelay - refreshTolerance,
    }));
    resolve();
  }, refreshDelay);

  setTimeout(() => {
    refreshedAt = performance.now() - refreshStart;
    refreshReturnSelf = refreshedTimeout.refresh() === refreshedTimeout;
  }, refreshWait);
});
"#,
    );

    let host = run_host_probe(&cwd, &entrypoint);
    let guest = run_guest_probe_with_config(
        "timer-handle-ref-refresh",
        &cwd,
        &entrypoint,
        HashMap::new(),
        wire_permissions_allow_all(),
        &["perf_hooks", "timers"],
    );

    assert_eq!(
        guest,
        host,
        "guest timer handle behavior diverged from host Node\nhost: {}\nguest: {}",
        serde_json::to_string_pretty(&host).expect("pretty host JSON"),
        serde_json::to_string_pretty(&guest).expect("pretty guest JSON")
    );
    assert_eq!(guest["afterUnref"]["timeout"], Value::Bool(false));
    assert_eq!(guest["afterUnref"]["interval"], Value::Bool(false));
    assert_eq!(guest["afterRef"]["timeout"], Value::Bool(true));
    assert_eq!(guest["afterRef"]["interval"], Value::Bool(true));
    assert_eq!(guest["refreshHonored"], Value::Bool(true));
}

#[test]
fn timer_handle_ref_refresh_matches_host_node() {
    run_isolated_builtin_conformance_test("timer-handle-ref-refresh");
}

fn unrefd_timeout_does_not_keep_guest_process_alive_impl() {
    let cwd = temp_dir("builtin-timer-unref-exit");
    let entrypoint = cwd.join("entry.mjs");
    write_fixture(
        &entrypoint,
        r#"
const timer = setTimeout(() => {
  console.error("timer-fired");
  process.exitCode = 1;
}, 10_000);

timer.unref();
console.log(JSON.stringify({ hasRefAfterUnref: timer.hasRef() }));
"#,
    );

    let mut sidecar = new_sidecar("timer-unref-exit");
    let connection_id = authenticate_wire(&mut sidecar, "conn-timer-unref-exit");
    let session_id = open_session_wire(&mut sidecar, 2, &connection_id);
    let allowed_builtins =
        serde_json::to_string(&["timers"]).expect("serialize timer builtin allowlist");
    let mut metadata = HashMap::new();
    metadata.insert(
        String::from("env.AGENTOS_ALLOWED_NODE_BUILTINS"),
        allowed_builtins,
    );
    let vm_id = create_vm_with_metadata_and_permissions(
        &mut sidecar,
        3,
        &connection_id,
        &session_id,
        GuestRuntimeKind::JavaScript,
        &cwd,
        metadata,
        wire_permissions_allow_all(),
    );

    let started_at = Instant::now();
    execute_wire(
        &mut sidecar,
        4,
        &connection_id,
        &session_id,
        &vm_id,
        "proc-timer-unref-exit",
        GuestRuntimeKind::JavaScript,
        &entrypoint,
        Vec::new(),
    );

    let (stdout, stderr, exit_code) = collect_builtin_process_output_with_timeout(
        &mut sidecar,
        &connection_id,
        &session_id,
        &vm_id,
        "proc-timer-unref-exit",
        Duration::from_secs(2),
    );
    let elapsed = started_at.elapsed();
    dispose_vm_and_close_session_wire(&mut sidecar, &connection_id, &session_id, &vm_id);

    assert_eq!(exit_code, 0, "guest process should exit cleanly: {stderr}");
    assert!(
        stderr.trim().is_empty(),
        "guest process should not wait long enough to fire the timer:\n{stderr}"
    );
    if run_timing_sensitive_tests() {
        assert!(
            elapsed < Duration::from_millis(1_500),
            "guest process waited too long for an unref'd timer: {elapsed:?}"
        );
    }

    let payload: Value = serde_json::from_str(stdout.trim()).expect("parse timer stdout JSON");
    assert_eq!(payload["hasRefAfterUnref"], Value::Bool(false));
}

#[test]
fn unrefd_timeout_does_not_keep_guest_process_alive() {
    run_isolated_builtin_conformance_test("timer-unref-exit");
}

fn unrefd_http_server_does_not_keep_guest_process_alive_impl() {
    let cwd = temp_dir("builtin-http-server-unref-exit");
    let entrypoint = cwd.join("entry.mjs");
    write_fixture(
        &entrypoint,
        r#"
import http from "node:http";

const server = http.createServer((_request, response) => response.end("ok"));
server.listen(0, "127.0.0.1", () => {
  server.unref();
  console.log(JSON.stringify({ listening: server.listening }));
});
"#,
    );

    let mut sidecar = new_sidecar("http-server-unref-exit");
    let connection_id = authenticate_wire(&mut sidecar, "conn-http-server-unref-exit");
    let session_id = open_session_wire(&mut sidecar, 2, &connection_id);
    let allowed_builtins =
        serde_json::to_string(&["http", "net"]).expect("serialize http builtin allowlist");
    let mut metadata = HashMap::new();
    metadata.insert(
        String::from("env.AGENTOS_ALLOWED_NODE_BUILTINS"),
        allowed_builtins,
    );
    let vm_id = create_vm_with_metadata_and_permissions(
        &mut sidecar,
        3,
        &connection_id,
        &session_id,
        GuestRuntimeKind::JavaScript,
        &cwd,
        metadata,
        wire_permissions_allow_all(),
    );

    execute_wire(
        &mut sidecar,
        4,
        &connection_id,
        &session_id,
        &vm_id,
        "proc-http-server-unref-exit",
        GuestRuntimeKind::JavaScript,
        &entrypoint,
        Vec::new(),
    );

    // Host Node exits as soon as the only listener is unref'd; the guest must
    // not wait for the server to close.
    let (stdout, stderr, exit_code) = collect_builtin_process_output_with_timeout(
        &mut sidecar,
        &connection_id,
        &session_id,
        &vm_id,
        "proc-http-server-unref-exit",
        Duration::from_secs(5),
    );
    dispose_vm_and_close_session_wire(&mut sidecar, &connection_id, &session_id, &vm_id);

    assert_eq!(exit_code, 0, "guest process should exit cleanly: {stderr}");
    let payload: Value = serde_json::from_str(stdout.trim()).expect("parse server stdout JSON");
    assert_eq!(payload["listening"], Value::Bool(true));
}

#[test]
fn unrefd_http_server_does_not_keep_guest_process_alive() {
    run_isolated_builtin_conformance_test("http-server-unref-exit");
}

fn run_named_case(case_name: &str) {
    match case_name {
        "fs" => fs_conformance_matches_host_node(),
        "console" => console_conformance_matches_host_node(),
        "child_process" => child_process_conformance_matches_host_node(),
        "path" => path_conformance_matches_host_node(),
        "crypto" => crypto_conformance_matches_host_node(),
        "dns" => dns_conformance_matches_host_node(),
        "events" => events_conformance_matches_host_node(),
        "stream" => stream_conformance_matches_host_node(),
        "buffer" => buffer_conformance_matches_host_node(),
        "global_base64" => global_base64_conformance_matches_host_node(),
        "url" => url_conformance_matches_host_node(),
        "stdlib_polyfill" => stdlib_polyfill_conformance_matches_host_node(),
        "web_streams" => web_streams_conformance_matches_host_node(),
        "regexp" => regexp_conformance_matches_host_node(),
        "extended_builtin_polyfills" => extended_builtin_polyfills_work_in_guest_v8(),
        other => panic!("unknown builtin conformance case: {other}"),
    }
}

#[test]
fn builtin_conformance_cases() {
    let current_exe = std::env::current_exe().expect("current test binary path");

    for case_name in BUILTIN_CONFORMANCE_CASES {
        let status = Command::new(&current_exe)
            .arg("--exact")
            .arg("__builtin_conformance_case_runner")
            .arg("--nocapture")
            .env("AGENTOS_BUILTIN_CONFORMANCE_CASE", case_name)
            .status()
            .unwrap_or_else(|error| {
                panic!("spawn builtin conformance runner for {case_name}: {error}")
            });

        assert!(
            status.success(),
            "builtin conformance case {case_name} failed with status {status}"
        );
    }
}

#[test]
fn __builtin_conformance_case_runner() {
    let Ok(case_name) = std::env::var("AGENTOS_BUILTIN_CONFORMANCE_CASE") else {
        return;
    };

    run_named_case(&case_name);
}

// `node:async_hooks` is not in the default probe allowlist.
const ASYNC_CONTEXT_PROBE_BUILTINS: &[&str] = &[
    "async_hooks",
    "child_process",
    "dns",
    "events",
    "dgram",
    "fs",
    "http",
    "http2",
    "module",
    "net",
    "path",
    "stream",
    "timers",
    "tls",
    "url",
    "vm",
    "zlib",
];

fn run_async_context_guest_probe(case_name: &str, cwd: &Path, entrypoint: &Path) -> Value {
    run_guest_probe_with_config(
        case_name,
        cwd,
        entrypoint,
        HashMap::new(),
        wire_permissions_allow_all(),
        ASYNC_CONTEXT_PROBE_BUILTINS,
    )
}

fn assert_async_context_conformance(case_name: &str, script: &str) {
    assert_node_available();

    let cwd = temp_dir(&format!("builtin-conformance-{case_name}"));
    let entrypoint = cwd.join("entry.mjs");
    write_fixture(&entrypoint, script);

    let host = run_host_probe(&cwd, &entrypoint);
    let guest = run_async_context_guest_probe(case_name, &cwd, &entrypoint);

    assert_eq!(
        guest,
        host,
        "guest V8 result diverged from host Node for {case_name}\nhost: {}\nguest: {}",
        serde_json::to_string_pretty(&host).expect("pretty host JSON"),
        serde_json::to_string_pretty(&guest).expect("pretty guest JSON")
    );
}

fn run_async_context_probe_script(case_name: &str, script: &str) -> Value {
    assert_node_available();

    let cwd = temp_dir(&format!("builtin-conformance-{case_name}"));
    let entrypoint = cwd.join("entry.mjs");
    write_fixture(&entrypoint, script);
    run_async_context_guest_probe(case_name, &cwd, &entrypoint)
}

fn async_local_storage_isolates_overlapping_calls_impl() {
    assert_async_context_conformance(
        "als-overlapping-calls",
        r#"
import { AsyncLocalStorage } from "node:async_hooks";
import { once } from "node:events";
import fs from "node:fs/promises";
import net from "node:net";

const als = new AsyncLocalStorage();
const wrong = [];
const expectStore = (expected, step) => {
  const seen = als.getStore() ?? null;
  if (seen !== expected) wrong.push({ expected, step, seen });
};
const sleep = (ms) => new Promise((resolve) => setTimeout(resolve, ms));

const echoServer = net.createServer((socket) => socket.on("data", (chunk) => socket.write(chunk)));
echoServer.listen(0, "127.0.0.1");
await once(echoServer, "listening");

async function socketRoundTrip(payload) {
  const socket = net.connect(echoServer.address().port, "127.0.0.1");
  await once(socket, "connect");
  socket.write(payload);
  const [echoed] = await once(socket, "data");
  socket.destroy();
  return String(echoed);
}

const call = (id, delayMs) =>
  als.run(id, async () => {
    expectStore(id, "sync start");
    await null;
    expectStore(id, "after microtask");
    await sleep(delayMs);
    expectStore(id, "after timer");
    await new Promise((resolve) => setImmediate(resolve));
    expectStore(id, "after setImmediate");
    await new Promise((resolve) => process.nextTick(resolve));
    expectStore(id, "after nextTick");
    await new Promise((resolve) => queueMicrotask(resolve));
    expectStore(id, "after queueMicrotask");

    const file = new URL(`./overlap-${id}.txt`, import.meta.url);
    await fs.writeFile(file, id);
    expectStore(id, "after fs write");
    if ((await fs.readFile(file, "utf8")) !== id) wrong.push({ id, step: "fs content" });
    expectStore(id, "after fs read");

    if ((await socketRoundTrip(id)) !== id) wrong.push({ id, step: "socket content" });
    expectStore(id, "after socket round trip");

    // The nested context is parked while the outer one keeps running.
    const innerId = `${id}/inner`;
    const inner = als.run(innerId, async () => {
      await sleep(3);
      expectStore(innerId, "nested after timer");
      return als.getStore();
    });
    expectStore(id, "outer after nested run returned");
    await sleep(1);
    expectStore(id, "outer while nested is parked");
    if ((await inner) !== innerId) wrong.push({ id, step: "nested result" });
    expectStore(id, "outer after nested finished");
    return als.getStore();
  });

// This context ends while every other call is still parked.
const shortLived = als.run("short-lived", async () => {
  await null;
  expectStore("short-lived", "short-lived");
  return "ended";
});
const ids = Array.from({ length: 40 }, (_, index) => `call-${index}`);
const calls = ids.map((id, index) => call(id, 1 + ((index * 7) % 23)));
const callerAfterRun = als.getStore() ?? null;
// Runs while every call is parked, from a context with no store.
const callerTimer = await new Promise((resolve) => setTimeout(() => resolve(als.getStore() ?? null), 2));
const shortLivedResult = await shortLived;
const callerAfterShortLived = als.getStore() ?? null;
const returned = await Promise.all(calls);
echoServer.close();

console.log(JSON.stringify({
  wrong,
  everyCallReturnedItsOwnId: returned.every((value, index) => value === ids[index]),
  callerAfterRun,
  callerTimer,
  shortLivedResult,
  callerAfterShortLived,
  callerAtEnd: als.getStore() ?? null,
}));
"#,
    );
}

#[test]
fn async_local_storage_isolates_overlapping_calls() {
    run_isolated_builtin_conformance_test("als-overlapping-calls");
}

fn async_local_storage_api_matches_host_node_impl() {
    assert_async_context_conformance(
        "als-api",
        r#"
import { AsyncLocalStorage, AsyncResource } from "node:async_hooks";
import fs from "node:fs";
import { createRequire } from "node:module";
import vm from "node:vm";

const require = createRequire(import.meta.url);
const als = new AsyncLocalStorage();
const store = () => als.getStore() ?? null;
const out = {};
const sleep = (ms) => new Promise((resolve) => setTimeout(resolve, ms));

als.run("sync", () => { out.syncRun = store(); });
out.afterSyncRun = store();
try {
  als.run("throws", () => { throw new Error("boom"); });
} catch {
  out.afterRunThrew = store();
}

als.run("then", () => { Promise.resolve().then(() => sleep(1)).then(() => { out.thenChain = store(); }); });
await sleep(10);

// A reaction belongs to the context that registered it, not the one that resolved.
let resolveLate;
const late = new Promise((resolve) => { resolveLate = resolve; });
const registered = als.run("registered", () => late.then(store));
als.run("resolver", () => resolveLate());
out.reactionContext = await registered;

out.thenable = await als.run("thenable", async () => await { then(resolve) { resolve(store()); } });
out.asyncGenerator = await als.run("generator", async () => {
  async function* values() {
    yield store();
    await sleep(1);
    yield store();
  }
  const seen = [];
  for await (const value of values()) seen.push(value);
  return seen;
});
await als.run("interval", () => new Promise((resolve) => {
  const interval = setInterval(() => {
    out.interval = store();
    clearInterval(interval);
    resolve();
  }, 1);
}));

await als.run("outer", async () => {
  als.enterWith("entered");
  out.enterWithSync = store();
  await sleep(1);
  out.enterWithAfterAwait = store();
});
out.afterEnterWith = store();

await als.run("outer", async () => {
  const inside = als.exit(async () => {
    const sync = store();
    await sleep(1);
    return [sync, store()];
  });
  out.afterExitReturned = store();
  out.insideExit = await inside;
});

// Stores are per instance, and falsy stores are real stores.
const second = new AsyncLocalStorage();
als.run("first", () => second.run(0, () => { out.twoInstances = [store(), second.getStore()]; }));

const disabled = new AsyncLocalStorage();
await disabled.run("set", async () => {
  disabled.disable();
  out.disabledSync = disabled.getStore() ?? null;
  await null;
  out.disabledAfterAwait = disabled.getStore() ?? null;
  disabled.run("again", () => { out.runAfterDisable = disabled.getStore(); });
});

const resource = als.run("resource", () => new AsyncResource("probe"));
als.run("caller", () => {
  out.runInAsyncScope = resource.runInAsyncScope(store);
  out.afterRunInAsyncScope = store();
  out.resourceBind = resource.bind(store)();
  out.resourceBindThis = resource.bind(function () { return this; }, "this-value")();
  out.staticBind = als.run("static-bind", () => AsyncResource.bind(store))();
  out.alsBind = als.run("als-bind", () => AsyncLocalStorage.bind(store))();
  out.snapshot = als.run("snapshot", () => AsyncLocalStorage.snapshot())(store);
  out.afterBound = store();
});

// Synchronous re-entry into V8 keeps the caller's store.
await sleep(1);
out.vmScript = als.run("vm", () => {
  vm.runInThisContext("Promise.resolve().then(() => {})");
  return store();
});
fs.writeFileSync(new URL("./dependency.mjs", import.meta.url), "export const value = 1;");
out.requireEsm = als.run("require-esm", () => {
  require("./dependency.mjs");
  return store();
});

out.final = store();
console.log(JSON.stringify(out));
"#,
    );
}

#[test]
fn async_local_storage_api_matches_host_node() {
    run_isolated_builtin_conformance_test("als-api");
}

fn async_local_storage_runtime_callbacks_match_host_node_impl() {
    assert_async_context_conformance(
        "als-runtime-callbacks",
        r#"
import { AsyncLocalStorage } from "node:async_hooks";
import childProcess from "node:child_process";
import dns from "node:dns";
import fs from "node:fs";
import http from "node:http";
import net from "node:net";
import { PassThrough, pipeline, Readable } from "node:stream";
import zlib from "node:zlib";

const als = new AsyncLocalStorage();
const store = () => als.getStore() ?? null;
const out = {};
const fixture = new URL(import.meta.url);
// A stuck step reports how far the probe got, not a blank timeout.
setTimeout(() => {
  console.log(JSON.stringify({ stuckAfter: Object.keys(out).at(-1), out }));
  process.exit(3);
}, 8000).unref();

// Socket events run in the context that created the handle, not the context that
// added the listener. A handle made in a server's accept has no context.
await new Promise((done) => {
  const server = als.run("net-server", () => net.createServer((accepted) => {
    out.netConnection = store();
    accepted.on("data", () => {
      out.netAcceptedData = store();
      accepted.end("pong");
    });
  }));
  als.run("net-listen", () => server.listen(0, "127.0.0.1", () => {
    out.netListening = store();
    const client = als.run("net-client", () => net.connect(server.address().port, "127.0.0.1"));
    als.run("net-listeners", () => {
      client.on("connect", () => {
        out.netClientConnect = store();
        client.write("ping");
      });
      client.on("data", () => { out.netClientData = store(); });
      client.on("close", () => {
        out.netClientClose = store();
        server.close();
        done();
      });
    });
  }));
});

await new Promise((done) => {
  const server = als.run("http-server", () => http.createServer((request, response) => {
    out.httpRequest = store();
    request.on("end", () => {
      out.httpRequestEnd = store();
      response.end("ok");
    });
    request.resume();
  }));
  als.run("http-listen", () => server.listen(0, "127.0.0.1", () => {
    als.run("http-client", () => {
      http.get({ host: "127.0.0.1", port: server.address().port, agent: false }, (response) => {
        out.httpResponse = store();
        response.on("data", () => { out.httpResponseData = store(); });
        response.on("end", () => {
          out.httpResponseEnd = store();
          server.close();
          done();
        });
      });
    });
  }));
});

out.fetch = await als.run("fetch", async () => {
  const server = http.createServer((request, response) => response.end("ok"));
  await new Promise((resolve) => server.listen(0, "127.0.0.1", resolve));
  const response = await fetch(`http://127.0.0.1:${server.address().port}/`, { headers: { connection: "close" } });
  await response.text();
  server.close();
  return store();
});

const callback = (name, start) =>
  als.run(name, () => new Promise((resolve) => start(() => {
    out[name] = store();
    resolve();
  })));
await callback("fsReadFile", (done) => fs.readFile(fixture, done));
await callback("fsStat", (done) => fs.stat(fixture, done));
await callback("zlibGzip", (done) => zlib.gzip("x", done));
await callback("dnsLookup", (done) => dns.lookup("localhost", done));
await callback("childExecFile", (done) => childProcess.execFile("node", ["-e", "0"], done));
await callback("fsReadStream", (done) => {
  const stream = fs.createReadStream(fixture);
  stream.on("data", () => { out.fsReadStreamData = store(); });
  stream.on("close", done);
});
await callback("streamPipeline", (done) => pipeline(Readable.from(["x"]), new PassThrough().resume(), done));

await new Promise((done) => {
  const child = als.run("child", () => childProcess.spawn("node", ["-e", "console.log('x')"]));
  als.run("child-listeners", () => {
    child.stdout.on("data", () => { out.childStdout = store(); });
    child.on("exit", () => { out.childExit = store(); });
    child.on("close", () => {
      out.childClose = store();
      done();
    });
  });
});

await new Promise((done) => {
  const keepAlive = setInterval(() => {}, 10);
  als.run("signal", () => process.once("SIGHUP", () => {
    out.signal = store();
    clearInterval(keepAlive);
    done();
  }));
  als.run("signal-sender", () => process.kill(process.pid, "SIGHUP"));
});

// A handler for an uncaught error runs in the context that threw.
await new Promise((done) => {
  process.once("uncaughtException", () => {
    out.uncaughtException = store();
    done();
  });
  als.run("thrower", () => setTimeout(() => { throw new Error("boom"); }, 1));
});

out.final = store();
console.log(JSON.stringify(out));
"#,
    );
}

#[test]
fn async_local_storage_runtime_callbacks_match_host_node() {
    run_isolated_builtin_conformance_test("als-runtime-callbacks");
}

fn async_local_storage_http2_tls_and_udp_callbacks_match_host_node_impl() {
    assert_async_context_conformance(
        "als-http2-tls-udp",
        r#"
import { AsyncLocalStorage } from "node:async_hooks";
import http2 from "node:http2";
import dgram from "node:dgram";
import net from "node:net";
import tls from "node:tls";

const als = new AsyncLocalStorage();
const s = () => als.getStore() ?? null;
const out = {};
setTimeout(() => {
  console.log(JSON.stringify({ stuckAfter: Object.keys(out).at(-1), out }));
  process.exit(3);
}, 8000).unref();
await new Promise((done) => {
  const server = als.run("h2-server", () => http2.createServer());
  server.on("session", () => { out.h2ServerSession = s(); });
  als.run("h2-on", () => server.on("stream", (stream) => {
    out.h2ServerStream = s();
    stream.on("data", () => { out.h2ServerStreamData = s(); });
    stream.on("end", () => { out.h2ServerStreamEnd = s(); stream.respond({ ":status": 200 }); stream.end("ok"); });
  }));
  als.run("h2-listen", () => server.listen(0, "127.0.0.1", () => {
    out.h2Listening = s();
    const client = als.run("h2-connect", () => http2.connect(`http://127.0.0.1:${server.address().port}`));
    client.on("connect", () => { out.h2ClientConnect = s(); });
    const req = als.run("h2-request", () => client.request({ ":path": "/", ":method": "POST" }));
    als.run("h2-reg", () => {
      req.on("response", () => { out.h2Response = s(); });
      req.on("data", () => { out.h2ResponseData = s(); });
      req.on("end", () => { out.h2ResponseEnd = s(); });
      req.on("close", () => { out.h2RequestClose = s(); client.close(); server.close(); done(); });
    });
    req.end("body");
  }));
});
const key = `-----BEGIN PRIVATE KEY-----
MIGHAgEAMBMGByqGSM49AgEGCCqGSM49AwEHBG0wawIBAQQgGU2u3Uut27dfKu/J
PhJQU1d4qsWR2fRhsz6QgMDgkpKhRANCAATPt/xCvp7MGxe3bORLNY5MCZk+/h83
I20gtTbpL1OEZ8Xs5rhg1yq/v/wMVpihbW46atrd7yikTp+w/KfKsoGS
-----END PRIVATE KEY-----`;
const cert = `-----BEGIN CERTIFICATE-----
MIIBfzCCASWgAwIBAgIUSyBXZ/hpG6jMQau8+4Kf1oEhXAowCgYIKoZIzj0EAwIw
FDESMBAGA1UEAwwJbG9jYWxob3N0MCAXDTI2MTEwNTA5NTA0OFoYDzIxMjYxMDEy
MDk1MDQ4WjAUMRIwEAYDVQQDDAlsb2NhbGhvc3QwWTATBgcqhkjOPQIBBggqhkjO
PQMBBwNCAATPt/xCvp7MGxe3bORLNY5MCZk+/h83I20gtTbpL1OEZ8Xs5rhg1yq/
v/wMVpihbW46atrd7yikTp+w/KfKsoGSo1MwUTAdBgNVHQ4EFgQU5Wk27H2od303
qSzbKd4GL7eS91YwHwYDVR0jBBgwFoAU5Wk27H2od303qSzbKd4GL7eS91YwDwYD
VR0TAQH/BAUwAwEB/zAKBggqhkjOPQQDAgNIADBFAiEAuZ1ZxmTLFryq/rGJM7bk
jdDldJGq33Wr55EsOtZgZ8wCIBJzgvhLdwQ6W7xOXt+7Ec3/8VHa92EZA/yOYKFp
W95w
-----END CERTIFICATE-----`;
await new Promise((done) => {
  const server = als.run("tls-create", () => tls.createServer({ key, cert }, (socket) => {
    out.tlsSecureConnection = s();
    socket.on("data", () => {
      out.tlsServerData = s();
      socket.end("y");
    });
  }));
  als.run("tls-listen", () => server.listen(0, "127.0.0.1", () => {
    const client = als.run("tls-client", () => tls.connect({ port: server.address().port, host: "127.0.0.1", rejectUnauthorized: false }));
    als.run("tls-listeners", () => {
      client.on("secureConnect", () => {
        out.tlsSecureConnect = s();
        client.write("x");
      });
      client.on("data", () => { out.tlsClientData = s(); });
      client.on("close", () => {
        out.tlsClientClose = s();
        server.close();
        done();
      });
    });
  }));
});
// A TLS wrap around an existing socket owns its events from the context that wrapped.
await new Promise((done) => {
  const server = tls.createServer({ key, cert }, (socket) => {
    socket.on("data", () => socket.end("y"));
    socket.on("error", () => {});
  });
  server.listen(0, "127.0.0.1", () => {
    const raw = als.run("tls-raw", () => net.connect(server.address().port, "127.0.0.1"));
    raw.once("connect", () => {
      const wrapped = als.run("tls-wrap", () => tls.connect({ socket: raw, rejectUnauthorized: false }));
      wrapped.on("secureConnect", () => {
        out.tlsWrapSecureConnect = s();
        wrapped.write("x");
      });
      wrapped.on("data", () => { out.tlsWrapData = s(); });
      wrapped.on("close", () => {
        server.close();
        done();
      });
    });
  });
});
await new Promise((done) => {
  const sock = als.run("udp", () => dgram.createSocket("udp4"));
  als.run("udp-reg", () => sock.on("message", () => { out.udpMessage = s(); sock.close(); done(); }));
  als.run("udp-bind", () => sock.bind(0, "127.0.0.1", () => { out.udpBound = s(); sock.send("x", sock.address().port, "127.0.0.1"); }));
});
console.log(JSON.stringify(out));
"#,
    );
}

#[test]
fn async_local_storage_http2_tls_and_udp_callbacks_match_host_node() {
    run_isolated_builtin_conformance_test("als-http2-tls-udp");
}

fn async_local_storage_resource_lifecycle_matches_host_node_impl() {
    assert_async_context_conformance(
        "als-resource-lifecycle",
        r#"
import { AsyncLocalStorage } from "node:async_hooks";
import net from "node:net"; import http from "node:http"; import cp from "node:child_process";
const als = new AsyncLocalStorage(); const s = () => als.getStore() ?? null; const out = {};
const sleep = (ms) => new Promise((r) => setTimeout(r, ms));
{ const d = new AsyncLocalStorage(); d.run(1, () => { d.disable(); d.exit(() => {}); out.disableThenExit = d.getStore() ?? null; }); }
{ const d = new AsyncLocalStorage(); d.run(1, () => { d.disable(); d.enterWith(3); out.disableThenEnter = d.getStore() ?? null; }); }
{ const d = new AsyncLocalStorage(); const p = d.run("other", async () => { await sleep(5); return d.getStore() ?? null; }); d.run(1, () => d.disable()); d.run(9, () => {}); out.disableThenReenableOther = await p; }
await new Promise((done) => { const keep = setInterval(() => {}, 5); als.run("prep", () => process.prependOnceListener("SIGHUP", () => { out.signalPrependOnce = s(); clearInterval(keep); done(); })); process.kill(process.pid, "SIGHUP"); });
await new Promise((done) => { const keep = setInterval(() => {}, 5); als.run("prep2", () => process.prependListener("SIGHUP", function f() { process.removeListener("SIGHUP", f); out.signalPrepend = s(); clearInterval(keep); done(); })); process.kill(process.pid, "SIGHUP"); });
await new Promise((done) => { const keep = setInterval(() => {}, 5); const a = () => {}; als.run("A", () => process.on("SIGHUP", a)); process.removeListener("SIGHUP", a); als.run("B", () => process.once("SIGHUP", () => { out.signalAfterRemove = s(); clearInterval(keep); done(); })); process.kill(process.pid, "SIGHUP"); });
const server = als.run("srv", () => net.createServer((c) => { c.on("error", () => {}); c.on("data", () => {}); }));
await new Promise((r) => als.run("listen", () => server.listen(0, "127.0.0.1", r)));
const port = server.address().port;
await new Promise((done) => { const c = als.run("client", () => net.connect(port, "127.0.0.1")); c.on("connect", () => { als.run("destroyer", () => c.destroy()); }); c.on("close", () => { out.closeAfterLocalDestroy = s(); done(); }); });
await new Promise((done) => { const c = als.run("client2", () => net.connect(port, "127.0.0.1")); c.on("error", () => { out.errorAfterDestroyErr = s(); }); c.on("connect", () => { als.run("destroyer2", () => c.destroy(new Error("x"))); }); c.on("close", () => { out.closeAfterDestroyErr = s(); done(); }); });
await new Promise((done) => { const c = als.run("client3", () => net.connect(port, "127.0.0.1")); c.on("connect", () => { als.run("ender", () => c.end()); }); c.on("finish", () => { out.finishAfterEnd = s(); }); c.on("close", () => { out.closeAfterEnd = s(); done(); }); c.resume(); setTimeout(() => c.destroySoon?.(), 20); });
await new Promise((done) => { const c = als.run("client4", () => net.connect(port, "127.0.0.1")); als.run("timeout-setter", () => c.setTimeout(5)); c.on("timeout", () => { out.socketTimeout = s(); c.destroy(); }); c.on("close", done); });
await new Promise((done) => { const c = als.run("client5", () => net.connect(1, "127.0.0.1")); c.on("error", () => { out.connectError = s(); }); c.on("close", () => done()); });
await new Promise((done) => { als.run("closer", () => server.close(() => { out.serverCloseCallback = s(); done(); })); server.on("close", () => { out.serverCloseEvent = s(); }); });
await new Promise((done) => { const ch = als.run("child", () => cp.spawn("node", ["-e", "setTimeout(()=>{},5000)"])); ch.on("exit", () => { out.childExitAfterKill = s(); }); ch.on("close", () => { out.childCloseAfterKill = s(); done(); }); setTimeout(() => als.run("killer", () => ch.kill()), 50); });
await new Promise((done) => { const hs = http.createServer((q, r) => { r.end("ok"); }); hs.listen(0, "127.0.0.1", () => { const rq = als.run("hreq", () => http.get({ host: "127.0.0.1", port: hs.address().port, agent: false })); rq.on("response", (r) => { r.resume(); als.run("hdestroyer", () => rq.destroy()); }); rq.on("close", () => { out.httpCloseAfterDestroy = s(); hs.close(); hs.closeAllConnections?.(); done(); }); }); });
out.final = s();
console.log(JSON.stringify(out));
process.exit(0);
"#,
    );
}

#[test]
fn async_local_storage_resource_lifecycle_matches_host_node() {
    run_isolated_builtin_conformance_test("als-resource-lifecycle");
}

// A store entered at the top level follows the main module's own async work.
fn async_local_storage_top_level_enter_with_matches_host_node_impl() {
    assert_async_context_conformance(
        "als-top-level-enter-with",
        r#"
import { AsyncLocalStorage } from "node:async_hooks";
import net from "node:net";

const als = new AsyncLocalStorage();
const store = () => als.getStore() ?? null;
const out = {};

als.enterWith("main");
await null;
out.afterAwait = store();

await new Promise((done) => {
  const server = net.createServer((accepted) => {
    out.connection = store();
    accepted.on("data", () => accepted.end());
    accepted.on("close", () => {
      server.close();
      done();
    });
  });
  server.listen(0, "127.0.0.1", () => {
    const client = net.connect(server.address().port, "127.0.0.1", () => client.end("x"));
    client.resume();
  });
});

await new Promise((resolve) => setTimeout(resolve, 1));
out.afterTimer = store();
console.log(JSON.stringify(out));
"#,
    );
}

#[test]
fn async_local_storage_top_level_enter_with_matches_host_node() {
    run_isolated_builtin_conformance_test("als-top-level-enter-with");
}

// Pooled sockets, shared sockets, and aborts: a call must never read
// the store of the call it shares an object with.
fn async_local_storage_shared_objects_match_host_node_impl() {
    assert_async_context_conformance(
        "als-shared-objects",
        r#"
import { AsyncLocalStorage } from "node:async_hooks";
import http from "node:http"; import net from "node:net";
const als = new AsyncLocalStorage(); const s = () => als.getStore() ?? null; const out = {};
setTimeout(() => { console.log(JSON.stringify({ stuckAfter: Object.keys(out).at(-1), out })); process.exit(3); }, 8000).unref();
const sleep = (ms) => new Promise((r) => setTimeout(r, ms));
// A raw responder, because a kept-alive socket needs a server that answers every
// request on one connection.
const server = net.createServer((c) => {
  c.on("error", () => {});
  c.on("data", () => c.write("HTTP/1.1 200 OK\r\nContent-Length: 2\r\n\r\nok"));
});
await new Promise((r) => server.listen(0, "127.0.0.1", r));
const port = server.address().port;
const get = (agent, tag) => new Promise((done) => {
  const q = http.get({ host: "127.0.0.1", port, agent }, (res) => {
    out[tag + "Response"] = s(); res.resume(); res.on("end", () => { out[tag + "End"] = s(); done(); });
  });
  q.on("socket", () => { out[tag + "Socket"] = s(); });
});
// Queued on a pool of one: D waits for C's socket.
{ const agent = new http.Agent({ maxSockets: 1, keepAlive: true }); await Promise.all([als.run("C", () => get(agent, "queuedC")), als.run("D", () => get(agent, "queuedD"))]); agent.destroy(); }
// Kept-alive socket reused by a later call.
{ const agent = new http.Agent({ keepAlive: true }); await als.run("first", () => get(agent, "reuseFirst")); await sleep(5); await als.run("second", () => get(agent, "reuseSecond")); agent.destroy(); }
// Abort from another context.
await new Promise((done) => { const q = als.run("hq", () => http.get({ host: "127.0.0.1", port, agent: false })); q.on("error", () => { out.abortError = s(); }); q.on("close", () => { out.abortClose = s(); done(); }); als.run("aborter", () => q.destroy()); });
await new Promise((done) => {
  const responder = net.createServer((c) => {
    c.on("error", () => {});
    c.once("data", () => c.write("HTTP/1.1 200 OK\r\nContent-Length: 1\r\n\r\nx"));
  });
  responder.listen(0, "127.0.0.1", () => {
    const q = als.run("hq2", () => http.get({ host: "127.0.0.1", port: responder.address().port, agent: false }, (res) => {
      res.on("error", () => {});
      res.on("close", () => {
        out.resClose = s();
        responder.close();
        done();
      });
      als.run("aborter2", () => q.destroy());
    }));
    q.on("error", () => {});
  });
});
// Write callbacks on one socket from two contexts.
{ const srv = net.createServer((c) => { c.resume(); c.on("error", () => {}); }); await new Promise((r) => srv.listen(0, "127.0.0.1", r));
  const c = net.connect(srv.address().port, "127.0.0.1"); await new Promise((r) => c.on("connect", r));
  await Promise.all([als.run("writeA", () => new Promise((r) => c.write("a", () => { out.writeCbA = s(); r(); }))), als.run("writeB", () => new Promise((r) => c.write("b", () => { out.writeCbB = s(); r(); })))]);
  await sleep(2); await als.run("writeC", () => new Promise((r) => c.write("c", () => { out.writeCbLater = s(); r(); })));
  await als.run("endD", () => new Promise((r) => c.end("d", () => { out.endCb = s(); r(); }))); c.destroy(); srv.close(); }
server.close();
out.final = s();
console.log(JSON.stringify(out));
process.exit(0);
"#,
    );
}

#[test]
fn async_local_storage_shared_objects_match_host_node() {
    run_isolated_builtin_conformance_test("als-shared-objects");
}

// Node 24 starts each event from the owner's context. Node 22's hook implementation
// lets the store bleed into the next event, so this pins the isolated behavior instead
// of comparing with whichever Node the host runs.
fn async_local_storage_store_does_not_bleed_between_events_impl() {
    let result = run_async_context_probe_script(
        "als-event-bleed",
        r#"
// A store entered in one event must not reach the next event on the same resource.
import { AsyncLocalStorage } from "node:async_hooks"; import net from "node:net";
const als = new AsyncLocalStorage(); const out = { connections: [], chunksRead: 0, chunksSeeingStore: 0 };
setTimeout(() => { console.log(JSON.stringify({ stuck: true, out })); process.exit(3); }, 5000).unref();
const server = net.createServer((c) => { out.connections.push(als.getStore() ?? null); als.enterWith("from-connection"); c.on("error", () => {}); c.resume(); c.on("close", () => { if (--open === 0) server.close(() => done2()); }); });
let open = 3; let done2; const closed = new Promise((r) => { done2 = r; });
await new Promise((r) => server.listen(0, "127.0.0.1", r));
for (let i = 0; i < 3; i++) { const c = net.connect(server.address().port, "127.0.0.1", () => c.end("x")); c.resume(); }
await closed;
// One large write arrives as many chunks, so one read pump delivers several in a row.
await new Promise((done) => {
  const srv = net.createServer((c) => {
    c.on("data", () => {
      out.chunksRead += 1;
      if (als.getStore() !== undefined) out.chunksSeeingStore += 1;
      als.enterWith("from-chunk");
    });
    c.on("end", () => { c.end(); srv.close(); done(); });
  });
  srv.listen(0, "127.0.0.1", () => {
    const c = net.connect(srv.address().port, "127.0.0.1", () => c.end(Buffer.alloc(4 * 1024 * 1024)));
    c.resume();
  });
});
out.readManyChunks = out.chunksRead > 2;
delete out.chunksRead;
console.log(JSON.stringify(out)); process.exit(0);
"#,
    );
    assert_eq!(
        result,
        json!({ "connections": [null, null, null], "chunksSeeingStore": 0, "readManyChunks": true })
    );
}

#[test]
fn async_local_storage_store_does_not_bleed_between_events() {
    run_isolated_builtin_conformance_test("als-event-bleed");
}

// The VM reports Node 22, and Node 24 changed these results, so host Node cannot be
// the reference. Expected values come from `node --no-async-context-frame`, the
// Node 22 implementation.
fn async_local_storage_disable_follows_node_22_impl() {
    let result = run_async_context_probe_script(
        "als-disable-node-22",
        r#"
import { AsyncLocalStorage } from "node:async_hooks";

const sleep = (ms) => new Promise((resolve) => setTimeout(resolve, ms));
const out = {};

const reenabled = new AsyncLocalStorage();
reenabled.run(1, () => {
  reenabled.disable();
  reenabled.run(2, () => {});
  out.storeReturnsAfterRunReenables = reenabled.getStore() ?? null;
});

const shared = new AsyncLocalStorage();
const other = shared.run("other", async () => {
  await sleep(5);
  return shared.getStore() ?? null;
});
shared.run(1, () => shared.disable());
out.disableReachesOtherContexts = await other;

console.log(JSON.stringify(out));
"#,
    );
    assert_eq!(
        result,
        json!({ "storeReturnsAfterRunReenables": 1, "disableReachesOtherContexts": null })
    );
}

#[test]
fn async_local_storage_disable_follows_node_22() {
    run_isolated_builtin_conformance_test("als-disable-node-22");
}

// Host Node cannot share this harness's piped stdin, so this one has fixed
// expectations taken from host Node 24 (`echo x | node probe.mjs`). Stdin events run
// in the context of the first read of `process.stdin`.
fn run_async_context_stdin_probe(case_name: &str, stdin_read: &str, expected_owner: Value) {
    assert_node_available();

    let cwd = temp_dir(&format!("builtin-{case_name}"));
    let entrypoint = cwd.join("entry.mjs");
    write_fixture(
        &entrypoint,
        r#"
import { AsyncLocalStorage } from "node:async_hooks";

const als = new AsyncLocalStorage();
const store = () => als.getStore() ?? null;
const out = {};

const overlapping = als.run("overlapping", async () => {
  await new Promise((resolve) => setTimeout(resolve, 200));
  return store();
});

await new Promise((resolve) => {
  STDIN_READ
  als.run("stdin-owner", () => {
    stdin().on("data", () => {
      out.stdinData = store();
      stdin().pause();
      resolve();
    });
  });
  process.stdout.write("__READY__\n");
});
out.overlapping = await overlapping;
out.final = store();
console.log(JSON.stringify(out));
"#
        .replace("STDIN_READ", stdin_read),
    );

    let mut sidecar = new_sidecar(case_name);
    let connection_id = authenticate_wire(&mut sidecar, case_name);
    let session_id = open_session_wire(&mut sidecar, 2, &connection_id);
    let vm_id = create_vm_with_metadata_and_permissions(
        &mut sidecar,
        3,
        &connection_id,
        &session_id,
        GuestRuntimeKind::JavaScript,
        &cwd,
        HashMap::from([
            (
                String::from("env.AGENTOS_ALLOWED_NODE_BUILTINS"),
                serde_json::to_string(ASYNC_CONTEXT_PROBE_BUILTINS)
                    .expect("serialize builtin allowlist"),
            ),
            (
                String::from("env.AGENTOS_KEEP_STDIN_OPEN"),
                String::from("1"),
            ),
        ]),
        wire_permissions_allow_all(),
    );
    execute_wire(
        &mut sidecar,
        4,
        &connection_id,
        &session_id,
        &vm_id,
        "proc-als-stdin",
        GuestRuntimeKind::JavaScript,
        &entrypoint,
        Vec::new(),
    );
    let ownership = wire_session(&connection_id, &session_id);
    let deadline = Instant::now() + Duration::from_secs(20);
    let mut stdout = String::new();
    let mut stderr = String::new();
    let mut exit_code = None;
    let mut stdin_sent = false;
    while exit_code.is_none() {
        assert!(
            Instant::now() < deadline,
            "timed out waiting for stdin probe\nstdout:\n{stdout}\nstderr:\n{stderr}"
        );
        if let Some(event) = sidecar
            .poll_event_wire_blocking(&ownership, Duration::from_millis(50))
            .expect("poll stdin probe wire event")
        {
            match event.payload {
                EventPayload::ProcessOutputEvent(output) => match output.channel {
                    StreamChannel::Stdout => append_probe_output(
                        &mut stdout,
                        &output.chunk,
                        &output.process_id,
                        "stdout",
                    ),
                    StreamChannel::Stderr => append_probe_output(
                        &mut stderr,
                        &output.chunk,
                        &output.process_id,
                        "stderr",
                    ),
                },
                EventPayload::ProcessExitedEvent(exited) => exit_code = Some(exited.exit_code),
                _ => {}
            }
        }
        if !stdin_sent && stdout.contains("__READY__\n") {
            write_process_stdin(
                &mut sidecar,
                5,
                &connection_id,
                &session_id,
                &vm_id,
                "proc-als-stdin",
                "x\n",
            );
            stdin_sent = true;
        }
    }
    dispose_vm_and_close_session_wire(&mut sidecar, &connection_id, &session_id, &vm_id);

    assert_eq!(
        exit_code,
        Some(0),
        "stdin probe failed\nstdout:\n{stdout}\nstderr:\n{stderr}"
    );
    let payload: Value = serde_json::from_str(stdout.replace("__READY__\n", "").trim())
        .expect("parse stdin probe JSON");
    assert_eq!(
        payload,
        json!({ "stdinData": expected_owner, "overlapping": "overlapping", "final": null })
    );
}

fn async_local_storage_stdin_keeps_owner_context_impl() {
    run_async_context_stdin_probe(
        "als-stdin",
        "const stdin = () => process.stdin;",
        json!("stdin-owner"),
    );
}

fn async_local_storage_stdin_owner_is_first_read_impl() {
    run_async_context_stdin_probe(
        "als-stdin-outer-owner",
        "const firstRead = process.stdin; const stdin = () => firstRead;",
        Value::Null,
    );
}

#[test]
fn async_local_storage_stdin_keeps_owner_context() {
    run_isolated_builtin_conformance_test("als-stdin");
}

#[test]
fn async_local_storage_stdin_owner_is_first_read() {
    run_isolated_builtin_conformance_test("als-stdin-outer-owner");
}

#[test]
fn __builtin_conformance_extra_test_runner() {
    let Ok(test_name) = std::env::var("AGENTOS_BUILTIN_CONFORMANCE_EXTRA_TEST") else {
        return;
    };

    match test_name.as_str() {
        "als-overlapping-calls" => async_local_storage_isolates_overlapping_calls_impl(),
        "als-api" => async_local_storage_api_matches_host_node_impl(),
        "als-runtime-callbacks" => async_local_storage_runtime_callbacks_match_host_node_impl(),
        "als-http2-tls-udp" => {
            async_local_storage_http2_tls_and_udp_callbacks_match_host_node_impl()
        }
        "als-resource-lifecycle" => async_local_storage_resource_lifecycle_matches_host_node_impl(),
        "als-disable-node-22" => async_local_storage_disable_follows_node_22_impl(),
        "als-shared-objects" => async_local_storage_shared_objects_match_host_node_impl(),
        "als-event-bleed" => async_local_storage_store_does_not_bleed_between_events_impl(),
        "als-top-level-enter-with" => {
            async_local_storage_top_level_enter_with_matches_host_node_impl()
        }
        "als-stdin" => async_local_storage_stdin_keeps_owner_context_impl(),
        "als-stdin-outer-owner" => async_local_storage_stdin_owner_is_first_read_impl(),
        "http-request-keepalive" => http_request_custom_agent_reuses_keepalive_socket_impl(),
        "http-request-denied" => http_request_denied_egress_returns_permission_error_impl(),
        "loopback-connect-refused" => loopback_connect_without_vm_listener_is_refused_impl(),
        "child-process-fork-ipc" => child_process_fork_supports_basic_ipc_impl(),
        "http-socket-writes" => http_socket_writes_do_not_silently_drop_data_impl(),
        "buffer-concat-truncation" => buffer_concat_truncation_matches_host_node_impl(),
        "guest-sqlite-host-files" => guest_sqlite_cannot_reach_host_files_impl(),
        "mkdtemp-sync-collision-safe" => mkdtemp_sync_collision_safe_matches_host_node_impl(),
        "crypto-basic-fixture" => crypto_basic_fixture_matches_shared_expected_impl(),
        "crypto-extended" => crypto_extended_surface_matches_host_node(),
        "child-process-exec-spawn-error-code" => {
            child_process_exec_preserves_spawn_error_codes_impl()
        }
        "child-process-native-elf-reject" => {
            child_process_rejects_native_elf_binaries_before_wasm_compile_impl()
        }
        "child-process-kill-numeric-signal" => {
            child_process_kill_numeric_signals_match_host_node_impl()
        }
        "child-process-abort-signal" => child_process_abort_reports_sigabrt_impl(),
        "net-socket-readable-state" => net_socket_readable_state_tracks_ssh2_writable_shape_impl(),
        "net-large-socket-write" => {
            net_socket_writes_larger_than_socket_buffer_match_host_node_impl()
        }
        "readable-on-data-explicit-pause" => {
            readable_on_data_respects_explicit_pause_matches_host_node_impl()
        }
        "readline-question" => readline_question_reads_real_stdin_impl(),
        "vm-is-context" => vm_is_context_only_accepts_create_context_tagged_sandboxes_impl(),
        "vm-context-isolation" => vm_context_isolation_and_script_options_match_host_node_impl(),
        "vm-optional-surface" => {
            vm_optional_surface_is_implemented_or_explicitly_not_implemented_impl()
        }
        "vm-timeout" => vm_timeout_terminates_within_deadline_impl(),
        "perf-hooks-observer" => perf_hooks_observer_and_histogram_match_host_node_impl(),
        "process-runtime-stats" => process_runtime_stats_are_live_impl(),
        "os-resource-limits" => os_resource_limits_are_vm_scoped_impl(),
        "timer-handle-ref-refresh" => timer_handle_ref_refresh_matches_host_node_impl(),
        "timer-unref-exit" => unrefd_timeout_does_not_keep_guest_process_alive_impl(),
        "http-server-unref-exit" => unrefd_http_server_does_not_keep_guest_process_alive_impl(),
        "fs-write-file-numeric-fd" => write_file_sync_numeric_fd_matches_host_node_impl(),
        "fs-rm-symlinks" => rm_does_not_follow_symlinks_matches_host_node_impl(),
        "process-chdir-cwd" => {
            process_chdir_moves_relative_paths_and_children_matches_host_node_impl()
        }
        "fs-cp-symlinks" => cp_copies_symlinks_as_links_matches_host_node_impl(),
        "fs-dirent-types" => readdir_dirent_types_match_host_node_impl(),
        "fs-rm-directory-without-recursive" => {
            rm_directory_without_recursive_matches_host_node_impl()
        }
        other => panic!("unknown builtin conformance extra test: {other}"),
    }
}
