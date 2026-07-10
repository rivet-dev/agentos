#ifndef NAPI_PERIODIC_GATE_H_
#define NAPI_PERIODIC_GATE_H_

#include <cstdint>

namespace napi {

class periodic_gate__ {
 public:
  explicit periodic_gate__(int64_t interval_ms);

  bool should_fire(int64_t now_ms);
  void reset();
  int64_t interval_ms() const;

 private:
  int64_t interval_ms_ = 0;
  int64_t last_ms_ = 0;
};

}  // namespace napi

#endif  // NAPI_PERIODIC_GATE_H_
