#ifndef EDGE_MODULE_LOADER_H_
#define EDGE_MODULE_LOADER_H_

#include "node_api.h"
#include "edge_task_queue.h"

napi_status EdgeInstallModuleLoader(napi_env env, const char* entry_script_path);

// Store primordials and internalBinding in loader state so they are passed from C++ when calling
// the module wrapper (Node-aligned: fn->Call(context, undefined, argc, argv) with argv from C++).
// Call after the bootstrap prelude so every user module receives the same reference.
void EdgeSetPrimordials(napi_env env, napi_value primordials);
void EdgeSetInternalBinding(napi_env env, napi_value internal_binding);
void EdgeSetPrivateSymbols(napi_env env, napi_value private_symbols);
void EdgeSetPerIsolateSymbols(napi_env env, napi_value per_isolate_symbols);
napi_value EdgeGetPerContextExports(napi_env env);
napi_value EdgeGetPrivateSymbols(napi_env env);
napi_value EdgeGetPerIsolateSymbols(napi_env env);
napi_value EdgeGetRequireFunction(napi_env env);
napi_value EdgeGetInternalBinding(napi_env env);
void EdgeFinalizeModuleLoaderEnv(napi_env env);
bool EdgeRequireBuiltin(napi_env env, const char* id, napi_value* out);
napi_value EdgeGetBuiltinInternalBinding(napi_env env);
bool EdgeExecuteBuiltin(napi_env env, const char* id, napi_value* out);

#endif  // EDGE_MODULE_LOADER_H_
