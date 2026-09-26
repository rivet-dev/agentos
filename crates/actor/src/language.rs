use std::sync::Arc;

use agentos_actor_contract::language::*;
use agentos_client::{AgentOs, ProcessDescriptor};
use anyhow::Result;
use rivetkit::{Ctx, Handles};

use crate::actions::BoxFuture;
use crate::process::{attach_process_events, ActorProcessId};
use crate::AgentOsActor;

const MAX_CONTEXTS: usize = 1_024;
const MAX_ARGUMENT_BYTES: usize = 16 * 1024;

crate::register_contract_action!(ContextsCreate);
crate::register_contract_action!(ContextsGet);
crate::register_contract_action!(ContextsReset);
crate::register_contract_action!(ContextsDelete);
crate::register_contract_action!(ContextsList);
crate::register_contract_action!(JavaScriptExecute);
crate::register_contract_action!(JavaScriptEvaluate);
crate::register_contract_action!(JavaScriptExecuteFile);
crate::register_contract_action!(JavaScriptSpawn);
crate::register_contract_action!(JavaScriptSpawnFile);
crate::register_contract_action!(JavaScriptNpmRunScript);
crate::register_contract_action!(JavaScriptNpmInstall);
crate::register_contract_action!(JavaScriptNpmRunPackage);
crate::register_contract_action!(TypeScriptExecute);
crate::register_contract_action!(TypeScriptEvaluate);
crate::register_contract_action!(TypeScriptExecuteFile);
crate::register_contract_action!(TypeScriptSpawn);
crate::register_contract_action!(TypeScriptSpawnFile);
crate::register_contract_action!(TypeScriptCheck);
crate::register_contract_action!(TypeScriptCheckProject);
crate::register_contract_action!(PythonExecute);
crate::register_contract_action!(PythonEvaluate);
crate::register_contract_action!(PythonExecuteFile);
crate::register_contract_action!(PythonExecuteModule);
crate::register_contract_action!(PythonSpawn);
crate::register_contract_action!(PythonSpawnFile);
crate::register_contract_action!(PythonSpawnModule);
crate::register_contract_action!(PythonInstall);

impl AgentOsActor {
    async fn language_vm(&self, context: Option<&ActorContextId>) -> Result<(AgentOs, u64)> {
        if let Some(context) = context {
            validate_context_handle(context)?;
            let vm = self.runtime.vm_at_generation(context.generation).await?;
            return Ok((vm, context.generation));
        }
        let status = self.runtime.status().await;
        let vm = self.runtime.vm_at_generation(status.generation).await?;
        Ok((vm, status.generation))
    }
}

/// Generates the mechanical hosted-action adapter for foreground language
/// operations. Validation and result shaping stay shared; each declaration
/// names only the Core method and the one field it forwards.
macro_rules! foreground_language_handler {
    ($action:ty => $output:ty, $field:ident, $validator:path, $label:literal, $method:ident, $map:path) => {
        impl Handles<$action> for AgentOsActor {
            type Future = BoxFuture<$output>;

            fn handle(self: Arc<Self>, _ctx: Ctx<Self>, action: $action) -> Self::Future {
                Box::pin(async move {
                    let _permit = self.admit_action()?;
                    $validator($label, &action.$field)?;
                    action.options.validate()?;
                    let (vm, _) = self.language_vm(action.options.context()).await?;
                    $map(
                        vm.$method(action.$field, action.options.into_core())
                            .await?,
                    )
                })
            }
        }
    };
}

/// Generates the corresponding adapter for background language operations.
macro_rules! spawned_language_handler {
    ($action:ty, $field:ident, $validator:path, $label:literal, $method:ident) => {
        impl Handles<$action> for AgentOsActor {
            type Future = BoxFuture<ActorProcessId>;

            fn handle(self: Arc<Self>, ctx: Ctx<Self>, action: $action) -> Self::Future {
                Box::pin(async move {
                    let _permit = self.admit_action()?;
                    $validator($label, &action.$field)?;
                    action.options.validate()?;
                    let (vm, generation) = self.language_vm(None).await?;
                    let descriptor = vm
                        .$method(action.$field, action.options.into_core())
                        .await?;
                    actor_spawn_result(&vm, ctx, generation, descriptor)
                })
            }
        }
    };
}

foreground_language_handler!(JavaScriptExecute => ActorCodeExecutionResult, source, validate_source, "JavaScript source", execute_javascript, actor_execution_result);
foreground_language_handler!(JavaScriptEvaluate => ActorCodeEvaluationResult, expression, validate_source, "JavaScript expression", evaluate_javascript, actor_evaluation_result);
foreground_language_handler!(JavaScriptExecuteFile => ActorCodeExecutionResult, path, validate_path, "JavaScript file path", execute_javascript_file, actor_execution_result);
spawned_language_handler!(
    JavaScriptSpawn,
    source,
    validate_source,
    "JavaScript source",
    spawn_javascript
);
spawned_language_handler!(
    JavaScriptSpawnFile,
    path,
    validate_path,
    "JavaScript file path",
    spawn_javascript_file
);
foreground_language_handler!(JavaScriptNpmRunScript => ActorCodeExecutionResult, script, validate_argument, "npm script", execute_npm_script, actor_execution_result);

foreground_language_handler!(TypeScriptExecute => ActorCodeExecutionResult, source, validate_source, "TypeScript source", execute_typescript, actor_execution_result);
foreground_language_handler!(TypeScriptEvaluate => ActorCodeEvaluationResult, expression, validate_source, "TypeScript expression", evaluate_typescript, actor_evaluation_result);
foreground_language_handler!(TypeScriptExecuteFile => ActorCodeExecutionResult, path, validate_path, "TypeScript file path", execute_typescript_file, actor_execution_result);
spawned_language_handler!(
    TypeScriptSpawn,
    source,
    validate_source,
    "TypeScript source",
    spawn_typescript
);
spawned_language_handler!(
    TypeScriptSpawnFile,
    path,
    validate_path,
    "TypeScript file path",
    spawn_typescript_file
);
foreground_language_handler!(TypeScriptCheck => ActorTypeScriptCheckResult, source, validate_source, "TypeScript source", check_typescript, actor_typescript_check_result);

foreground_language_handler!(PythonExecute => ActorCodeExecutionResult, source, validate_source, "Python source", execute_python, actor_execution_result);
foreground_language_handler!(PythonEvaluate => ActorCodeEvaluationResult, expression, validate_source, "Python expression", evaluate_python, actor_evaluation_result);
foreground_language_handler!(PythonExecuteFile => ActorCodeExecutionResult, path, validate_path, "Python file path", execute_python_file, actor_execution_result);
foreground_language_handler!(PythonExecuteModule => ActorCodeExecutionResult, module, validate_argument, "Python module", execute_python_module, actor_execution_result);
spawned_language_handler!(
    PythonSpawn,
    source,
    validate_source,
    "Python source",
    spawn_python
);
spawned_language_handler!(
    PythonSpawnFile,
    path,
    validate_path,
    "Python file path",
    spawn_python_file
);
spawned_language_handler!(
    PythonSpawnModule,
    module,
    validate_argument,
    "Python module",
    spawn_python_module
);

impl Handles<ContextsCreate> for AgentOsActor {
    type Future = BoxFuture<ActorContextDescriptor>;

    fn handle(self: Arc<Self>, _ctx: Ctx<Self>, action: ContextsCreate) -> Self::Future {
        Box::pin(async move {
            let _permit = self.admit_action()?;
            validate_context_id(&action.context_id)?;
            let (vm, generation) = self.language_vm(None).await?;
            let context = vm.create_context(&action.context_id).await?;
            Ok(actor_context_descriptor(generation, context))
        })
    }
}

impl Handles<ContextsGet> for AgentOsActor {
    type Future = BoxFuture<ActorContextDescriptor>;

    fn handle(self: Arc<Self>, _ctx: Ctx<Self>, action: ContextsGet) -> Self::Future {
        Box::pin(async move {
            let _permit = self.admit_action()?;
            validate_context_handle(&action.context)?;
            let vm = self
                .runtime
                .vm_at_generation(action.context.generation)
                .await?;
            let context = vm.get_context(&action.context.context_id).await?;
            Ok(actor_context_descriptor(action.context.generation, context))
        })
    }
}

impl Handles<ContextsList> for AgentOsActor {
    type Future = BoxFuture<Vec<ActorContextDescriptor>>;

    fn handle(self: Arc<Self>, _ctx: Ctx<Self>, _action: ContextsList) -> Self::Future {
        Box::pin(async move {
            let _permit = self.admit_action()?;
            let (vm, generation) = self.language_vm(None).await?;
            let contexts = vm.list_contexts().await?;
            validate_count("execution contexts", contexts.len(), MAX_CONTEXTS)?;
            Ok(contexts
                .into_iter()
                .map(|context| actor_context_descriptor(generation, context))
                .collect())
        })
    }
}

impl Handles<ContextsReset> for AgentOsActor {
    type Future = BoxFuture<ActorContextDescriptor>;

    fn handle(self: Arc<Self>, _ctx: Ctx<Self>, action: ContextsReset) -> Self::Future {
        Box::pin(async move {
            let _permit = self.admit_action()?;
            validate_context_handle(&action.context)?;
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

impl Handles<ContextsDelete> for AgentOsActor {
    type Future = BoxFuture<()>;

    fn handle(self: Arc<Self>, _ctx: Ctx<Self>, action: ContextsDelete) -> Self::Future {
        Box::pin(async move {
            let _permit = self.admit_action()?;
            validate_context_handle(&action.context)?;
            self.runtime
                .vm_at_generation(action.context.generation)
                .await?
                .delete_context(&action.context.context_id)
                .await?;
            Ok(())
        })
    }
}

impl Handles<JavaScriptNpmInstall> for AgentOsActor {
    type Future = BoxFuture<ActorCodeExecutionResult>;

    fn handle(self: Arc<Self>, _ctx: Ctx<Self>, action: JavaScriptNpmInstall) -> Self::Future {
        Box::pin(async move {
            let _permit = self.admit_action()?;
            action.options.validate(&action.packages)?;
            let (vm, _) = self.language_vm(action.options.context()).await?;
            let result = if action.packages.is_empty() {
                vm.install_npm_project(action.options.into_project_core())
                    .await?
            } else {
                vm.install_npm_packages(action.packages, action.options.into_package_core())
                    .await?
            };
            actor_execution_result(result)
        })
    }
}

impl Handles<JavaScriptNpmRunPackage> for AgentOsActor {
    type Future = BoxFuture<ActorCodeExecutionResult>;

    fn handle(self: Arc<Self>, _ctx: Ctx<Self>, action: JavaScriptNpmRunPackage) -> Self::Future {
        Box::pin(async move {
            let _permit = self.admit_action()?;
            validate_string("npm package spec", &action.package_spec, MAX_ARGUMENT_BYTES)?;
            validate_optional_string("npm binary", action.binary.as_deref(), MAX_ARGUMENT_BYTES)?;
            action.options.validate()?;
            let (vm, _) = self.language_vm(action.options.context()).await?;
            actor_execution_result(
                vm.execute_npm_package(
                    action.package_spec,
                    action.binary,
                    action.options.into_core(),
                )
                .await?,
            )
        })
    }
}

impl Handles<TypeScriptCheckProject> for AgentOsActor {
    type Future = BoxFuture<ActorTypeScriptCheckResult>;

    fn handle(self: Arc<Self>, _ctx: Ctx<Self>, action: TypeScriptCheckProject) -> Self::Future {
        Box::pin(async move {
            let _permit = self.admit_action()?;
            action.options.validate()?;
            let (vm, _) = self.language_vm(action.options.context()).await?;
            actor_typescript_check_result(
                vm.check_typescript_project(action.options.into_core())
                    .await?,
            )
        })
    }
}

impl Handles<PythonInstall> for AgentOsActor {
    type Future = BoxFuture<ActorCodeExecutionResult>;

    fn handle(self: Arc<Self>, _ctx: Ctx<Self>, action: PythonInstall) -> Self::Future {
        Box::pin(async move {
            let _permit = self.admit_action()?;
            action.options.validate(&action.packages)?;
            let (vm, _) = self.language_vm(action.options.context()).await?;
            actor_execution_result(
                vm.install_python_packages(action.packages, action.options.into_core())
                    .await?,
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
