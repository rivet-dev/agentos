//! Observed terminal status, captured before process cleanup consumes its owner.
use crate::VmError;
use agentos_vm_kernel::process_runtime::ProcessExit;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct ObservedProcessExit {
    pub(crate) exit_code: i32,
    pub(crate) exit_signal: Option<u32>,
}

impl ObservedProcessExit {
    pub(crate) fn new(exit_code: i32, exit_signal: Option<i32>) -> Result<Self, VmError> {
        let exit_signal = exit_signal
            .map(|signal| {
                if !(1..=64).contains(&signal) {
                    return Err(VmError::host(
                        "EINVAL",
                        format!("observed process exit signal {signal} is outside 1..=64"),
                    ));
                }
                Ok(signal as u32)
            })
            .transpose()?;
        Ok(Self {
            exit_code,
            exit_signal,
        })
    }

    pub(crate) fn from_kernel(exit: ProcessExit) -> Result<Self, VmError> {
        match exit {
            ProcessExit::Exited(code) => Self::new(code, None),
            ProcessExit::Signaled { signal, .. } => {
                // Validate before addition: malformed executor status must not overflow.
                Self::new(0, Some(signal)).map(|status| Self {
                    exit_code: 128 + signal,
                    ..status
                })
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn observed_signal_is_not_inferred_from_shell_status() {
        assert_eq!(
            ObservedProcessExit::from_kernel(ProcessExit::Exited(143)).unwrap(),
            ObservedProcessExit {
                exit_code: 143,
                exit_signal: None
            }
        );
        assert_eq!(
            ObservedProcessExit::from_kernel(ProcessExit::Signaled {
                signal: 15,
                core_dumped: false,
            })
            .unwrap(),
            ObservedProcessExit {
                exit_code: 143,
                exit_signal: Some(15)
            }
        );
        assert_eq!(
            ObservedProcessExit::from_kernel(ProcessExit::Exited(0))
                .unwrap()
                .exit_signal,
            None
        );
    }

    #[test]
    fn observed_signal_range_is_validated() {
        for signal in [i32::MIN, -1, 0, 65, i32::MAX] {
            assert_eq!(
                ObservedProcessExit::new(143, Some(signal))
                    .unwrap_err()
                    .code(),
                Some("EINVAL")
            );
        }
        for signal in [1, 15, 31, 32, 64] {
            assert_eq!(
                ObservedProcessExit::new(128 + signal, Some(signal))
                    .unwrap()
                    .exit_signal,
                Some(signal as u32)
            );
        }
    }
}
