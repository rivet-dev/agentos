mod common;
use agentos_client::fs::FileContent;

#[tokio::test]
async fn positional_file_reads_retain_byte_bounds_and_path_guards() {
    assert!(
        common::sidecar_available(),
        "set AGENTOS_SIDECAR_BIN to a built sidecar"
    );
    let vm = common::new_vm_with_sidecar_pool("file-range-control").await;
    vm.write_file(
        "/tmp/range",
        FileContent::Bytes(b"0123456789abcdef".to_vec()),
    )
    .await
    .expect("write data");
    assert_eq!(
        vm.read_file_range("/tmp/range", 3, 5).await.expect("range"),
        b"34567"
    );
    assert!(
        vm.read_file_range("/tmp/range", 16, 8)
            .await
            .expect("EOF")
            .is_empty()
    );
    assert!(vm.read_file_range("relative", 0, 1).await.is_err());
    assert!(vm.read_file_range("/tmp/range", u64::MAX, 1).await.is_err());
    vm.shutdown().await.expect("shutdown VM");
}
