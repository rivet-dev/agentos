#ifndef EDGE_HANDLE_WRAP_H_
#define EDGE_HANDLE_WRAP_H_

#include <cstdint>

#include <uv.h>

#include "node_api.h"

enum EdgeHandleState : uint8_t {
  kEdgeHandleUninitialized = 0,
  kEdgeHandleInitialized,
  kEdgeHandleClosing,
  kEdgeHandleClosed,
};

using EdgeHandleWrapCloseCallback = void (*)(void* data);

struct EdgeHandleWrap {
  napi_env env = nullptr;
  napi_ref wrapper_ref = nullptr;
  void* active_handle_token = nullptr;
  void* close_data = nullptr;
  uv_handle_t* uv_handle = nullptr;
  EdgeHandleWrapCloseCallback close_callback = nullptr;
  EdgeHandleWrap* prev = nullptr;
  EdgeHandleWrap* next = nullptr;
  bool attached = false;
  bool finalized = false;
  bool delete_on_close = false;
  bool wrapper_ref_held = false;
  // True while the uv close callback (OnClosed) is running its teardown, which
  // includes invoking a JS "onclose" callback that can re-enter the GC and run
  // this wrap's finalizer. While set, the finalizer must not free the wrap:
  // OnClosed owns the deletion and performs it once it resumes.
  bool in_close_callback = false;
  EdgeHandleState state = kEdgeHandleUninitialized;
};

void EdgeHandleWrapInit(EdgeHandleWrap* wrap, napi_env env);
void EdgeHandleWrapAttach(EdgeHandleWrap* wrap,
                         void* close_data,
                         uv_handle_t* handle,
                         EdgeHandleWrapCloseCallback close_callback);
void EdgeHandleWrapDetach(EdgeHandleWrap* wrap);
napi_value EdgeHandleWrapGetRefValue(napi_env env, napi_ref ref);
void EdgeHandleWrapDeleteRefIfPresent(napi_env env, napi_ref* ref);
void EdgeHandleWrapHoldWrapperRef(EdgeHandleWrap* wrap);
void EdgeHandleWrapReleaseWrapperRef(EdgeHandleWrap* wrap);
bool EdgeHandleWrapCancelFinalizer(EdgeHandleWrap* wrap, void* native_object);
napi_value EdgeHandleWrapGetActiveOwner(napi_env env, napi_ref wrapper_ref);
void EdgeHandleWrapSetOnCloseCallback(napi_env env, napi_value wrapper, napi_value callback);
void EdgeHandleWrapMaybeCallOnClose(EdgeHandleWrap* wrap);
bool EdgeHandleWrapHasRef(const EdgeHandleWrap* wrap, const uv_handle_t* handle);
bool EdgeHandleWrapEnvCleanupStarted(napi_env env);
void EdgeHandleWrapRunEnvCleanup(napi_env env);

// Frees the native object that owns `wrap` (e.g. `delete static_cast<T*>(p)`).
using EdgeHandleWrapDeleteFn = void (*)(void* native_object);

// Shared, re-entrancy-safe teardown for the canonical handle-wrap lifetime
// model (pin the JS wrapper strong while the uv handle is active; the finalizer
// ultimately frees the native object once the handle is closed). Every handle
// wrap should route its uv close callback and its napi finalizer through these
// instead of hand-rolling the logic, so the subtle ordering — in particular not
// freeing the wrap while its own onclose callback is on the stack — lives in
// exactly one place and cannot be reintroduced by a new wrap.
//
// Call from a wrap's uv_close() callback. `native` is the owning object pointer.
void EdgeHandleWrapCompleteClose(EdgeHandleWrap* wrap, void* native,
                                 EdgeHandleWrapDeleteFn delete_native);

// Call from a wrap's napi finalizer. Defers deletion to the close path when the
// handle is still active/closing; deletes immediately only when it is safe.
void EdgeHandleWrapFinalizeNative(napi_env env, EdgeHandleWrap* wrap, void* native,
                                  EdgeHandleWrapDeleteFn delete_native);

#endif  // EDGE_HANDLE_WRAP_H_
