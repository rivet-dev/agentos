//! First-class JavaScript, TypeScript, Python, and shared execution lifecycle.

use std::collections::HashMap;

use agentos_sidecar_client::wire;
use tokio::sync::broadcast;

use crate::agent_os::AgentOs;
use crate::error::{ClientError, ClientResult};

pub type ExecutionDescriptor = wire::ExecutionDescriptor;
pub type CodeExecutionResult = wire::ExecutionCompletedResponse;
pub type ExecutionOutputEvent = wire::ExecutionOutputEvent;
pub type ExecutionCompletedEvent = wire::ExecutionCompletedEvent;
pub type ExecutionOutputPage = wire::ExecutionOutputPageResponse;
pub type TypeScriptDiagnostic = wire::TypeScriptDiagnostic;

#[derive(Debug, Clone, Default)]
pub struct LanguageExecutionOptions {
    pub execution_id: Option<String>,
    pub create_if_missing: Option<bool>,
    pub cwd: Option<String>,
    pub env: HashMap<String, String>,
    pub args: Vec<String>,
    pub stdin: Option<Vec<u8>>,
    pub timeout_ms: Option<u64>,
    pub detached: bool,
    pub pty: Option<ExecutionPtyOptions>,
}

#[derive(Debug, Clone, Copy, Default)]
pub struct ExecutionPtyOptions {
    pub cols: Option<u16>,
    pub rows: Option<u16>,
}

#[derive(Debug, Clone, Default)]
pub struct InlineExecutionOptions {
    pub process: LanguageExecutionOptions,
    pub inputs: Option<serde_json::Map<String, serde_json::Value>>,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum JavaScriptModuleFormat {
    #[default]
    Module,
    CommonJs,
}

#[derive(Debug, Clone, Default)]
pub struct JavaScriptExecutionOptions {
    pub inline: InlineExecutionOptions,
    pub format: JavaScriptModuleFormat,
    pub file_path: Option<String>,
}

#[derive(Debug, Clone, Default)]
pub struct TypeScriptExecutionOptions {
    pub inline: InlineExecutionOptions,
    pub file_path: Option<String>,
    pub tsconfig_path: Option<String>,
    pub compiler_options: Option<serde_json::Map<String, serde_json::Value>>,
}

#[derive(Debug, Clone, Default)]
pub struct TypeScriptCheckOptions {
    pub execution_id: Option<String>,
    pub create_if_missing: Option<bool>,
    pub cwd: Option<String>,
    pub file_path: Option<String>,
    pub tsconfig_path: Option<String>,
    pub compiler_options: Option<serde_json::Map<String, serde_json::Value>>,
    pub timeout_ms: Option<u64>,
}

#[derive(Debug, Clone, Default)]
pub struct NpmProjectInstallOptions {
    pub execution_id: Option<String>,
    pub create_if_missing: Option<bool>,
    pub cwd: Option<String>,
    pub env: HashMap<String, String>,
    pub timeout_ms: Option<u64>,
    pub frozen: Option<bool>,
}

#[derive(Debug, Clone, Default)]
pub struct NpmPackageInstallOptions {
    pub execution_id: Option<String>,
    pub create_if_missing: Option<bool>,
    pub cwd: Option<String>,
    pub env: HashMap<String, String>,
    pub timeout_ms: Option<u64>,
    pub dev: Option<bool>,
    pub global: Option<bool>,
}

#[derive(Debug, Clone, Default)]
pub struct PythonInstallOptions {
    pub execution_id: Option<String>,
    pub create_if_missing: Option<bool>,
    pub cwd: Option<String>,
    pub env: HashMap<String, String>,
    pub timeout_ms: Option<u64>,
    pub upgrade: Option<bool>,
    pub requirements_file: Option<String>,
    pub index_url: Option<String>,
    pub extra_index_urls: Vec<String>,
}

#[derive(Debug, Clone)]
pub struct TypeScriptCheckResult {
    pub result: CodeExecutionResult,
    pub has_errors: Option<bool>,
    pub diagnostics: Vec<TypeScriptDiagnostic>,
}

#[derive(Debug, Clone)]
pub enum ExecutionSubmission {
    Completed(CodeExecutionResult),
    Detached(ExecutionDescriptor),
}

#[derive(Debug, Clone)]
pub struct CodeEvaluationResult {
    pub result: CodeExecutionResult,
    pub value: Option<serde_json::Value>,
}

fn identity(options: &LanguageExecutionOptions) -> wire::ExecutionIdentityOptions {
    wire::ExecutionIdentityOptions {
        execution_id: options.execution_id.clone(),
        create_if_missing: options.create_if_missing,
    }
}

fn process(options: &LanguageExecutionOptions) -> wire::ProcessExecutionOptions {
    wire::ProcessExecutionOptions {
        identity: identity(options),
        detached: Some(options.detached),
        cwd: options.cwd.clone(),
        env: (!options.env.is_empty()).then(|| options.env.clone()),
        args: options.args.clone(),
        stdin: options.stdin.clone(),
        timeout_ms: options.timeout_ms,
        pty: options.pty.map(|pty| wire::ExecutionPtyOptions {
            cols: pty.cols,
            rows: pty.rows,
        }),
    }
}

fn json_inputs(options: &InlineExecutionOptions) -> ClientResult<Option<String>> {
    options
        .inputs
        .as_ref()
        .map(serde_json::to_string)
        .transpose()
        .map_err(|error| ClientError::Sidecar(format!("failed to serialize inputs: {error}")))
}

impl AgentOs {
    fn execution_ownership(&self) -> wire::OwnershipScope {
        let inner = self.inner();
        wire::OwnershipScope::VmOwnership(wire::VmOwnership {
            connection_id: inner.connection_id.clone(),
            session_id: inner.session_id.clone(),
            vm_id: inner.vm_id.clone(),
        })
    }

    async fn submit_execution(
        &self,
        payload: wire::RequestPayload,
        detached: bool,
    ) -> ClientResult<ExecutionSubmission> {
        let mut events = self.transport().subscribe_wire_events();
        let accepted = match self
            .transport()
            .request_wire(self.execution_ownership(), payload)
            .await?
        {
            wire::ResponsePayload::ExecutionAcceptedResponse(response) => response.execution,
            wire::ResponsePayload::RejectedResponse(rejected) => {
                return Err(ClientError::from_rejection(rejected))
            }
            response => {
                return Err(ClientError::Sidecar(format!(
                    "unexpected execution response: {response:?}"
                )))
            }
        };
        if detached {
            return Ok(ExecutionSubmission::Detached(accepted));
        }
        wait_for_completion_event(&mut events, &accepted.execution_id).await?;
        Ok(ExecutionSubmission::Completed(
            self.wait_execution(&accepted.execution_id).await?,
        ))
    }

    pub async fn exec(
        &self,
        command: impl Into<String>,
        options: LanguageExecutionOptions,
    ) -> ClientResult<ExecutionSubmission> {
        let detached = options.detached;
        self.submit_execution(
            wire::RequestPayload::ShellExecutionRequest(wire::ShellExecutionRequest {
                process: process(&options),
                command: command.into(),
            }),
            detached,
        )
        .await
    }

    pub async fn exec_argv(
        &self,
        command: impl Into<String>,
        args: Vec<String>,
        mut options: LanguageExecutionOptions,
    ) -> ClientResult<ExecutionSubmission> {
        options.args = args;
        let detached = options.detached;
        self.submit_execution(
            wire::RequestPayload::ArgvExecutionRequest(wire::ArgvExecutionRequest {
                process: process(&options),
                command: command.into(),
            }),
            detached,
        )
        .await
    }

    pub async fn spawn(
        &self,
        command: impl Into<String>,
        args: Vec<String>,
        mut options: LanguageExecutionOptions,
    ) -> ClientResult<ExecutionDescriptor> {
        options.args = args;
        options.detached = true;
        match self
            .exec_argv(command, options.args.clone(), options)
            .await?
        {
            ExecutionSubmission::Detached(descriptor) => Ok(descriptor),
            ExecutionSubmission::Completed(_) => Err(ClientError::Sidecar(String::from(
                "spawn unexpectedly returned an attached result",
            ))),
        }
    }

    pub async fn execute_javascript(
        &self,
        source: impl Into<String>,
        options: JavaScriptExecutionOptions,
    ) -> ClientResult<ExecutionSubmission> {
        let inputs = json_inputs(&options.inline)?;
        self.submit_execution(
            wire::RequestPayload::JavaScriptExecutionRequest(wire::JavaScriptExecutionRequest {
                process: process(&options.inline.process),
                source: source.into(),
                format: Some(match options.format {
                    JavaScriptModuleFormat::Module => wire::JavaScriptModuleFormat::Module,
                    JavaScriptModuleFormat::CommonJs => wire::JavaScriptModuleFormat::CommonJs,
                }),
                file_path: options.file_path,
                inputs,
            }),
            options.inline.process.detached,
        )
        .await
    }

    pub async fn evaluate_javascript(
        &self,
        expression: impl Into<String>,
        options: JavaScriptExecutionOptions,
    ) -> ClientResult<CodeEvaluationResult> {
        let inputs = json_inputs(&options.inline)?;
        let submission = self
            .submit_execution(
                wire::RequestPayload::JavaScriptEvaluationRequest(
                    wire::JavaScriptEvaluationRequest {
                        process: process(&options.inline.process),
                        expression: expression.into(),
                        format: Some(match options.format {
                            JavaScriptModuleFormat::Module => wire::JavaScriptModuleFormat::Module,
                            JavaScriptModuleFormat::CommonJs => {
                                wire::JavaScriptModuleFormat::CommonJs
                            }
                        }),
                        file_path: options.file_path,
                        inputs,
                    },
                ),
                false,
            )
            .await?;
        evaluation_result(submission)
    }

    pub async fn execute_javascript_file(
        &self,
        path: impl Into<String>,
        options: LanguageExecutionOptions,
    ) -> ClientResult<ExecutionSubmission> {
        let detached = options.detached;
        self.submit_execution(
            wire::RequestPayload::JavaScriptFileExecutionRequest(
                wire::JavaScriptFileExecutionRequest {
                    process: process(&options),
                    path: path.into(),
                },
            ),
            detached,
        )
        .await
    }

    pub async fn execute_typescript(
        &self,
        source: impl Into<String>,
        options: TypeScriptExecutionOptions,
    ) -> ClientResult<ExecutionSubmission> {
        let inputs = json_inputs(&options.inline)?;
        self.submit_execution(
            wire::RequestPayload::TypeScriptExecutionRequest(wire::TypeScriptExecutionRequest {
                process: process(&options.inline.process),
                source: source.into(),
                file_path: options.file_path,
                tsconfig_path: options.tsconfig_path,
                compiler_options: options
                    .compiler_options
                    .as_ref()
                    .map(serde_json::to_string)
                    .transpose()
                    .map_err(|error| ClientError::Sidecar(error.to_string()))?,
                inputs,
            }),
            options.inline.process.detached,
        )
        .await
    }

    pub async fn evaluate_typescript(
        &self,
        expression: impl Into<String>,
        options: TypeScriptExecutionOptions,
    ) -> ClientResult<CodeEvaluationResult> {
        let inputs = json_inputs(&options.inline)?;
        let submission = self
            .submit_execution(
                wire::RequestPayload::TypeScriptEvaluationRequest(
                    wire::TypeScriptEvaluationRequest {
                        process: process(&options.inline.process),
                        expression: expression.into(),
                        file_path: options.file_path,
                        tsconfig_path: options.tsconfig_path,
                        compiler_options: options
                            .compiler_options
                            .as_ref()
                            .map(serde_json::to_string)
                            .transpose()
                            .map_err(|error| ClientError::Sidecar(error.to_string()))?,
                        inputs,
                    },
                ),
                false,
            )
            .await?;
        evaluation_result(submission)
    }

    pub async fn execute_typescript_file(
        &self,
        path: impl Into<String>,
        options: TypeScriptExecutionOptions,
    ) -> ClientResult<ExecutionSubmission> {
        let detached = options.inline.process.detached;
        self.submit_execution(
            wire::RequestPayload::TypeScriptFileExecutionRequest(
                wire::TypeScriptFileExecutionRequest {
                    process: process(&options.inline.process),
                    path: path.into(),
                    tsconfig_path: options.tsconfig_path,
                    compiler_options: options
                        .compiler_options
                        .as_ref()
                        .map(serde_json::to_string)
                        .transpose()
                        .map_err(|error| ClientError::Sidecar(error.to_string()))?,
                },
            ),
            detached,
        )
        .await
    }

    pub async fn check_typescript(
        &self,
        source: impl Into<String>,
        options: TypeScriptCheckOptions,
    ) -> ClientResult<TypeScriptCheckResult> {
        let compiler_options = options
            .compiler_options
            .as_ref()
            .map(serde_json::to_string)
            .transpose()
            .map_err(|error| ClientError::Sidecar(error.to_string()))?;
        let submission = self
            .submit_execution(
                wire::RequestPayload::TypeScriptCheckRequest(wire::TypeScriptCheckRequest {
                    identity: wire::ExecutionIdentityOptions {
                        execution_id: options.execution_id,
                        create_if_missing: options.create_if_missing,
                    },
                    source: source.into(),
                    cwd: options.cwd,
                    file_path: options.file_path,
                    tsconfig_path: options.tsconfig_path,
                    compiler_options,
                    timeout_ms: options.timeout_ms,
                }),
                false,
            )
            .await?;
        let result = completed_submission(submission)?;
        typescript_check_result(result)
    }

    pub async fn check_typescript_project(
        &self,
        options: TypeScriptCheckOptions,
    ) -> ClientResult<TypeScriptCheckResult> {
        let submission = self
            .submit_execution(
                wire::RequestPayload::TypeScriptProjectCheckRequest(
                    wire::TypeScriptProjectCheckRequest {
                        identity: wire::ExecutionIdentityOptions {
                            execution_id: options.execution_id,
                            create_if_missing: options.create_if_missing,
                        },
                        cwd: options.cwd,
                        tsconfig_path: options.tsconfig_path,
                        timeout_ms: options.timeout_ms,
                    },
                ),
                false,
            )
            .await?;
        let result = completed_submission(submission)?;
        typescript_check_result(result)
    }

    pub async fn install_npm_project(
        &self,
        options: NpmProjectInstallOptions,
    ) -> ClientResult<CodeExecutionResult> {
        completed_submission(
            self.submit_execution(
                wire::RequestPayload::NpmProjectInstallRequest(wire::NpmProjectInstallRequest {
                    identity: wire::ExecutionIdentityOptions {
                        execution_id: options.execution_id,
                        create_if_missing: options.create_if_missing,
                    },
                    cwd: options.cwd,
                    env: (!options.env.is_empty()).then_some(options.env),
                    timeout_ms: options.timeout_ms,
                    frozen: options.frozen,
                }),
                false,
            )
            .await?,
        )
    }

    pub async fn install_npm_packages(
        &self,
        packages: Vec<String>,
        options: NpmPackageInstallOptions,
    ) -> ClientResult<CodeExecutionResult> {
        completed_submission(
            self.submit_execution(
                wire::RequestPayload::NpmPackageInstallRequest(wire::NpmPackageInstallRequest {
                    identity: wire::ExecutionIdentityOptions {
                        execution_id: options.execution_id,
                        create_if_missing: options.create_if_missing,
                    },
                    cwd: options.cwd,
                    env: (!options.env.is_empty()).then_some(options.env),
                    timeout_ms: options.timeout_ms,
                    packages,
                    dev: options.dev,
                    global: options.global,
                }),
                false,
            )
            .await?,
        )
    }

    pub async fn execute_npm_script(
        &self,
        script: impl Into<String>,
        options: LanguageExecutionOptions,
    ) -> ClientResult<ExecutionSubmission> {
        let detached = options.detached;
        self.submit_execution(
            wire::RequestPayload::NpmScriptExecutionRequest(wire::NpmScriptExecutionRequest {
                process: process(&options),
                script: script.into(),
            }),
            detached,
        )
        .await
    }

    pub async fn execute_npm_package(
        &self,
        package_spec: impl Into<String>,
        binary: Option<String>,
        options: LanguageExecutionOptions,
    ) -> ClientResult<ExecutionSubmission> {
        let detached = options.detached;
        self.submit_execution(
            wire::RequestPayload::NpmPackageExecutionRequest(wire::NpmPackageExecutionRequest {
                process: process(&options),
                package_spec: package_spec.into(),
                binary,
            }),
            detached,
        )
        .await
    }

    pub async fn execute_python(
        &self,
        source: impl Into<String>,
        options: InlineExecutionOptions,
    ) -> ClientResult<ExecutionSubmission> {
        let inputs = json_inputs(&options)?;
        self.submit_execution(
            wire::RequestPayload::PythonExecutionRequest(wire::PythonExecutionRequest {
                process: process(&options.process),
                source: source.into(),
                inputs,
            }),
            options.process.detached,
        )
        .await
    }

    pub async fn evaluate_python(
        &self,
        expression: impl Into<String>,
        options: InlineExecutionOptions,
    ) -> ClientResult<CodeEvaluationResult> {
        let inputs = json_inputs(&options)?;
        let submission = self
            .submit_execution(
                wire::RequestPayload::PythonEvaluationRequest(wire::PythonEvaluationRequest {
                    process: process(&options.process),
                    expression: expression.into(),
                    inputs,
                }),
                false,
            )
            .await?;
        evaluation_result(submission)
    }

    pub async fn execute_python_file(
        &self,
        path: impl Into<String>,
        options: LanguageExecutionOptions,
    ) -> ClientResult<ExecutionSubmission> {
        let detached = options.detached;
        self.submit_execution(
            wire::RequestPayload::PythonFileExecutionRequest(wire::PythonFileExecutionRequest {
                process: process(&options),
                path: path.into(),
            }),
            detached,
        )
        .await
    }

    pub async fn execute_python_module(
        &self,
        module: impl Into<String>,
        options: LanguageExecutionOptions,
    ) -> ClientResult<ExecutionSubmission> {
        let detached = options.detached;
        self.submit_execution(
            wire::RequestPayload::PythonModuleExecutionRequest(
                wire::PythonModuleExecutionRequest {
                    process: process(&options),
                    module: module.into(),
                },
            ),
            detached,
        )
        .await
    }

    pub async fn install_python_packages(
        &self,
        packages: Vec<String>,
        options: PythonInstallOptions,
    ) -> ClientResult<CodeExecutionResult> {
        if !packages.is_empty() && options.requirements_file.is_some() {
            return Err(ClientError::Sidecar(String::from(
                "install_python_packages cannot combine packages with requirements_file",
            )));
        }
        completed_submission(
            self.submit_execution(
                wire::RequestPayload::PythonInstallRequest(wire::PythonInstallRequest {
                    identity: wire::ExecutionIdentityOptions {
                        execution_id: options.execution_id,
                        create_if_missing: options.create_if_missing,
                    },
                    cwd: options.cwd,
                    env: (!options.env.is_empty()).then_some(options.env),
                    timeout_ms: options.timeout_ms,
                    packages,
                    upgrade: options.upgrade,
                    requirements_file: options.requirements_file,
                    index_url: options.index_url,
                    extra_index_urls: options.extra_index_urls,
                }),
                false,
            )
            .await?,
        )
    }

    pub async fn get_execution(&self, execution_id: &str) -> ClientResult<ExecutionDescriptor> {
        match self
            .transport()
            .request_wire(
                self.execution_ownership(),
                wire::RequestPayload::GetExecutionRequest(wire::GetExecutionRequest {
                    execution_id: execution_id.to_owned(),
                }),
            )
            .await?
        {
            wire::ResponsePayload::ExecutionDescriptorResponse(response) => Ok(response.execution),
            wire::ResponsePayload::RejectedResponse(rejected) => {
                Err(ClientError::from_rejection(rejected))
            }
            response => Err(ClientError::Sidecar(format!(
                "unexpected get_execution response: {response:?}"
            ))),
        }
    }

    pub async fn list_executions(&self) -> ClientResult<Vec<ExecutionDescriptor>> {
        match self
            .transport()
            .request_wire(
                self.execution_ownership(),
                wire::RequestPayload::ListExecutionsRequest,
            )
            .await?
        {
            wire::ResponsePayload::ExecutionListResponse(response) => Ok(response.executions),
            wire::ResponsePayload::RejectedResponse(rejected) => {
                Err(ClientError::from_rejection(rejected))
            }
            response => Err(ClientError::Sidecar(format!(
                "unexpected list_executions response: {response:?}"
            ))),
        }
    }

    pub async fn wait_execution(&self, execution_id: &str) -> ClientResult<CodeExecutionResult> {
        let mut events = self.transport().subscribe_wire_events();
        let first = self
            .transport()
            .request_wire(
                self.execution_ownership(),
                wire::RequestPayload::WaitExecutionRequest(wire::WaitExecutionRequest {
                    execution_id: execution_id.to_owned(),
                }),
            )
            .await?;
        let response = match first {
            wire::ResponsePayload::RejectedResponse(rejected)
                if rejected.code == "execution_busy" =>
            {
                wait_for_completion_event(&mut events, execution_id).await?;
                self.transport()
                    .request_wire(
                        self.execution_ownership(),
                        wire::RequestPayload::WaitExecutionRequest(wire::WaitExecutionRequest {
                            execution_id: execution_id.to_owned(),
                        }),
                    )
                    .await?
            }
            response => response,
        };
        match response {
            wire::ResponsePayload::ExecutionCompletedResponse(response) => Ok(response),
            wire::ResponsePayload::RejectedResponse(rejected) => {
                Err(ClientError::from_rejection(rejected))
            }
            response => Err(ClientError::Sidecar(format!(
                "unexpected wait_execution response: {response:?}"
            ))),
        }
    }

    pub async fn cancel_execution(&self, execution_id: &str) -> ClientResult<ExecutionDescriptor> {
        self.execution_descriptor_request(wire::RequestPayload::CancelExecutionRequest(
            wire::CancelExecutionRequest {
                execution_id: execution_id.to_owned(),
            },
        ))
        .await
    }

    pub async fn signal_execution(
        &self,
        execution_id: &str,
        signal: impl Into<String>,
    ) -> ClientResult<ExecutionDescriptor> {
        self.execution_descriptor_request(wire::RequestPayload::SignalExecutionRequest(
            wire::SignalExecutionRequest {
                execution_id: execution_id.to_owned(),
                signal: signal.into(),
            },
        ))
        .await
    }

    pub async fn reset_execution(&self, execution_id: &str) -> ClientResult<ExecutionDescriptor> {
        self.execution_descriptor_request(wire::RequestPayload::ResetExecutionRequest(
            wire::ResetExecutionRequest {
                execution_id: execution_id.to_owned(),
            },
        ))
        .await
    }

    async fn execution_descriptor_request(
        &self,
        payload: wire::RequestPayload,
    ) -> ClientResult<ExecutionDescriptor> {
        match self
            .transport()
            .request_wire(self.execution_ownership(), payload)
            .await?
        {
            wire::ResponsePayload::ExecutionDescriptorResponse(response) => Ok(response.execution),
            wire::ResponsePayload::RejectedResponse(rejected) => {
                Err(ClientError::from_rejection(rejected))
            }
            response => Err(ClientError::Sidecar(format!(
                "unexpected execution lifecycle response: {response:?}"
            ))),
        }
    }

    pub async fn delete_execution(&self, execution_id: &str) -> ClientResult<()> {
        match self
            .transport()
            .request_wire(
                self.execution_ownership(),
                wire::RequestPayload::DeleteExecutionRequest(wire::DeleteExecutionRequest {
                    execution_id: execution_id.to_owned(),
                }),
            )
            .await?
        {
            wire::ResponsePayload::ExecutionDeletedResponse(_) => Ok(()),
            wire::ResponsePayload::RejectedResponse(rejected) => {
                Err(ClientError::from_rejection(rejected))
            }
            response => Err(ClientError::Sidecar(format!(
                "unexpected delete_execution response: {response:?}"
            ))),
        }
    }

    pub async fn read_execution_output(
        &self,
        execution_id: &str,
        cursor: Option<String>,
        limit: Option<u32>,
    ) -> ClientResult<ExecutionOutputPage> {
        match self
            .transport()
            .request_wire(
                self.execution_ownership(),
                wire::RequestPayload::ReadExecutionOutputRequest(
                    wire::ReadExecutionOutputRequest {
                        execution_id: execution_id.to_owned(),
                        cursor,
                        limit,
                    },
                ),
            )
            .await?
        {
            wire::ResponsePayload::ExecutionOutputPageResponse(response) => Ok(response),
            wire::ResponsePayload::RejectedResponse(rejected) => {
                Err(ClientError::from_rejection(rejected))
            }
            response => Err(ClientError::Sidecar(format!(
                "unexpected read_execution_output response: {response:?}"
            ))),
        }
    }

    pub async fn write_execution_stdin(
        &self,
        execution_id: &str,
        chunk: Vec<u8>,
    ) -> ClientResult<()> {
        self.execution_io_request(wire::RequestPayload::WriteExecutionStdinRequest(
            wire::WriteExecutionStdinRequest {
                execution_id: execution_id.to_owned(),
                chunk,
            },
        ))
        .await
    }

    pub async fn close_execution_stdin(&self, execution_id: &str) -> ClientResult<()> {
        self.execution_io_request(wire::RequestPayload::CloseExecutionStdinRequest(
            wire::CloseExecutionStdinRequest {
                execution_id: execution_id.to_owned(),
            },
        ))
        .await
    }

    pub async fn resize_execution_pty(
        &self,
        execution_id: &str,
        cols: u16,
        rows: u16,
    ) -> ClientResult<()> {
        self.execution_io_request(wire::RequestPayload::ResizeExecutionPtyRequest(
            wire::ResizeExecutionPtyRequest {
                execution_id: execution_id.to_owned(),
                cols,
                rows,
            },
        ))
        .await
    }

    async fn execution_io_request(&self, payload: wire::RequestPayload) -> ClientResult<()> {
        match self
            .transport()
            .request_wire(self.execution_ownership(), payload)
            .await?
        {
            wire::ResponsePayload::ExecutionIoResponse(_) => Ok(()),
            wire::ResponsePayload::RejectedResponse(rejected) => {
                Err(ClientError::from_rejection(rejected))
            }
            response => Err(ClientError::Sidecar(format!(
                "unexpected execution I/O response: {response:?}"
            ))),
        }
    }
}

async fn wait_for_completion_event(
    events: &mut broadcast::Receiver<(wire::OwnershipScope, wire::EventPayload)>,
    execution_id: &str,
) -> ClientResult<()> {
    loop {
        match events.recv().await {
            Ok((_, wire::EventPayload::ExecutionCompletedEvent(event)))
                if event.execution_id == execution_id =>
            {
                return Ok(())
            }
            Ok(_) | Err(broadcast::error::RecvError::Lagged(_)) => {}
            Err(broadcast::error::RecvError::Closed) => {
                return Err(ClientError::Sidecar(String::from(
                    "execution event stream closed before completion",
                )))
            }
        }
    }
}

fn evaluation_result(submission: ExecutionSubmission) -> ClientResult<CodeEvaluationResult> {
    let ExecutionSubmission::Completed(result) = submission else {
        return Err(ClientError::Sidecar(String::from(
            "evaluation unexpectedly returned detached execution",
        )));
    };
    let value = serde_json::from_str::<serde_json::Value>(&result.outputs)
        .ok()
        .and_then(|outputs| outputs.as_array().cloned())
        .and_then(|outputs| {
            outputs
                .into_iter()
                .find_map(|output| output.get("data").cloned())
        });
    Ok(CodeEvaluationResult { result, value })
}

fn typescript_check_result(result: CodeExecutionResult) -> ClientResult<TypeScriptCheckResult> {
    if result.outcome != wire::ExecutionOutcome::Succeeded {
        return Ok(TypeScriptCheckResult {
            result,
            has_errors: None,
            diagnostics: Vec::new(),
        });
    }
    let outputs: serde_json::Value = serde_json::from_str(&result.outputs).map_err(|error| {
        ClientError::Sidecar(format!(
            "failed to decode TypeScript checker output: {error}"
        ))
    })?;
    let data = outputs
        .as_array()
        .and_then(|outputs| outputs.iter().find_map(|output| output.get("data")))
        .and_then(serde_json::Value::as_object)
        .ok_or_else(|| {
            ClientError::Sidecar(String::from(
                "TypeScript checker returned no diagnostic result",
            ))
        })?;
    let has_errors = data
        .get("hasErrors")
        .and_then(serde_json::Value::as_bool)
        .ok_or_else(|| {
            ClientError::Sidecar(String::from(
                "TypeScript checker returned an invalid hasErrors value",
            ))
        })?;
    let diagnostics = data
        .get("diagnostics")
        .and_then(serde_json::Value::as_array)
        .ok_or_else(|| {
            ClientError::Sidecar(String::from(
                "TypeScript checker returned invalid diagnostics",
            ))
        })?
        .iter()
        .map(|diagnostic| {
            let code = diagnostic
                .get("code")
                .and_then(serde_json::Value::as_u64)
                .and_then(|code| u32::try_from(code).ok())
                .ok_or_else(|| {
                    ClientError::Sidecar(String::from(
                        "TypeScript checker returned an invalid diagnostic code",
                    ))
                })?;
            let category = diagnostic
                .get("category")
                .and_then(serde_json::Value::as_str)
                .ok_or_else(|| {
                    ClientError::Sidecar(String::from(
                        "TypeScript checker returned an invalid diagnostic category",
                    ))
                })?
                .to_owned();
            let message = diagnostic
                .get("message")
                .and_then(serde_json::Value::as_str)
                .ok_or_else(|| {
                    ClientError::Sidecar(String::from(
                        "TypeScript checker returned an invalid diagnostic message",
                    ))
                })?
                .to_owned();
            Ok(wire::TypeScriptDiagnostic {
                code,
                category,
                message,
                file_path: diagnostic
                    .get("filePath")
                    .and_then(serde_json::Value::as_str)
                    .map(str::to_owned),
                line: diagnostic
                    .get("line")
                    .and_then(serde_json::Value::as_u64)
                    .and_then(|line| u32::try_from(line).ok()),
                column: diagnostic
                    .get("column")
                    .and_then(serde_json::Value::as_u64)
                    .and_then(|column| u32::try_from(column).ok()),
            })
        })
        .collect::<ClientResult<Vec<_>>>()?;
    Ok(TypeScriptCheckResult {
        result,
        has_errors: Some(has_errors),
        diagnostics,
    })
}

fn completed_submission(submission: ExecutionSubmission) -> ClientResult<CodeExecutionResult> {
    match submission {
        ExecutionSubmission::Completed(result) => Ok(result),
        ExecutionSubmission::Detached(_) => Err(ClientError::Sidecar(String::from(
            "attached operation unexpectedly returned a detached execution",
        ))),
    }
}
