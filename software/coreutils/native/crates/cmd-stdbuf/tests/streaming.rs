use std::io::{BufRead, BufReader};
use std::process::{Child, ChildStdout, Command, Stdio};
use std::sync::mpsc::{self, Receiver};
use std::thread;
use std::time::Duration;

struct ChildGuard(Child);

impl ChildGuard {
    fn new(child: Child) -> Self {
        Self(child)
    }

    fn child_mut(&mut self) -> &mut Child {
        &mut self.0
    }

    fn wait(mut self) -> std::io::Result<std::process::ExitStatus> {
        self.0.wait()
    }
}

impl Drop for ChildGuard {
    fn drop(&mut self) {
        if matches!(self.0.try_wait(), Ok(None)) {
            let _ = self.0.kill();
            let _ = self.0.wait();
        }
    }
}

fn spawn_line_reader(stdout: ChildStdout) -> Receiver<Option<String>> {
    let (tx, rx) = mpsc::channel();
    thread::spawn(move || {
        let mut reader = BufReader::new(stdout);
        loop {
            let mut line = String::new();
            match reader.read_line(&mut line) {
                Ok(0) => break,
                Ok(_) => {
                    if tx.send(Some(line)).is_err() {
                        return;
                    }
                }
                Err(error) => panic!("failed to read stdbuf stdout: {error}"),
            }
        }
        let _ = tx.send(None);
    });
    rx
}

fn spawn_stdbuf(script: &str) -> Child {
    Command::new(env!("CARGO_BIN_EXE_stdbuf"))
        .args(["-oL", "sh", "-c", script])
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::inherit())
        .spawn()
        .expect("failed to spawn stdbuf test command")
}

#[test]
fn stdbuf_preserves_line_streaming_for_child_output() {
    let script = r#"
printf 'line-1\n'
sleep 1
printf 'line-2\n'
"#;

    let mut child = ChildGuard::new(spawn_stdbuf(script));
    let stdout = child
        .child_mut()
        .stdout
        .take()
        .expect("missing stdbuf stdout");
    let rx = spawn_line_reader(stdout);

    let first_line = rx
        .recv_timeout(Duration::from_millis(500))
        .expect("expected stdbuf child to emit the first line before exit")
        .expect("stdbuf stdout closed before the first line");
    assert_eq!(first_line, "line-1\n");
    assert_eq!(
        child
            .child_mut()
            .try_wait()
            .expect("failed to poll stdbuf child"),
        None,
        "stdbuf child exited before the first line was observed"
    );

    let second_line = rx
        .recv_timeout(Duration::from_secs(2))
        .expect("expected stdbuf child to emit the second line")
        .expect("stdbuf stdout closed before the second line");
    assert_eq!(second_line, "line-2\n");

    let status = child.wait().expect("failed to wait for stdbuf child");
    assert!(status.success(), "stdbuf exited with {status:?}");
    assert_eq!(
        rx.recv_timeout(Duration::from_secs(1))
            .expect("expected stdbuf reader shutdown marker"),
        None
    );
}
