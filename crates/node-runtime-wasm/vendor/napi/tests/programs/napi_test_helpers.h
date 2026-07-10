#pragma once

#include <stdio.h>
#include <string.h>

#include "js_native_api.h"

// Custom import: get an initialized napi_env from the host.
// On WASM, this is imported from the "napi" module.
// On native, this is linked from napi_native_init.cc.
#ifdef __cplusplus
extern "C" {
#endif

#ifdef __wasm__
__attribute__((__import_module__("napi")))
#endif
napi_env napi_wasm_init_env(void);

#ifdef __cplusplus
}
#endif

#define NAPI_CALL(env, call)                                                   \
  do {                                                                         \
    napi_status _status = (call);                                              \
    if (_status != napi_ok) {                                                  \
      printf("FAIL: %s returned %d at %s:%d\n", #call, (int)_status,          \
             __FILE__, __LINE__);                                              \
      return 1;                                                                \
    }                                                                          \
  } while (0)

#define CHECK_OR_FAIL(cond, msg)                                               \
  do {                                                                         \
    if (!(cond)) {                                                             \
      printf("FAIL: %s\n", (msg));                                             \
      return 1;                                                                \
    }                                                                          \
  } while (0)

static inline int PrintSuccess(const char* case_id) {
  printf("%s_OK=1\n", case_id);
  return 0;
}
