#ifndef EDGE_STREAM_BASE_H_
#define EDGE_STREAM_BASE_H_

#include <cstddef>
#include <cstdint>
#include <string>

#include <uv.h>

#include "node_api.h"
#include "edge_stream_listener.h"

struct EdgeStreamBase;

struct EdgeStreamBaseOps {
  uv_handle_t* (*get_handle)(EdgeStreamBase* base) = nullptr;
  uv_stream_t* (*get_stream)(EdgeStreamBase* base) = nullptr;
  uv_close_cb on_close = nullptr;
  void (*destroy_self)(EdgeStreamBase* base) = nullptr;
  napi_value (*accept_pending_handle)(EdgeStreamBase* base) = nullptr;
  int (*read_start)(EdgeStreamBase* base) = nullptr;
  int (*read_stop)(EdgeStreamBase* base) = nullptr;
  int (*shutdown_direct)(EdgeStreamBase* base, napi_value req_obj) = nullptr;
  int (*write_buffer_direct)(EdgeStreamBase* base,
                             napi_value req_obj,
                             napi_value payload,
                             bool* async_out) = nullptr;
};

struct EdgeStreamBase {
  napi_env env = nullptr;
  napi_ref wrapper_ref = nullptr;
  napi_ref onread_ref = nullptr;
  napi_ref user_read_buffer_ref = nullptr;
  EdgeStreamListenerState listener_state{};
  EdgeStreamListener default_listener{};
  EdgeStreamListener user_buffer_listener{};
  char* user_buffer_base = nullptr;
  size_t user_buffer_len = 0;
  bool user_buffer_listener_active = false;
  bool closing = false;
  bool closed = false;
  bool eof_emitted = false;
  bool finalized = false;
  bool delete_on_close = false;
  bool close_callback_active = false;
  bool destroyed = false;
  bool destroy_notified = false;
  bool async_init_emitted = false;
  bool attached = false;
  uint64_t bytes_read = 0;
  uint64_t bytes_written = 0;
  int64_t async_id = -1;
  int32_t provider_type = 0;
  void* active_handle_token = nullptr;
  const EdgeStreamBaseOps* ops = nullptr;
  EdgeStreamBase* prev = nullptr;
  EdgeStreamBase* next = nullptr;
};

void EdgeStreamBaseInit(EdgeStreamBase* base,
                       napi_env env,
                       const EdgeStreamBaseOps* ops,
                       int32_t provider_type);
void EdgeStreamBaseSetWrapperRef(EdgeStreamBase* base, napi_ref wrapper_ref);
napi_value EdgeStreamBaseGetWrapper(EdgeStreamBase* base);
void EdgeStreamBaseSetInitialStreamProperties(EdgeStreamBase* base,
                                             bool set_owner_symbol,
                                             bool set_onconnection);

void EdgeStreamBaseFinalize(EdgeStreamBase* base);
void EdgeStreamBaseOnClosed(EdgeStreamBase* base);
void EdgeStreamBaseEmitAfterWrite(EdgeStreamBase* base, napi_value req_obj, int status);
void EdgeStreamBaseEmitAfterShutdown(EdgeStreamBase* base, napi_value req_obj, int status);
void EdgeStreamBaseEmitAfterShutdown(EdgeStreamBase* base,
                                    napi_env callback_env,
                                    napi_value req_obj,
                                    int status);
void EdgeStreamBaseEmitWantsWrite(EdgeStreamBase* base, size_t suggested_size);
bool EdgeStreamBaseEmitReadBuffer(EdgeStreamBase* base, const uint8_t* data, size_t len);
bool EdgeStreamBaseEmitEOF(EdgeStreamBase* base);

bool EdgeStreamBasePushListener(EdgeStreamBase* base, EdgeStreamListener* listener);
bool EdgeStreamBaseRemoveListener(EdgeStreamBase* base, EdgeStreamListener* listener);
bool EdgeStreamBaseOnUvAlloc(EdgeStreamBase* base, size_t suggested_size, uv_buf_t* out);
void EdgeStreamBaseOnUvRead(EdgeStreamBase* base, ssize_t nread, const uv_buf_t* buf);

void EdgeStreamBaseSetReading(EdgeStreamBase* base, bool reading);
napi_value EdgeStreamBaseClose(EdgeStreamBase* base, napi_value close_callback);
void EdgeStreamBaseSetCloseCallback(EdgeStreamBase* base, napi_value close_callback);
bool EdgeStreamBaseHasRef(EdgeStreamBase* base);
void EdgeStreamBaseRef(EdgeStreamBase* base);
void EdgeStreamBaseUnref(EdgeStreamBase* base);
bool EdgeStreamBaseEnvCleanupStarted(napi_env env);
void EdgeStreamBaseRunEnvCleanup(napi_env env);

napi_value EdgeStreamBaseGetOnRead(EdgeStreamBase* base);
napi_value EdgeStreamBaseSetOnRead(EdgeStreamBase* base, napi_value value);
napi_value EdgeStreamBaseUseUserBuffer(EdgeStreamBase* base, napi_value value);

napi_value EdgeStreamBaseGetBytesRead(EdgeStreamBase* base);
napi_value EdgeStreamBaseGetBytesWritten(EdgeStreamBase* base);
napi_value EdgeStreamBaseGetFd(EdgeStreamBase* base);
napi_value EdgeStreamBaseGetExternal(EdgeStreamBase* base);
napi_value EdgeStreamBaseGetAsyncId(EdgeStreamBase* base);
napi_value EdgeStreamBaseGetProviderType(EdgeStreamBase* base);
napi_value EdgeStreamBaseAsyncReset(EdgeStreamBase* base);
napi_value EdgeStreamBaseHasRefValue(EdgeStreamBase* base);
napi_value EdgeStreamBaseGetWriteQueueSize(EdgeStreamBase* base);
uv_stream_t* EdgeStreamBaseGetLibuvStream(napi_env env, napi_value value);
EdgeStreamBase* EdgeStreamBaseFromValue(napi_env env, napi_value value);

napi_value EdgeStreamBaseMakeInt32(napi_env env, int32_t value);
napi_value EdgeStreamBaseMakeInt64(napi_env env, int64_t value);
napi_value EdgeStreamBaseMakeDouble(napi_env env, double value);
napi_value EdgeStreamBaseMakeBool(napi_env env, bool value);
napi_value EdgeStreamBaseUndefined(napi_env env);

void EdgeStreamBaseSetReqError(napi_env env, napi_value req_obj, int status);
void EdgeStreamBaseInvokeReqOnComplete(napi_env env,
                                      napi_value req_obj,
                                      int status,
                                      napi_value* argv,
                                      size_t argc);
int EdgeStreamBaseReadStart(EdgeStreamBase* base);
int EdgeStreamBaseReadStop(EdgeStreamBase* base);
int EdgeStreamBaseShutdownDirect(EdgeStreamBase* base, napi_value req_obj);
int EdgeStreamBaseWriteBufferDirect(EdgeStreamBase* base,
                                   napi_value req_obj,
                                   napi_value payload,
                                   bool* async_out);
int EdgeStreamBaseWritevDirect(EdgeStreamBase* base,
                              napi_value req_obj,
                              napi_value chunks,
                              bool* async_out);

size_t EdgeTypedArrayElementSize(napi_typedarray_type type);
bool EdgeStreamBaseExtractByteSpan(napi_env env,
                                  napi_value value,
                                  const uint8_t** data,
                                  size_t* len,
                                  bool* refable,
                                  std::string* temp_utf8);
napi_value EdgeStreamBufferFromWithEncoding(napi_env env,
                                          napi_value value,
                                          napi_value encoding);

napi_value EdgeLibuvStreamWriteBuffer(EdgeStreamBase* base,
                                     napi_value req_obj,
                                     napi_value payload,
                                     uv_stream_t* send_handle,
                                     napi_value send_handle_obj);
napi_value EdgeLibuvStreamWriteString(EdgeStreamBase* base,
                                     napi_value req_obj,
                                     napi_value payload,
                                     const char* encoding_name,
                                     uv_stream_t* send_handle,
                                     napi_value send_handle_obj);
napi_value EdgeLibuvStreamWriteV(EdgeStreamBase* base,
                                napi_value req_obj,
                                napi_value chunks,
                                bool all_buffers,
                                uv_stream_t* send_handle,
                                napi_value send_handle_obj);
napi_value EdgeLibuvStreamShutdown(EdgeStreamBase* base, napi_value req_obj);

#endif  // EDGE_STREAM_BASE_H_
