use std::sync::Arc;
use std::time::Duration;

pub(crate) use agentos_actor_contract::process::*;
#[cfg(test)]
use agentos_client::{ProcessStatus, ProcessTreeNode};
use anyhow::{bail, Result};
use rivetkit::{Ctx, Handles};

use crate::actions::BoxFuture;
use crate::events::{ProcessExitEvent, ProcessOutputEvent, TerminalExitEvent, TerminalOutputEvent};
use crate::{AgentOsActor, FileBytes};

crate::register_contract_action!(ProcessRun);
crate::register_contract_action!(ProcessSpawn);
crate::register_contract_action!(ProcessGet);
crate::register_contract_action!(ProcessList);
crate::register_contract_action!(ProcessTree);
crate::register_contract_action!(ProcessWait);
crate::register_contract_action!(ProcessSignal);
crate::register_contract_action!(ProcessStdinWrite);
crate::register_contract_action!(ProcessStdinClose);
crate::register_contract_action!(ProcessPtyResize);
crate::register_contract_action!(ProcessOutputRead);
crate::register_contract_action!(TerminalOpen);
crate::register_contract_action!(TerminalList);
crate::register_contract_action!(TerminalOutputRead);
crate::register_contract_action!(TerminalStdinWrite);
crate::register_contract_action!(TerminalPtyResize);
crate::register_contract_action!(TerminalWait);
crate::register_contract_action!(TerminalClose);

impl Handles<ProcessRun> for AgentOsActor {
    type Future = BoxFuture<ActorExecResult>;
    fn handle(self: Arc<Self>, _ctx: Ctx<Self>, action: ProcessRun) -> Self::Future {
        Box::pin(async move {
            let _permit = self.admit_action()?;
            validate_command(&action.command)?;
            validate_arguments(&action.args)?;
            validate_exec_options(&action.options)?;
            // Acquiring the ready VM handle is part of the caller's run budget. Reserve the Core
            // cleanup window below the actor's six-minute action deadline.
            let deadline = tokio::time::Instant::now()
                + Duration::from_millis(action.options.timeout_ms.unwrap_or(MAX_WAIT_MS));
            let vm = tokio::time::timeout_at(deadline, self.runtime.vm())
                .await
                .map_err(|_| {
                    anyhow::anyhow!(
                        "timeout: process.run exceeded its deadline before execution started"
                    )
                })??;
            let remaining = deadline.saturating_duration_since(tokio::time::Instant::now());
            if remaining.is_zero() {
                bail!("timeout: process.run exceeded its deadline before execution started");
            }
            let mut options = exec_options_into_core(action.options);
            options.timeout = Some(remaining.as_secs_f64() * 1_000.0);
            let result = vm
                .exec_argv_process(&action.command, &action.args, options)
                .await?;
            actor_exec_result(result)
        })
    }
}

impl Handles<ProcessSpawn> for AgentOsActor {
    type Future = BoxFuture<ActorProcessId>;
    fn handle(self: Arc<Self>, ctx: Ctx<Self>, action: ProcessSpawn) -> Self::Future {
        Box::pin(async move {
            let _permit = self.admit_action()?;
            validate_command(&action.command)?;
            validate_arguments(&action.args)?;
            validate_spawn_options(&action.options)?;
            let status = self.runtime.status().await;
            let vm = self.runtime.vm_at_generation(status.generation).await?;
            let handle = vm.spawn(
                &action.command,
                action.args,
                spawn_options_into_core(action.options),
            )?;
            let process = ActorProcessId {
                generation: status.generation,
                pid: handle.pid,
            };
            attach_process_events(&vm, ctx, process)?;
            Ok(process)
        })
    }
}

impl Handles<ProcessGet> for AgentOsActor {
    type Future = BoxFuture<ActorProcessInfo>;
    fn handle(self: Arc<Self>, _ctx: Ctx<Self>, action: ProcessGet) -> Self::Future {
        Box::pin(async move {
            let _permit = self.admit_action()?;
            let info = self
                .runtime
                .vm_at_generation(action.process.generation)
                .await?
                .get_process(action.process.pid)?;
            Ok(actor_process_info(action.process.generation, info))
        })
    }
}

impl Handles<ProcessList> for AgentOsActor {
    type Future = BoxFuture<Vec<ActorProcessInfo>>;
    fn handle(self: Arc<Self>, _ctx: Ctx<Self>, _action: ProcessList) -> Self::Future {
        Box::pin(async move {
            let _permit = self.admit_action()?;
            let status = self.runtime.status().await;
            let entries = self
                .runtime
                .vm_at_generation(status.generation)
                .await?
                .list_processes();
            validate_count(
                "process.list result",
                entries.len(),
                MAX_PROCESS_LIST_ENTRIES,
            )?;
            Ok(entries
                .into_iter()
                .map(|info| actor_process_info(status.generation, info))
                .collect())
        })
    }
}

impl Handles<ProcessTree> for AgentOsActor {
    type Future = BoxFuture<ActorProcessTree>;
    fn handle(self: Arc<Self>, _ctx: Ctx<Self>, _action: ProcessTree) -> Self::Future {
        Box::pin(async move {
            let _permit = self.admit_action()?;
            let status = self.runtime.status().await;
            let vm = self.runtime.vm_at_generation(status.generation).await?;
            let roots = vm.process_tree().await?;
            validate_count(
                "process.tree result",
                count_process_tree_nodes(&roots),
                MAX_PROCESS_LIST_ENTRIES,
            )?;
            Ok(ActorProcessTree {
                generation: status.generation,
                roots: roots
                    .into_iter()
                    .map(|node| actor_process_tree_node(node, status.generation))
                    .collect(),
            })
        })
    }
}

impl Handles<ProcessWait> for AgentOsActor {
    type Future = BoxFuture<ActorProcessExit>;
    fn handle(self: Arc<Self>, _ctx: Ctx<Self>, action: ProcessWait) -> Self::Future {
        Box::pin(async move {
            let _permit = self.admit_action()?;
            let vm = self
                .runtime
                .vm_at_generation(action.process.generation)
                .await?;
            let exit_code = tokio::time::timeout(
                Duration::from_millis(DEFAULT_WAIT_MS),
                vm.wait_process(action.process.pid),
            )
            .await
            .map_err(|_| {
                anyhow::anyhow!(
                    "timeout: process.wait exceeded {DEFAULT_WAIT_MS}ms; call again to continue waiting"
                )
            })??;
            Ok(ActorProcessExit {
                process: action.process,
                status: ActorExitStatus {
                    exit_code,
                    signal: None,
                },
            })
        })
    }
}

impl Handles<ProcessSignal> for AgentOsActor {
    type Future = BoxFuture<()>;
    fn handle(self: Arc<Self>, _ctx: Ctx<Self>, action: ProcessSignal) -> Self::Future {
        Box::pin(async move {
            let _permit = self.admit_action()?;
            self.runtime
                .vm_at_generation(action.process.generation)
                .await?
                .signal_process_awaited(action.process.pid, action.signal.as_str())
                .await?;
            Ok(())
        })
    }
}

impl Handles<ProcessStdinWrite> for AgentOsActor {
    type Future = BoxFuture<()>;
    fn handle(self: Arc<Self>, _ctx: Ctx<Self>, action: ProcessStdinWrite) -> Self::Future {
        Box::pin(async move {
            let _permit = self.admit_action()?;
            validate_bytes("process stdin", action.data.byte_len(), MAX_STDIN_BYTES)?;
            self.runtime
                .vm_at_generation(action.process.generation)
                .await?
                .write_process_stdin(action.process.pid, file_content_to_stdin(action.data))
                .await?;
            Ok(())
        })
    }
}

impl Handles<ProcessStdinClose> for AgentOsActor {
    type Future = BoxFuture<()>;
    fn handle(self: Arc<Self>, _ctx: Ctx<Self>, action: ProcessStdinClose) -> Self::Future {
        Box::pin(async move {
            let _permit = self.admit_action()?;
            self.runtime
                .vm_at_generation(action.process.generation)
                .await?
                .close_process_stdin(action.process.pid)
                .await?;
            Ok(())
        })
    }
}

impl Handles<ProcessPtyResize> for AgentOsActor {
    type Future = BoxFuture<()>;
    fn handle(self: Arc<Self>, _ctx: Ctx<Self>, action: ProcessPtyResize) -> Self::Future {
        Box::pin(async move {
            let _permit = self.admit_action()?;
            validate_dimensions(Some(action.cols), Some(action.rows))?;
            self.runtime
                .vm_at_generation(action.process.generation)
                .await?
                .resize_process_pty_awaited(action.process.pid, action.cols, action.rows)
                .await?;
            Ok(())
        })
    }
}

impl Handles<ProcessOutputRead> for AgentOsActor {
    type Future = BoxFuture<ActorOutputReplay>;
    fn handle(self: Arc<Self>, _ctx: Ctx<Self>, action: ProcessOutputRead) -> Self::Future {
        Box::pin(async move {
            let _permit = self.admit_action()?;
            let vm = self
                .runtime
                .vm_at_generation(action.process.generation)
                .await?;
            let replay = vm
                .read_process_output(
                    action.process.pid,
                    action.after,
                    action.max_events,
                    action.max_bytes,
                )
                .await?;
            Ok(ActorOutputReplay {
                generation: action.process.generation,
                events: replay
                    .events
                    .into_iter()
                    .map(|event| ActorOutputEvent {
                        sequence: event.sequence,
                        stream: event.stream,
                        data: FileBytes(event.data),
                        timestamp_ms: event.timestamp_ms,
                    })
                    .collect(),
                next_cursor: replay.next_cursor,
                has_more: replay.has_more,
                truncated: replay.truncated,
                end: exit_status(replay.exit_code),
            })
        })
    }
}

impl Handles<TerminalOpen> for AgentOsActor {
    type Future = BoxFuture<ActorTerminalId>;
    fn handle(self: Arc<Self>, ctx: Ctx<Self>, action: TerminalOpen) -> Self::Future {
        Box::pin(async move {
            let _permit = self.admit_action()?;
            validate_terminal_options(&action.options)?;
            let status = self.runtime.status().await;
            let vm = self.runtime.vm_at_generation(status.generation).await?;
            let handle = vm.open_shell(terminal_options_into_core(action.options))?;
            let terminal = ActorTerminalId {
                generation: status.generation,
                shell_id: handle.shell_id,
            };
            let output_ctx = ctx.clone();
            let output_terminal = terminal.clone();
            vm.on_shell_output(&terminal.shell_id, move |event| {
                if let Err(error) = output_ctx.emit(TerminalOutputEvent {
                    terminal: output_terminal.clone(),
                    sequence: event.sequence,
                    stream: event.stream.clone(),
                    data: FileBytes(event.data.clone()),
                    timestamp_ms: event.timestamp_ms,
                }) {
                    tracing::warn!(?error, shell_id = %output_terminal.shell_id, "emit terminal.output failed");
                }
            })?
            .detach();
            let exit_ctx = ctx;
            let exit_terminal = terminal.clone();
            vm.on_shell_exit(&terminal.shell_id, move |event| {
                if let Err(error) = exit_ctx.emit(TerminalExitEvent {
                    terminal: exit_terminal.clone(),
                    status: ActorExitStatus {
                        exit_code: event.exit_code,
                        signal: None,
                    },
                }) {
                    tracing::warn!(?error, shell_id = %exit_terminal.shell_id, "emit terminal.exit failed");
                }
            })?
            .detach();
            Ok(terminal)
        })
    }
}

impl Handles<TerminalList> for AgentOsActor {
    type Future = BoxFuture<Vec<ActorTerminalInfo>>;
    fn handle(self: Arc<Self>, _ctx: Ctx<Self>, _action: TerminalList) -> Self::Future {
        Box::pin(async move {
            let _permit = self.admit_action()?;
            let status = self.runtime.status().await;
            let terminals = self
                .runtime
                .vm_at_generation(status.generation)
                .await?
                .list_shells();
            validate_count(
                "terminal.list result",
                terminals.len(),
                MAX_PROCESS_LIST_ENTRIES,
            )?;
            Ok(terminals
                .into_iter()
                .map(|info| ActorTerminalInfo {
                    terminal: ActorTerminalId {
                        generation: status.generation,
                        shell_id: info.shell_id,
                    },
                    pid: info.pid,
                    running: info.running,
                    exit: exit_status(info.exit_code),
                })
                .collect())
        })
    }
}

impl Handles<TerminalOutputRead> for AgentOsActor {
    type Future = BoxFuture<ActorOutputReplay>;
    fn handle(self: Arc<Self>, _ctx: Ctx<Self>, action: TerminalOutputRead) -> Self::Future {
        Box::pin(async move {
            let _permit = self.admit_action()?;
            let vm = self
                .runtime
                .vm_at_generation(action.terminal.generation)
                .await?;
            let snapshot = vm
                .snapshot_shell_page(
                    &action.terminal.shell_id,
                    action.after,
                    action.max_events,
                    action.max_bytes,
                )
                .await?;
            let end = exit_status(snapshot.exit_code);
            Ok(ActorOutputReplay {
                generation: action.terminal.generation,
                events: snapshot
                    .events
                    .into_iter()
                    .map(|event| ActorOutputEvent {
                        sequence: event.sequence,
                        stream: event.stream,
                        data: FileBytes(event.data),
                        timestamp_ms: event.timestamp_ms,
                    })
                    .collect(),
                next_cursor: snapshot.next_cursor,
                has_more: snapshot.has_more,
                truncated: snapshot.truncated,
                end,
            })
        })
    }
}

impl Handles<TerminalStdinWrite> for AgentOsActor {
    type Future = BoxFuture<()>;
    fn handle(self: Arc<Self>, _ctx: Ctx<Self>, action: TerminalStdinWrite) -> Self::Future {
        Box::pin(async move {
            let _permit = self.admit_action()?;
            validate_bytes("terminal write", action.data.byte_len(), MAX_STDIN_BYTES)?;
            self.runtime
                .vm_at_generation(action.terminal.generation)
                .await?
                .write_shell_awaited(
                    &action.terminal.shell_id,
                    file_content_to_stdin(action.data),
                )
                .await?;
            Ok(())
        })
    }
}

impl Handles<TerminalPtyResize> for AgentOsActor {
    type Future = BoxFuture<()>;
    fn handle(self: Arc<Self>, _ctx: Ctx<Self>, action: TerminalPtyResize) -> Self::Future {
        Box::pin(async move {
            let _permit = self.admit_action()?;
            validate_dimensions(Some(action.cols), Some(action.rows))?;
            self.runtime
                .vm_at_generation(action.terminal.generation)
                .await?
                .resize_shell_awaited(&action.terminal.shell_id, action.cols, action.rows)
                .await?;
            Ok(())
        })
    }
}

impl Handles<TerminalWait> for AgentOsActor {
    type Future = BoxFuture<ActorTerminalExit>;
    fn handle(self: Arc<Self>, _ctx: Ctx<Self>, action: TerminalWait) -> Self::Future {
        Box::pin(async move {
            let _permit = self.admit_action()?;
            let vm = self
                .runtime
                .vm_at_generation(action.terminal.generation)
                .await?;
            let exit_code = tokio::time::timeout(
                Duration::from_millis(DEFAULT_WAIT_MS),
                vm.wait_shell(&action.terminal.shell_id),
            )
            .await
            .map_err(|_| {
                anyhow::anyhow!(
                    "timeout: terminal.wait exceeded {DEFAULT_WAIT_MS}ms; call again to continue waiting"
                )
            })??;
            Ok(ActorTerminalExit {
                terminal: action.terminal,
                status: ActorExitStatus {
                    exit_code,
                    signal: None,
                },
            })
        })
    }
}

impl Handles<TerminalClose> for AgentOsActor {
    type Future = BoxFuture<()>;
    fn handle(self: Arc<Self>, _ctx: Ctx<Self>, action: TerminalClose) -> Self::Future {
        Box::pin(async move {
            let _permit = self.admit_action()?;
            self.runtime
                .vm_at_generation(action.terminal.generation)
                .await?
                .close_shell_awaited(&action.terminal.shell_id)
                .await?;
            Ok(())
        })
    }
}

pub(crate) fn attach_process_events(
    vm: &agentos_client::AgentOs,
    ctx: Ctx<AgentOsActor>,
    process: ActorProcessId,
) -> Result<()> {
    let output_ctx = ctx.clone();
    let output_process = process;
    vm.on_process_output(process.pid, move |event| {
        let (Some(sequence), Some(timestamp_ms)) = (event.sequence, event.timestamp_ms) else {
            tracing::warn!(
                pid = event.pid,
                "retained process output lacked sequence metadata"
            );
            return;
        };
        if let Err(error) = output_ctx.emit(ProcessOutputEvent {
            process: output_process,
            sequence,
            stream: event.stream,
            data: FileBytes(event.data),
            timestamp_ms,
        }) {
            tracing::warn!(?error, pid = event.pid, "emit process.output failed");
        }
    })?
    .detach();
    vm.on_process_exit(process.pid, move |event| {
        if let Err(error) = ctx.emit(ProcessExitEvent {
            process,
            status: ActorExitStatus {
                exit_code: event.exit_code,
                signal: None,
            },
        }) {
            tracing::warn!(?error, pid = event.pid, "emit process.exit failed");
        }
    })?
    .detach();
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn actor_handles_are_runtime_scoped() {
        let process = ActorProcessId {
            generation: 4,
            pid: 100,
        };
        let encoded = serde_json::to_value(process).expect("encode handle");
        assert_eq!(encoded["generation"], 4);
        assert_eq!(encoded["pid"], 100);
    }

    #[test]
    fn empty_process_argument_is_distinct_from_missing_argument() {
        validate_arguments(&[]).expect("no argument entries is valid");
        validate_arguments(&[String::new()]).expect("empty argv element is valid");
        assert!(validate_command("")
            .unwrap_err()
            .to_string()
            .starts_with("invalid_input:"));
        assert!(validate_arguments(&["x".repeat(MAX_ARGUMENT_BYTES + 1)])
            .unwrap_err()
            .to_string()
            .starts_with("limit_exceeded:"));
        validate_arguments(&vec![String::new(); MAX_ARGUMENTS])
            .expect("empty entries still fit the argument-count boundary");
        assert!(validate_arguments(&vec![String::new(); MAX_ARGUMENTS + 1])
            .unwrap_err()
            .to_string()
            .starts_with("limit_exceeded: process argument count"));
        validate_arguments(&["é".repeat(MAX_ARGUMENT_BYTES / 2)])
            .expect("argument byte boundary is inclusive");
        assert!(
            validate_arguments(&["é".repeat(MAX_ARGUMENT_BYTES / 2 + 1)])
                .unwrap_err()
                .to_string()
                .starts_with("limit_exceeded: process argument")
        );
    }

    #[test]
    fn process_action_dtos_preserve_empty_and_whitespace_arguments() {
        let args = serde_json::json!(["", " two words ", ""]);
        let input = serde_json::json!({ "command": "echo", "args": args });
        let run: ProcessRun = serde_json::from_value(input.clone()).unwrap();
        let spawn: ProcessSpawn = serde_json::from_value(input.clone()).unwrap();
        let terminal: ActorTerminalOptions = serde_json::from_value(input).unwrap();
        validate_arguments(&run.args).unwrap();
        validate_arguments(&spawn.args).unwrap();
        validate_terminal_options(&terminal).unwrap();
        assert_eq!(serde_json::to_value(&run).unwrap()["args"], args);
        assert_eq!(serde_json::to_value(&spawn).unwrap()["args"], args);
        assert_eq!(
            serde_json::to_value(terminal_options_into_core(terminal).args).unwrap(),
            args
        );
    }

    #[test]
    fn process_and_terminal_cwd_validation_has_consistent_public_errors() {
        for (cwd, expected) in [
            (String::new(), "invalid_input"),
            ("x".repeat(MAX_ARGUMENT_BYTES + 1), "limit_exceeded"),
        ] {
            for result in [
                validate_exec_options(&ActorExecOptions {
                    cwd: Some(cwd.clone()),
                    ..Default::default()
                }),
                validate_spawn_options(&ActorSpawnOptions {
                    cwd: Some(cwd.clone()),
                    ..Default::default()
                }),
                validate_terminal_options(&ActorTerminalOptions {
                    cwd: Some(cwd.clone()),
                    ..Default::default()
                }),
            ] {
                let error =
                    crate::action_set::classify_public_error(result.expect_err("invalid cwd"));
                let error = error
                    .downcast_ref::<rivet_error::RivetError>()
                    .expect("typed error");
                let rivet_error::RivetErrorKind::Dynamic { code, .. } = &error.kind else {
                    panic!("expected a public agentOS error");
                };
                assert_eq!(code, expected);
                assert!(error.to_string().contains("cwd"));
            }
        }
        validate_terminal_options(&ActorTerminalOptions {
            cwd: Some("/".into()),
            ..Default::default()
        })
        .expect("nonempty terminal cwd is valid");
    }

    #[test]
    fn process_start_timestamp_names_its_unit() {
        let info = ActorProcessInfo {
            process: ActorProcessId {
                generation: 4,
                pid: 100,
            },
            command: "sh".into(),
            args: Vec::new(),
            running: true,
            exit: None,
            started_at_ms: 1_700_000_000_000,
        };
        let encoded = serde_json::to_value(info).expect("encode process info");
        assert_eq!(encoded["startedAtMs"], 1_700_000_000_000_i64);
        assert!(encoded.get("startedAt").is_none());
    }

    #[test]
    fn process_tree_timestamps_name_their_units() {
        let tree = ActorProcessTree {
            generation: 4,
            roots: vec![actor_process_tree_node(
                ProcessTreeNode {
                    info: agentos_client::ProcessInfo {
                        pid: 1_000_000,
                        tracked_pid: Some(123),
                        ppid: 1,
                        pgid: 100,
                        sid: 100,
                        driver: "node".into(),
                        command: "node".into(),
                        args: Vec::new(),
                        cwd: "/".into(),
                        status: ProcessStatus::Exited,
                        exit_code: Some(0),
                        start_time: 1_700_000_000_000.25,
                        exit_time: Some(1_700_000_000_001.5),
                    },
                    children: vec![ProcessTreeNode {
                        info: agentos_client::ProcessInfo {
                            // An untracked kernel PID can overlap the synthetic range.
                            pid: 1_000_000,
                            tracked_pid: None,
                            ppid: 1_000_000,
                            pgid: 100,
                            sid: 100,
                            driver: "wasm".into(),
                            command: "sleep".into(),
                            args: vec!["5".into()],
                            cwd: "/work".into(),
                            status: ProcessStatus::Running,
                            exit_code: None,
                            start_time: 1_700_000_000_000.5,
                            exit_time: None,
                        },
                        children: Vec::new(),
                    }],
                },
                4,
            )],
        };
        let encoded = serde_json::to_value(tree).expect("encode process tree");
        let root = &encoded["roots"][0];
        assert_eq!(root["startTimeMs"], 1_700_000_000_000.25);
        assert_eq!(root["exitTimeMs"], 1_700_000_000_001.5);
        assert_eq!(root["exit"]["exitCode"], 0);
        assert_eq!(root["process"]["generation"], 4);
        assert_eq!(root["pid"], 1_000_000);
        assert_eq!(root["process"]["pid"], 123);
        assert!(root.get("startTime").is_none());
        assert!(root.get("exitTime").is_none());
        assert!(root.get("exitCode").is_none());
        let child = &root["children"][0];
        assert_eq!(child["pid"], 1_000_000);
        assert_eq!(child["ppid"], 1_000_000);
        assert_eq!(child["args"], serde_json::json!(["5"]));
        assert_eq!(child["cwd"], "/work");
        assert_eq!(child["status"], "running");
        assert!(child.get("process").is_none());
        assert_eq!(child["startTimeMs"], 1_700_000_000_000.5);
        assert!(child["exitTimeMs"].is_null());
        assert!(child["exit"].is_null());
        assert!(child.get("startTime").is_none());
        assert!(child.get("exitCode").is_none());
    }

    #[test]
    fn command_and_terminal_limits_are_bounded() {
        assert!(validate_command("sh").is_ok());
        assert!(validate_command("").is_err());
        assert!(validate_dimensions(Some(80), Some(24)).is_ok());
        assert!(validate_dimensions(Some(0), Some(24)).is_err());
    }

    #[test]
    fn captured_run_forwards_a_bounded_core_timeout_by_default() {
        let default = ActorExecOptions::default();
        validate_exec_options(&default).expect("default options are valid");
        assert_eq!(
            exec_options_into_core(default).timeout,
            Some(MAX_WAIT_MS as f64)
        );
        assert!(validate_exec_options(&ActorExecOptions {
            timeout_ms: Some(0),
            ..Default::default()
        })
        .is_err());
    }
}
