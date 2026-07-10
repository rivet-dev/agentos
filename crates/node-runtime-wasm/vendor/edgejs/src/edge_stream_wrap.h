#ifndef EDGE_STREAM_WRAP_H_
#define EDGE_STREAM_WRAP_H_

#include <cstdint>

#include "node_api.h"

enum EdgeStreamStateIndex : int {
  kEdgeReadBytesOrError = 0,
  kEdgeArrayBufferOffset = 1,
  kEdgeBytesWritten = 2,
  kEdgeLastWriteWasAsync = 3,
  kEdgeStreamStateLength = 4,
};

napi_value EdgeInstallStreamWrapBinding(napi_env env);
int32_t* EdgeGetStreamBaseState(napi_env env);
napi_value EdgeCreateStreamReqObject(napi_env env);

int64_t EdgeStreamReqGetAsyncId(napi_env env, napi_value req_obj);
int32_t EdgeStreamReqGetProviderType(napi_env env, napi_value req_obj);
void EdgeStreamReqActivate(napi_env env,
                          napi_value req_obj,
                          int32_t provider_type,
                          int64_t trigger_async_id);
void EdgeStreamReqMarkDone(napi_env env, napi_value req_obj);

#endif  // EDGE_STREAM_WRAP_H_
