#ifndef NAPI_V8_HANDLE_SCOPE_WRAPPER_H_
#define NAPI_V8_HANDLE_SCOPE_WRAPPER_H_

#include <v8.h>

struct napi_handle_scope_wrapper__ {
  explicit napi_handle_scope_wrapper__(v8::Isolate* isolate);

 private:
  v8::HandleScope scope_;
};

#endif  // NAPI_V8_HANDLE_SCOPE_WRAPPER_H_
