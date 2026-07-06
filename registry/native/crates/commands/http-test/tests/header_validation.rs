use cmd_http_test::parse_header;

#[test]
fn rejects_header_without_colon() {
    assert!(parse_header("X-Test").is_err());
}

#[test]
fn rejects_header_injection() {
    assert!(parse_header("X-Test: ok\r\nInjected: value").is_err());
}

#[test]
fn accepts_valid_header() {
    assert_eq!(
        parse_header("X-Test:  ok\t"),
        Ok(("X-Test".to_string(), "ok".to_string()))
    );
}
