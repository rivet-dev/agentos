use std::io::ErrorKind;

fn assert_unsupported(result: Result<(), ctrlc::Error>) {
    match result {
        Err(ctrlc::Error::System(error)) => assert_eq!(error.kind(), ErrorKind::Unsupported),
        Err(ctrlc::Error::MultipleHandlers) => panic!("unexpected multiple handlers error"),
        Ok(()) => panic!("signal registration unexpectedly succeeded"),
    }
}

#[test]
fn set_handler_reports_unsupported() {
    assert_unsupported(ctrlc::set_handler(|| {}));
}

#[test]
fn try_set_handler_reports_unsupported() {
    assert_unsupported(ctrlc::try_set_handler(|| {}));
}
