#ifndef EDGE_RUNTIME_PLATFORM_H_
#define EDGE_RUNTIME_PLATFORM_H_

#include "node_api.h"

using EdgeRuntimePlatformTaskCallback = void (*)(napi_env env, void* data);
using EdgeRuntimePlatformTaskCleanup = void (*)(napi_env env, void* data);

enum EdgeRuntimePlatformTaskFlags : int {
  kEdgeRuntimePlatformTaskNone = 0,
  kEdgeRuntimePlatformTaskRefed = 1 << 0,
};

// Queue a native immediate/platform task for the current env. Tasks run on the
// owning thread before JS immediates, mirroring Node's native immediate queue.
// Immediate-task APIs are owning-thread-only; cross-thread engine work must use
// the foreground task enqueue hook instead.
napi_status EdgeRuntimePlatformEnqueueTask(napi_env env,
                                         EdgeRuntimePlatformTaskCallback callback,
                                         void* data,
                                         EdgeRuntimePlatformTaskCleanup cleanup,
                                         int flags);

// Drain queued native immediate tasks. Returns the number of tasks run.
size_t EdgeRuntimePlatformDrainImmediateTasks(napi_env env, bool only_refed = false);

bool EdgeRuntimePlatformHasImmediateTasks(napi_env env);
bool EdgeRuntimePlatformHasRefedImmediateTasks(napi_env env);

// Attach the current env to the embedder-owned foreground task queue hook.
// Edge owns queueing and drain policy; engine backends only post work into it.
napi_status EdgeRuntimePlatformInstallHooks(napi_env env);

napi_status EdgeRuntimePlatformEnqueueForegroundTask(napi_env env,
                                                    EdgeRuntimePlatformTaskCallback callback,
                                                    void* data,
                                                    EdgeRuntimePlatformTaskCleanup cleanup,
                                                    uint64_t delay_millis = 0);

napi_status EdgeRuntimePlatformAddRef(napi_env env);
napi_status EdgeRuntimePlatformReleaseRef(napi_env env);

// Drain Edge-owned foreground tasks that were posted by the engine adapter.
napi_status EdgeRuntimePlatformDrainTasks(napi_env env);
void EdgeRunRuntimePlatformEnvCleanup(napi_env env);

#endif  // EDGE_RUNTIME_PLATFORM_H_
