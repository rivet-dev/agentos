#ifndef EDGE_UTIL_H_
#define EDGE_UTIL_H_

#include "node_api.h"

napi_value EdgeCreatePrivateSymbolsObject(napi_env env);
napi_value EdgeCreatePerIsolateSymbolsObject(napi_env env);
napi_value EdgeInstallUtilBinding(napi_env env);
napi_value EdgeGetTypesBinding(napi_env env);

#endif  // EDGE_UTIL_H_
