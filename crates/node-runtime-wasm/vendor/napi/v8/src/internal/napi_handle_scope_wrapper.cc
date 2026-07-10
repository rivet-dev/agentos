#include "internal/napi_handle_scope_wrapper.h"

napi_handle_scope_wrapper__::napi_handle_scope_wrapper__(v8::Isolate* isolate)
    : scope_(isolate) {}
