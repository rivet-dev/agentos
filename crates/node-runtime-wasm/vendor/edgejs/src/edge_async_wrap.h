#ifndef EDGE_ASYNC_WRAP_H_
#define EDGE_ASYNC_WRAP_H_

#include <cstdint>

#include "node_api.h"

enum EdgeAsyncProviderType : int32_t {
  kEdgeProviderNone = 0,
  kEdgeProviderHttpClientRequest = 10,
  kEdgeProviderHttpIncomingMessage = 11,
  kEdgeProviderJsStream = 20,
  kEdgeProviderJsUdpWrap = 21,
  kEdgeProviderMessagePort = 22,
  kEdgeProviderPipeConnectWrap = 23,
  kEdgeProviderPipeServerWrap = 24,
  kEdgeProviderPipeWrap = 25,
  kEdgeProviderProcessWrap = 26,
  kEdgeProviderShutdownWrap = 35,
  kEdgeProviderTcpConnectWrap = 39,
  kEdgeProviderTcpServerWrap = 40,
  kEdgeProviderTcpWrap = 41,
  kEdgeProviderTtyWrap = 42,
  kEdgeProviderUdpSendWrap = 43,
  kEdgeProviderUdpWrap = 44,
  kEdgeProviderWriteWrap = 52,
  kEdgeProviderZlib = 53,
  kEdgeProviderWorker = 54,
  kEdgeProviderTlsWrap = 55,
};

int64_t EdgeAsyncWrapNextId(napi_env env);
void EdgeAsyncWrapQueueDestroyId(napi_env env, int64_t async_id);
void EdgeAsyncWrapReset(napi_env env, int64_t* async_id);
int64_t EdgeAsyncWrapExecutionAsyncId(napi_env env);
int64_t EdgeAsyncWrapCurrentExecutionAsyncId(napi_env env);
const char* EdgeAsyncWrapProviderName(int32_t provider_type);
void EdgeAsyncWrapEmitInitString(napi_env env,
                                int64_t async_id,
                                const char* type,
                                int64_t trigger_async_id,
                                napi_value resource);
void EdgeAsyncWrapEmitInit(napi_env env,
                          int64_t async_id,
                          int32_t provider_type,
                          int64_t trigger_async_id,
                          napi_value resource);
void EdgeAsyncWrapEmitBefore(napi_env env, int64_t async_id);
void EdgeAsyncWrapEmitAfter(napi_env env, int64_t async_id);
void EdgeAsyncWrapPushContext(napi_env env,
                             int64_t async_id,
                             int64_t trigger_async_id,
                             napi_value resource);
bool EdgeAsyncWrapPopContext(napi_env env, int64_t async_id);
napi_status EdgeAsyncWrapMakeCallback(napi_env env,
                                     int64_t async_id,
                                     napi_value resource,
                                     napi_value recv,
                                     napi_value callback,
                                     size_t argc,
                                     napi_value* argv,
                                     napi_value* result,
                                     int flags);

#endif  // EDGE_ASYNC_WRAP_H_
