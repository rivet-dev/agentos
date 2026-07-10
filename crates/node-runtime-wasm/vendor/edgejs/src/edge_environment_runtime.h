#ifndef EDGE_ENVIRONMENT_RUNTIME_H_
#define EDGE_ENVIRONMENT_RUNTIME_H_

#include "edge_environment.h"

#if defined(ENABLE_TRACING)
using EdgeStartupTraceCallback = void (*)(void* data, const char* phase);
#endif

bool EdgeAttachEnvironmentForRuntime(napi_env env,
                                     const EdgeEnvironmentConfig* config = nullptr
#if defined(ENABLE_TRACING)
                                     ,
                                     EdgeStartupTraceCallback trace_callback = nullptr,
                                     void* trace_data = nullptr
#endif
);

#endif  // EDGE_ENVIRONMENT_RUNTIME_H_
