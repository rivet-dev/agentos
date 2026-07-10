#include "edge_environment_runtime.h"

#include "edge_cares_wrap.h"
#include "edge_runtime_platform.h"
#include "edge_worker_control.h"
#include "unofficial_napi.h"

namespace {

constexpr int kCleanupStopWorkers = 10;
constexpr int kCleanupRuntimePlatform = 30;
constexpr int kCleanupCares = 35;
void StopWorkersCleanup(napi_env env, void* /*arg*/) {
  EdgeWorkerStopAllForEnv(env);
}

void RuntimePlatformCleanup(napi_env env, void* /*arg*/) {
  EdgeRunRuntimePlatformEnvCleanup(env);
}

void CaresCleanup(napi_env env, void* /*arg*/) {
  EdgeRunCaresWrapEnvCleanup(env);
}

void ExitCurrentEnvironment(edge::Environment* environment, bool stop_sub_workers) {
  if (environment == nullptr) return;

  napi_env env = environment->env();
  if (stop_sub_workers) {
    EdgeWorkerStopAllForEnv(env);
  }

  if (uv_loop_t* loop = environment->GetExistingEventLoop(); loop != nullptr) {
    uv_stop(loop);
  }

  // Node hard-exits the main thread here. Edge still needs to unwind through
  // the embedder cleanup path, so terminate the current env instead.
  (void)unofficial_napi_terminate_execution(env);
}

void MainThreadProcessExitHandler(edge::Environment* environment, int /*exit_code*/) {
  ExitCurrentEnvironment(environment, true);
}

void WorkerThreadProcessExitHandler(edge::Environment* environment, int /*exit_code*/) {
  ExitCurrentEnvironment(environment, false);
}
}  // namespace

bool EdgeAttachEnvironmentForRuntime(napi_env env,
                                     const EdgeEnvironmentConfig* config
#if defined(ENABLE_TRACING)
                                     ,
                                     EdgeStartupTraceCallback trace_callback,
                                     void* trace_data
#endif
) {
  if (!EdgeEnvironmentAttach(env, config)) return false;
#if defined(ENABLE_TRACING)
  if (trace_callback != nullptr) trace_callback(trace_data, "cli.env.attach-runtime.environment");
#endif

  if (auto* environment = EdgeEnvironmentGet(env); environment != nullptr) {
    environment->SetProcessExitHandler(
        environment->owns_process_state() ? MainThreadProcessExitHandler
                                          : WorkerThreadProcessExitHandler);
  }
#if defined(ENABLE_TRACING)
  if (trace_callback != nullptr) trace_callback(trace_data, "cli.env.attach-runtime.process-exit-handler");
#endif

  EdgeEnvironmentRegisterCleanupStage(env, StopWorkersCleanup, nullptr, kCleanupStopWorkers);
  EdgeEnvironmentRegisterCleanupStage(
      env, RuntimePlatformCleanup, nullptr, kCleanupRuntimePlatform);
  EdgeEnvironmentRegisterCleanupStage(env, CaresCleanup, nullptr, kCleanupCares);
#if defined(ENABLE_TRACING)
  if (trace_callback != nullptr) trace_callback(trace_data, "cli.env.attach-runtime.cleanup-stages");
#endif
  return true;
}
