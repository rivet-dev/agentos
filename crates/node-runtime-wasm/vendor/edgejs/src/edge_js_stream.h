#ifndef EDGE_JS_STREAM_H_
#define EDGE_JS_STREAM_H_

#include "node_api.h"

struct EdgeStreamBase;

napi_value EdgeInstallJsStreamBinding(napi_env env);
int EdgeJsStreamWriteBuffer(EdgeStreamBase* base,
                           napi_value req_obj,
                           napi_value payload,
                           bool* async_out);

#endif  // EDGE_JS_STREAM_H_
