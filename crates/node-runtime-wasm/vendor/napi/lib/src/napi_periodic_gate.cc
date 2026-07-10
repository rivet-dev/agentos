#include "napi_periodic_gate.h"

namespace napi {

periodic_gate__::periodic_gate__(int64_t interval_ms) : interval_ms_(interval_ms) {}

bool periodic_gate__::should_fire(int64_t now_ms) {
  if (interval_ms_ <= 0) {
    return false;
  }
  if (last_ms_ == 0) {
    last_ms_ = now_ms;
    return false;
  }
  if (now_ms - last_ms_ < interval_ms_) {
    return false;
  }
  last_ms_ = now_ms;
  return true;
}

void periodic_gate__::reset() {
  last_ms_ = 0;
}

int64_t periodic_gate__::interval_ms() const {
  return interval_ms_;
}

}  // namespace napi
