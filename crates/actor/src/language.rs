use crate::budgets::{HostedActorBudgets, ValidationBound};
use std::collections::{BTreeMap, HashMap};
use std::sync::Arc;

use agentos_client::language_execution::{
    ExecutionOutputOptions, InlineExecutionOptions, JavaScriptExecutionOptions,
    JavaScriptModuleFormat, LanguageExecutionOptions, LanguageSpawnOptions,
    NpmPackageInstallOptions, NpmProjectInstallOptions, OutputCapture, PythonInstallOptions,
    TypeScriptCheckOptions, TypeScriptCheckResult, TypeScriptExecutionOptions,
};
use agentos_client::{
    AgentOs, CodeEvaluationResult, CodeExecutionResult, ContextDescriptor, ExecutionPtyOptions,
    ProcessDescriptor,
};
use agentos_sidecar_client::wire;
use anyhow::{anyhow, bail, Context, Result};
use rivetkit::{Ctx, Handler};
use serde::{Deserialize, Serialize};

use crate::process::{attach_process_events, ActorProcessId};
use crate::{AgentOsActor, FileBytes, FileContentInput};

#[cfg(test)]
const MAX_CONTEXT_ID_BYTES: usize = 256;
#[cfg(test)]
const MAX_SOURCE_BYTES: usize = 512 * 1024;
#[cfg(test)]
const MAX_TIMEOUT_MS: u64 = 5 * 60 * 1_000;

#[cfg_attr(feature = "contract", derive(ts_rs::TS))]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ActorContextId {
    pub generation: u64,
    pub context_id: String,
}

#[cfg_attr(feature = "contract", derive(ts_rs::TS))]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ActorContextDescriptor {
    pub context: ActorContextId,
    pub state: String,
    pub language: Option<String>,
    pub created_at_ms: u64,
    pub last_started_at_ms: Option<u64>,
    pub last_completed_at_ms: Option<u64>,
}

#[cfg_attr(feature = "contract", derive(ts_rs::TS))]
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ActorOutputCapture {
    #[default]
    None,
    Stderr,
    All,
}

#[cfg_attr(feature = "contract", derive(ts_rs::TS))]
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ActorExecutionOutputOptions {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub capture: Option<ActorOutputCapture>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub retain_events: Option<bool>,
}

impl ActorExecutionOutputOptions {
    fn into_core(self) -> ExecutionOutputOptions {
        ExecutionOutputOptions {
            capture: match self.capture.unwrap_or_default() {
                ActorOutputCapture::None => OutputCapture::None,
                ActorOutputCapture::Stderr => OutputCapture::Stderr,
                ActorOutputCapture::All => OutputCapture::All,
            },
            retain_events: self.retain_events.unwrap_or(false),
        }
    }
}

#[cfg_attr(feature = "contract", derive(ts_rs::TS))]
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ActorExecutionPtyOptions {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cols: Option<u16>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rows: Option<u16>,
}

impl ActorExecutionPtyOptions {
    fn validate(self, budgets: &HostedActorBudgets) -> Result<()> {
        for (name, value) in [("cols", self.cols), ("rows", self.rows)] {
            if let Some(value) = value {
                if value == 0 {
                    bail!("limit_exceeded: language PTY {name} must be positive");
                }
                budgets
                    .execution
                    .max_terminal_dimension
                    .check(name, value as usize)?;
            }
        }
        Ok(())
    }

    fn into_core(self) -> ExecutionPtyOptions {
        ExecutionPtyOptions {
            cols: self.cols,
            rows: self.rows,
        }
    }
}

#[cfg_attr(feature = "contract", derive(ts_rs::TS))]
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ActorLanguageExecutionOptions {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub context: Option<ActorContextId>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cwd: Option<String>,
    #[cfg_attr(feature = "contract", ts(optional, as = "Option<_>"))]
    #[serde(default)]
    pub env: BTreeMap<String, String>,
    #[cfg_attr(feature = "contract", ts(optional, as = "Option<_>"))]
    #[serde(default)]
    pub args: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub stdin: Option<FileContentInput>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub timeout_ms: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pty: Option<ActorExecutionPtyOptions>,
    #[cfg_attr(feature = "contract", ts(optional, as = "Option<_>"))]
    #[serde(default)]
    pub output: ActorExecutionOutputOptions,
}

impl ActorLanguageExecutionOptions {
    fn validate(&self, budgets: &HostedActorBudgets) -> Result<()> {
        if let Some(context) = &self.context {
            validate_context_handle(context, budgets)?;
        }
        validate_optional_path("language cwd", self.cwd.as_deref(), budgets)?;
        validate_environment(&self.env, budgets)?;
        validate_arguments(&self.args, budgets)?;
        if let Some(stdin) = &self.stdin {
            validate_bytes(
                "language stdin",
                stdin.byte_len(),
                budgets.execution.max_stdin_bytes,
            )?;
        }
        validate_timeout(self.timeout_ms, budgets)?;
        if let Some(pty) = self.pty {
            pty.validate(budgets)?;
        }
        if self.output.retain_events == Some(true) && self.context.is_none() {
            bail!("invalid_input: retainEvents requires a named execution context");
        }
        Ok(())
    }

    fn context(&self) -> Option<&ActorContextId> {
        self.context.as_ref()
    }

    fn into_core(self, budgets: &HostedActorBudgets) -> LanguageExecutionOptions {
        LanguageExecutionOptions {
            context_id: self.context.map(|context| context.context_id),
            cwd: self.cwd,
            env: self.env.into_iter().collect::<HashMap<_, _>>(),
            args: self.args,
            stdin: self.stdin.map(file_content_into_bytes),
            timeout_ms: Some(
                self.timeout_ms
                    .unwrap_or(budgets.language.max_timeout_ms.maximum as u64),
            ),
            pty: self.pty.map(ActorExecutionPtyOptions::into_core),
            output: self.output.into_core(),
        }
    }
}

#[cfg_attr(feature = "contract", derive(ts_rs::TS))]
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ActorLanguageSpawnOptions {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cwd: Option<String>,
    #[cfg_attr(feature = "contract", ts(optional, as = "Option<_>"))]
    #[serde(default)]
    pub env: BTreeMap<String, String>,
    #[cfg_attr(feature = "contract", ts(optional, as = "Option<_>"))]
    #[serde(default)]
    pub args: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub stdin: Option<FileContentInput>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub timeout_ms: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pty: Option<ActorExecutionPtyOptions>,
}

impl ActorLanguageSpawnOptions {
    fn validate(&self, budgets: &HostedActorBudgets) -> Result<()> {
        validate_optional_path("language cwd", self.cwd.as_deref(), budgets)?;
        validate_environment(&self.env, budgets)?;
        validate_arguments(&self.args, budgets)?;
        if let Some(stdin) = &self.stdin {
            validate_bytes(
                "language stdin",
                stdin.byte_len(),
                budgets.execution.max_stdin_bytes,
            )?;
        }
        validate_timeout(self.timeout_ms, budgets)?;
        if let Some(pty) = self.pty {
            pty.validate(budgets)?;
        }
        Ok(())
    }

    fn into_core(self, _budgets: &HostedActorBudgets) -> LanguageSpawnOptions {
        LanguageSpawnOptions {
            cwd: self.cwd,
            env: self.env.into_iter().collect::<HashMap<_, _>>(),
            args: self.args,
            stdin: self.stdin.map(file_content_into_bytes),
            timeout_ms: self.timeout_ms,
            pty: self.pty.map(ActorExecutionPtyOptions::into_core),
            retain_events: true,
        }
    }
}

#[cfg_attr(feature = "contract", derive(ts_rs::TS))]
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ActorInlineExecutionOptions {
    #[cfg_attr(feature = "contract", ts(optional, as = "Option<_>"))]
    #[serde(default)]
    pub process: ActorLanguageExecutionOptions,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub inputs: Option<serde_json::Map<String, serde_json::Value>>,
}

impl ActorInlineExecutionOptions {
    fn validate(&self, budgets: &HostedActorBudgets) -> Result<()> {
        self.process.validate(budgets)?;
        validate_json_map("language inputs", self.inputs.as_ref(), budgets)
    }

    fn context(&self) -> Option<&ActorContextId> {
        self.process.context()
    }

    fn into_core(self, budgets: &HostedActorBudgets) -> InlineExecutionOptions {
        InlineExecutionOptions {
            process: self.process.into_core(budgets),
            inputs: self.inputs,
        }
    }
}

#[cfg_attr(feature = "contract", derive(ts_rs::TS))]
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ActorJavaScriptModuleFormat {
    #[default]
    Module,
    CommonJs,
}

#[cfg_attr(feature = "contract", derive(ts_rs::TS))]
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ActorJavaScriptExecutionOptions {
    #[cfg_attr(feature = "contract", ts(optional, as = "Option<_>"))]
    #[serde(default)]
    pub inline: ActorInlineExecutionOptions,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub format: Option<ActorJavaScriptModuleFormat>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub file_path: Option<String>,
}

impl ActorJavaScriptExecutionOptions {
    fn validate(&self, budgets: &HostedActorBudgets) -> Result<()> {
        self.inline.validate(budgets)?;
        validate_optional_path(
            "JavaScript virtual file path",
            self.file_path.as_deref(),
            budgets,
        )
    }

    fn context(&self) -> Option<&ActorContextId> {
        self.inline.context()
    }

    fn into_core(self, budgets: &HostedActorBudgets) -> JavaScriptExecutionOptions {
        JavaScriptExecutionOptions {
            inline: self.inline.into_core(budgets),
            format: match self.format.unwrap_or_default() {
                ActorJavaScriptModuleFormat::Module => JavaScriptModuleFormat::Module,
                ActorJavaScriptModuleFormat::CommonJs => JavaScriptModuleFormat::CommonJs,
            },
            file_path: self.file_path,
        }
    }
}

#[cfg_attr(feature = "contract", derive(ts_rs::TS))]
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ActorTypeScriptExecutionOptions {
    #[cfg_attr(feature = "contract", ts(optional, as = "Option<_>"))]
    #[serde(default)]
    pub inline: ActorInlineExecutionOptions,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub file_path: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tsconfig_path: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub compiler_options: Option<serde_json::Map<String, serde_json::Value>>,
}

impl ActorTypeScriptExecutionOptions {
    fn validate(&self, budgets: &HostedActorBudgets) -> Result<()> {
        self.inline.validate(budgets)?;
        validate_optional_path(
            "TypeScript virtual file path",
            self.file_path.as_deref(),
            budgets,
        )?;
        validate_optional_path(
            "TypeScript config path",
            self.tsconfig_path.as_deref(),
            budgets,
        )?;
        validate_json_map(
            "TypeScript compiler options",
            self.compiler_options.as_ref(),
            budgets,
        )
    }

    fn context(&self) -> Option<&ActorContextId> {
        self.inline.context()
    }

    fn into_core(self, budgets: &HostedActorBudgets) -> TypeScriptExecutionOptions {
        TypeScriptExecutionOptions {
            inline: self.inline.into_core(budgets),
            file_path: self.file_path,
            tsconfig_path: self.tsconfig_path,
            compiler_options: self.compiler_options,
        }
    }
}

#[cfg_attr(feature = "contract", derive(ts_rs::TS))]
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ActorTypeScriptCheckOptions {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub context: Option<ActorContextId>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cwd: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub file_path: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tsconfig_path: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub compiler_options: Option<serde_json::Map<String, serde_json::Value>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub timeout_ms: Option<u64>,
    #[cfg_attr(feature = "contract", ts(optional, as = "Option<_>"))]
    #[serde(default)]
    pub output: ActorExecutionOutputOptions,
}

impl ActorTypeScriptCheckOptions {
    fn validate(&self, budgets: &HostedActorBudgets) -> Result<()> {
        if let Some(context) = &self.context {
            validate_context_handle(context, budgets)?;
        }
        validate_optional_path("TypeScript cwd", self.cwd.as_deref(), budgets)?;
        validate_optional_path(
            "TypeScript virtual file path",
            self.file_path.as_deref(),
            budgets,
        )?;
        validate_optional_path(
            "TypeScript config path",
            self.tsconfig_path.as_deref(),
            budgets,
        )?;
        validate_json_map(
            "TypeScript compiler options",
            self.compiler_options.as_ref(),
            budgets,
        )?;
        validate_timeout(self.timeout_ms, budgets)?;
        if self.output.retain_events == Some(true) && self.context.is_none() {
            bail!("invalid_input: retainEvents requires a named execution context");
        }
        Ok(())
    }

    fn context(&self) -> Option<&ActorContextId> {
        self.context.as_ref()
    }

    fn into_core(self, budgets: &HostedActorBudgets) -> TypeScriptCheckOptions {
        TypeScriptCheckOptions {
            context_id: self.context.map(|context| context.context_id),
            cwd: self.cwd,
            file_path: self.file_path,
            tsconfig_path: self.tsconfig_path,
            compiler_options: self.compiler_options,
            timeout_ms: Some(
                self.timeout_ms
                    .unwrap_or(budgets.language.max_timeout_ms.maximum as u64),
            ),
            output: self.output.into_core(),
        }
    }
}

#[cfg_attr(feature = "contract", derive(ts_rs::TS))]
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ActorNpmInstallOptions {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub context: Option<ActorContextId>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cwd: Option<String>,
    #[cfg_attr(feature = "contract", ts(optional, as = "Option<_>"))]
    #[serde(default)]
    pub env: BTreeMap<String, String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub timeout_ms: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub frozen: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub dev: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub global: Option<bool>,
    #[cfg_attr(feature = "contract", ts(optional, as = "Option<_>"))]
    #[serde(default)]
    pub output: ActorExecutionOutputOptions,
}

impl ActorNpmInstallOptions {
    fn validate(&self, packages: &[String], budgets: &HostedActorBudgets) -> Result<()> {
        if let Some(context) = &self.context {
            validate_context_handle(context, budgets)?;
        }
        validate_optional_path("npm cwd", self.cwd.as_deref(), budgets)?;
        validate_environment(&self.env, budgets)?;
        validate_timeout(self.timeout_ms, budgets)?;
        validate_packages("npm package", packages, budgets)?;
        if packages.is_empty() && (self.dev.is_some() || self.global.is_some()) {
            bail!("invalid_input: dev and global require explicit npm packages");
        }
        if !packages.is_empty() && self.frozen.is_some() {
            bail!("invalid_input: frozen applies only to npm project installation");
        }
        if self.output.retain_events == Some(true) && self.context.is_none() {
            bail!("invalid_input: retainEvents requires a named execution context");
        }
        Ok(())
    }

    fn context(&self) -> Option<&ActorContextId> {
        self.context.as_ref()
    }

    fn into_project_core(self, budgets: &HostedActorBudgets) -> NpmProjectInstallOptions {
        NpmProjectInstallOptions {
            context_id: self.context.map(|context| context.context_id),
            cwd: self.cwd,
            env: self.env.into_iter().collect(),
            timeout_ms: Some(
                self.timeout_ms
                    .unwrap_or(budgets.language.max_timeout_ms.maximum as u64),
            ),
            frozen: self.frozen,
            output: self.output.into_core(),
        }
    }

    fn into_package_core(self, budgets: &HostedActorBudgets) -> NpmPackageInstallOptions {
        NpmPackageInstallOptions {
            context_id: self.context.map(|context| context.context_id),
            cwd: self.cwd,
            env: self.env.into_iter().collect(),
            timeout_ms: Some(
                self.timeout_ms
                    .unwrap_or(budgets.language.max_timeout_ms.maximum as u64),
            ),
            dev: self.dev,
            global: self.global,
            output: self.output.into_core(),
        }
    }
}

#[cfg_attr(feature = "contract", derive(ts_rs::TS))]
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ActorPythonInstallOptions {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub context: Option<ActorContextId>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cwd: Option<String>,
    #[cfg_attr(feature = "contract", ts(optional, as = "Option<_>"))]
    #[serde(default)]
    pub env: BTreeMap<String, String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub timeout_ms: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub upgrade: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub requirements_file: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub index_url: Option<String>,
    #[cfg_attr(feature = "contract", ts(optional, as = "Option<_>"))]
    #[serde(default)]
    pub extra_index_urls: Vec<String>,
    #[cfg_attr(feature = "contract", ts(optional, as = "Option<_>"))]
    #[serde(default)]
    pub output: ActorExecutionOutputOptions,
}

impl ActorPythonInstallOptions {
    fn validate(&self, packages: &[String], budgets: &HostedActorBudgets) -> Result<()> {
        if let Some(context) = &self.context {
            validate_context_handle(context, budgets)?;
        }
        validate_optional_path("Python cwd", self.cwd.as_deref(), budgets)?;
        validate_environment(&self.env, budgets)?;
        validate_timeout(self.timeout_ms, budgets)?;
        validate_packages("Python package", packages, budgets)?;
        validate_optional_path(
            "Python requirements file",
            self.requirements_file.as_deref(),
            budgets,
        )?;
        if !packages.is_empty() && self.requirements_file.is_some() {
            bail!("invalid_input: Python packages and requirementsFile are mutually exclusive");
        }
        validate_optional_string(
            "Python index URL",
            self.index_url.as_deref(),
            budgets.execution.max_argument_bytes,
        )?;
        validate_count(
            "Python extra index URLs",
            self.extra_index_urls.len(),
            budgets.language.max_index_urls,
        )?;
        for url in &self.extra_index_urls {
            validate_string(
                "Python extra index URL",
                url,
                budgets.execution.max_argument_bytes,
            )?;
        }
        if self.output.retain_events == Some(true) && self.context.is_none() {
            bail!("invalid_input: retainEvents requires a named execution context");
        }
        Ok(())
    }

    fn context(&self) -> Option<&ActorContextId> {
        self.context.as_ref()
    }

    fn into_core(self, budgets: &HostedActorBudgets) -> PythonInstallOptions {
        PythonInstallOptions {
            context_id: self.context.map(|context| context.context_id),
            cwd: self.cwd,
            env: self.env.into_iter().collect(),
            timeout_ms: Some(
                self.timeout_ms
                    .unwrap_or(budgets.language.max_timeout_ms.maximum as u64),
            ),
            upgrade: self.upgrade,
            requirements_file: self.requirements_file,
            index_url: self.index_url,
            extra_index_urls: self.extra_index_urls,
            output: self.output.into_core(),
        }
    }
}

#[cfg_attr(feature = "contract", derive(ts_rs::TS))]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ActorExecutionOutcome {
    Succeeded,
    Failed,
    Cancelled,
    TimedOut,
}

#[cfg_attr(feature = "contract", derive(ts_rs::TS))]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ActorExecutionError {
    pub code: String,
    pub name: String,
    pub message: String,
    pub stack: Option<String>,
    pub details: Option<serde_json::Value>,
}

#[cfg_attr(feature = "contract", derive(ts_rs::TS))]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ActorExecutionDescriptor {
    pub execution_id: String,
    pub generation: u64,
    pub state: String,
    pub language: Option<String>,
    pub process_id: Option<String>,
    pub pid: Option<u32>,
    pub created_at_ms: u64,
    pub last_started_at_ms: Option<u64>,
    pub last_completed_at_ms: Option<u64>,
    pub last_outcome: Option<ActorExecutionOutcome>,
    pub last_exit_code: Option<i32>,
}

#[cfg_attr(feature = "contract", derive(ts_rs::TS))]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ActorCodeExecutionResult {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub exit_signal: Option<u32>,
    pub execution: Option<ActorExecutionDescriptor>,
    pub outcome: ActorExecutionOutcome,
    pub exit_code: Option<i32>,
    pub error: Option<ActorExecutionError>,
    pub stdout: Option<FileBytes>,
    pub stderr: Option<FileBytes>,
    pub stdout_truncated: Option<bool>,
    pub stderr_truncated: Option<bool>,
}

#[cfg_attr(feature = "contract", derive(ts_rs::TS))]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ActorCodeEvaluationResult {
    pub result: ActorCodeExecutionResult,
    pub value: Option<serde_json::Value>,
}

#[cfg_attr(feature = "contract", derive(ts_rs::TS))]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ActorTypeScriptDiagnostic {
    pub code: u32,
    pub category: String,
    pub message: String,
    pub file_path: Option<String>,
    pub line: Option<u32>,
    pub column: Option<u32>,
}

#[cfg_attr(feature = "contract", derive(ts_rs::TS))]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ActorTypeScriptCheckResult {
    pub result: ActorCodeExecutionResult,
    pub has_errors: Option<bool>,
    pub diagnostics: Vec<ActorTypeScriptDiagnostic>,
}

#[cfg_attr(feature = "contract", derive(ts_rs::TS))]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ContextsCreate {
    pub context_id: String,
}

crate::register_action!(ContextsCreate => ActorContextDescriptor, "contexts.create");

macro_rules! context_action {
    ($name:ident, $output:ty, $action_name:literal) => {
        #[cfg_attr(feature = "contract", derive(ts_rs::TS))]
        #[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
        #[serde(rename_all = "camelCase", deny_unknown_fields)]
        pub struct $name {
            pub context: ActorContextId,
        }

        crate::register_action!($name => $output, $action_name);
    };
}

context_action!(ContextsGet, ActorContextDescriptor, "contexts.get");
context_action!(ContextsReset, ActorContextDescriptor, "contexts.reset");
context_action!(ContextsDelete, (), "contexts.delete");

#[cfg_attr(feature = "contract", derive(ts_rs::TS))]
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ContextsList {}

crate::register_action!(ContextsList => Vec<ActorContextDescriptor>, "contexts.list");

macro_rules! source_action {
    ($name:ident, $options:ty, $output:ty, $action_name:literal, $field:ident) => {
        #[cfg_attr(feature = "contract", derive(ts_rs::TS))]
        #[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
        #[serde(rename_all = "camelCase", deny_unknown_fields)]
        pub struct $name {
            pub $field: String,
            #[cfg_attr(feature = "contract", ts(optional, as = "Option<_>"))]
            #[serde(default)]
            pub options: $options,
        }

        crate::register_action!($name => $output, $action_name);
    };
}

source_action!(
    JavaScriptExecute,
    ActorJavaScriptExecutionOptions,
    ActorCodeExecutionResult,
    "javascript.execute",
    source
);
source_action!(
    JavaScriptEvaluate,
    ActorJavaScriptExecutionOptions,
    ActorCodeEvaluationResult,
    "javascript.evaluate",
    expression
);
source_action!(
    JavaScriptExecuteFile,
    ActorLanguageExecutionOptions,
    ActorCodeExecutionResult,
    "javascript.executeFile",
    path
);
source_action!(
    JavaScriptSpawn,
    ActorLanguageSpawnOptions,
    ActorProcessId,
    "javascript.spawn",
    source
);
source_action!(
    JavaScriptSpawnFile,
    ActorLanguageSpawnOptions,
    ActorProcessId,
    "javascript.spawnFile",
    path
);
source_action!(
    JavaScriptNpmRunScript,
    ActorLanguageExecutionOptions,
    ActorCodeExecutionResult,
    "javascript.npm.runScript",
    script
);

#[cfg_attr(feature = "contract", derive(ts_rs::TS))]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct JavaScriptNpmInstall {
    #[cfg_attr(feature = "contract", ts(optional, as = "Option<_>"))]
    #[serde(default)]
    pub packages: Vec<String>,
    #[cfg_attr(feature = "contract", ts(optional, as = "Option<_>"))]
    #[serde(default)]
    pub options: ActorNpmInstallOptions,
}

crate::register_action!(JavaScriptNpmInstall => ActorCodeExecutionResult, "javascript.npm.install");

#[cfg_attr(feature = "contract", derive(ts_rs::TS))]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct JavaScriptNpmRunPackage {
    pub package_spec: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub binary: Option<String>,
    #[cfg_attr(feature = "contract", ts(optional, as = "Option<_>"))]
    #[serde(default)]
    pub options: ActorLanguageExecutionOptions,
}

crate::register_action!(JavaScriptNpmRunPackage => ActorCodeExecutionResult, "javascript.npm.runPackage");

source_action!(
    TypeScriptExecute,
    ActorTypeScriptExecutionOptions,
    ActorCodeExecutionResult,
    "typescript.execute",
    source
);
source_action!(
    TypeScriptEvaluate,
    ActorTypeScriptExecutionOptions,
    ActorCodeEvaluationResult,
    "typescript.evaluate",
    expression
);
source_action!(
    TypeScriptExecuteFile,
    ActorTypeScriptExecutionOptions,
    ActorCodeExecutionResult,
    "typescript.executeFile",
    path
);
source_action!(
    TypeScriptSpawn,
    ActorLanguageSpawnOptions,
    ActorProcessId,
    "typescript.spawn",
    source
);
source_action!(
    TypeScriptSpawnFile,
    ActorLanguageSpawnOptions,
    ActorProcessId,
    "typescript.spawnFile",
    path
);
source_action!(
    TypeScriptCheck,
    ActorTypeScriptCheckOptions,
    ActorTypeScriptCheckResult,
    "typescript.check",
    source
);

#[cfg_attr(feature = "contract", derive(ts_rs::TS))]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct TypeScriptCheckProject {
    #[cfg_attr(feature = "contract", ts(optional, as = "Option<_>"))]
    #[serde(default)]
    pub options: ActorTypeScriptCheckOptions,
}

crate::register_action!(TypeScriptCheckProject => ActorTypeScriptCheckResult, "typescript.checkProject");

source_action!(
    PythonExecute,
    ActorInlineExecutionOptions,
    ActorCodeExecutionResult,
    "python.execute",
    source
);
source_action!(
    PythonEvaluate,
    ActorInlineExecutionOptions,
    ActorCodeEvaluationResult,
    "python.evaluate",
    expression
);
source_action!(
    PythonExecuteFile,
    ActorLanguageExecutionOptions,
    ActorCodeExecutionResult,
    "python.executeFile",
    path
);
source_action!(
    PythonExecuteModule,
    ActorLanguageExecutionOptions,
    ActorCodeExecutionResult,
    "python.executeModule",
    module
);
source_action!(
    PythonSpawn,
    ActorLanguageSpawnOptions,
    ActorProcessId,
    "python.spawn",
    source
);
source_action!(
    PythonSpawnFile,
    ActorLanguageSpawnOptions,
    ActorProcessId,
    "python.spawnFile",
    path
);
source_action!(
    PythonSpawnModule,
    ActorLanguageSpawnOptions,
    ActorProcessId,
    "python.spawnModule",
    module
);

#[cfg_attr(feature = "contract", derive(ts_rs::TS))]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PythonInstall {
    #[cfg_attr(feature = "contract", ts(optional, as = "Option<_>"))]
    #[serde(default)]
    pub packages: Vec<String>,
    #[cfg_attr(feature = "contract", ts(optional, as = "Option<_>"))]
    #[serde(default)]
    pub options: ActorPythonInstallOptions,
}

crate::register_action!(PythonInstall => ActorCodeExecutionResult, "python.install");

impl AgentOsActor {
    async fn language_vm(&self, context: Option<&ActorContextId>) -> Result<(AgentOs, u64)> {
        let budgets = self.budgets.as_ref();
        if let Some(context) = context {
            validate_context_handle(context, budgets)?;
            let vm = self.runtime.vm_at_generation(context.generation).await?;
            return Ok((vm, context.generation));
        }
        let status = self.runtime.status().await;
        let vm = self.runtime.vm_at_generation(status.generation).await?;
        Ok((vm, status.generation))
    }
}

impl Handler<ContextsCreate> for AgentOsActor {
    fn handle(
        self: Arc<Self>,
        _ctx: Ctx<Self>,
        action: ContextsCreate,
    ) -> impl std::future::Future<Output = anyhow::Result<ActorContextDescriptor>> + Send + 'static
    {
        Box::pin(async move {
            let _permit = self.admit_action()?;
            let budgets = self.budgets.as_ref();
            validate_context_id(&action.context_id, budgets)?;
            let (vm, generation) = self.language_vm(None).await?;
            let context = vm.create_context(&action.context_id).await?;
            Ok(actor_context_descriptor(generation, context))
        })
    }
}

impl Handler<ContextsGet> for AgentOsActor {
    fn handle(
        self: Arc<Self>,
        _ctx: Ctx<Self>,
        action: ContextsGet,
    ) -> impl std::future::Future<Output = anyhow::Result<ActorContextDescriptor>> + Send + 'static
    {
        Box::pin(async move {
            let _permit = self.admit_action()?;
            let budgets = self.budgets.as_ref();
            validate_context_handle(&action.context, budgets)?;
            let vm = self
                .runtime
                .vm_at_generation(action.context.generation)
                .await?;
            let context = vm.get_context(&action.context.context_id).await?;
            Ok(actor_context_descriptor(action.context.generation, context))
        })
    }
}

impl Handler<ContextsList> for AgentOsActor {
    fn handle(
        self: Arc<Self>,
        _ctx: Ctx<Self>,
        _action: ContextsList,
    ) -> impl std::future::Future<Output = anyhow::Result<Vec<ActorContextDescriptor>>> + Send + 'static
    {
        Box::pin(async move {
            let _permit = self.admit_action()?;
            let budgets = self.budgets.as_ref();
            let (vm, generation) = self.language_vm(None).await?;
            let contexts = vm.list_contexts().await?;
            validate_count(
                "execution contexts",
                contexts.len(),
                budgets.language.max_contexts,
            )?;
            Ok(contexts
                .into_iter()
                .map(|context| actor_context_descriptor(generation, context))
                .collect())
        })
    }
}

impl Handler<ContextsReset> for AgentOsActor {
    fn handle(
        self: Arc<Self>,
        _ctx: Ctx<Self>,
        action: ContextsReset,
    ) -> impl std::future::Future<Output = anyhow::Result<ActorContextDescriptor>> + Send + 'static
    {
        Box::pin(async move {
            let _permit = self.admit_action()?;
            let budgets = self.budgets.as_ref();
            validate_context_handle(&action.context, budgets)?;
            let vm = self
                .runtime
                .vm_at_generation(action.context.generation)
                .await?;
            vm.reset_context(&action.context.context_id).await?;
            let context = vm.get_context(&action.context.context_id).await?;
            Ok(actor_context_descriptor(action.context.generation, context))
        })
    }
}

impl Handler<ContextsDelete> for AgentOsActor {
    fn handle(
        self: Arc<Self>,
        _ctx: Ctx<Self>,
        action: ContextsDelete,
    ) -> impl std::future::Future<Output = anyhow::Result<()>> + Send + 'static {
        Box::pin(async move {
            let _permit = self.admit_action()?;
            let budgets = self.budgets.as_ref();
            validate_context_handle(&action.context, budgets)?;
            self.runtime
                .vm_at_generation(action.context.generation)
                .await?
                .delete_context(&action.context.context_id)
                .await?;
            Ok(())
        })
    }
}

impl Handler<JavaScriptExecute> for AgentOsActor {
    fn handle(
        self: Arc<Self>,
        _ctx: Ctx<Self>,
        action: JavaScriptExecute,
    ) -> impl std::future::Future<Output = anyhow::Result<ActorCodeExecutionResult>> + Send + 'static
    {
        Box::pin(async move {
            let _permit = self.admit_action()?;
            let budgets = self.budgets.as_ref();
            validate_source("JavaScript source", &action.source, budgets)?;
            action.options.validate(budgets)?;
            let (vm, _) = self.language_vm(action.options.context()).await?;
            actor_execution_result(
                vm.execute_javascript(action.source, action.options.into_core(budgets))
                    .await?,
                budgets,
            )
        })
    }
}

impl Handler<JavaScriptEvaluate> for AgentOsActor {
    fn handle(
        self: Arc<Self>,
        _ctx: Ctx<Self>,
        action: JavaScriptEvaluate,
    ) -> impl std::future::Future<Output = anyhow::Result<ActorCodeEvaluationResult>> + Send + 'static
    {
        Box::pin(async move {
            let _permit = self.admit_action()?;
            let budgets = self.budgets.as_ref();
            validate_source("JavaScript expression", &action.expression, budgets)?;
            action.options.validate(budgets)?;
            let (vm, _) = self.language_vm(action.options.context()).await?;
            actor_evaluation_result(
                vm.evaluate_javascript(action.expression, action.options.into_core(budgets))
                    .await?,
                budgets,
            )
        })
    }
}

impl Handler<JavaScriptExecuteFile> for AgentOsActor {
    fn handle(
        self: Arc<Self>,
        _ctx: Ctx<Self>,
        action: JavaScriptExecuteFile,
    ) -> impl std::future::Future<Output = anyhow::Result<ActorCodeExecutionResult>> + Send + 'static
    {
        Box::pin(async move {
            let _permit = self.admit_action()?;
            let budgets = self.budgets.as_ref();
            validate_path("JavaScript file path", &action.path, budgets)?;
            action.options.validate(budgets)?;
            let (vm, _) = self.language_vm(action.options.context()).await?;
            actor_execution_result(
                vm.execute_javascript_file(action.path, action.options.into_core(budgets))
                    .await?,
                budgets,
            )
        })
    }
}

impl Handler<JavaScriptSpawn> for AgentOsActor {
    fn handle(
        self: Arc<Self>,
        ctx: Ctx<Self>,
        action: JavaScriptSpawn,
    ) -> impl std::future::Future<Output = anyhow::Result<ActorProcessId>> + Send + 'static {
        Box::pin(async move {
            let _permit = self.admit_action()?;
            let budgets = self.budgets.as_ref();
            validate_source("JavaScript source", &action.source, budgets)?;
            action.options.validate(budgets)?;
            let (vm, generation) = self.language_vm(None).await?;
            let descriptor = vm
                .spawn_javascript(action.source, action.options.into_core(budgets))
                .await?;
            actor_spawn_result(&vm, ctx, generation, descriptor)
        })
    }
}

impl Handler<JavaScriptSpawnFile> for AgentOsActor {
    fn handle(
        self: Arc<Self>,
        ctx: Ctx<Self>,
        action: JavaScriptSpawnFile,
    ) -> impl std::future::Future<Output = anyhow::Result<ActorProcessId>> + Send + 'static {
        Box::pin(async move {
            let _permit = self.admit_action()?;
            let budgets = self.budgets.as_ref();
            validate_path("JavaScript file path", &action.path, budgets)?;
            action.options.validate(budgets)?;
            let (vm, generation) = self.language_vm(None).await?;
            let descriptor = vm
                .spawn_javascript_file(action.path, action.options.into_core(budgets))
                .await?;
            actor_spawn_result(&vm, ctx, generation, descriptor)
        })
    }
}

impl Handler<JavaScriptNpmInstall> for AgentOsActor {
    fn handle(
        self: Arc<Self>,
        _ctx: Ctx<Self>,
        action: JavaScriptNpmInstall,
    ) -> impl std::future::Future<Output = anyhow::Result<ActorCodeExecutionResult>> + Send + 'static
    {
        Box::pin(async move {
            let _permit = self.admit_action()?;
            let budgets = self.budgets.as_ref();
            action.options.validate(&action.packages, budgets)?;
            let (vm, _) = self.language_vm(action.options.context()).await?;
            let result = if action.packages.is_empty() {
                vm.install_npm_project(action.options.into_project_core(budgets))
                    .await?
            } else {
                vm.install_npm_packages(action.packages, action.options.into_package_core(budgets))
                    .await?
            };
            actor_execution_result(result, budgets)
        })
    }
}

impl Handler<JavaScriptNpmRunScript> for AgentOsActor {
    fn handle(
        self: Arc<Self>,
        _ctx: Ctx<Self>,
        action: JavaScriptNpmRunScript,
    ) -> impl std::future::Future<Output = anyhow::Result<ActorCodeExecutionResult>> + Send + 'static
    {
        Box::pin(async move {
            let _permit = self.admit_action()?;
            let budgets = self.budgets.as_ref();
            validate_string(
                "npm script",
                &action.script,
                budgets.execution.max_argument_bytes,
            )?;
            action.options.validate(budgets)?;
            let (vm, _) = self.language_vm(action.options.context()).await?;
            actor_execution_result(
                vm.execute_npm_script(action.script, action.options.into_core(budgets))
                    .await?,
                budgets,
            )
        })
    }
}

impl Handler<JavaScriptNpmRunPackage> for AgentOsActor {
    fn handle(
        self: Arc<Self>,
        _ctx: Ctx<Self>,
        action: JavaScriptNpmRunPackage,
    ) -> impl std::future::Future<Output = anyhow::Result<ActorCodeExecutionResult>> + Send + 'static
    {
        Box::pin(async move {
            let _permit = self.admit_action()?;
            let budgets = self.budgets.as_ref();
            validate_string(
                "npm package spec",
                &action.package_spec,
                budgets.execution.max_argument_bytes,
            )?;
            validate_optional_string(
                "npm binary",
                action.binary.as_deref(),
                budgets.execution.max_argument_bytes,
            )?;
            action.options.validate(budgets)?;
            let (vm, _) = self.language_vm(action.options.context()).await?;
            actor_execution_result(
                vm.execute_npm_package(
                    action.package_spec,
                    action.binary,
                    action.options.into_core(budgets),
                )
                .await?,
                budgets,
            )
        })
    }
}

impl Handler<TypeScriptExecute> for AgentOsActor {
    fn handle(
        self: Arc<Self>,
        _ctx: Ctx<Self>,
        action: TypeScriptExecute,
    ) -> impl std::future::Future<Output = anyhow::Result<ActorCodeExecutionResult>> + Send + 'static
    {
        Box::pin(async move {
            let _permit = self.admit_action()?;
            let budgets = self.budgets.as_ref();
            validate_source("TypeScript source", &action.source, budgets)?;
            action.options.validate(budgets)?;
            let (vm, _) = self.language_vm(action.options.context()).await?;
            actor_execution_result(
                vm.execute_typescript(action.source, action.options.into_core(budgets))
                    .await?,
                budgets,
            )
        })
    }
}

impl Handler<TypeScriptEvaluate> for AgentOsActor {
    fn handle(
        self: Arc<Self>,
        _ctx: Ctx<Self>,
        action: TypeScriptEvaluate,
    ) -> impl std::future::Future<Output = anyhow::Result<ActorCodeEvaluationResult>> + Send + 'static
    {
        Box::pin(async move {
            let _permit = self.admit_action()?;
            let budgets = self.budgets.as_ref();
            validate_source("TypeScript expression", &action.expression, budgets)?;
            action.options.validate(budgets)?;
            let (vm, _) = self.language_vm(action.options.context()).await?;
            actor_evaluation_result(
                vm.evaluate_typescript(action.expression, action.options.into_core(budgets))
                    .await?,
                budgets,
            )
        })
    }
}

impl Handler<TypeScriptExecuteFile> for AgentOsActor {
    fn handle(
        self: Arc<Self>,
        _ctx: Ctx<Self>,
        action: TypeScriptExecuteFile,
    ) -> impl std::future::Future<Output = anyhow::Result<ActorCodeExecutionResult>> + Send + 'static
    {
        Box::pin(async move {
            let _permit = self.admit_action()?;
            let budgets = self.budgets.as_ref();
            validate_path("TypeScript file path", &action.path, budgets)?;
            action.options.validate(budgets)?;
            let (vm, _) = self.language_vm(action.options.context()).await?;
            actor_execution_result(
                vm.execute_typescript_file(action.path, action.options.into_core(budgets))
                    .await?,
                budgets,
            )
        })
    }
}

impl Handler<TypeScriptSpawn> for AgentOsActor {
    fn handle(
        self: Arc<Self>,
        ctx: Ctx<Self>,
        action: TypeScriptSpawn,
    ) -> impl std::future::Future<Output = anyhow::Result<ActorProcessId>> + Send + 'static {
        Box::pin(async move {
            let _permit = self.admit_action()?;
            let budgets = self.budgets.as_ref();
            validate_source("TypeScript source", &action.source, budgets)?;
            action.options.validate(budgets)?;
            let (vm, generation) = self.language_vm(None).await?;
            let descriptor = vm
                .spawn_typescript(action.source, action.options.into_core(budgets))
                .await?;
            actor_spawn_result(&vm, ctx, generation, descriptor)
        })
    }
}

impl Handler<TypeScriptSpawnFile> for AgentOsActor {
    fn handle(
        self: Arc<Self>,
        ctx: Ctx<Self>,
        action: TypeScriptSpawnFile,
    ) -> impl std::future::Future<Output = anyhow::Result<ActorProcessId>> + Send + 'static {
        Box::pin(async move {
            let _permit = self.admit_action()?;
            let budgets = self.budgets.as_ref();
            validate_path("TypeScript file path", &action.path, budgets)?;
            action.options.validate(budgets)?;
            let (vm, generation) = self.language_vm(None).await?;
            let descriptor = vm
                .spawn_typescript_file(action.path, action.options.into_core(budgets))
                .await?;
            actor_spawn_result(&vm, ctx, generation, descriptor)
        })
    }
}

impl Handler<TypeScriptCheck> for AgentOsActor {
    fn handle(
        self: Arc<Self>,
        _ctx: Ctx<Self>,
        action: TypeScriptCheck,
    ) -> impl std::future::Future<Output = anyhow::Result<ActorTypeScriptCheckResult>> + Send + 'static
    {
        Box::pin(async move {
            let _permit = self.admit_action()?;
            let budgets = self.budgets.as_ref();
            validate_source("TypeScript source", &action.source, budgets)?;
            action.options.validate(budgets)?;
            let (vm, _) = self.language_vm(action.options.context()).await?;
            actor_typescript_check_result(
                vm.check_typescript(action.source, action.options.into_core(budgets))
                    .await?,
                budgets,
            )
        })
    }
}

impl Handler<TypeScriptCheckProject> for AgentOsActor {
    fn handle(
        self: Arc<Self>,
        _ctx: Ctx<Self>,
        action: TypeScriptCheckProject,
    ) -> impl std::future::Future<Output = anyhow::Result<ActorTypeScriptCheckResult>> + Send + 'static
    {
        Box::pin(async move {
            let _permit = self.admit_action()?;
            let budgets = self.budgets.as_ref();
            action.options.validate(budgets)?;
            let (vm, _) = self.language_vm(action.options.context()).await?;
            actor_typescript_check_result(
                vm.check_typescript_project(action.options.into_core(budgets))
                    .await?,
                budgets,
            )
        })
    }
}

impl Handler<PythonExecute> for AgentOsActor {
    fn handle(
        self: Arc<Self>,
        _ctx: Ctx<Self>,
        action: PythonExecute,
    ) -> impl std::future::Future<Output = anyhow::Result<ActorCodeExecutionResult>> + Send + 'static
    {
        Box::pin(async move {
            let _permit = self.admit_action()?;
            let budgets = self.budgets.as_ref();
            validate_source("Python source", &action.source, budgets)?;
            action.options.validate(budgets)?;
            let (vm, _) = self.language_vm(action.options.context()).await?;
            actor_execution_result(
                vm.execute_python(action.source, action.options.into_core(budgets))
                    .await?,
                budgets,
            )
        })
    }
}

impl Handler<PythonEvaluate> for AgentOsActor {
    fn handle(
        self: Arc<Self>,
        _ctx: Ctx<Self>,
        action: PythonEvaluate,
    ) -> impl std::future::Future<Output = anyhow::Result<ActorCodeEvaluationResult>> + Send + 'static
    {
        Box::pin(async move {
            let _permit = self.admit_action()?;
            let budgets = self.budgets.as_ref();
            validate_source("Python expression", &action.expression, budgets)?;
            action.options.validate(budgets)?;
            let (vm, _) = self.language_vm(action.options.context()).await?;
            actor_evaluation_result(
                vm.evaluate_python(action.expression, action.options.into_core(budgets))
                    .await?,
                budgets,
            )
        })
    }
}

impl Handler<PythonExecuteFile> for AgentOsActor {
    fn handle(
        self: Arc<Self>,
        _ctx: Ctx<Self>,
        action: PythonExecuteFile,
    ) -> impl std::future::Future<Output = anyhow::Result<ActorCodeExecutionResult>> + Send + 'static
    {
        Box::pin(async move {
            let _permit = self.admit_action()?;
            let budgets = self.budgets.as_ref();
            validate_path("Python file path", &action.path, budgets)?;
            action.options.validate(budgets)?;
            let (vm, _) = self.language_vm(action.options.context()).await?;
            actor_execution_result(
                vm.execute_python_file(action.path, action.options.into_core(budgets))
                    .await?,
                budgets,
            )
        })
    }
}

impl Handler<PythonExecuteModule> for AgentOsActor {
    fn handle(
        self: Arc<Self>,
        _ctx: Ctx<Self>,
        action: PythonExecuteModule,
    ) -> impl std::future::Future<Output = anyhow::Result<ActorCodeExecutionResult>> + Send + 'static
    {
        Box::pin(async move {
            let _permit = self.admit_action()?;
            let budgets = self.budgets.as_ref();
            validate_string(
                "Python module",
                &action.module,
                budgets.execution.max_argument_bytes,
            )?;
            action.options.validate(budgets)?;
            let (vm, _) = self.language_vm(action.options.context()).await?;
            actor_execution_result(
                vm.execute_python_module(action.module, action.options.into_core(budgets))
                    .await?,
                budgets,
            )
        })
    }
}

impl Handler<PythonSpawn> for AgentOsActor {
    fn handle(
        self: Arc<Self>,
        ctx: Ctx<Self>,
        action: PythonSpawn,
    ) -> impl std::future::Future<Output = anyhow::Result<ActorProcessId>> + Send + 'static {
        Box::pin(async move {
            let _permit = self.admit_action()?;
            let budgets = self.budgets.as_ref();
            validate_source("Python source", &action.source, budgets)?;
            action.options.validate(budgets)?;
            let (vm, generation) = self.language_vm(None).await?;
            let descriptor = vm
                .spawn_python(action.source, action.options.into_core(budgets))
                .await?;
            actor_spawn_result(&vm, ctx, generation, descriptor)
        })
    }
}

impl Handler<PythonSpawnFile> for AgentOsActor {
    fn handle(
        self: Arc<Self>,
        ctx: Ctx<Self>,
        action: PythonSpawnFile,
    ) -> impl std::future::Future<Output = anyhow::Result<ActorProcessId>> + Send + 'static {
        Box::pin(async move {
            let _permit = self.admit_action()?;
            let budgets = self.budgets.as_ref();
            validate_path("Python file path", &action.path, budgets)?;
            action.options.validate(budgets)?;
            let (vm, generation) = self.language_vm(None).await?;
            let descriptor = vm
                .spawn_python_file(action.path, action.options.into_core(budgets))
                .await?;
            actor_spawn_result(&vm, ctx, generation, descriptor)
        })
    }
}

impl Handler<PythonSpawnModule> for AgentOsActor {
    fn handle(
        self: Arc<Self>,
        ctx: Ctx<Self>,
        action: PythonSpawnModule,
    ) -> impl std::future::Future<Output = anyhow::Result<ActorProcessId>> + Send + 'static {
        Box::pin(async move {
            let _permit = self.admit_action()?;
            let budgets = self.budgets.as_ref();
            validate_string(
                "Python module",
                &action.module,
                budgets.execution.max_argument_bytes,
            )?;
            action.options.validate(budgets)?;
            let (vm, generation) = self.language_vm(None).await?;
            let descriptor = vm
                .spawn_python_module(action.module, action.options.into_core(budgets))
                .await?;
            actor_spawn_result(&vm, ctx, generation, descriptor)
        })
    }
}

impl Handler<PythonInstall> for AgentOsActor {
    fn handle(
        self: Arc<Self>,
        _ctx: Ctx<Self>,
        action: PythonInstall,
    ) -> impl std::future::Future<Output = anyhow::Result<ActorCodeExecutionResult>> + Send + 'static
    {
        Box::pin(async move {
            let _permit = self.admit_action()?;
            let budgets = self.budgets.as_ref();
            action.options.validate(&action.packages, budgets)?;
            let (vm, _) = self.language_vm(action.options.context()).await?;
            actor_execution_result(
                vm.install_python_packages(action.packages, action.options.into_core(budgets))
                    .await?,
                budgets,
            )
        })
    }
}

fn actor_spawn_result(
    vm: &AgentOs,
    ctx: Ctx<AgentOsActor>,
    generation: u64,
    descriptor: ProcessDescriptor,
) -> Result<ActorProcessId> {
    let process = ActorProcessId {
        generation,
        pid: descriptor.pid,
    };
    attach_process_events(vm, ctx, process)?;
    Ok(process)
}

fn actor_context_descriptor(
    generation: u64,
    descriptor: ContextDescriptor,
) -> ActorContextDescriptor {
    ActorContextDescriptor {
        context: ActorContextId {
            generation,
            context_id: descriptor.context_id,
        },
        state: descriptor.state,
        language: descriptor.language,
        created_at_ms: descriptor.created_at_ms,
        last_started_at_ms: descriptor.last_started_at_ms,
        last_completed_at_ms: descriptor.last_completed_at_ms,
    }
}

fn actor_execution_result(
    result: CodeExecutionResult,
    budgets: &HostedActorBudgets,
) -> Result<ActorCodeExecutionResult> {
    let output_bytes = result
        .stdout
        .as_ref()
        .map_or(0, Vec::len)
        .saturating_add(result.stderr.as_ref().map_or(0, Vec::len))
        .saturating_add(result.evaluation_value.as_ref().map_or(0, String::len))
        .saturating_add(
            result
                .type_script_check_result
                .as_ref()
                .map_or(0, String::len),
        );
    validate_bytes(
        "language result",
        output_bytes,
        budgets.language.max_result_bytes,
    )?;

    let execution = result.execution.map(actor_execution_descriptor);
    let error = result
        .error
        .map(|error| actor_execution_error(error, budgets))
        .transpose()?;
    let result = ActorCodeExecutionResult {
        exit_signal: result.exit_signal,
        execution,
        outcome: actor_execution_outcome(result.outcome),
        exit_code: result.exit_code,
        error,
        stdout: result.stdout.map(FileBytes),
        stderr: result.stderr.map(FileBytes),
        stdout_truncated: result.stdout_truncated,
        stderr_truncated: result.stderr_truncated,
    };
    validate_cbor_result("language result", &result, budgets)?;
    Ok(result)
}

fn actor_evaluation_result(
    result: CodeEvaluationResult,
    budgets: &HostedActorBudgets,
) -> Result<ActorCodeEvaluationResult> {
    if let Some(value) = &result.value {
        validate_json_value("language evaluation value", value, budgets)?;
    }
    let result = ActorCodeEvaluationResult {
        result: actor_execution_result(result.result, budgets)?,
        value: result.value,
    };
    validate_cbor_result("language evaluation result", &result, budgets)?;
    Ok(result)
}

fn actor_typescript_check_result(
    result: TypeScriptCheckResult,
    budgets: &HostedActorBudgets,
) -> Result<ActorTypeScriptCheckResult> {
    validate_count(
        "TypeScript diagnostics",
        result.diagnostics.len(),
        budgets.language.max_diagnostics,
    )?;
    let diagnostic_bytes = result
        .diagnostics
        .iter()
        .try_fold(0usize, |total, diagnostic| {
            total
                .checked_add(diagnostic.category.len())
                .and_then(|value| value.checked_add(diagnostic.message.len()))
                .and_then(|value| {
                    value.checked_add(diagnostic.file_path.as_ref().map_or(0, String::len))
                })
                .ok_or_else(|| anyhow!("limit_exceeded: TypeScript diagnostic byte count overflow"))
        })?;
    validate_bytes(
        "TypeScript diagnostics",
        diagnostic_bytes,
        budgets.language.max_diagnostic_bytes,
    )?;
    let result = ActorTypeScriptCheckResult {
        result: actor_execution_result(result.result, budgets)?,
        has_errors: result.has_errors,
        diagnostics: result
            .diagnostics
            .into_iter()
            .map(|diagnostic| ActorTypeScriptDiagnostic {
                code: diagnostic.code,
                category: diagnostic.category,
                message: diagnostic.message,
                file_path: diagnostic.file_path,
                line: diagnostic.line,
                column: diagnostic.column,
            })
            .collect(),
    };
    validate_cbor_result("TypeScript check result", &result, budgets)?;
    Ok(result)
}

fn actor_execution_descriptor(descriptor: wire::ExecutionDescriptor) -> ActorExecutionDescriptor {
    ActorExecutionDescriptor {
        execution_id: descriptor.execution_id,
        generation: descriptor.generation,
        state: match descriptor.state {
            wire::ExecutionState::Creating => "creating",
            wire::ExecutionState::Idle => "idle",
            wire::ExecutionState::Running => "running",
            wire::ExecutionState::Resetting => "resetting",
            wire::ExecutionState::Deleting => "deleting",
            wire::ExecutionState::Failed => "failed",
        }
        .to_owned(),
        language: descriptor.retained_language.map(|language| match language {
            wire::RetainedExecutionLanguage::JavaScript => String::from("javascript"),
            wire::RetainedExecutionLanguage::Python => String::from("python"),
        }),
        process_id: descriptor.process_id,
        pid: descriptor.pid,
        created_at_ms: descriptor.created_at_ms,
        last_started_at_ms: descriptor.last_started_at_ms,
        last_completed_at_ms: descriptor.last_completed_at_ms,
        last_outcome: descriptor.last_outcome.map(actor_execution_outcome),
        last_exit_code: descriptor.last_exit_code,
    }
}

fn actor_execution_outcome(outcome: wire::ExecutionOutcome) -> ActorExecutionOutcome {
    match outcome {
        wire::ExecutionOutcome::Succeeded => ActorExecutionOutcome::Succeeded,
        wire::ExecutionOutcome::Failed => ActorExecutionOutcome::Failed,
        wire::ExecutionOutcome::Cancelled => ActorExecutionOutcome::Cancelled,
        wire::ExecutionOutcome::TimedOut => ActorExecutionOutcome::TimedOut,
    }
}

fn actor_execution_error(
    error: wire::ExecutionErrorData,
    budgets: &HostedActorBudgets,
) -> Result<ActorExecutionError> {
    validate_string(
        "language error code",
        &error.code,
        budgets.execution.max_argument_bytes,
    )?;
    validate_string(
        "language error name",
        &error.name,
        budgets.execution.max_argument_bytes,
    )?;
    validate_string(
        "language error message",
        &error.message,
        budgets.language.max_json_bytes,
    )?;
    validate_optional_string(
        "language error stack",
        error.stack.as_deref(),
        budgets.language.max_json_bytes,
    )?;
    let details = error
        .details
        .as_deref()
        .map(serde_json::from_str)
        .transpose()
        .context("decode language error details")?;
    if let Some(details) = &details {
        validate_json_value("language error details", details, budgets)?;
    }
    Ok(ActorExecutionError {
        code: error.code,
        name: error.name,
        message: error.message,
        stack: error.stack,
        details,
    })
}

fn validate_context_handle(context: &ActorContextId, budgets: &HostedActorBudgets) -> Result<()> {
    if context.generation == 0 {
        bail!("invalid_input: execution context generation must be greater than zero");
    }
    validate_context_id(&context.context_id, budgets)
}

fn validate_context_id(context_id: &str, budgets: &HostedActorBudgets) -> Result<()> {
    validate_string(
        "execution context id",
        context_id,
        budgets.language.max_context_id_bytes,
    )
}

fn validate_source(label: &str, source: &str, budgets: &HostedActorBudgets) -> Result<()> {
    validate_string(label, source, budgets.language.max_source_bytes)
}

fn validate_path(label: &str, path: &str, budgets: &HostedActorBudgets) -> Result<()> {
    validate_string(label, path, budgets.path_bound())
}

fn validate_optional_path(
    label: &str,
    path: Option<&str>,
    budgets: &HostedActorBudgets,
) -> Result<()> {
    if let Some(path) = path {
        validate_path(label, path, budgets)?;
    }
    Ok(())
}

fn validate_optional_string(label: &str, value: Option<&str>, max: ValidationBound) -> Result<()> {
    if let Some(value) = value {
        validate_string(label, value, max)?;
    }
    Ok(())
}

fn validate_string(label: &str, value: &str, max: ValidationBound) -> Result<()> {
    if value.is_empty() {
        bail!("invalid_input: {label} must not be empty");
    }
    max.check(label, value.len())
}

fn validate_arguments(arguments: &[String], budgets: &HostedActorBudgets) -> Result<()> {
    validate_count(
        "language arguments",
        arguments.len(),
        budgets.execution.max_arguments,
    )?;
    for argument in arguments {
        validate_bytes(
            "language argument",
            argument.len(),
            budgets.execution.max_argument_bytes,
        )?;
    }
    Ok(())
}

fn validate_environment(
    environment: &BTreeMap<String, String>,
    budgets: &HostedActorBudgets,
) -> Result<()> {
    validate_count(
        "language environment entries",
        environment.len(),
        budgets.execution.max_environment_entries,
    )?;
    let bytes = environment.iter().try_fold(0usize, |total, (key, value)| {
        total
            .checked_add(key.len())
            .and_then(|value_bytes| value_bytes.checked_add(value.len()))
            .ok_or_else(|| anyhow!("limit_exceeded: language environment byte count overflow"))
    })?;
    validate_bytes(
        "language environment",
        bytes,
        budgets.execution.max_environment_bytes,
    )
}

fn validate_packages(label: &str, packages: &[String], budgets: &HostedActorBudgets) -> Result<()> {
    validate_count(label, packages.len(), budgets.language.max_packages)?;
    for package in packages {
        validate_string(label, package, budgets.execution.max_argument_bytes)?;
    }
    Ok(())
}

fn validate_timeout(timeout_ms: Option<u64>, budgets: &HostedActorBudgets) -> Result<()> {
    if timeout_ms == Some(0) {
        bail!("invalid_input: language timeoutMs must be greater than zero");
    }
    if let Some(timeout) = timeout_ms {
        budgets.language.max_timeout_ms.check(
            "language timeoutMs",
            usize::try_from(timeout)
                .map_err(|_| anyhow!("invalid_input: timeoutMs exceeds platform representation"))?,
        )?;
    }
    Ok(())
}

fn validate_json_map(
    label: &str,
    value: Option<&serde_json::Map<String, serde_json::Value>>,
    budgets: &HostedActorBudgets,
) -> Result<()> {
    if let Some(value) = value {
        validate_count(label, value.len(), budgets.language.max_packages)?;
        validate_json_value(label, value, budgets)?;
    }
    Ok(())
}

fn validate_json_value(
    label: &str,
    value: &impl Serialize,
    budgets: &HostedActorBudgets,
) -> Result<()> {
    let bytes = serde_json::to_vec(value).with_context(|| format!("serialize {label}"))?;
    validate_bytes(label, bytes.len(), budgets.language.max_json_bytes)
}

fn validate_cbor_result(
    label: &str,
    value: &impl Serialize,
    budgets: &HostedActorBudgets,
) -> Result<()> {
    let mut encoded = Vec::new();
    ciborium::into_writer(value, &mut encoded).with_context(|| format!("encode {label}"))?;
    validate_bytes(label, encoded.len(), budgets.language.max_result_bytes)
}

fn validate_count(label: &str, actual: usize, max: ValidationBound) -> Result<()> {
    max.check(label, actual)
}

fn validate_bytes(label: &str, actual: usize, max: ValidationBound) -> Result<()> {
    max.check(label, actual)
}

fn file_content_into_bytes(content: FileContentInput) -> Vec<u8> {
    match content {
        FileContentInput::Text(value) => value.into_bytes(),
        FileContentInput::Bytes(value) => value,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn language_inputs_and_contexts_are_bounded() {
        assert!(validate_context_id(
            "",
            &HostedActorBudgets::normalize(Default::default()).unwrap()
        )
        .is_err());
        assert!(validate_context_id(
            &"c".repeat(MAX_CONTEXT_ID_BYTES + 1),
            &HostedActorBudgets::normalize(Default::default()).unwrap()
        )
        .is_err());
        assert!(validate_source(
            "source",
            &"x".repeat(MAX_SOURCE_BYTES + 1),
            &HostedActorBudgets::normalize(Default::default()).unwrap()
        )
        .is_err());
        assert!(ActorExecutionPtyOptions {
            cols: Some(0),
            rows: Some(24),
        }
        .validate(&HostedActorBudgets::normalize(Default::default()).unwrap())
        .is_err());
    }

    #[test]
    fn spawn_options_always_enable_bounded_replay() {
        let options = ActorLanguageSpawnOptions::default()
            .into_core(&HostedActorBudgets::normalize(Default::default()).unwrap());
        assert!(options.retain_events);
    }

    #[test]
    fn synchronous_language_actions_have_sidecar_visible_timeouts() {
        assert_eq!(
            ActorLanguageExecutionOptions::default()
                .into_core(&HostedActorBudgets::normalize(Default::default()).unwrap())
                .timeout_ms,
            Some(MAX_TIMEOUT_MS)
        );
        assert_eq!(
            ActorTypeScriptCheckOptions::default()
                .into_core(&HostedActorBudgets::normalize(Default::default()).unwrap())
                .timeout_ms,
            Some(MAX_TIMEOUT_MS)
        );
        assert_eq!(
            ActorNpmInstallOptions::default()
                .into_project_core(&HostedActorBudgets::normalize(Default::default()).unwrap())
                .timeout_ms,
            Some(MAX_TIMEOUT_MS)
        );
        assert_eq!(
            ActorPythonInstallOptions::default()
                .into_core(&HostedActorBudgets::normalize(Default::default()).unwrap())
                .timeout_ms,
            Some(MAX_TIMEOUT_MS)
        );
        assert!(validate_timeout(
            Some(0),
            &HostedActorBudgets::normalize(Default::default()).unwrap()
        )
        .is_err());
    }

    #[test]
    fn execution_results_encode_binary_output_as_cbor_bytes() {
        let result = ActorCodeExecutionResult {
            exit_signal: None,
            execution: None,
            outcome: ActorExecutionOutcome::Succeeded,
            exit_code: Some(0),
            error: None,
            stdout: Some(FileBytes(vec![0, 1, 2])),
            stderr: None,
            stdout_truncated: Some(false),
            stderr_truncated: Some(false),
        };
        let mut encoded = Vec::new();
        ciborium::into_writer(&result, &mut encoded).expect("encode result");
        assert!(encoded.windows(4).any(|window| window == [0x43, 0, 1, 2]));
    }
}

#[cfg(test)]
#[path = "../tests/unit/language_worker_budgets.rs"]
mod worker_budget_tests;
