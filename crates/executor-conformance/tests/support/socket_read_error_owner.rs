fn javascript_execution_v8_socket_read_reset_reaches_socket_owner() {
    // Run the detached case first so the unmodified bridge's late failure is
    // observed independently of the already-proven first-read close flag.
    run_socket_read_reset_owner_control(true);
    run_socket_read_reset_owner_control(false);
}

fn run_socket_read_reset_owner_control(late_readiness: bool) {
    use agentos_executor_contract::backend::{
        ExecutionBackend, ExecutionReadyFlags, ExecutionWakeIdentity,
    };

    let temp = tempdir().expect("create socket reset fixture");
    write_fixture(
        &temp.path().join("entry.mjs"),
        r#"
import net from "node:net";
import { AsyncLocalStorage } from "node:async_hooks";

const context = new AsyncLocalStorage();
const events = [];
const lateReadiness = process.env.TEST_SOCKET_LATE_RESET === "1";
let connected = 0;
let socket;
let unhandled;
await new Promise((resolve, reject) => {
  unhandled = (error) => {
    // A failing preimage must still release this fixture's live socket.
    socket.destroy();
    reject(new Error(
      `raw read escaped its socket after ${lateReadiness ? "readiness" : "connect"}: ${error?.code} ${error?.message}`,
      { cause: error },
    ));
  };
  process.on("unhandledRejection", unhandled);
  context.run("socket-owner", () => {
    socket = net.connect({ host: "127.0.0.1", port: 80 });
    socket.on("connect", () => {
      connected++;
      if (lateReadiness) {
        // Allow the connect pump and any initially queued _read to finish.
        setImmediate(() => {
          if (socket.connecting || socket._bridgeReadLoopRunning || socket._bridgeReadPumpQueued) {
            reject(new Error("readiness barrier reached with connect or read work still active"));
            return;
          }
          console.log("SOCKET_READ_WAKE_ARMED");
        });
      }
    });
    socket.on("error", (error) => {
      events.push(["error", error.code, error.message, context.getStore()]);
    });
    socket.on("close", (hadError) => {
      events.push(["close", hadError, context.getStore()]);
      resolve();
    });
    socket.resume();
  });
});
await new Promise((resolve) => setImmediate(resolve));
process.off("unhandledRejection", unhandled);
const expected = [
  ["error", "ECONNRESET", "controlled socket reset", "socket-owner"],
  ["close", true, "socket-owner"],
];
if (connected !== 1 || !socket.destroyed || JSON.stringify(events) !== JSON.stringify(expected)) {
  throw new Error("unexpected socket terminal events: " + JSON.stringify(events));
}
console.log("SOCKET_READ_RESET_OWNED");
"#,
    );

    let mut engine = support::javascript_engine();
    let context = engine.create_context(CreateJavascriptContextRequest {
        vm_id: String::from("vm-js"),
        bootstrap_module: None,
        compile_cache_root: None,
    });
    let mut execution = engine
        .start_execution(StartJavascriptExecutionRequest {
            limits: Default::default(),
            guest_runtime: Default::default(),
            vm_id: String::from("vm-js"),
            context_id: context.context_id,
            argv: vec![String::from("./entry.mjs")],
            argv0: None,
            env: BTreeMap::from([(
                String::from("TEST_SOCKET_LATE_RESET"),
                String::from(if late_readiness { "1" } else { "0" }),
            )]),
            cwd: temp.path().to_path_buf(),
            wasm_module_bytes: None,
            inline_code: None,
        })
        .expect("start socket reset execution");
    let deadline = std::time::Instant::now() + Duration::from_secs(5);
    let mut connects = 0;
    let mut connect_waits = 0;
    let mut startup_reads = 0;
    let mut reset_reads = 0;
    let mut destroys = 0;
    let mut readiness_published = false;
    let mut stdout = Vec::new();
    let mut stderr = Vec::new();
    let exit_code = loop {
        let remaining = deadline.saturating_duration_since(std::time::Instant::now());
        assert!(!remaining.is_zero(), "socket reset fixture deadline");
        match execution
            .poll_event_blocking(remaining)
            .expect("poll socket reset execution")
        {
            Some(JavascriptExecutionEvent::Stdout(bytes)) => {
                stdout.extend(bytes);
                if !readiness_published
                    && String::from_utf8_lossy(&stdout)
                        .lines()
                        .any(|line| line == "SOCKET_READ_WAKE_ARMED")
                {
                    assert!(late_readiness, "first-read case must not publish readiness");
                    assert_eq!(connects, 1);
                    assert_eq!(connect_waits, 1);
                    assert!(startup_reads > 0, "startup must return would-block first");
                    let wake = execution
                        .wake_handle(ExecutionWakeIdentity {
                            generation: 1,
                            pid: 1,
                        })
                        .expect("socket reset execution wake handle");
                    wake.publish_readiness(100, 1, ExecutionReadyFlags::READABLE)
                        .expect("publish late socket readiness");
                    readiness_published = true;
                }
            }
            Some(JavascriptExecutionEvent::Stderr(bytes)) => stderr.extend(bytes),
            Some(JavascriptExecutionEvent::SignalState { .. }) => {}
            Some(JavascriptExecutionEvent::LimitExceeded(limit)) => {
                panic!("unexpected socket reset fixture limit: {limit:?}");
            }
            Some(JavascriptExecutionEvent::SyncRpcRequest(request)) => {
                if execution
                    .try_service_standalone_module_sync_rpc(&request)
                    .expect("service module sync RPC")
                {
                    continue;
                }
                let response = match request.method.as_str() {
                    "net.connect" => {
                        connects += 1;
                        json!({
                            "socketId": 1, "capabilityId": 100, "capabilityGeneration": 1,
                            "localAddress": "127.0.0.1", "localPort": 12345, "localFamily": "IPv4",
                            "remoteAddress": "127.0.0.1", "remotePort": 80, "remoteFamily": "IPv4",
                        })
                    }
                    "net.socket_wait_connect" => {
                        connect_waits += 1;
                        json!(
                            "{\"localAddress\":\"127.0.0.1\",\"localPort\":12345,\"localFamily\":\"IPv4\",\
                             \"remoteAddress\":\"127.0.0.1\",\"remotePort\":80,\"remoteFamily\":\"IPv4\"}"
                        )
                    }
                    "net.socket_read" if late_readiness && !readiness_published => {
                        startup_reads += 1;
                        assert!(
                            startup_reads <= 4,
                            "startup must stop polling without readiness"
                        );
                        json!("__agentos_net_timeout__")
                    }
                    "net.socket_read" => {
                        assert_eq!(connects, 1);
                        assert_eq!(connect_waits, 1);
                        reset_reads += 1;
                        assert_eq!(reset_reads, 1, "a reset must stop further reads");
                        execution
                            .respond_sync_rpc_error(
                                request.id,
                                "ECONNRESET",
                                "controlled socket reset",
                            )
                            .expect("respond with structured socket error");
                        continue;
                    }
                    "net.socket_set_read_interest" => Value::Null,
                    "net.destroy" => {
                        destroys += 1;
                        Value::Null
                    }
                    other => panic!("unexpected socket reset RPC: {other}"),
                };
                execution
                    .respond_sync_rpc_success(request.id, response)
                    .expect("respond to socket reset fixture RPC");
            }
            Some(JavascriptExecutionEvent::Exited(code)) => break code,
            None => panic!(
                "socket reset fixture deadline: late={late_readiness}; connects={connects}; \
                 connect_waits={connect_waits}; startup_reads={startup_reads}; \
                 readiness_published={readiness_published}; reset_reads={reset_reads}; \
                 destroys={destroys}; stdout={}; stderr={}",
                String::from_utf8_lossy(&stdout),
                String::from_utf8_lossy(&stderr),
            ),
        }
    };
    assert_eq!(
        exit_code,
        0,
        "late={late_readiness}; connects={connects}; connect_waits={connect_waits}; \
         startup_reads={startup_reads}; readiness_published={readiness_published}; \
         reset_reads={reset_reads}; destroys={destroys}; stderr: {}",
        String::from_utf8_lossy(&stderr)
    );
    assert_eq!(connects, 1);
    assert_eq!(connect_waits, 1);
    assert_eq!(readiness_published, late_readiness);
    assert_eq!(startup_reads > 0, late_readiness);
    assert_eq!(reset_reads, 1);
    assert_eq!(destroys, 1, "release the socket capability exactly once");
    assert!(
        stderr.is_empty(),
        "stderr: {}",
        String::from_utf8_lossy(&stderr)
    );
    let stdout = String::from_utf8_lossy(&stdout);
    let expected_stdout = if late_readiness {
        "SOCKET_READ_WAKE_ARMED\nSOCKET_READ_RESET_OWNED"
    } else {
        "SOCKET_READ_RESET_OWNED"
    };
    assert_eq!(stdout.trim(), expected_stdout);
}
