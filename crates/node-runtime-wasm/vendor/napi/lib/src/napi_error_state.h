#ifndef NAPI_ERROR_STATE_H_
#define NAPI_ERROR_STATE_H_

#include "js_native_api.h"

#include <string>

namespace napi {

class error_state__ {
 public:
  const napi_extended_error_info* info() const;
  napi_status set(napi_status status, const char* message);
  napi_status clear();

 private:
  napi_extended_error_info info_{};
  std::string message_;
};

}  // namespace napi

#endif  // NAPI_ERROR_STATE_H_
