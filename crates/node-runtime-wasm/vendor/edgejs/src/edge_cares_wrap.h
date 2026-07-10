#ifndef EDGE_CARES_WRAP_H_
#define EDGE_CARES_WRAP_H_

#include "node_api.h"

napi_value EdgeInstallCaresWrapBinding(napi_env env);
void EdgeRunCaresWrapEnvCleanup(napi_env env);

#endif  // EDGE_CARES_WRAP_H_
