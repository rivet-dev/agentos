#include "napi_error_state.h"

namespace napi {

const napi_extended_error_info* error_state__::info() const {
  return &info_;
}

napi_status error_state__::set(napi_status status, const char* message) {
  info_.error_code = status;
  info_.engine_error_code = 0;
  info_.engine_reserved = nullptr;
  message_ = (message == nullptr) ? "" : message;
  info_.error_message = message_.empty() ? nullptr : message_.c_str();
  return status;
}

napi_status error_state__::clear() {
  return set(napi_ok, nullptr);
}

}  // namespace napi
