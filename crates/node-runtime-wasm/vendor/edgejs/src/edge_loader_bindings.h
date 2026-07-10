#ifndef EDGE_LOADER_BINDINGS_H_
#define EDGE_LOADER_BINDINGS_H_

#include "node_api.h"

napi_value EdgeInstallBuiltinsBinding(napi_env env);
napi_value EdgeInstallContextifyBinding(napi_env env);
napi_value EdgeInstallModulesBinding(napi_env env);
napi_value EdgeInstallOptionsBinding(napi_env env);
napi_value EdgeInstallTraceEventsBinding(napi_env env);
napi_value EdgeInstallUvBinding(napi_env env);

#endif  // EDGE_LOADER_BINDINGS_H_
