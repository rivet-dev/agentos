#ifndef EDGE_ERRORS_BINDING_H_
#define EDGE_ERRORS_BINDING_H_

#include <string>

#include "node_api.h"

napi_value EdgeGetOrCreateErrorsBinding(napi_env env);
std::string EdgeFormatFatalExceptionAfterInspector(napi_env env, napi_value exception);

#endif  // EDGE_ERRORS_BINDING_H_
