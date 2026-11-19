use std::io::Write;
use std::process::{Child, Command, Output, Stdio};
use std::time::{Duration, Instant};

/// Runs awk with a stdin pipe that the test keeps open, as a caller that
/// never closes stdin does. Kills awk if it has not exited by the deadline.
fn run_with_open_stdin(args: &[&str], input: &[u8]) -> Option<Output> {
    let mut child = Command::new(env!("CARGO_BIN_EXE_awk"))
        .args(args)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawn awk");
    let mut stdin = child.stdin.take().expect("awk stdin");
    stdin.write_all(input).expect("write awk stdin");
    let deadline = Instant::now() + Duration::from_secs(5);
    while Instant::now() < deadline {
        if child.try_wait().expect("poll awk").is_some() {
            drop(stdin);
            return Some(child.wait_with_output().expect("collect awk output"));
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    kill(child);
    None
}

fn kill(mut child: Child) {
    let _ = child.kill();
    let _ = child.wait();
}

#[test]
fn begin_only_program_exits_while_stdin_stays_open() {
    let output = run_with_open_stdin(&[r#"BEGIN { print "AWK" }"#], b"")
        .expect("a BEGIN-only program must exit without reading stdin");
    assert!(output.status.success(), "{output:?}");
    assert_eq!(output.stdout, b"AWK\n");
}

#[test]
fn begin_only_program_does_not_open_its_operands() {
    let output = Command::new(env!("CARGO_BIN_EXE_awk"))
        .args([r#"BEGIN { print "x" }"#, "/nonexistent/input"])
        .stdin(Stdio::null())
        .output()
        .expect("run awk");
    assert!(output.status.success(), "{output:?}");
    assert_eq!(output.stdout, b"x\n");
}

#[test]
fn program_with_an_end_rule_still_reads_stdin_to_its_end() {
    let output = Command::new(env!("CARGO_BIN_EXE_awk"))
        .args(["BEGIN { print \"start\" } END { print NR }"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .and_then(|mut child| {
            child
                .stdin
                .take()
                .expect("awk stdin")
                .write_all(b"a\nb\n")?;
            child.wait_with_output()
        })
        .expect("run awk");
    assert!(output.status.success(), "{output:?}");
    assert_eq!(output.stdout, b"start\n2\n");
}
