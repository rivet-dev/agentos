#ifndef NAPI_V8_LIFETIME_MACROS_H_
#define NAPI_V8_LIFETIME_MACROS_H_

#include <napi_lifetime_tracker.h>

#ifdef NAPI_ENABLE_LIFETIME_TRACKER
#include "internal/napi_lifetime_tracker.h"
#endif

#define NAPI_V8_LIFETIME_DUMP(env, reason) \
  NAPI_LIFETIME_DUMP(v8impl::detail::napi_lifetime_tracker__, env, reason)

#endif  // NAPI_V8_LIFETIME_MACROS_H_
