#ifndef EDGE_ENV_LOOP_H_
#define EDGE_ENV_LOOP_H_

#include "node_api.h"

#include <uv.h>

napi_status EdgeEnsureEnvLoop(napi_env env, uv_loop_t** loop_out);
uv_loop_t* EdgeGetEnvLoop(napi_env env);
uv_loop_t* EdgeGetExistingEnvLoop(napi_env env);

#endif  // EDGE_ENV_LOOP_H_
