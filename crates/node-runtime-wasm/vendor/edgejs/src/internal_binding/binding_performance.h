#ifndef EDGE_INTERNAL_BINDING_PERFORMANCE_H_
#define EDGE_INTERNAL_BINDING_PERFORMANCE_H_

#include "node_api.h"

namespace internal_binding {

bool PerformanceHasObserver(napi_env env, uint32_t type_index);
void PerformanceEmitEntry(napi_env env,
                          const char* name,
                          const char* entry_type,
                          double start_time,
                          double duration,
                          napi_value details);

}  // namespace internal_binding

#endif  // EDGE_INTERNAL_BINDING_PERFORMANCE_H_
