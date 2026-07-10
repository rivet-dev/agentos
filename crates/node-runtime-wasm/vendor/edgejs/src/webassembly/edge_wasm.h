#ifndef EDGE_WEBASSEMBLY_EDGE_WASM_H_
#define EDGE_WEBASSEMBLY_EDGE_WASM_H_

#include <string>

#include "node_api.h"

bool EdgeInstallQuickJsWebAssembly(napi_env env, std::string *error_out);

#endif // EDGE_WEBASSEMBLY_EDGE_WASM_H_
