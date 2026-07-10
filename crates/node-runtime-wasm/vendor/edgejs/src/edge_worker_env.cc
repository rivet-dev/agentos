#include "edge_worker_env.h"

void EdgeWorkerEnvConfigure(napi_env env, const EdgeWorkerEnvConfig& config) {
  if (env == nullptr) return;
  (void)EdgeEnvironmentAttach(env, &config);
}

bool EdgeWorkerEnvGetConfig(napi_env env, EdgeWorkerEnvConfig* out) {
  return EdgeEnvironmentGetConfig(env, out);
}

bool EdgeWorkerEnvIsMainThread(napi_env env) {
  EdgeWorkerEnvConfig config;
  EdgeWorkerEnvGetConfig(env, &config);
  return config.is_main_thread;
}

bool EdgeWorkerEnvIsInternalThread(napi_env env) {
  EdgeWorkerEnvConfig config;
  EdgeWorkerEnvGetConfig(env, &config);
  return config.is_internal_thread;
}

bool EdgeWorkerEnvOwnsProcessState(napi_env env) {
  if (auto* environment = EdgeEnvironmentGet(env); environment != nullptr) {
    return environment->owns_process_state();
  }
  return true;
}

bool EdgeWorkerEnvSharesEnvironment(napi_env env) {
  if (auto* environment = EdgeEnvironmentGet(env); environment != nullptr) {
    return environment->shares_environment();
  }
  return true;
}

bool EdgeWorkerEnvTracksUnmanagedFds(napi_env env) {
  if (auto* environment = EdgeEnvironmentGet(env); environment != nullptr) {
    return environment->tracks_unmanaged_fds();
  }
  return false;
}

void EdgeWorkerEnvAddUnmanagedFd(napi_env env, int fd) {
  if (auto* environment = EdgeEnvironmentGet(env); environment != nullptr) {
    environment->AddUnmanagedFd(fd);
  }
}

void EdgeWorkerEnvRemoveUnmanagedFd(napi_env env, int fd) {
  if (auto* environment = EdgeEnvironmentGet(env); environment != nullptr) {
    environment->RemoveUnmanagedFd(fd);
  }
}

bool EdgeWorkerEnvStopRequested(napi_env env) {
  if (auto* environment = EdgeEnvironmentGet(env); environment != nullptr) {
    return environment->stop_requested();
  }
  return false;
}

int32_t EdgeWorkerEnvThreadId(napi_env env) {
  if (auto* environment = EdgeEnvironmentGet(env); environment != nullptr) {
    return environment->thread_id();
  }
  return 0;
}

std::string EdgeWorkerEnvThreadName(napi_env env) {
  if (auto* environment = EdgeEnvironmentGet(env); environment != nullptr) {
    return environment->thread_name();
  }
  return "main";
}

std::array<double, 4> EdgeWorkerEnvResourceLimits(napi_env env) {
  if (auto* environment = EdgeEnvironmentGet(env); environment != nullptr) {
    return environment->resource_limits();
  }
  return {-1, -1, -1, -1};
}

std::string EdgeWorkerEnvGetProcessTitle(napi_env env) {
  if (auto* environment = EdgeEnvironmentGet(env); environment != nullptr) {
    return environment->process_title();
  }
  return {};
}

void EdgeWorkerEnvSetProcessTitle(napi_env env, const std::string& title) {
  if (auto* environment = EdgeEnvironmentGet(env); environment != nullptr) {
    environment->set_process_title(title);
  }
}

uint32_t EdgeWorkerEnvGetDebugPort(napi_env env) {
  if (auto* environment = EdgeEnvironmentGet(env); environment != nullptr) {
    return environment->debug_port();
  }
  return 0;
}

void EdgeWorkerEnvSetDebugPort(napi_env env, uint32_t port) {
  if (auto* environment = EdgeEnvironmentGet(env); environment != nullptr) {
    environment->set_debug_port(port);
  }
}

std::map<std::string, std::string> EdgeWorkerEnvSnapshotEnvVars(napi_env env) {
  if (auto* environment = EdgeEnvironmentGet(env); environment != nullptr) {
    return environment->snapshot_env_vars();
  }
  return {};
}

void EdgeWorkerEnvSetLocalEnvVar(napi_env env, const std::string& key, const std::string& value) {
  if (auto* environment = EdgeEnvironmentGet(env); environment != nullptr) {
    environment->set_local_env_var(key, value);
  }
}

void EdgeWorkerEnvUnsetLocalEnvVar(napi_env env, const std::string& key) {
  if (auto* environment = EdgeEnvironmentGet(env); environment != nullptr) {
    environment->unset_local_env_var(key);
  }
}

void EdgeWorkerEnvRequestStop(napi_env env) {
  if (auto* environment = EdgeEnvironmentGet(env); environment != nullptr) {
    environment->RequestStop();
  }
}

void EdgeWorkerEnvForget(napi_env env) {
  edge::Environment::Detach(env);
}

void EdgeWorkerEnvRunCleanup(napi_env env) {
  EdgeEnvironmentRunCleanup(env);
}

void EdgeWorkerEnvRunCleanupPreserveLoop(napi_env env) {
  EdgeEnvironmentRunCleanupPreserveLoop(env);
}

uv_loop_t* EdgeWorkerEnvReleaseEventLoop(napi_env env) {
  return EdgeEnvironmentReleaseEventLoop(env);
}

void EdgeWorkerEnvDestroyReleasedEventLoop(uv_loop_t* loop) {
  EdgeEnvironmentDestroyReleasedEventLoop(loop);
}

napi_value EdgeWorkerEnvGetBinding(napi_env env) {
  if (auto* environment = EdgeEnvironmentGet(env); environment != nullptr) {
    return environment->binding();
  }
  return nullptr;
}

void EdgeWorkerEnvSetBinding(napi_env env, napi_value binding) {
  if (auto* environment = EdgeEnvironmentGet(env); environment != nullptr) {
    environment->set_binding(binding);
  }
}

napi_value EdgeWorkerEnvGetEnvMessagePort(napi_env env) {
  if (auto* environment = EdgeEnvironmentGet(env); environment != nullptr) {
    return environment->env_message_port();
  }
  return nullptr;
}

void EdgeWorkerEnvSetEnvMessagePort(napi_env env, napi_value port) {
  if (auto* environment = EdgeEnvironmentGet(env); environment != nullptr) {
    environment->set_env_message_port(port);
  }
}

internal_binding::EdgeMessagePortDataPtr EdgeWorkerEnvGetEnvMessagePortData(napi_env env) {
  if (auto* environment = EdgeEnvironmentGet(env); environment != nullptr) {
    return environment->env_message_port_data();
  }
  return {};
}
