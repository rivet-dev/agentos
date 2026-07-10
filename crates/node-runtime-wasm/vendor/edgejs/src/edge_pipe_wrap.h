#ifndef EDGE_PIPE_WRAP_H_
#define EDGE_PIPE_WRAP_H_

#include <uv.h>

#include "node_api.h"

struct EdgeStreamListener;
struct EdgeStreamBase;

napi_value EdgeInstallPipeWrapBinding(napi_env env);
uv_stream_t* EdgePipeWrapGetStream(napi_env env, napi_value value);
EdgeStreamBase* EdgePipeWrapGetStreamBase(napi_env env, napi_value value);
bool EdgePipeWrapPushStreamListener(uv_stream_t* stream, EdgeStreamListener* listener);
bool EdgePipeWrapRemoveStreamListener(uv_stream_t* stream, EdgeStreamListener* listener);

#endif  // EDGE_PIPE_WRAP_H_
