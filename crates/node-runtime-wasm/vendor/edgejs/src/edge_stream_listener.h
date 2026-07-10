#ifndef EDGE_STREAM_LISTENER_H_
#define EDGE_STREAM_LISTENER_H_

#include <cstddef>

#include <uv.h>

#include "node_api.h"

struct EdgeStreamListener;

using EdgeStreamOnAlloc = bool (*)(EdgeStreamListener* listener,
                                  size_t suggested_size,
                                  uv_buf_t* out);
using EdgeStreamOnRead = bool (*)(EdgeStreamListener* listener,
                                 ssize_t nread,
                                 const uv_buf_t* buf);
using EdgeStreamOnAfterWrite = bool (*)(EdgeStreamListener* listener,
                                       napi_value req_obj,
                                       int status);
using EdgeStreamOnAfterShutdown = bool (*)(EdgeStreamListener* listener,
                                          napi_value req_obj,
                                          int status);
using EdgeStreamOnWantsWrite = bool (*)(EdgeStreamListener* listener,
                                       size_t suggested_size);
using EdgeStreamOnClose = void (*)(EdgeStreamListener* listener);

struct EdgeStreamListener {
  EdgeStreamListener* previous = nullptr;
  napi_env env = nullptr;
  EdgeStreamOnAlloc on_alloc = nullptr;
  EdgeStreamOnRead on_read = nullptr;
  EdgeStreamOnAfterWrite on_after_write = nullptr;
  EdgeStreamOnAfterShutdown on_after_shutdown = nullptr;
  EdgeStreamOnWantsWrite on_wants_write = nullptr;
  EdgeStreamOnClose on_close = nullptr;
  void* data = nullptr;
};

struct EdgeStreamListenerState {
  EdgeStreamListener* current = nullptr;
  napi_env env = nullptr;
};

void EdgeInitStreamListenerState(EdgeStreamListenerState* state,
                                EdgeStreamListener* initial);
void EdgePushStreamListener(EdgeStreamListenerState* state,
                           EdgeStreamListener* listener);
bool EdgeRemoveStreamListener(EdgeStreamListenerState* state,
                             EdgeStreamListener* listener);
bool EdgeStreamEmitAlloc(EdgeStreamListenerState* state,
                        size_t suggested_size,
                        uv_buf_t* out);
bool EdgeStreamEmitRead(EdgeStreamListenerState* state,
                       ssize_t nread,
                       const uv_buf_t* buf);
bool EdgeStreamEmitAfterWrite(EdgeStreamListenerState* state,
                             napi_value req_obj,
                             int status);
bool EdgeStreamEmitAfterShutdown(EdgeStreamListenerState* state,
                                napi_value req_obj,
                                int status,
                                napi_env callback_env = nullptr);
bool EdgeStreamEmitWantsWrite(EdgeStreamListenerState* state,
                             size_t suggested_size);
bool EdgeStreamPassAfterWrite(EdgeStreamListener* listener,
                             napi_value req_obj,
                             int status);
bool EdgeStreamPassAfterShutdown(EdgeStreamListener* listener,
                                napi_value req_obj,
                                int status);
bool EdgeStreamPassWantsWrite(EdgeStreamListener* listener,
                             size_t suggested_size);
void EdgeStreamNotifyClosed(EdgeStreamListenerState* state);

#endif  // EDGE_STREAM_LISTENER_H_
