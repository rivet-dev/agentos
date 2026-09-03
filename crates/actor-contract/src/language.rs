//! Transport-safe language execution action DTOs and Core adapters.

use std::collections::{BTreeMap, HashMap};

use agentos_client::language_execution::{
    ExecutionOutputOptions, InlineExecutionOptions, JavaScriptExecutionOptions,
    JavaScriptModuleFormat, LanguageExecutionOptions, LanguageSpawnOptions,
    NpmPackageInstallOptions, NpmProjectInstallOptions, OutputCapture, PythonInstallOptions,
    TypeScriptCheckOptions, TypeScriptCheckResult, TypeScriptExecutionOptions,
};
use agentos_client::{
    CodeEvaluationResult, CodeExecutionResult, ContextDescriptor, ExecutionPtyOptions,
};
use agentos_sidecar_client::wire;
use anyhow::{anyhow, bail, Context, Result};
use serde::{Deserialize, Serialize};

use crate::filesystem::{FileBytes, FileContentInput};
use crate::process::ActorProcessId;

macro_rules! action {
    ($name:ident => $output:ty, $wire_name:literal) => {
        impl rivetkit::Action for $name {
            type Output = $output;
            const NAME: &'static str = $wire_name;
        }
    };
}

const MAX_CONTEXT_ID_BYTES: usize = 256;
const MAX_SOURCE_BYTES: usize = 512 * 1024;
const MAX_PATH_BYTES: usize = 4 * 1024;
const MAX_ARGUMENTS: usize = 1_024;
const MAX_ARGUMENT_BYTES: usize = 16 * 1024;
const MAX_ENVIRONMENT_ENTRIES: usize = 1_024;
const MAX_ENVIRONMENT_BYTES: usize = 256 * 1024;
const MAX_STDIN_BYTES: usize = 256 * 1024;
const MAX_JSON_BYTES: usize = 256 * 1024;
const MAX_PACKAGES: usize = 256;
const MAX_INDEX_URLS: usize = 16;
const MAX_TIMEOUT_MS: u64 = 5 * 60 * 1_000;
const MAX_RESULT_BYTES: usize = 768 * 1024;
const MAX_DIAGNOSTICS: usize = 1_024;
const MAX_DIAGNOSTIC_BYTES: usize = 512 * 1024;
const MAX_TERMINAL_DIMENSION: u16 = 4_096;

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
    pub fn into_core(self) -> ExecutionOutputOptions {
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
    pub fn validate(self) -> Result<()> {
        for (name, value) in [("cols", self.cols), ("rows", self.rows)] {
            if value.is_some_and(|value| value == 0 || value > MAX_TERMINAL_DIMENSION) {
                bail!(
                    "limit_exceeded: language PTY {name} must be between 1 and {MAX_TERMINAL_DIMENSION}"
                );
            }
        }
        Ok(())
    }

    pub fn into_core(self) -> ExecutionPtyOptions {
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
    pub fn validate(&self) -> Result<()> {
        if let Some(context) = &self.context {
            validate_context_handle(context)?;
        }
        validate_optional_path("language cwd", self.cwd.as_deref())?;
        validate_environment(&self.env)?;
        validate_arguments(&self.args)?;
        if let Some(stdin) = &self.stdin {
            validate_bytes("language stdin", stdin.byte_len(), MAX_STDIN_BYTES)?;
        }
        validate_timeout(self.timeout_ms)?;
        if let Some(pty) = self.pty {
            pty.validate()?;
        }
        if self.output.retain_events == Some(true) && self.context.is_none() {
            bail!("invalid_input: retainEvents requires a named execution context");
        }
        Ok(())
    }

    pub fn context(&self) -> Option<&ActorContextId> {
        self.context.as_ref()
    }

    pub fn into_core(self) -> LanguageExecutionOptions {
        LanguageExecutionOptions {
            context_id: self.context.map(|context| context.context_id),
            cwd: self.cwd,
            env: self.env.into_iter().collect::<HashMap<_, _>>(),
            args: self.args,
            stdin: self.stdin.map(file_content_into_bytes),
            timeout_ms: Some(self.timeout_ms.unwrap_or(MAX_TIMEOUT_MS)),
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
    pub fn validate(&self) -> Result<()> {
        validate_optional_path("language cwd", self.cwd.as_deref())?;
        validate_environment(&self.env)?;
        validate_arguments(&self.args)?;
        if let Some(stdin) = &self.stdin {
            validate_bytes("language stdin", stdin.byte_len(), MAX_STDIN_BYTES)?;
        }
        validate_timeout(self.timeout_ms)?;
        if let Some(pty) = self.pty {
            pty.validate()?;
        }
        Ok(())
    }

    pub fn into_core(self) -> LanguageSpawnOptions {
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
    pub fn validate(&self) -> Result<()> {
        self.process.validate()?;
        validate_json_map("language inputs", self.inputs.as_ref())
    }

    pub fn context(&self) -> Option<&ActorContextId> {
        self.process.context()
    }

    pub fn into_core(self) -> InlineExecutionOptions {
        InlineExecutionOptions {
            process: self.process.into_core(),
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
    pub fn validate(&self) -> Result<()> {
        self.inline.validate()?;
        validate_optional_path("JavaScript virtual file path", self.file_path.as_deref())
    }

    pub fn context(&self) -> Option<&ActorContextId> {
        self.inline.context()
    }

    pub fn into_core(self) -> JavaScriptExecutionOptions {
        JavaScriptExecutionOptions {
            inline: self.inline.into_core(),
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
    pub fn validate(&self) -> Result<()> {
        self.inline.validate()?;
        validate_optional_path("TypeScript virtual file path", self.file_path.as_deref())?;
        validate_optional_path("TypeScript config path", self.tsconfig_path.as_deref())?;
        validate_json_map(
            "TypeScript compiler options",
            self.compiler_options.as_ref(),
        )
    }

    pub fn context(&self) -> Option<&ActorContextId> {
        self.inline.context()
    }

    pub fn into_core(self) -> TypeScriptExecutionOptions {
        TypeScriptExecutionOptions {
            inline: self.inline.into_core(),
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
    pub fn validate(&self) -> Result<()> {
        if let Some(context) = &self.context {
            validate_context_handle(context)?;
        }
        validate_optional_path("TypeScript cwd", self.cwd.as_deref())?;
        validate_optional_path("TypeScript virtual file path", self.file_path.as_deref())?;
        validate_optional_path("TypeScript config path", self.tsconfig_path.as_deref())?;
        validate_json_map(
            "TypeScript compiler options",
            self.compiler_options.as_ref(),
        )?;
        validate_timeout(self.timeout_ms)?;
        if self.output.retain_events == Some(true) && self.context.is_none() {
            bail!("invalid_input: retainEvents requires a named execution context");
        }
        Ok(())
    }

    pub fn context(&self) -> Option<&ActorContextId> {
        self.context.as_ref()
    }

    pub fn into_core(self) -> TypeScriptCheckOptions {
        TypeScriptCheckOptions {
            context_id: self.context.map(|context| context.context_id),
            cwd: self.cwd,
            file_path: self.file_path,
            tsconfig_path: self.tsconfig_path,
            compiler_options: self.compiler_options,
            timeout_ms: Some(self.timeout_ms.unwrap_or(MAX_TIMEOUT_MS)),
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
    pub fn validate(&self, packages: &[String]) -> Result<()> {
        if let Some(context) = &self.context {
            validate_context_handle(context)?;
        }
        validate_optional_path("npm cwd", self.cwd.as_deref())?;
        validate_environment(&self.env)?;
        validate_timeout(self.timeout_ms)?;
        validate_packages("npm package", packages)?;
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

    pub fn context(&self) -> Option<&ActorContextId> {
        self.context.as_ref()
    }

    pub fn into_project_core(self) -> NpmProjectInstallOptions {
        NpmProjectInstallOptions {
            context_id: self.context.map(|context| context.context_id),
            cwd: self.cwd,
            env: self.env.into_iter().collect(),
            timeout_ms: Some(self.timeout_ms.unwrap_or(MAX_TIMEOUT_MS)),
            frozen: self.frozen,
            output: self.output.into_core(),
        }
    }

    pub fn into_package_core(self) -> NpmPackageInstallOptions {
        NpmPackageInstallOptions {
            context_id: self.context.map(|context| context.context_id),
            cwd: self.cwd,
            env: self.env.into_iter().collect(),
            timeout_ms: Some(self.timeout_ms.unwrap_or(MAX_TIMEOUT_MS)),
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
    pub fn validate(&self, packages: &[String]) -> Result<()> {
        if let Some(context) = &self.context {
            validate_context_handle(context)?;
        }
        validate_optional_path("Python cwd", self.cwd.as_deref())?;
        validate_environment(&self.env)?;
        validate_timeout(self.timeout_ms)?;
        validate_packages("Python package", packages)?;
        validate_optional_path(
            "Python requirements file",
            self.requirements_file.as_deref(),
        )?;
        if !packages.is_empty() && self.requirements_file.is_some() {
            bail!("invalid_input: Python packages and requirementsFile are mutually exclusive");
        }
        validate_optional_string(
            "Python index URL",
            self.index_url.as_deref(),
            MAX_ARGUMENT_BYTES,
        )?;
        validate_count(
            "Python extra index URLs",
            self.extra_index_urls.len(),
            MAX_INDEX_URLS,
        )?;
        for url in &self.extra_index_urls {
            validate_string("Python extra index URL", url, MAX_ARGUMENT_BYTES)?;
        }
        if self.output.retain_events == Some(true) && self.context.is_none() {
            bail!("invalid_input: retainEvents requires a named execution context");
        }
        Ok(())
    }

    pub fn context(&self) -> Option<&ActorContextId> {
        self.context.as_ref()
    }

    pub fn into_core(self) -> PythonInstallOptions {
        PythonInstallOptions {
            context_id: self.context.map(|context| context.context_id),
            cwd: self.cwd,
            env: self.env.into_iter().collect(),
            timeout_ms: Some(self.timeout_ms.unwrap_or(MAX_TIMEOUT_MS)),
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

action!(ContextsCreate => ActorContextDescriptor, "contexts.create");

macro_rules! context_action {
    ($name:ident, $output:ty, $action_name:literal) => {
        #[cfg_attr(feature = "contract", derive(ts_rs::TS))]
        #[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
        #[serde(rename_all = "camelCase", deny_unknown_fields)]
        pub struct $name {
            pub context: ActorContextId,
        }

        action!($name => $output, $action_name);
    };
}

context_action!(ContextsGet, ActorContextDescriptor, "contexts.get");
context_action!(ContextsReset, ActorContextDescriptor, "contexts.reset");
context_action!(ContextsDelete, (), "contexts.delete");

#[cfg_attr(feature = "contract", derive(ts_rs::TS))]
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ContextsList {}

action!(ContextsList => Vec<ActorContextDescriptor>, "contexts.list");

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

        action!($name => $output, $action_name);
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

action!(JavaScriptNpmInstall => ActorCodeExecutionResult, "javascript.npm.install");

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

action!(JavaScriptNpmRunPackage => ActorCodeExecutionResult, "javascript.npm.runPackage");

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

action!(TypeScriptCheckProject => ActorTypeScriptCheckResult, "typescript.checkProject");

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

action!(PythonInstall => ActorCodeExecutionResult, "python.install");

pub fn actor_context_descriptor(
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

pub fn actor_execution_result(result: CodeExecutionResult) -> Result<ActorCodeExecutionResult> {
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
    validate_bytes("language result", output_bytes, MAX_RESULT_BYTES)?;

    let execution = result.execution.map(actor_execution_descriptor);
    let error = result.error.map(actor_execution_error).transpose()?;
    let result = ActorCodeExecutionResult {
        execution,
        outcome: actor_execution_outcome(result.outcome),
        exit_code: result.exit_code,
        error,
        stdout: result.stdout.map(FileBytes),
        stderr: result.stderr.map(FileBytes),
        stdout_truncated: result.stdout_truncated,
        stderr_truncated: result.stderr_truncated,
    };
    validate_cbor_result("language result", &result)?;
    Ok(result)
}

pub fn actor_evaluation_result(result: CodeEvaluationResult) -> Result<ActorCodeEvaluationResult> {
    if let Some(value) = &result.value {
        validate_json_value("language evaluation value", value)?;
    }
    let result = ActorCodeEvaluationResult {
        result: actor_execution_result(result.result)?,
        value: result.value,
    };
    validate_cbor_result("language evaluation result", &result)?;
    Ok(result)
}

pub fn actor_typescript_check_result(
    result: TypeScriptCheckResult,
) -> Result<ActorTypeScriptCheckResult> {
    validate_count(
        "TypeScript diagnostics",
        result.diagnostics.len(),
        MAX_DIAGNOSTICS,
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
        MAX_DIAGNOSTIC_BYTES,
    )?;
    let result = ActorTypeScriptCheckResult {
        result: actor_execution_result(result.result)?,
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
    validate_cbor_result("TypeScript check result", &result)?;
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

fn actor_execution_error(error: wire::ExecutionErrorData) -> Result<ActorExecutionError> {
    validate_string("language error code", &error.code, MAX_ARGUMENT_BYTES)?;
    validate_string("language error name", &error.name, MAX_ARGUMENT_BYTES)?;
    validate_string("language error message", &error.message, MAX_JSON_BYTES)?;
    validate_optional_string(
        "language error stack",
        error.stack.as_deref(),
        MAX_JSON_BYTES,
    )?;
    let details = error
        .details
        .as_deref()
        .map(serde_json::from_str)
        .transpose()
        .context("decode language error details")?;
    if let Some(details) = &details {
        validate_json_value("language error details", details)?;
    }
    Ok(ActorExecutionError {
        code: error.code,
        name: error.name,
        message: error.message,
        stack: error.stack,
        details,
    })
}

pub fn validate_context_handle(context: &ActorContextId) -> Result<()> {
    if context.generation == 0 {
        bail!("invalid_input: execution context generation must be greater than zero");
    }
    validate_context_id(&context.context_id)
}

pub fn validate_context_id(context_id: &str) -> Result<()> {
    validate_string("execution context id", context_id, MAX_CONTEXT_ID_BYTES)
}

pub fn validate_source(label: &str, source: &str) -> Result<()> {
    validate_string(label, source, MAX_SOURCE_BYTES)
}

pub fn validate_path(label: &str, path: &str) -> Result<()> {
    validate_string(label, path, MAX_PATH_BYTES)
}

pub fn validate_argument(label: &str, value: &str) -> Result<()> {
    validate_string(label, value, MAX_ARGUMENT_BYTES)
}

pub fn validate_optional_path(label: &str, path: Option<&str>) -> Result<()> {
    if let Some(path) = path {
        validate_path(label, path)?;
    }
    Ok(())
}

pub fn validate_optional_string(label: &str, value: Option<&str>, max: usize) -> Result<()> {
    if let Some(value) = value {
        validate_string(label, value, max)?;
    }
    Ok(())
}

pub fn validate_string(label: &str, value: &str, max: usize) -> Result<()> {
    if value.is_empty() {
        bail!("invalid_input: {label} cannot be empty");
    }
    validate_bytes(label, value.len(), max)
}

pub fn validate_arguments(arguments: &[String]) -> Result<()> {
    validate_count("language arguments", arguments.len(), MAX_ARGUMENTS)?;
    for argument in arguments {
        validate_bytes("language argument", argument.len(), MAX_ARGUMENT_BYTES)?;
    }
    Ok(())
}

pub fn validate_environment(environment: &BTreeMap<String, String>) -> Result<()> {
    validate_count(
        "language environment entries",
        environment.len(),
        MAX_ENVIRONMENT_ENTRIES,
    )?;
    let bytes = environment.iter().try_fold(0usize, |total, (key, value)| {
        total
            .checked_add(key.len())
            .and_then(|value_bytes| value_bytes.checked_add(value.len()))
            .ok_or_else(|| anyhow!("limit_exceeded: language environment byte count overflow"))
    })?;
    validate_bytes("language environment", bytes, MAX_ENVIRONMENT_BYTES)
}

pub fn validate_packages(label: &str, packages: &[String]) -> Result<()> {
    validate_count(label, packages.len(), MAX_PACKAGES)?;
    for package in packages {
        validate_string(label, package, MAX_ARGUMENT_BYTES)?;
    }
    Ok(())
}

pub fn validate_timeout(timeout_ms: Option<u64>) -> Result<()> {
    if timeout_ms == Some(0) {
        bail!("invalid_input: language timeoutMs must be greater than zero");
    }
    if timeout_ms.is_some_and(|timeout| timeout > MAX_TIMEOUT_MS) {
        bail!("limit_exceeded: language timeout exceeds {MAX_TIMEOUT_MS}ms; lower timeoutMs");
    }
    Ok(())
}

pub fn validate_json_map(
    label: &str,
    value: Option<&serde_json::Map<String, serde_json::Value>>,
) -> Result<()> {
    if let Some(value) = value {
        validate_count(label, value.len(), MAX_PACKAGES)?;
        validate_json_value(label, value)?;
    }
    Ok(())
}

pub fn validate_json_value(label: &str, value: &impl Serialize) -> Result<()> {
    let bytes = serde_json::to_vec(value).with_context(|| format!("serialize {label}"))?;
    validate_bytes(label, bytes.len(), MAX_JSON_BYTES)
}

pub fn validate_cbor_result(label: &str, value: &impl Serialize) -> Result<()> {
    let mut encoded = Vec::new();
    ciborium::into_writer(value, &mut encoded).with_context(|| format!("encode {label}"))?;
    validate_bytes(label, encoded.len(), MAX_RESULT_BYTES)
}

pub fn validate_count(label: &str, actual: usize, max: usize) -> Result<()> {
    if actual > max {
        bail!("limit_exceeded: {label} has {actual} entries; maximum is {max}");
    }
    Ok(())
}

pub fn validate_bytes(label: &str, actual: usize, max: usize) -> Result<()> {
    if actual > max {
        bail!("limit_exceeded: {label} is {actual} bytes; maximum is {max} bytes");
    }
    Ok(())
}

fn file_content_into_bytes(content: FileContentInput) -> Vec<u8> {
    content.into_bytes()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn language_inputs_and_contexts_are_bounded() {
        assert!(validate_context_id("").is_err());
        assert!(validate_context_id(&"c".repeat(MAX_CONTEXT_ID_BYTES + 1)).is_err());
        assert!(validate_source("source", &"x".repeat(MAX_SOURCE_BYTES + 1)).is_err());
        assert!(ActorExecutionPtyOptions {
            cols: Some(0),
            rows: Some(24),
        }
        .validate()
        .is_err());
    }

    #[test]
    fn spawn_options_always_enable_bounded_replay() {
        let options = ActorLanguageSpawnOptions::default().into_core();
        assert!(options.retain_events);
    }

    #[test]
    fn synchronous_language_actions_have_sidecar_visible_timeouts() {
        assert_eq!(
            ActorLanguageExecutionOptions::default()
                .into_core()
                .timeout_ms,
            Some(MAX_TIMEOUT_MS)
        );
        assert_eq!(
            ActorTypeScriptCheckOptions::default()
                .into_core()
                .timeout_ms,
            Some(MAX_TIMEOUT_MS)
        );
        assert_eq!(
            ActorNpmInstallOptions::default()
                .into_project_core()
                .timeout_ms,
            Some(MAX_TIMEOUT_MS)
        );
        assert_eq!(
            ActorPythonInstallOptions::default().into_core().timeout_ms,
            Some(MAX_TIMEOUT_MS)
        );
        assert!(validate_timeout(Some(0)).is_err());
    }

    #[test]
    fn execution_results_encode_binary_output_as_cbor_bytes() {
        let result = ActorCodeExecutionResult {
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
