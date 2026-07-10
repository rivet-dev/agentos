#ifndef EDGE_PROCESS_WRAP_H_
#define EDGE_PROCESS_WRAP_H_

#include "node_api.h"

napi_value EdgeInstallProcessWrapBinding(napi_env env);
void EdgeProcessWrapForceKillTrackedChildren();

#endif  // EDGE_PROCESS_WRAP_H_
