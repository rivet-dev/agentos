#include "internal/napi_escapable_handle_scope_wrapper.h"

napi_escapable_handle_scope_wrapper__::napi_escapable_handle_scope_wrapper__(
    v8::Isolate* isolate)
    : scope_(isolate) {}
