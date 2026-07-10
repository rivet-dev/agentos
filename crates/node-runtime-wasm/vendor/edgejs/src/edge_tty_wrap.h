#ifndef EDGE_TTY_WRAP_H_
#define EDGE_TTY_WRAP_H_

#include <uv.h>

#include "node_api.h"

struct EdgeStreamBase;

napi_value EdgeInstallTtyWrapBinding(napi_env env);
uv_stream_t* EdgeTtyWrapGetStream(napi_env env, napi_value value);
EdgeStreamBase* EdgeTtyWrapGetStreamBase(napi_env env, napi_value value);

#endif  // EDGE_TTY_WRAP_H_
