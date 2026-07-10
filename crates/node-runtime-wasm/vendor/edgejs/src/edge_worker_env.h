#ifndef EDGE_WORKER_ENV_H_
#define EDGE_WORKER_ENV_H_

#include "edge_environment.h"

void EdgeWorkerEnvConfigure(napi_env env, const EdgeWorkerEnvConfig& config);
bool EdgeWorkerEnvGetConfig(napi_env env, EdgeWorkerEnvConfig* out);

bool EdgeWorkerEnvIsMainThread(napi_env env);
bool EdgeWorkerEnvIsInternalThread(napi_env env);
bool EdgeWorkerEnvOwnsProcessState(napi_env env);
bool EdgeWorkerEnvSharesEnvironment(napi_env env);
bool EdgeWorkerEnvTracksUnmanagedFds(napi_env env);
void EdgeWorkerEnvAddUnmanagedFd(napi_env env, int fd);
void EdgeWorkerEnvRemoveUnmanagedFd(napi_env env, int fd);
bool EdgeWorkerEnvStopRequested(napi_env env);
int32_t EdgeWorkerEnvThreadId(napi_env env);
std::string EdgeWorkerEnvThreadName(napi_env env);
std::array<double, 4> EdgeWorkerEnvResourceLimits(napi_env env);
std::string EdgeWorkerEnvGetProcessTitle(napi_env env);
void EdgeWorkerEnvSetProcessTitle(napi_env env, const std::string& title);
uint32_t EdgeWorkerEnvGetDebugPort(napi_env env);
void EdgeWorkerEnvSetDebugPort(napi_env env, uint32_t port);
std::map<std::string, std::string> EdgeWorkerEnvSnapshotEnvVars(napi_env env);
void EdgeWorkerEnvSetLocalEnvVar(napi_env env, const std::string& key, const std::string& value);
void EdgeWorkerEnvUnsetLocalEnvVar(napi_env env, const std::string& key);
void EdgeWorkerEnvRequestStop(napi_env env);
void EdgeWorkerEnvForget(napi_env env);
void EdgeWorkerEnvRunCleanup(napi_env env);
void EdgeWorkerEnvRunCleanupPreserveLoop(napi_env env);
uv_loop_t* EdgeWorkerEnvReleaseEventLoop(napi_env env);
void EdgeWorkerEnvDestroyReleasedEventLoop(uv_loop_t* loop);

napi_value EdgeWorkerEnvGetBinding(napi_env env);
void EdgeWorkerEnvSetBinding(napi_env env, napi_value binding);

napi_value EdgeWorkerEnvGetEnvMessagePort(napi_env env);
void EdgeWorkerEnvSetEnvMessagePort(napi_env env, napi_value port);
internal_binding::EdgeMessagePortDataPtr EdgeWorkerEnvGetEnvMessagePortData(napi_env env);

#endif  // EDGE_WORKER_ENV_H_
