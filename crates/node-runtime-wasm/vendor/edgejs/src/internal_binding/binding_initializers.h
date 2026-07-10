#ifndef EDGE_INTERNAL_BINDING_BINDING_INITIALIZERS_H_
#define EDGE_INTERNAL_BINDING_BINDING_INITIALIZERS_H_

#include "node_api.h"

namespace internal_binding {

napi_value InitAsyncContextFrame(napi_env env);
napi_value InitAsyncWrap(napi_env env);
napi_value InitBlob(napi_env env);
napi_value InitBlockList(napi_env env);
napi_value InitConfig(napi_env env);
napi_value InitConstants(napi_env env);
napi_value InitCredentials(napi_env env);
napi_value InitCrypto(napi_env env);
napi_value InitFs(napi_env env);
napi_value InitFsDir(napi_env env);
napi_value InitFsEventWrap(napi_env env);
napi_value InitHeapUtils(napi_env env);
napi_value InitHttp2(napi_env env);
napi_value InitIcu(napi_env env);
napi_value InitInspector(napi_env env);
napi_value InitInternalOnlyV8(napi_env env);
napi_value InitMessaging(napi_env env);
napi_value InitMksnapshot(napi_env env);
napi_value InitModuleWrap(napi_env env);
napi_value InitPerformance(napi_env env);
napi_value InitPermission(napi_env env);
napi_value InitSea(napi_env env);
napi_value InitSerdes(napi_env env);
napi_value InitStreamPipe(napi_env env);
napi_value InitSymbols(napi_env env);
napi_value InitUndici(napi_env env);
napi_value InitUtil(napi_env env);
napi_value InitV8(napi_env env);
napi_value InitWasmWebApi(napi_env env);
napi_value InitWatchdog(napi_env env);
napi_value InitWorker(napi_env env);
napi_value InitZlib(napi_env env);

}  // namespace internal_binding

#endif  // EDGE_INTERNAL_BINDING_BINDING_INITIALIZERS_H_
