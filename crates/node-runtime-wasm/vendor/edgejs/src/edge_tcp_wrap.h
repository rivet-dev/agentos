#ifndef EDGE_TCP_WRAP_H_
#define EDGE_TCP_WRAP_H_

#include "node_api.h"
#include <uv.h>

struct EdgeStreamListener;
struct EdgeStreamBase;

napi_value EdgeInstallTcpWrapBinding(napi_env env);
napi_value EdgeGetTcpWrapConstructor(napi_env env);
uv_stream_t* EdgeTcpWrapGetStream(napi_env env, napi_value value);
EdgeStreamBase* EdgeTcpWrapGetStreamBase(napi_env env, napi_value value);
bool EdgeTcpWrapPushStreamListener(uv_stream_t* stream, EdgeStreamListener* listener);
bool EdgeTcpWrapRemoveStreamListener(uv_stream_t* stream, EdgeStreamListener* listener);

#endif  // EDGE_TCP_WRAP_H_
