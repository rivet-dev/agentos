#ifndef EDGE_HANDLE_SCOPE_H_
#define EDGE_HANDLE_SCOPE_H_

#include "node_api.h"

#include <cstdio>
#include <cstdlib>

namespace edge {

[[noreturn]] inline void FatalHandleScopeEnv(const char* scope_name) {
  std::fprintf(stderr, "FATAL ERROR: %s requires a non-null napi_env\n", scope_name);
  std::abort();
}

class HandleScope {
 public:
  explicit HandleScope(napi_env env) : env_(env) {
    if (env_ == nullptr) {
      FatalHandleScopeEnv("edge::HandleScope");
    }
    status_ = napi_open_handle_scope(env_, &scope_);
  }

  ~HandleScope() {
    if (env_ != nullptr && scope_ != nullptr) {
      napi_close_handle_scope(env_, scope_);
    }
  }

  HandleScope(const HandleScope&) = delete;
  HandleScope& operator=(const HandleScope&) = delete;
  HandleScope(HandleScope&&) = delete;
  HandleScope& operator=(HandleScope&&) = delete;

  bool is_open() const { return status_ == napi_ok && scope_ != nullptr; }
  napi_status status() const { return status_; }

 private:
  napi_env env_ = nullptr;
  napi_handle_scope scope_ = nullptr;
  napi_status status_ = napi_invalid_arg;
};

class EscapableHandleScope {
 public:
  explicit EscapableHandleScope(napi_env env) : env_(env) {
    if (env_ == nullptr) {
      FatalHandleScopeEnv("edge::EscapableHandleScope");
    }
    status_ = napi_open_escapable_handle_scope(env_, &scope_);
  }

  ~EscapableHandleScope() {
    if (env_ != nullptr && scope_ != nullptr) {
      napi_close_escapable_handle_scope(env_, scope_);
    }
  }

  EscapableHandleScope(const EscapableHandleScope&) = delete;
  EscapableHandleScope& operator=(const EscapableHandleScope&) = delete;
  EscapableHandleScope(EscapableHandleScope&&) = delete;
  EscapableHandleScope& operator=(EscapableHandleScope&&) = delete;

  napi_value Escape(napi_value value) {
    if (!is_open() || value == nullptr) return nullptr;
    napi_value result = nullptr;
    if (napi_escape_handle(env_, scope_, value, &result) != napi_ok) return nullptr;
    return result;
  }

  bool is_open() const { return status_ == napi_ok && scope_ != nullptr; }
  napi_status status() const { return status_; }

 private:
  napi_env env_ = nullptr;
  napi_escapable_handle_scope scope_ = nullptr;
  napi_status status_ = napi_invalid_arg;
};

}  // namespace edge

#endif  // EDGE_HANDLE_SCOPE_H_
