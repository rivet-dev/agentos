//! A sidecar started with the default runtime config must hold real VM density:
//! one live guest executor per VM, far more than one per CPU. The default is
//! still bounded, and the start past it fails with a typed error that names
//! the field that raises it.

use agentos_driver_tokio::{DriverConfig, TokioDriver, VmExecutorAdmissionError};

/// 0.4.9 held 672 live VMs per sidecar on a 16-vCPU GKE node.
const MEASURED_VMS_PER_SIDECAR: usize = 672;

#[test]
fn default_config_admits_measured_density_and_rejects_past_its_bound() {
    let driver = TokioDriver::process(&DriverConfig::default()).expect("default driver");
    let admission = driver.handle().vm_executor_admission().clone();

    let mut live = Vec::new();
    let error = loop {
        match admission.try_acquire() {
            Ok(permit) => live.push(permit),
            Err(error) => break error,
        }
        assert!(
            live.len() <= 1 << 20,
            "default executor admission is unbounded"
        );
    };

    assert!(
        live.len() >= MEASURED_VMS_PER_SIDECAR,
        "default admits only {} live guest executors; the 17th VM on a 16-CPU host must start",
        live.len()
    );
    assert_eq!(
        error,
        VmExecutorAdmissionError::Limit {
            active: live.len(),
            maximum: live.len(),
        }
    );
    assert_eq!(error.code(), "ERR_AGENTOS_VM_EXECUTOR_LIMIT");
    assert!(error.to_string().contains("runtime.executor.maxActiveVms"));

    // A VM that exits frees its slot for the next start.
    live.pop();
    admission
        .try_acquire()
        .expect("freed slot admits the next VM");
}
