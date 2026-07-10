#ifndef NAPI_V8_ESCAPABLE_HANDLE_SCOPE_WRAPPER_H_
#define NAPI_V8_ESCAPABLE_HANDLE_SCOPE_WRAPPER_H_

#include <v8.h>

struct napi_escapable_handle_scope_wrapper__ {
  explicit napi_escapable_handle_scope_wrapper__(v8::Isolate* isolate);

  bool escape_called() const { return escape_called_; }

  template <typename T>
  v8::Local<T> Escape(v8::Local<T> handle) {
    escape_called_ = true;
    return scope_.Escape(handle);
  }

 private:
  v8::EscapableHandleScope scope_;
  bool escape_called_ = false;
};

#endif  // NAPI_V8_ESCAPABLE_HANDLE_SCOPE_WRAPPER_H_
