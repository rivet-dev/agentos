use super::*;

#[test]
fn actor_budget_json_is_validated_before_startup_and_supports_inline_syntax() {
    let json = r#"{"admission":{"maxConcurrentActions":80},"transport":{"maxMessageBytes":4194304},"codec":{"maxBinaryBytes":2097152},"filesystem":{"maxTransferBytes":2097152}}"#;
    for args in [
        vec!["--actor-budgets-json".to_owned(), json.to_owned()],
        vec![format!("--actor-budgets-json={json}")],
    ] {
        let parsed = parse_actor_options(args.into_iter()).unwrap();
        assert_eq!(
            parsed.actor_budgets.transport.max_message_bytes,
            Some(4 * 1024 * 1024)
        );
        assert_eq!(
            parsed.actor_budgets.filesystem.max_transfer_bytes,
            Some(2 * 1024 * 1024)
        );
    }
    for json in [
        r#"{"admission":{"maxConcurrentActions":0}}"#,
        r#"{"transport":{"maxMessageBytes":18446744073709551615}}"#,
        r#"{"filesystem":{"unknown":1}}"#,
        "not json",
    ] {
        let error =
            match parse_actor_options(vec![format!("--actor-budgets-json={json}")].into_iter()) {
                Ok(_) => panic!("invalid budget JSON was accepted"),
                Err(error) => error,
            };
        assert!(error.contains("--actor-budgets-json"), "{error}");
    }
}
