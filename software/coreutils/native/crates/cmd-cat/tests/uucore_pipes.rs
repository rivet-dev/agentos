#![cfg(any(target_os = "linux", target_os = "android"))]

use std::fs::OpenOptions;

use uucore::pipes::{Error, pipe, splice_exact};

#[test]
fn splice_exact_returns_error_on_unexpected_eof() {
    let (pipe_rd, pipe_wr) = pipe().unwrap();
    drop(pipe_wr);

    let dest = OpenOptions::new().write(true).open("/dev/null").unwrap();

    assert_eq!(splice_exact(&pipe_rd, &dest, 1), Err(Error::EPIPE));
}
