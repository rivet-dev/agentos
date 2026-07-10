#include "edge_tls_wrap.h"

#include <cstdint>
#include <cstring>
#include <deque>
#include <cctype>
#include <string>
#include <unordered_map>
#include <unordered_set>
#include <utility>
#include <vector>

#include <openssl/err.h>
#include <openssl/ocsp.h>
#include <openssl/ssl.h>
#include <openssl/x509v3.h>
#include <uv.h>

#include "binding_registry/binding_registry.h"
#include "crypto/edge_crypto_bio.h"
#include "crypto/edge_secure_context_bridge.h"
#include "ncrypto.h"
#include "internal_binding/helpers.h"
#include "edge_async_wrap.h"
#include "edge_environment.h"
#include "edge_env_loop.h"
#include "edge_handle_scope.h"
#include "edge_handle_wrap.h"
#include "edge_module_loader.h"
#include "edge_runtime.h"
#include "edge_stream_base.h"
#include "edge_stream_wrap.h"

namespace {

struct PendingEncryptedWrite {
  size_t size = 0;
  napi_ref completion_req_ref = nullptr;
  bool force_parent_turn = false;
};

struct KeepaliveHandle {
  uv_idle_t idle{};
};

struct TlsWrap;
using TlsCertCb = void (*)(void*);
TlsWrap* FindWrapBySelf(napi_env env, napi_value self);
int32_t CallParentMethodInt(TlsWrap* wrap,
                            const char* method,
                            size_t argc,
                            napi_value* argv,
                            napi_value* result_out);
int TLSExtStatusCallback(SSL* ssl, void* arg);
void DeleteRefIfPresent(napi_env env, napi_ref* ref);

struct ClientHelloData {
  const uint8_t* session_id = nullptr;
  uint8_t session_size = 0;
  const uint8_t* servername = nullptr;
  uint16_t servername_size = 0;
  bool has_ticket = false;
};

class ClientHelloParser {
 public:
  using OnHelloCb = void (*)(TlsWrap* wrap, const ClientHelloData& hello);

  void Start(OnHelloCb on_hello) {
    if (!IsEnded()) return;
    Reset();
    state_ = kWaiting;
    on_hello_ = on_hello;
  }

  void End() {
    state_ = kEnded;
  }

  bool IsEnded() const {
    return state_ == kEnded;
  }

  bool IsPaused() const {
    return state_ == kPaused;
  }

  void Parse(TlsWrap* wrap, const uint8_t* data, size_t avail) {
    if (wrap == nullptr || data == nullptr) return;
    switch (state_) {
      case kWaiting:
        if (!ParseRecordHeader(data, avail)) return;
        [[fallthrough]];
      case kTLSHeader:
        ParseHeader(wrap, data, avail);
        break;
      case kPaused:
      case kEnded:
        break;
    }
  }

 private:
  enum ParseState { kWaiting, kTLSHeader, kPaused, kEnded };

  static constexpr size_t kMaxTLSFrameLen = 16 * 1024 + 5;
  static constexpr uint8_t kChangeCipherSpec = 20;
  static constexpr uint8_t kAlert = 21;
  static constexpr uint8_t kHandshake = 22;
  static constexpr uint8_t kApplicationData = 23;
  static constexpr uint8_t kClientHello = 1;
  static constexpr uint16_t kServerName = 0;
  static constexpr uint16_t kTLSSessionTicket = 35;
  static constexpr uint8_t kServernameHostname = 0;

  void Reset() {
    state_ = kEnded;
    frame_len_ = 0;
    session_id_ = nullptr;
    session_size_ = 0;
    servername_ = nullptr;
    servername_size_ = 0;
    tls_ticket_ = nullptr;
    tls_ticket_size_ = static_cast<uint16_t>(-1);
    on_hello_ = nullptr;
  }

  bool ParseRecordHeader(const uint8_t* data, size_t avail) {
    if (avail < 5) return false;
    switch (data[0]) {
      case kChangeCipherSpec:
      case kAlert:
      case kHandshake:
      case kApplicationData:
        frame_len_ = (static_cast<size_t>(data[3]) << 8) + data[4];
        state_ = kTLSHeader;
        body_offset_ = 5;
        break;
      default:
        End();
        return false;
    }
    if (frame_len_ >= kMaxTLSFrameLen) {
      End();
      return false;
    }
    return true;
  }

  void ParseHeader(TlsWrap* wrap, const uint8_t* data, size_t avail) {
    if (frame_len_ < 6) {
      End();
      return;
    }
    if (body_offset_ + frame_len_ > avail) return;
    if (data[body_offset_ + 4] != 0x03 ||
        data[body_offset_ + 5] < 0x01 ||
        data[body_offset_ + 5] > 0x03) {
      End();
      return;
    }
    if (data[body_offset_] == kClientHello && !ParseTLSClientHello(data, avail)) {
      End();
      return;
    }
    if (session_id_ == nullptr ||
        session_size_ > 32 ||
        session_id_ + session_size_ > data + avail) {
      End();
      return;
    }
    state_ = kPaused;
    if (on_hello_ != nullptr) {
      ClientHelloData hello;
      hello.session_id = session_id_;
      hello.session_size = session_size_;
      hello.servername = servername_;
      hello.servername_size = servername_size_;
      hello.has_ticket = tls_ticket_ != nullptr && tls_ticket_size_ != 0;
      on_hello_(wrap, hello);
    }
  }

  void ParseExtension(uint16_t type, const uint8_t* data, size_t len) {
    switch (type) {
      case kServerName: {
        if (len < 2) return;
        const uint32_t server_names_len = (static_cast<uint32_t>(data[0]) << 8) + data[1];
        if (server_names_len + 2 > len) return;
        for (size_t offset = 2; offset < 2 + server_names_len;) {
          if (offset + 3 > len) return;
          if (data[offset] != kServernameHostname) return;
          const uint16_t name_len = (static_cast<uint16_t>(data[offset + 1]) << 8) + data[offset + 2];
          offset += 3;
          if (offset + name_len > len) return;
          servername_ = data + offset;
          servername_size_ = name_len;
          offset += name_len;
        }
        break;
      }
      case kTLSSessionTicket:
        tls_ticket_size_ = static_cast<uint16_t>(len);
        tls_ticket_ = data + len;
        break;
      default:
        break;
    }
  }

  bool ParseTLSClientHello(const uint8_t* data, size_t avail) {
    const size_t session_offset = body_offset_ + 4 + 2 + 32;
    if (session_offset + 1 >= avail) return false;
    session_size_ = data[session_offset];
    session_id_ = data + session_offset + 1;

    const size_t cipher_offset = session_offset + 1 + session_size_;
    if (cipher_offset + 1 >= avail) return false;
    const uint16_t cipher_len = (static_cast<uint16_t>(data[cipher_offset]) << 8) + data[cipher_offset + 1];

    const size_t comp_offset = cipher_offset + 2 + cipher_len;
    if (comp_offset >= avail) return false;
    const uint8_t comp_len = data[comp_offset];
    const size_t extension_offset = comp_offset + 1 + comp_len;
    if (extension_offset > avail) return false;
    if (extension_offset == avail) return true;

    size_t ext_off = extension_offset + 2;
    while (ext_off < avail) {
      if (ext_off + 4 > avail) return false;
      const uint16_t ext_type = (static_cast<uint16_t>(data[ext_off]) << 8) + data[ext_off + 1];
      const uint16_t ext_len = (static_cast<uint16_t>(data[ext_off + 2]) << 8) + data[ext_off + 3];
      ext_off += 4;
      if (ext_off + ext_len > avail) return false;
      ParseExtension(ext_type, data + ext_off, ext_len);
      ext_off += ext_len;
    }
    return ext_off <= avail;
  }

  ParseState state_ = kEnded;
  OnHelloCb on_hello_ = nullptr;
  size_t frame_len_ = 0;
  size_t body_offset_ = 0;
  const uint8_t* session_id_ = nullptr;
  uint8_t session_size_ = 0;
  const uint8_t* servername_ = nullptr;
  uint16_t servername_size_ = 0;
  uint16_t tls_ticket_size_ = static_cast<uint16_t>(-1);
  const uint8_t* tls_ticket_ = nullptr;
};

struct TlsWrap {
  napi_env env = nullptr;
  EdgeStreamBase base{};
  EdgeStreamListener parent_stream_listener{};
  EdgeStreamBase* parent_stream_base = nullptr;
  napi_ref wrapper_ref = nullptr;
  napi_ref parent_ref = nullptr;
  napi_ref context_ref = nullptr;
  napi_ref pending_shutdown_req_ref = nullptr;
  napi_ref active_write_req_ref = nullptr;
  napi_ref active_empty_write_req_ref = nullptr;
  napi_ref active_parent_write_req_ref = nullptr;
  napi_ref user_read_buffer_ref = nullptr;
  bool is_server = false;
  bool started = false;
  bool established = false;
  bool eof = false;
  bool parent_write_in_progress = false;
  bool has_active_write_issued_by_prev_listener = false;
  bool waiting_cert_cb = false;
  bool cert_cb_running = false;
  TlsCertCb cert_cb = nullptr;
  void* cert_cb_arg = nullptr;
  bool alpn_callback_enabled = false;
  bool session_callbacks_enabled = false;
  bool awaiting_new_session = false;
  bool client_session_fallback_emitted = false;
  bool client_session_fallback_scheduled = false;
  uint32_t client_session_event_count = 0;
  bool pending_client_ocsp_event = false;
  bool client_ocsp_event_emitted = false;
  bool request_cert = false;
  bool reject_unauthorized = false;
  bool shutdown_started = false;
  bool write_callback_scheduled = false;
  bool in_dowrite = false;
  bool parent_terminal_read_emitted = false;
  bool refed = true;
  bool keepalive_needed = false;
  int64_t async_id = 0;
  int cycle_depth = 0;
  size_t parent_write_size = 0;
  size_t queued_encrypted_bytes = 0;
  SSL* ssl = nullptr;
  BIO* enc_in = nullptr;
  BIO* enc_out = nullptr;
  edge::crypto::SecureContextHolder* secure_context = nullptr;
  std::vector<uint8_t> pending_cleartext_input;
  std::deque<PendingEncryptedWrite> pending_encrypted_writes;
  std::vector<uint8_t> pending_session;
  std::vector<uint8_t> deferred_parent_input;
  std::vector<uint8_t> ocsp_response;
  std::vector<unsigned char> alpn_protos;
  ClientHelloParser hello_parser;
  SSL_SESSION* next_session = nullptr;
  KeepaliveHandle* keepalive = nullptr;
  std::vector<char> parent_read_buffer;
  char* user_buffer_base = nullptr;
  size_t user_buffer_len = 0;
};

struct TlsBindingState {
  explicit TlsBindingState(napi_env env_in) : env(env_in) {}
  ~TlsBindingState() {
    for (TlsWrap* wrap : wraps) {
      if (wrap == nullptr) continue;
      DeleteRefIfPresent(env, &wrap->base.onread_ref);
      DeleteRefIfPresent(env, &wrap->base.user_read_buffer_ref);
      DeleteRefIfPresent(env, &wrap->base.wrapper_ref);
      wrap->wrapper_ref = nullptr;
      DeleteRefIfPresent(env, &wrap->parent_ref);
      DeleteRefIfPresent(env, &wrap->context_ref);
      DeleteRefIfPresent(env, &wrap->pending_shutdown_req_ref);
      DeleteRefIfPresent(env, &wrap->active_write_req_ref);
      DeleteRefIfPresent(env, &wrap->active_empty_write_req_ref);
      DeleteRefIfPresent(env, &wrap->active_parent_write_req_ref);
      DeleteRefIfPresent(env, &wrap->user_read_buffer_ref);
      for (auto& pending : wrap->pending_encrypted_writes) {
        DeleteRefIfPresent(env, &pending.completion_req_ref);
      }
    }
    DeleteRefIfPresent(env, &binding_ref);
    DeleteRefIfPresent(env, &tls_wrap_ctor_ref);
    wraps.clear();
  }

  napi_env env = nullptr;
  napi_ref binding_ref = nullptr;
  napi_ref tls_wrap_ctor_ref = nullptr;
  int64_t next_async_id = 300000;
  std::vector<TlsWrap*> wraps;
};

struct ClientSessionFallbackTask {
  napi_ref self_ref = nullptr;
};

struct ParentWriteTask {
  napi_ref self_ref = nullptr;
  napi_ref req_ref = nullptr;
  int status = 0;
};

struct InvokeQueuedTask {
  napi_ref self_ref = nullptr;
  int status = 0;
};

int TlsWrapStreamBaseWriteBuffer(EdgeStreamBase* base,
                                 napi_value req_obj,
                                 napi_value payload,
                                 bool* async_out);
void ConsumeEncryptedWriteQueue(TlsWrap* wrap, size_t committed, int status);
bool ParentStreamOnAlloc(EdgeStreamListener* listener, size_t suggested_size, uv_buf_t* out);
bool ParentStreamOnRead(EdgeStreamListener* listener, ssize_t nread, const uv_buf_t* buf);

const EdgeStreamBaseOps kTlsWrapOps = {
    nullptr,
    nullptr,
    nullptr,
    nullptr,
    nullptr,
    nullptr,
    nullptr,
    nullptr,
    TlsWrapStreamBaseWriteBuffer,
};

napi_value Undefined(napi_env env) {
  napi_value out = nullptr;
  napi_get_undefined(env, &out);
  return out;
}

napi_value Null(napi_env env) {
  napi_value out = nullptr;
  napi_get_null(env, &out);
  return out;
}

napi_value MakeInt32(napi_env env, int32_t value) {
  napi_value out = nullptr;
  napi_create_int32(env, value, &out);
  return out;
}

napi_value MakeInt64(napi_env env, int64_t value) {
  napi_value out = nullptr;
  napi_create_int64(env, value, &out);
  return out;
}

napi_value MakeBool(napi_env env, bool value) {
  napi_value out = nullptr;
  napi_get_boolean(env, value, &out);
  return out;
}

void DeleteRefIfPresent(napi_env env, napi_ref* ref) {
  if (env == nullptr || ref == nullptr || *ref == nullptr) return;
  napi_delete_reference(env, *ref);
  *ref = nullptr;
}

void KeepaliveNoop(uv_idle_t* /*handle*/) {}

void EnsureKeepaliveHandle(TlsWrap* wrap) {
  if (wrap == nullptr || !wrap->keepalive_needed || wrap->keepalive != nullptr || wrap->env == nullptr) return;
  uv_loop_t* loop = EdgeGetEnvLoop(wrap->env);
  if (loop == nullptr) return;

  auto* keepalive = new KeepaliveHandle();
  if (uv_idle_init(loop, &keepalive->idle) != 0) {
    delete keepalive;
    return;
  }
  keepalive->idle.data = keepalive;
  if (uv_idle_start(&keepalive->idle, KeepaliveNoop) != 0) {
    uv_close(reinterpret_cast<uv_handle_t*>(&keepalive->idle),
             [](uv_handle_t* handle) {
               delete static_cast<KeepaliveHandle*>(handle->data);
             });
    return;
  }
  wrap->keepalive = keepalive;
}

void SyncKeepaliveRef(TlsWrap* wrap) {
  if (wrap == nullptr || wrap->keepalive == nullptr) return;
  uv_handle_t* handle = reinterpret_cast<uv_handle_t*>(&wrap->keepalive->idle);
  if (wrap->refed) {
    uv_ref(handle);
  } else {
    uv_unref(handle);
  }
}

void ReleaseKeepaliveHandle(TlsWrap* wrap) {
  if (wrap == nullptr || wrap->keepalive == nullptr) return;
  auto* keepalive = wrap->keepalive;
  wrap->keepalive = nullptr;
  uv_idle_stop(&keepalive->idle);
  uv_close(reinterpret_cast<uv_handle_t*>(&keepalive->idle),
           [](uv_handle_t* handle) {
             delete static_cast<KeepaliveHandle*>(handle->data);
           });
}

napi_value GetRefValue(napi_env env, napi_ref ref) {
  if (env == nullptr || ref == nullptr) return nullptr;
  napi_value out = nullptr;
  if (napi_get_reference_value(env, ref, &out) != napi_ok) return nullptr;
  return out;
}

napi_value GetNamedValue(napi_env env, napi_value obj, const char* key) {
  if (env == nullptr || obj == nullptr || key == nullptr) return nullptr;
  napi_value out = nullptr;
  if (napi_get_named_property(env, obj, key, &out) != napi_ok) return nullptr;
  return out;
}

napi_value ResolveInternalBinding(napi_env env, const char* name) {
  if (env == nullptr || name == nullptr) return nullptr;
  return edge::binding_registry::Get(env, name);
}

napi_value GetInternalBindingSymbol(napi_env env, const char* name) {
  napi_value symbols = ResolveInternalBinding(env, "symbols");
  if (symbols == nullptr || name == nullptr) return nullptr;
  return GetNamedValue(env, symbols, name);
}

napi_value GetPropertyBySymbol(napi_env env, napi_value obj, const char* symbol_name) {
  if (env == nullptr || obj == nullptr || symbol_name == nullptr) return nullptr;
  napi_value symbol = GetInternalBindingSymbol(env, symbol_name);
  if (symbol == nullptr) return nullptr;
  napi_value out = nullptr;
  if (napi_get_property(env, obj, symbol, &out) != napi_ok) return nullptr;
  return out;
}

void InvokeReqWithStatus(TlsWrap* wrap, napi_ref* req_ref, int status) {
  if (wrap == nullptr || wrap->env == nullptr || req_ref == nullptr || *req_ref == nullptr) return;
  napi_value req_obj = GetRefValue(wrap->env, *req_ref);
  napi_value self = GetRefValue(wrap->env, wrap->wrapper_ref);
  if (req_obj != nullptr && status < 0) {
    EdgeStreamBaseSetReqError(wrap->env, req_obj, status);
  }
  napi_value argv[3] = {
      MakeInt32(wrap->env, status),
      self != nullptr ? self : Undefined(wrap->env),
      status < 0 && req_obj != nullptr ? GetNamedValue(wrap->env, req_obj, "error") : Undefined(wrap->env),
  };
  if (req_obj != nullptr) {
    EdgeStreamBaseInvokeReqOnComplete(wrap->env, req_obj, status, argv, 3);
  }
  DeleteRefIfPresent(wrap->env, req_ref);
}

bool IsFunction(napi_env env, napi_value value) {
  napi_valuetype type = napi_undefined;
  return value != nullptr && napi_typeof(env, value, &type) == napi_ok && type == napi_function;
}

bool GetArrayBufferViewSpan(napi_env env, napi_value value, const uint8_t** data, size_t* len) {
  static uint8_t kEmptySentinel = 0;
  if (env == nullptr || value == nullptr || data == nullptr || len == nullptr) return false;

  bool is_buffer = false;
  if (napi_is_buffer(env, value, &is_buffer) == napi_ok && is_buffer) {
    void* raw = nullptr;
    size_t byte_len = 0;
    if (napi_get_buffer_info(env, value, &raw, &byte_len) != napi_ok) return false;
    if (raw == nullptr && byte_len != 0) return false;
    *data = raw != nullptr ? static_cast<const uint8_t*>(raw) : &kEmptySentinel;
    *len = byte_len;
    return true;
  }

  bool is_typedarray = false;
  if (napi_is_typedarray(env, value, &is_typedarray) == napi_ok && is_typedarray) {
    napi_typedarray_type ta_type = napi_uint8_array;
    size_t element_len = 0;
    void* raw = nullptr;
    napi_value arraybuffer = nullptr;
    size_t byte_offset = 0;
    if (napi_get_typedarray_info(
            env, value, &ta_type, &element_len, &raw, &arraybuffer, &byte_offset) != napi_ok) {
      return false;
    }
    const size_t byte_len = element_len * EdgeTypedArrayElementSize(ta_type);
    if (raw == nullptr && byte_len != 0) return false;
    *data = raw != nullptr ? static_cast<const uint8_t*>(raw) : &kEmptySentinel;
    *len = byte_len;
    return true;
  }

  bool is_dataview = false;
  if (napi_is_dataview(env, value, &is_dataview) == napi_ok && is_dataview) {
    size_t byte_len = 0;
    void* raw = nullptr;
    napi_value arraybuffer = nullptr;
    size_t byte_offset = 0;
    if (napi_get_dataview_info(env, value, &byte_len, &raw, &arraybuffer, &byte_offset) != napi_ok) {
      return false;
    }
    if (raw == nullptr && byte_len != 0) return false;
    *data = raw != nullptr ? static_cast<const uint8_t*>(raw) : &kEmptySentinel;
    *len = byte_len;
    return true;
  }

  return false;
}

std::string ValueToUtf8(napi_env env, napi_value value) {
  if (value == nullptr) return "";
  size_t len = 0;
  if (napi_get_value_string_utf8(env, value, nullptr, 0, &len) != napi_ok) return "";
  std::string out(len + 1, '\0');
  size_t copied = 0;
  if (napi_get_value_string_utf8(env, value, out.data(), out.size(), &copied) != napi_ok) return "";
  out.resize(copied);
  return out;
}

void SetState(napi_env env, int idx, int32_t value) {
  int32_t* state = EdgeGetStreamBaseState(env);
  if (state == nullptr) return;
  state[idx] = value;
}

TlsBindingState& EnsureState(napi_env env) {
  return EdgeEnvironmentGetOrCreateSlotData<TlsBindingState>(
      env, kEdgeEnvironmentSlotTlsBindingState);
}

TlsBindingState* GetState(napi_env env) {
  return EdgeEnvironmentGetSlotData<TlsBindingState>(
      env, kEdgeEnvironmentSlotTlsBindingState);
}

TlsWrap* UnwrapTlsWrap(napi_env env, napi_value value) {
  if (env == nullptr || value == nullptr) return nullptr;
  TlsWrap* wrap = nullptr;
  if (napi_unwrap(env, value, reinterpret_cast<void**>(&wrap)) != napi_ok) return nullptr;
  return wrap;
}

TlsWrap* FindWrapBySelf(napi_env env, napi_value self) {
  TlsBindingState* state = GetState(env);
  if (state == nullptr) return nullptr;
  for (TlsWrap* wrap : state->wraps) {
    if (wrap == nullptr) continue;
    napi_value candidate = GetRefValue(env, wrap->wrapper_ref);
    bool same = false;
    if (candidate != nullptr && napi_strict_equals(env, candidate, self, &same) == napi_ok && same) {
      return wrap;
    }
  }
  return nullptr;
}

TlsWrap* UnwrapThis(napi_env env,
                    napi_callback_info info,
                    size_t* argc,
                    napi_value* argv,
                    napi_value* self_out) {
  size_t local_argc = argc != nullptr ? *argc : 0;
  napi_value self = nullptr;
  if (napi_get_cb_info(env, info, &local_argc, argv, &self, nullptr) != napi_ok) return nullptr;
  if (argc != nullptr) *argc = local_argc;
  if (self_out != nullptr) *self_out = self;
  return UnwrapTlsWrap(env, self);
}

void RemoveWrapFromState(TlsWrap* wrap) {
  if (wrap == nullptr || wrap->env == nullptr) return;
  TlsBindingState* state = GetState(wrap->env);
  if (state == nullptr) return;
  auto& wraps = state->wraps;
  for (auto vec_it = wraps.begin(); vec_it != wraps.end(); ++vec_it) {
    if (*vec_it == wrap) {
      wraps.erase(vec_it);
      break;
    }
  }
}

std::vector<uint8_t> ReadAllPendingBio(BIO* bio) {
  std::vector<uint8_t> out;
  if (bio == nullptr) return out;
  const size_t pending = static_cast<size_t>(BIO_ctrl_pending(bio));
  if (pending == 0) return out;
  out.resize(pending);
  const int read = BIO_read(bio, out.data(), static_cast<int>(pending));
  if (read <= 0) {
    out.clear();
    return out;
  }
  out.resize(static_cast<size_t>(read));
  return out;
}

size_t PendingBioSize(BIO* bio) {
  return bio != nullptr ? static_cast<size_t>(BIO_ctrl_pending(bio)) : 0;
}

size_t PendingUnqueuedEncryptedBytes(TlsWrap* wrap) {
  if (wrap == nullptr || wrap->enc_out == nullptr) return 0;
  const size_t pending = PendingBioSize(wrap->enc_out);
  return pending > wrap->queued_encrypted_bytes ? pending - wrap->queued_encrypted_bytes : 0;
}

constexpr uint8_t kAsn1Sequence = 0x30;
constexpr uint8_t kAsn1Enumerated = 0x0A;
constexpr uint8_t kAsn1OctetString = 0x04;
constexpr uint8_t kAsn1Context0 = 0xA0;
constexpr uint8_t kSyntheticOcspResponseTypeOid[] = {
    0x2B, 0x06, 0x01, 0x05, 0x05, 0x07, 0x30, 0x01, 0x63,
};

void AppendDerLength(std::vector<uint8_t>* out, size_t len) {
  if (out == nullptr) return;
  if (len < 0x80) {
    out->push_back(static_cast<uint8_t>(len));
    return;
  }

  uint8_t encoded[sizeof(size_t)];
  size_t count = 0;
  while (len > 0) {
    encoded[count++] = static_cast<uint8_t>(len & 0xFF);
    len >>= 8;
  }
  out->push_back(static_cast<uint8_t>(0x80 | count));
  while (count > 0) {
    out->push_back(encoded[--count]);
  }
}

void AppendDerTlv(std::vector<uint8_t>* out, uint8_t tag, const uint8_t* data, size_t len) {
  if (out == nullptr) return;
  out->push_back(tag);
  AppendDerLength(out, len);
  if (len > 0 && data != nullptr) {
    out->insert(out->end(), data, data + len);
  }
}

bool ReadDerLength(const uint8_t* data, size_t len, size_t* offset, size_t* out_len) {
  if (data == nullptr || offset == nullptr || out_len == nullptr || *offset >= len) return false;
  const uint8_t first = data[(*offset)++];
  if ((first & 0x80) == 0) {
    *out_len = first;
    return *offset + *out_len <= len;
  }

  const size_t byte_count = first & 0x7F;
  if (byte_count == 0 || byte_count > sizeof(size_t) || *offset + byte_count > len) return false;
  size_t value = 0;
  for (size_t i = 0; i < byte_count; ++i) {
    value = (value << 8) | data[(*offset)++];
  }
  *out_len = value;
  return *offset + *out_len <= len;
}

bool ConsumeDerTlv(const uint8_t* data,
                   size_t len,
                   size_t* offset,
                   uint8_t expected_tag,
                   size_t* content_offset,
                   size_t* content_len) {
  if (data == nullptr || offset == nullptr || *offset >= len || data[*offset] != expected_tag) return false;
  (*offset)++;
  if (!ReadDerLength(data, len, offset, content_len)) return false;
  if (content_offset != nullptr) *content_offset = *offset;
  *offset += *content_len;
  return true;
}

bool IsValidOcspResponseDer(const uint8_t* data, size_t len) {
  if (data == nullptr || len == 0) return false;
  const unsigned char* ptr = data;
  OCSP_RESPONSE* response = d2i_OCSP_RESPONSE(nullptr, &ptr, static_cast<long>(len));
  if (response == nullptr) return false;
  OCSP_RESPONSE_free(response);
  return ptr == data + len;
}

std::vector<uint8_t> WrapSyntheticOcspResponse(const uint8_t* data, size_t len) {
  std::vector<uint8_t> response_bytes;
  AppendDerTlv(&response_bytes,
               0x06,
               kSyntheticOcspResponseTypeOid,
               sizeof(kSyntheticOcspResponseTypeOid));
  AppendDerTlv(&response_bytes, kAsn1OctetString, data, len);

  std::vector<uint8_t> explicit_zero;
  AppendDerTlv(&explicit_zero, kAsn1Sequence, response_bytes.data(), response_bytes.size());

  std::vector<uint8_t> outer;
  const uint8_t success_value = 0;
  AppendDerTlv(&outer, kAsn1Enumerated, &success_value, 1);
  AppendDerTlv(&outer, kAsn1Context0, explicit_zero.data(), explicit_zero.size());

  std::vector<uint8_t> wrapped;
  AppendDerTlv(&wrapped, kAsn1Sequence, outer.data(), outer.size());
  return wrapped;
}

bool UnwrapSyntheticOcspResponse(const uint8_t* data, size_t len, std::vector<uint8_t>* out) {
  if (data == nullptr || out == nullptr) return false;

  size_t offset = 0;
  size_t seq_offset = 0;
  size_t seq_len = 0;
  if (!ConsumeDerTlv(data, len, &offset, kAsn1Sequence, &seq_offset, &seq_len)) return false;
  if (offset != len) return false;

  size_t inner = seq_offset;
  const size_t seq_end = seq_offset + seq_len;

  size_t enum_offset = 0;
  size_t enum_len = 0;
  if (!ConsumeDerTlv(data, seq_end, &inner, kAsn1Enumerated, &enum_offset, &enum_len)) return false;
  if (enum_len != 1 || data[enum_offset] != 0) return false;

  size_t explicit_offset = 0;
  size_t explicit_len = 0;
  if (!ConsumeDerTlv(data, seq_end, &inner, kAsn1Context0, &explicit_offset, &explicit_len)) return false;
  if (inner != seq_end) return false;

  size_t response_bytes_offset = 0;
  size_t response_bytes_len = 0;
  size_t explicit_inner = explicit_offset;
  const size_t explicit_end = explicit_offset + explicit_len;
  if (!ConsumeDerTlv(
          data, explicit_end, &explicit_inner, kAsn1Sequence, &response_bytes_offset, &response_bytes_len)) {
    return false;
  }
  if (explicit_inner != explicit_end) return false;

  size_t rb_inner = response_bytes_offset;
  const size_t rb_end = response_bytes_offset + response_bytes_len;
  size_t oid_offset = 0;
  size_t oid_len = 0;
  if (!ConsumeDerTlv(data, rb_end, &rb_inner, 0x06, &oid_offset, &oid_len)) return false;
  if (oid_len != sizeof(kSyntheticOcspResponseTypeOid) ||
      std::memcmp(data + oid_offset, kSyntheticOcspResponseTypeOid, sizeof(kSyntheticOcspResponseTypeOid)) != 0) {
    return false;
  }

  size_t octet_offset = 0;
  size_t octet_len = 0;
  if (!ConsumeDerTlv(data, rb_end, &rb_inner, kAsn1OctetString, &octet_offset, &octet_len)) return false;
  if (rb_inner != rb_end) return false;

  out->assign(data + octet_offset, data + octet_offset + octet_len);
  return true;
}

napi_value CreateBufferCopy(napi_env env, const uint8_t* data, size_t len) {
  napi_value array_buffer = nullptr;
  void* copied = nullptr;
  if (napi_create_arraybuffer(env, len, &copied, &array_buffer) != napi_ok ||
      array_buffer == nullptr ||
      (len > 0 && copied == nullptr)) {
    return nullptr;
  }
  if (len > 0) {
    std::memcpy(copied, data, len);
  }

  napi_value view = nullptr;
  if (napi_create_typedarray(env, napi_uint8_array, len, array_buffer, 0, &view) != napi_ok ||
      view == nullptr) {
    return array_buffer;
  }

  napi_value global = nullptr;
  napi_value buffer_ctor = nullptr;
  napi_value from_fn = nullptr;
  napi_value out = nullptr;
  if (napi_get_global(env, &global) != napi_ok || global == nullptr) return view;
  if (napi_get_named_property(env, global, "Buffer", &buffer_ctor) != napi_ok || buffer_ctor == nullptr) return view;
  if (napi_get_named_property(env, buffer_ctor, "from", &from_fn) != napi_ok || from_fn == nullptr) return view;
  napi_value argv[1] = {view};
  if (napi_call_function(env, buffer_ctor, from_fn, 1, argv, &out) != napi_ok || out == nullptr) {
    bool pending = false;
    if (napi_is_exception_pending(env, &pending) == napi_ok && pending) {
      napi_value ignored = nullptr;
      napi_get_and_clear_last_exception(env, &ignored);
    }
    return view;
  }
  return out;
}

bool RefMatches(napi_env env, napi_ref ref, napi_value value) {
  if (env == nullptr || ref == nullptr || value == nullptr) return false;
  napi_value expected = GetRefValue(env, ref);
  if (expected == nullptr) return false;
  bool same = false;
  return napi_strict_equals(env, expected, value, &same) == napi_ok && same;
}

void NotifyTlsStreamClosed(TlsWrap* wrap) {
  if (wrap == nullptr) return;
  EdgeStreamBaseSetReading(&wrap->base, false);
  if (!wrap->base.destroy_notified) {
    EdgeStreamBaseOnClosed(&wrap->base);
    wrap->async_id = wrap->base.async_id;
  }
}

void EmitOnReadData(TlsWrap* wrap, const uint8_t* data, size_t len) {
  if (wrap == nullptr || wrap->env == nullptr) return;
  size_t offset = 0;
  while (offset < len && wrap->ssl != nullptr) {
    uv_buf_t buf = uv_buf_init(nullptr, 0);
    if (!EdgeStreamEmitAlloc(&wrap->base.listener_state, len - offset, &buf) ||
        buf.base == nullptr ||
        buf.len == 0) {
      return;
    }

    const size_t chunk_len = std::min<size_t>(buf.len, len - offset);
    if (chunk_len > 0) {
      std::memcpy(buf.base, data + offset, chunk_len);
    }
    buf.len = static_cast<unsigned int>(chunk_len);
    wrap->base.bytes_read += chunk_len;
    if (!EdgeStreamEmitRead(&wrap->base.listener_state, static_cast<ssize_t>(chunk_len), &buf) &&
        buf.base != nullptr) {
      free(buf.base);
    }
    offset += chunk_len;
    if (chunk_len == 0 || wrap->ssl == nullptr || wrap->base.destroy_notified) return;
  }
}

void EmitOnReadStatus(TlsWrap* wrap, int32_t status) {
  if (wrap == nullptr || wrap->env == nullptr) return;
  if (status == UV_EOF) {
    wrap->base.eof_emitted = true;
  }
  (void)EdgeStreamEmitRead(&wrap->base.listener_state, status, nullptr);
}

napi_value CreateErrorWithCode(napi_env env, const char* code, const std::string& message) {
  napi_value code_v = nullptr;
  napi_value msg_v = nullptr;
  napi_value err_v = nullptr;
  if (code != nullptr) {
    napi_create_string_utf8(env, code, NAPI_AUTO_LENGTH, &code_v);
  } else {
    napi_get_undefined(env, &code_v);
  }
  napi_create_string_utf8(env, message.c_str(), NAPI_AUTO_LENGTH, &msg_v);
  napi_create_error(env, code_v, msg_v, &err_v);
  if (err_v != nullptr && code != nullptr && code_v != nullptr) {
    napi_set_named_property(env, err_v, "code", code_v);
  }
  return err_v;
}

std::string DeriveSslCodeFromReason(const char* reason) {
  if (reason == nullptr || reason[0] == '\0') return {};
  std::string code = "ERR_SSL_";
  for (const unsigned char ch : std::string(reason)) {
    if (ch == ' ') {
      code.push_back('_');
    } else {
      code.push_back(static_cast<char>(std::toupper(ch)));
    }
  }
  return code;
}

std::string DeriveOpenSslCode(unsigned long err, const char* reason) {
  if (reason == nullptr || reason[0] == '\0') return {};
  std::string normalized(reason);
  for (char& ch : normalized) {
    if (ch == ' ') {
      ch = '_';
    } else {
      ch = static_cast<char>(std::toupper(static_cast<unsigned char>(ch)));
    }
  }

#define OSSL_ERROR_CODES_MAP(V)                                               \
  V(SYS)                                                                      \
  V(BN)                                                                       \
  V(RSA)                                                                      \
  V(DH)                                                                       \
  V(EVP)                                                                      \
  V(BUF)                                                                      \
  V(OBJ)                                                                      \
  V(PEM)                                                                      \
  V(DSA)                                                                      \
  V(X509)                                                                     \
  V(ASN1)                                                                     \
  V(CONF)                                                                     \
  V(CRYPTO)                                                                   \
  V(EC)                                                                       \
  V(SSL)                                                                      \
  V(BIO)                                                                      \
  V(PKCS7)                                                                    \
  V(X509V3)                                                                   \
  V(PKCS12)                                                                   \
  V(RAND)                                                                     \
  V(DSO)                                                                      \
  V(ENGINE)                                                                   \
  V(OCSP)                                                                     \
  V(UI)                                                                       \
  V(COMP)                                                                     \
  V(ECDSA)                                                                    \
  V(ECDH)                                                                     \
  V(OSSL_STORE)                                                               \
  V(FIPS)                                                                     \
  V(CMS)                                                                      \
  V(TS)                                                                       \
  V(HMAC)                                                                     \
  V(CT)                                                                       \
  V(ASYNC)                                                                    \
  V(KDF)                                                                      \
  V(SM2)                                                                      \
  V(USER)

  const char* lib = "";
  const char* prefix = "OSSL_";
  switch (ERR_GET_LIB(err)) {
#define V(name) case ERR_LIB_##name: lib = #name "_"; break;
    OSSL_ERROR_CODES_MAP(V)
#undef V
    default:
      break;
  }
#undef OSSL_ERROR_CODES_MAP

  if (std::strcmp(lib, "SSL_") == 0) prefix = "";
  std::string code = "ERR_";
  code += prefix;
  code += lib;
  code += normalized;
  return code;
}

void SetErrorStackProperty(napi_env env, napi_value err, const std::vector<unsigned long>& errors) {
  if (err == nullptr || errors.size() <= 1) return;
  napi_value stack = nullptr;
  if (napi_create_array_with_length(env, errors.size() - 1, &stack) != napi_ok || stack == nullptr) return;
  uint32_t index = 0;
  for (size_t i = 1; i < errors.size(); ++i, ++index) {
    char buf[256];
    ERR_error_string_n(errors[i], buf, sizeof(buf));
    napi_value entry = nullptr;
    if (napi_create_string_utf8(env, buf, NAPI_AUTO_LENGTH, &entry) == napi_ok && entry != nullptr) {
      napi_set_element(env, stack, index, entry);
    }
  }
  napi_set_named_property(env, err, "opensslErrorStack", stack);
}

void SetErrorStringProperty(napi_env env, napi_value err, const char* name, const char* value) {
  if (err == nullptr || name == nullptr || value == nullptr || value[0] == '\0') return;
  napi_value out = nullptr;
  if (napi_create_string_utf8(env, value, NAPI_AUTO_LENGTH, &out) == napi_ok && out != nullptr) {
    napi_set_named_property(env, err, name, out);
  }
}

void SetReqErrorString(napi_env env, napi_value req_obj, const char* value) {
  if (env == nullptr || req_obj == nullptr || value == nullptr || value[0] == '\0') return;
  napi_value out = nullptr;
  if (napi_create_string_utf8(env, value, NAPI_AUTO_LENGTH, &out) == napi_ok && out != nullptr) {
    napi_set_named_property(env, req_obj, "error", out);
  }
}

std::string PeekLastOpenSslErrorString(const char* fallback_message) {
  const unsigned long err = ERR_peek_error();
  if (err == 0) {
    return fallback_message != nullptr ? std::string(fallback_message) : std::string();
  }
  char buf[256];
  ERR_error_string_n(err, buf, sizeof(buf));
  return std::string(buf);
}

napi_value CreateLastOpenSslError(napi_env env, const char* fallback_code, const char* fallback_message) {
  std::vector<unsigned long> errors;
  while (const unsigned long err = ERR_get_error()) {
    errors.push_back(err);
  }
  if (errors.empty()) {
    return CreateErrorWithCode(env, fallback_code, fallback_message != nullptr ? fallback_message : "OpenSSL error");
  }

  const unsigned long err = errors.front();
  char buf[256];
  ERR_error_string_n(err, buf, sizeof(buf));
  const char* library = ERR_lib_error_string(err);
  const char* reason = ERR_reason_error_string(err);
  std::string derived_code;
  if (fallback_code == nullptr || std::strncmp(fallback_code, "ERR_TLS_", 8) == 0) {
    derived_code = DeriveOpenSslCode(err, reason);
  }
  const char* code = !derived_code.empty() ? derived_code.c_str() : fallback_code;
  napi_value error = CreateErrorWithCode(env, code, buf);
  SetErrorStringProperty(env, error, "library", library);
#if OPENSSL_VERSION_NUMBER < 0x30000000L
  SetErrorStringProperty(env, error, "function", ERR_func_error_string(err));
#endif
  SetErrorStringProperty(env, error, "reason", reason);
  SetErrorStackProperty(env, error, errors);
  return error;
}

void EmitError(TlsWrap* wrap, napi_value error) {
  if (wrap == nullptr || wrap->env == nullptr || error == nullptr) return;
  edge::HandleScope scope(wrap->env);
  if (!scope.is_open()) return;
  napi_value self = GetRefValue(wrap->env, wrap->wrapper_ref);
  napi_value onerror = GetNamedValue(wrap->env, self, "onerror");
  if (!IsFunction(wrap->env, onerror)) return;
  napi_value argv[1] = {error};
  napi_value ignored = nullptr;
  (void)EdgeAsyncWrapMakeCallback(
      wrap->env, wrap->async_id, self, self, onerror, 1, argv, &ignored, kEdgeMakeCallbackNone);
}

void CompleteReq(TlsWrap* wrap, napi_ref* req_ref, int status) {
  if (wrap == nullptr || wrap->env == nullptr || req_ref == nullptr || *req_ref == nullptr) return;
  edge::HandleScope scope(wrap->env);
  if (!scope.is_open()) return;
  napi_value req_obj = GetRefValue(wrap->env, *req_ref);
  if (req_obj != nullptr) {
    EdgeStreamBaseEmitAfterWrite(&wrap->base, req_obj, status);
  }
  DeleteRefIfPresent(wrap->env, req_ref);
}

void CompleteDetachedReq(TlsWrap* wrap, napi_ref req_ref, int status, const char* error_string = nullptr) {
  if (wrap == nullptr || wrap->env == nullptr || req_ref == nullptr) return;
  napi_value req_obj = GetRefValue(wrap->env, req_ref);
  if (req_obj != nullptr) {
    if (status < 0) {
      SetReqErrorString(wrap->env, req_obj, error_string);
    }
    EdgeStreamBaseEmitAfterWrite(&wrap->base, req_obj, status);
  }
  DeleteRefIfPresent(wrap->env, &req_ref);
}

void InvokeQueued(TlsWrap* wrap, int status, const char* error_string = nullptr) {
  if (wrap == nullptr || !wrap->write_callback_scheduled) return;
  wrap->write_callback_scheduled = false;
  napi_ref req_ref = wrap->active_write_req_ref;
  wrap->active_write_req_ref = nullptr;
  CompleteDetachedReq(wrap, req_ref, status, error_string);
}

bool GetArrayBufferBytes(napi_env env,
                         napi_value value,
                         const uint8_t** data,
                         size_t* len,
                         size_t* offset_out) {
  if (data == nullptr || len == nullptr || offset_out == nullptr) return false;
  *data = nullptr;
  *len = 0;
  *offset_out = 0;
  if (value == nullptr) return false;
  bool is_ab = false;
  if (napi_is_arraybuffer(env, value, &is_ab) == napi_ok && is_ab) {
    void* raw = nullptr;
    size_t byte_len = 0;
    if (napi_get_arraybuffer_info(env, value, &raw, &byte_len) != napi_ok || raw == nullptr) return false;
    *data = static_cast<const uint8_t*>(raw);
    *len = byte_len;
    return true;
  }
  bool is_typedarray = false;
  if (napi_is_typedarray(env, value, &is_typedarray) == napi_ok && is_typedarray) {
    napi_typedarray_type type = napi_uint8_array;
    size_t element_len = 0;
    void* raw = nullptr;
    napi_value ab = nullptr;
    size_t byte_offset = 0;
    if (napi_get_typedarray_info(env, value, &type, &element_len, &raw, &ab, &byte_offset) != napi_ok ||
        raw == nullptr) {
      return false;
    }
    *data = static_cast<const uint8_t*>(raw);
    *len = element_len * EdgeTypedArrayElementSize(type);
    *offset_out = byte_offset;
    return true;
  }
  return false;
}

int32_t CallParentMethodInt(TlsWrap* wrap, const char* method, size_t argc, napi_value* argv, napi_value* result_out) {
  if (wrap == nullptr || wrap->env == nullptr || method == nullptr) return UV_EINVAL;
  edge::HandleScope scope(wrap->env);
  if (!scope.is_open()) return UV_EINVAL;
  napi_value parent = GetRefValue(wrap->env, wrap->parent_ref);
  if (parent == nullptr) return UV_EINVAL;
  napi_value fn = GetNamedValue(wrap->env, parent, method);
  if (!IsFunction(wrap->env, fn)) return UV_EINVAL;
  napi_value result = nullptr;
  if (napi_call_function(wrap->env, parent, fn, argc, argv, &result) != napi_ok || result == nullptr) {
    return UV_EINVAL;
  }
  if (result_out != nullptr) *result_out = result;
  int32_t out = 0;
  if (napi_get_value_int32(wrap->env, result, &out) != napi_ok) return 0;
  return out;
}

void InjectParentStreamBytes(TlsWrap* wrap, const uint8_t* data, size_t len) {
  if (wrap == nullptr || data == nullptr) return;
  size_t offset = 0;
  while (wrap->ssl != nullptr && offset < len) {
    uv_buf_t buf = uv_buf_init(nullptr, 0);
    if (!ParentStreamOnAlloc(&wrap->parent_stream_listener, len - offset, &buf) ||
        buf.base == nullptr || buf.len == 0) {
      return;
    }
    size_t copy = buf.len;
    if (copy > (len - offset)) copy = len - offset;
    std::memcpy(buf.base, data + offset, copy);
    buf.len = static_cast<unsigned int>(copy);
    (void)ParentStreamOnRead(&wrap->parent_stream_listener, static_cast<ssize_t>(copy), &buf);
    offset += copy;
  }
}

bool SetSecureContextOnSsl(TlsWrap* wrap, edge::crypto::SecureContextHolder* holder) {
  if (wrap == nullptr || wrap->ssl == nullptr || holder == nullptr || holder->ctx == nullptr) return false;
  SSL_CTX_set_tlsext_status_cb(holder->ctx, TLSExtStatusCallback);
  SSL_CTX_set_tlsext_status_arg(holder->ctx, nullptr);
  if (SSL_set_SSL_CTX(wrap->ssl, holder->ctx) == nullptr) return false;
  SSL_set_options(wrap->ssl, SSL_CTX_get_options(holder->ctx));
  wrap->secure_context = holder;
  X509_STORE* store = SSL_CTX_get_cert_store(holder->ctx);
  if (store != nullptr && SSL_set1_verify_cert_store(wrap->ssl, store) != 1) return false;
  STACK_OF(X509_NAME)* list = SSL_dup_CA_list(SSL_CTX_get_client_CA_list(holder->ctx));
  SSL_set_client_CA_list(wrap->ssl, list);
  return true;
}

bool SetSecureContextOnSslAndRetain(TlsWrap* wrap,
                                    edge::crypto::SecureContextHolder* holder,
                                    napi_value context) {
  if (wrap == nullptr || wrap->env == nullptr || context == nullptr) return false;

  napi_ref next_ref = nullptr;
  if (napi_create_reference(wrap->env, context, 1, &next_ref) != napi_ok || next_ref == nullptr) {
    return false;
  }

  bool ok = SetSecureContextOnSsl(wrap, holder);
  if (ok || wrap->secure_context == holder) {
    DeleteRefIfPresent(wrap->env, &wrap->context_ref);
    wrap->context_ref = next_ref;
  } else {
    DeleteRefIfPresent(wrap->env, &next_ref);
  }

  return ok;
}

void InitSsl(TlsWrap* wrap);
void Cycle(TlsWrap* wrap);
void EncOut(TlsWrap* wrap);
void InvokeQueued(TlsWrap* wrap, int status, const char* error_string);
void TryStartParentWrite(TlsWrap* wrap);
void MaybeStartParentShutdown(TlsWrap* wrap);
void MaybeStartTlsShutdown(TlsWrap* wrap);
bool ReadCleartext(TlsWrap* wrap);
bool WritePendingCleartextInput(TlsWrap* wrap);

void ResumeCertCallback(void* arg) {
  Cycle(static_cast<TlsWrap*>(arg));
}

void CleanupPendingWrites(TlsWrap* wrap, int status) {
  if (wrap == nullptr) return;
  while (!wrap->pending_encrypted_writes.empty()) {
    PendingEncryptedWrite pending = std::move(wrap->pending_encrypted_writes.front());
    wrap->pending_encrypted_writes.pop_front();
    CompleteReq(wrap, &pending.completion_req_ref, status);
  }
  wrap->queued_encrypted_bytes = 0;
  wrap->parent_write_size = 0;
  CompleteReq(wrap, &wrap->active_write_req_ref, status);
  CompleteReq(wrap, &wrap->active_empty_write_req_ref, status);
  InvokeReqWithStatus(wrap, &wrap->pending_shutdown_req_ref, status);
}

void DestroySsl(TlsWrap* wrap) {
  if (wrap == nullptr) return;
  if (wrap->parent_stream_base != nullptr) {
    (void)EdgeStreamBaseRemoveListener(wrap->parent_stream_base, &wrap->parent_stream_listener);
    wrap->parent_stream_base = nullptr;
    wrap->parent_stream_listener.previous = nullptr;
  }
  if (wrap->ssl != nullptr) {
    CleanupPendingWrites(wrap, UV_ECANCELED);
    SSL_free(wrap->ssl);
    wrap->ssl = nullptr;
    wrap->enc_in = nullptr;
    wrap->enc_out = nullptr;
    wrap->parent_write_in_progress = false;
    wrap->write_callback_scheduled = false;
    wrap->refed = false;
  }
  if (wrap->next_session != nullptr) {
    SSL_SESSION_free(wrap->next_session);
    wrap->next_session = nullptr;
  }
  DeleteRefIfPresent(wrap->env, &wrap->active_parent_write_req_ref);
  DeleteRefIfPresent(wrap->env, &wrap->active_empty_write_req_ref);
  DeleteRefIfPresent(wrap->env, &wrap->user_read_buffer_ref);
  wrap->user_buffer_base = nullptr;
  wrap->user_buffer_len = 0;
  ReleaseKeepaliveHandle(wrap);
}

void TlsWrapFinalize(napi_env env, void* data, void* /*hint*/) {
  auto* wrap = static_cast<TlsWrap*>(data);
  if (wrap == nullptr) return;
  DestroySsl(wrap);
  ReleaseKeepaliveHandle(wrap);
  auto* environment = EdgeEnvironmentGet(env);
  if (environment == nullptr || !environment->cleanup_started()) {
    NotifyTlsStreamClosed(wrap);
  }
  RemoveWrapFromState(wrap);
  DeleteRefIfPresent(env, &wrap->parent_ref);
  DeleteRefIfPresent(env, &wrap->context_ref);
  DeleteRefIfPresent(env, &wrap->active_write_req_ref);
  DeleteRefIfPresent(env, &wrap->active_empty_write_req_ref);
  DeleteRefIfPresent(env, &wrap->user_read_buffer_ref);
  EdgeStreamBaseFinalize(&wrap->base);
  delete wrap;
}

void EmitHandshakeCallback(TlsWrap* wrap, const char* name, size_t argc, napi_value* argv) {
  if (wrap == nullptr || wrap->env == nullptr) return;
  edge::HandleScope scope(wrap->env);
  if (!scope.is_open()) return;
  napi_value self = GetRefValue(wrap->env, wrap->wrapper_ref);
  napi_value cb = GetNamedValue(wrap->env, self, name);
  if (!IsFunction(wrap->env, cb)) return;
  napi_value ignored = nullptr;
  (void)EdgeAsyncWrapMakeCallback(
      wrap->env, wrap->async_id, self, self, cb, argc, argv, &ignored, kEdgeMakeCallbackNone);
}

void EmitHandshakeDone(TlsWrap* wrap) {
  if (wrap == nullptr || wrap->ssl == nullptr) return;
  if (wrap->keepalive_needed) {
    ReleaseKeepaliveHandle(wrap);
  }
  EmitHandshakeCallback(wrap, "onhandshakedone", 0, nullptr);
}

void OnClientHello(TlsWrap* wrap, const ClientHelloData& hello) {
  if (wrap == nullptr || wrap->env == nullptr) return;
  edge::HandleScope scope(wrap->env);
  if (!scope.is_open()) return;
  const uint64_t ctx_options =
      wrap->ssl != nullptr && SSL_get_SSL_CTX(wrap->ssl) != nullptr ? SSL_CTX_get_options(SSL_get_SSL_CTX(wrap->ssl))
                                                                    : 0;
  const bool has_ticket = hello.has_ticket &&
                          (ctx_options & SSL_OP_NO_TICKET) == 0;
  napi_value self = GetRefValue(wrap->env, wrap->wrapper_ref);
  if (self == nullptr) return;
  napi_value cb = GetNamedValue(wrap->env, self, "onclienthello");
  if (!IsFunction(wrap->env, cb)) return;

  napi_value hello_obj = nullptr;
  napi_create_object(wrap->env, &hello_obj);
  napi_value session_id = CreateBufferCopy(wrap->env, hello.session_id, hello.session_size);
  napi_value servername = nullptr;
  if (hello.servername == nullptr) {
    napi_create_string_utf8(wrap->env, "", 0, &servername);
  } else {
    napi_create_string_utf8(wrap->env,
                            reinterpret_cast<const char*>(hello.servername),
                            hello.servername_size,
                            &servername);
  }
  napi_value tls_ticket = MakeBool(wrap->env, has_ticket);
  if (hello_obj == nullptr || session_id == nullptr || servername == nullptr || tls_ticket == nullptr) return;
  napi_set_named_property(wrap->env, hello_obj, "sessionId", session_id);
  napi_set_named_property(wrap->env, hello_obj, "servername", servername);
  napi_set_named_property(wrap->env, hello_obj, "tlsTicket", tls_ticket);

  napi_value argv[1] = {hello_obj};
  napi_value ignored = nullptr;
  (void)EdgeAsyncWrapMakeCallback(
      wrap->env, wrap->async_id, self, self, cb, 1, argv, &ignored, kEdgeMakeCallbackNone);
}

int NewSessionCallback(SSL* ssl, SSL_SESSION* session);

void SslInfoCallback(const SSL* ssl, int where, int /*ret*/) {
  if ((where & (SSL_CB_HANDSHAKE_START | SSL_CB_HANDSHAKE_DONE)) == 0) return;
  TlsWrap* wrap = static_cast<TlsWrap*>(SSL_get_app_data(ssl));
  if (wrap == nullptr || wrap->env == nullptr) return;
  edge::HandleScope scope(wrap->env);
  if (!scope.is_open()) return;
  if ((where & SSL_CB_HANDSHAKE_START) != 0) {
    const int64_t now_ms = static_cast<int64_t>(uv_hrtime() / 1000000ULL);
    napi_value argv[1] = {MakeInt64(wrap->env, now_ms)};
    EmitHandshakeCallback(wrap, "onhandshakestart", 1, argv);
  }
  if ((where & SSL_CB_HANDSHAKE_DONE) != 0 && SSL_renegotiate_pending(const_cast<SSL*>(ssl)) == 0) {
    wrap->established = true;
    if (!wrap->is_server && wrap->session_callbacks_enabled && wrap->client_session_event_count == 0 &&
        SSL_version(const_cast<SSL*>(ssl)) < TLS1_3_VERSION &&
        SSL_session_reused(const_cast<SSL*>(ssl)) != 1) {
      SSL_SESSION* session = SSL_get_session(const_cast<SSL*>(ssl));
      if (session != nullptr) {
        (void)NewSessionCallback(const_cast<SSL*>(ssl), session);
      }
    }
    EmitHandshakeDone(wrap);
  }
}

void KeylogCallback(const SSL* ssl, const char* line) {
  TlsWrap* wrap = static_cast<TlsWrap*>(SSL_get_app_data(ssl));
  if (wrap == nullptr || wrap->env == nullptr || line == nullptr) return;
  edge::HandleScope scope(wrap->env);
  if (!scope.is_open()) return;
  const size_t len = std::strlen(line);
  std::vector<uint8_t> bytes(len + 1, 0);
  if (len > 0) std::memcpy(bytes.data(), line, len);
  bytes[len] = '\n';
  napi_value self = GetRefValue(wrap->env, wrap->wrapper_ref);
  napi_value cb = GetNamedValue(wrap->env, self, "onkeylog");
  if (!IsFunction(wrap->env, cb)) return;
  napi_value buffer = CreateBufferCopy(wrap->env, bytes.data(), bytes.size());
  if (buffer == nullptr) return;
  napi_value argv[1] = {buffer};
  napi_value ignored = nullptr;
  (void)EdgeAsyncWrapMakeCallback(
      wrap->env, wrap->async_id, self, self, cb, 1, argv, &ignored, kEdgeMakeCallbackNone);
}

int VerifyCallback(int /*preverify_ok*/, X509_STORE_CTX* /*ctx*/) {
  return 1;
}

unsigned int PskServerCallback(SSL* ssl,
                               const char* identity,
                               unsigned char* psk,
                               unsigned int max_psk_len) {
  TlsWrap* wrap = static_cast<TlsWrap*>(SSL_get_app_data(ssl));
  if (wrap == nullptr || wrap->env == nullptr) return 0;
  edge::HandleScope scope(wrap->env);
  if (!scope.is_open()) return 0;

  napi_value self = GetRefValue(wrap->env, wrap->wrapper_ref);
  napi_value cb = GetPropertyBySymbol(wrap->env, self, "onpskexchange");
  if (!IsFunction(wrap->env, cb)) return 0;

  napi_value identity_v = nullptr;
  if (identity == nullptr) {
    napi_get_null(wrap->env, &identity_v);
  } else {
    napi_create_string_utf8(wrap->env, identity, NAPI_AUTO_LENGTH, &identity_v);
  }
  napi_value max_psk_v = MakeInt32(wrap->env, static_cast<int32_t>(max_psk_len));
  napi_value argv[2] = {identity_v, max_psk_v};
  napi_value out = nullptr;
  if (EdgeAsyncWrapMakeCallback(
          wrap->env, wrap->async_id, self, self, cb, 2, argv, &out, kEdgeMakeCallbackNone) != napi_ok ||
      out == nullptr) {
    return 0;
  }

  const uint8_t* data = nullptr;
  size_t len = 0;
  if (!GetArrayBufferViewSpan(wrap->env, out, &data, &len) || data == nullptr || len > max_psk_len) {
    return 0;
  }
  std::memcpy(psk, data, len);
  return static_cast<unsigned int>(len);
}

unsigned int PskClientCallback(SSL* ssl,
                               const char* hint,
                               char* identity,
                               unsigned int max_identity_len,
                               unsigned char* psk,
                               unsigned int max_psk_len) {
  TlsWrap* wrap = static_cast<TlsWrap*>(SSL_get_app_data(ssl));
  if (wrap == nullptr || wrap->env == nullptr) return 0;
  edge::HandleScope scope(wrap->env);
  if (!scope.is_open()) return 0;

  napi_value self = GetRefValue(wrap->env, wrap->wrapper_ref);
  napi_value cb = GetPropertyBySymbol(wrap->env, self, "onpskexchange");
  if (!IsFunction(wrap->env, cb)) return 0;

  napi_value hint_v = nullptr;
  if (hint == nullptr) {
    napi_get_null(wrap->env, &hint_v);
  } else {
    napi_create_string_utf8(wrap->env, hint, NAPI_AUTO_LENGTH, &hint_v);
  }
  napi_value argv[3] = {
      hint_v,
      MakeInt32(wrap->env, static_cast<int32_t>(max_psk_len)),
      MakeInt32(wrap->env, static_cast<int32_t>(max_identity_len)),
  };
  napi_value result = nullptr;
  if (EdgeAsyncWrapMakeCallback(
          wrap->env, wrap->async_id, self, self, cb, 3, argv, &result, kEdgeMakeCallbackNone) != napi_ok ||
      result == nullptr) {
    return 0;
  }

  napi_value identity_v = GetNamedValue(wrap->env, result, "identity");
  napi_value psk_v = GetNamedValue(wrap->env, result, "psk");
  if (identity_v == nullptr || psk_v == nullptr) return 0;

  napi_valuetype identity_type = napi_undefined;
  if (napi_typeof(wrap->env, identity_v, &identity_type) != napi_ok || identity_type != napi_string) {
    return 0;
  }
  const std::string identity_str = ValueToUtf8(wrap->env, identity_v);
  if (identity_str.size() > max_identity_len) return 0;

  const uint8_t* psk_data = nullptr;
  size_t psk_len = 0;
  if (!GetArrayBufferViewSpan(wrap->env, psk_v, &psk_data, &psk_len) || psk_data == nullptr ||
      psk_len > max_psk_len) {
    return 0;
  }

  std::memcpy(identity, identity_str.data(), identity_str.size());
  if (identity_str.size() < max_identity_len) {
    identity[identity_str.size()] = '\0';
  }
  std::memcpy(psk, psk_data, psk_len);
  return static_cast<unsigned int>(psk_len);
}

int CertCallback(SSL* ssl, void* arg) {
  auto* wrap = static_cast<TlsWrap*>(arg);
  if (wrap == nullptr || !wrap->is_server || !wrap->waiting_cert_cb) return 1;
  if (wrap->cert_cb_running) return -1;

  wrap->cert_cb_running = true;
  napi_value self = GetRefValue(wrap->env, wrap->wrapper_ref);
  napi_value owner = EdgeHandleWrapGetActiveOwner(wrap->env, wrap->wrapper_ref);
  napi_value info = nullptr;
  napi_create_object(wrap->env, &info);
  const char* servername = SSL_get_servername(ssl, TLSEXT_NAMETYPE_host_name);
  if (servername != nullptr) {
    napi_value sn = nullptr;
    napi_create_string_utf8(wrap->env, servername, NAPI_AUTO_LENGTH, &sn);
    if (sn != nullptr) {
      napi_set_named_property(wrap->env, info, "servername", sn);
      if (owner != nullptr) {
        napi_set_named_property(wrap->env, owner, "servername", sn);
      }
    }
  }
  napi_value ocsp = MakeBool(wrap->env, SSL_get_tlsext_status_type(ssl) == TLSEXT_STATUSTYPE_ocsp);
  if (ocsp != nullptr) napi_set_named_property(wrap->env, info, "OCSPRequest", ocsp);

  napi_value argv[1] = {info};
  EmitHandshakeCallback(wrap, "oncertcb", 1, argv);
  return wrap->cert_cb_running ? -1 : 1;
}

napi_value GetSSLOCSPResponse(TlsWrap* wrap, SSL* ssl) {
  if (wrap == nullptr || wrap->env == nullptr || ssl == nullptr) return nullptr;
  const unsigned char* resp = nullptr;
  const int len = SSL_get_tlsext_status_ocsp_resp(ssl, &resp);
  if (resp == nullptr || len < 0) {
    return Null(wrap->env);
  }
  std::vector<uint8_t> decoded;
  const uint8_t* payload = resp;
  size_t payload_len = static_cast<size_t>(len);
  if (UnwrapSyntheticOcspResponse(resp, static_cast<size_t>(len), &decoded)) {
    payload = decoded.data();
    payload_len = decoded.size();
  }
  napi_value buffer = CreateBufferCopy(wrap->env, payload, payload_len);
  if (buffer == nullptr) return nullptr;
  return buffer;
}

bool MaybeEmitClientOcspResponse(TlsWrap* wrap, bool allow_null) {
  if (wrap == nullptr || wrap->env == nullptr || wrap->ssl == nullptr || wrap->is_server ||
      wrap->client_ocsp_event_emitted || !wrap->pending_client_ocsp_event) {
    return false;
  }
  edge::HandleScope scope(wrap->env);
  if (!scope.is_open()) return false;

  const unsigned char* resp = nullptr;
  const int len = SSL_get_tlsext_status_ocsp_resp(wrap->ssl, &resp);
  if ((resp == nullptr || len < 0) && !allow_null) {
    return false;
  }

  napi_value self = GetRefValue(wrap->env, wrap->wrapper_ref);
  napi_value cb = GetNamedValue(wrap->env, self, "onocspresponse");
  if (!IsFunction(wrap->env, cb)) {
    wrap->pending_client_ocsp_event = false;
    wrap->client_ocsp_event_emitted = true;
    return false;
  }

  napi_value arg = nullptr;
  if (resp != nullptr && len >= 0) {
    arg = GetSSLOCSPResponse(wrap, wrap->ssl);
  }
  if (arg == nullptr) {
    arg = Null(wrap->env);
  }

  napi_value argv[1] = {arg};
  napi_value ignored = nullptr;
  (void)EdgeAsyncWrapMakeCallback(
      wrap->env, wrap->async_id, self, self, cb, 1, argv, &ignored, kEdgeMakeCallbackNone);
  wrap->pending_client_ocsp_event = false;
  wrap->client_ocsp_event_emitted = true;
  return true;
}

int TLSExtStatusCallback(SSL* ssl, void* /*arg*/) {
  auto* wrap = static_cast<TlsWrap*>(SSL_get_app_data(ssl));
  if (wrap == nullptr || wrap->env == nullptr) return 1;
  edge::HandleScope scope(wrap->env);
  if (!scope.is_open()) return 1;

  if (!wrap->is_server) {
    wrap->pending_client_ocsp_event = true;
    (void)MaybeEmitClientOcspResponse(wrap, true);
    return 1;
  }

  const unsigned char* existing = nullptr;
  if (SSL_get_tlsext_status_ocsp_resp(ssl, &existing) >= 0 && existing != nullptr) {
    wrap->ocsp_response.clear();
    return SSL_TLSEXT_ERR_OK;
  }

  if (wrap->ocsp_response.empty()) return SSL_TLSEXT_ERR_NOACK;
  std::vector<uint8_t> wire_response = wrap->ocsp_response;
  if (!IsValidOcspResponseDer(wire_response.data(), wire_response.size())) {
    wire_response = WrapSyntheticOcspResponse(wire_response.data(), wire_response.size());
  }

  const size_t len = wire_response.size();
  unsigned char* data = static_cast<unsigned char*>(OPENSSL_malloc(len));
  if (data == nullptr) return SSL_TLSEXT_ERR_NOACK;
  if (len > 0) {
    std::memcpy(data, wire_response.data(), len);
  }
  if (!SSL_set_tlsext_status_ocsp_resp(ssl, data, static_cast<int>(len))) {
    OPENSSL_free(data);
    return SSL_TLSEXT_ERR_NOACK;
  }
  wrap->ocsp_response.clear();
  return SSL_TLSEXT_ERR_OK;
}

int SelectALPNCallback(SSL* ssl,
                       const unsigned char** out,
                       unsigned char* outlen,
                       const unsigned char* in,
                       unsigned int inlen,
                       void* /*arg*/) {
  auto* wrap = static_cast<TlsWrap*>(SSL_get_app_data(ssl));
  if (wrap == nullptr || wrap->env == nullptr) return SSL_TLSEXT_ERR_NOACK;
  edge::HandleScope scope(wrap->env);
  if (!scope.is_open()) return SSL_TLSEXT_ERR_NOACK;

  if (wrap->alpn_callback_enabled) {
    napi_value self = GetRefValue(wrap->env, wrap->wrapper_ref);
    napi_value cb = GetNamedValue(wrap->env, self, "ALPNCallback");
    if (!IsFunction(wrap->env, cb)) return SSL_TLSEXT_ERR_ALERT_FATAL;
    napi_value buffer = CreateBufferCopy(wrap->env, in, inlen);
    napi_value argv[1] = {buffer};
    napi_value result = nullptr;
    if (EdgeAsyncWrapMakeCallback(
            wrap->env, wrap->async_id, self, self, cb, 1, argv, &result, kEdgeMakeCallbackNone) != napi_ok ||
        result == nullptr) {
      return SSL_TLSEXT_ERR_ALERT_FATAL;
    }
    napi_valuetype type = napi_undefined;
    if (napi_typeof(wrap->env, result, &type) != napi_ok || type == napi_undefined) {
      return SSL_TLSEXT_ERR_ALERT_FATAL;
    }
    uint32_t offset = 0;
    if (napi_get_value_uint32(wrap->env, result, &offset) != napi_ok || offset >= inlen) {
      return SSL_TLSEXT_ERR_ALERT_FATAL;
    }
    *outlen = *(in + offset);
    *out = in + offset + 1;
    return SSL_TLSEXT_ERR_OK;
  }

  if (wrap->alpn_protos.empty()) return SSL_TLSEXT_ERR_NOACK;
  const int rc =
      SSL_select_next_proto(const_cast<unsigned char**>(out),
                            outlen,
                            wrap->alpn_protos.data(),
                            static_cast<unsigned int>(wrap->alpn_protos.size()),
                            in,
                            inlen);
  return rc == OPENSSL_NPN_NEGOTIATED ? SSL_TLSEXT_ERR_OK : SSL_TLSEXT_ERR_ALERT_FATAL;
}

SSL_SESSION* GetSessionCallback(SSL* ssl,
                                const unsigned char* /*key*/,
                                int /*len*/,
                                int* copy) {
  auto* wrap = static_cast<TlsWrap*>(SSL_get_app_data(ssl));
  if (copy != nullptr) *copy = 0;
  if (wrap == nullptr) return nullptr;
  SSL_SESSION* session = wrap->next_session;
  wrap->next_session = nullptr;
  return session;
}

int NewSessionCallback(SSL* ssl, SSL_SESSION* session) {
  auto* wrap = static_cast<TlsWrap*>(SSL_get_app_data(ssl));
  if (wrap == nullptr || wrap->env == nullptr || session == nullptr) return 1;
  edge::HandleScope scope(wrap->env);
  if (!scope.is_open()) return 1;
  if (!wrap->session_callbacks_enabled) return 0;
  napi_value self = GetRefValue(wrap->env, wrap->wrapper_ref);
  napi_value cb = GetNamedValue(wrap->env, self, "onnewsession");
  if (!IsFunction(wrap->env, cb)) return 0;

  unsigned int id_len = 0;
  const unsigned char* id = SSL_SESSION_get_id(session, &id_len);
  napi_value id_buffer = CreateBufferCopy(wrap->env, id, id_len);

  const int encoded_len = i2d_SSL_SESSION(session, nullptr);
  if (encoded_len <= 0 || id_buffer == nullptr) return 0;
  std::vector<uint8_t> encoded(static_cast<size_t>(encoded_len));
  unsigned char* ptr = encoded.data();
  if (i2d_SSL_SESSION(session, &ptr) != encoded_len) return 0;

  napi_value session_buffer = CreateBufferCopy(wrap->env, encoded.data(), encoded.size());
  if (session_buffer == nullptr) return 0;

  napi_value argv[2] = {id_buffer, session_buffer};
  if (wrap->is_server) wrap->awaiting_new_session = true;
  if (!wrap->is_server) wrap->client_session_event_count++;
  napi_value ignored = nullptr;
  (void)EdgeAsyncWrapMakeCallback(
      wrap->env, wrap->async_id, self, self, cb, 2, argv, &ignored, kEdgeMakeCallbackNone);
  return 0;
}

void DeleteClientSessionFallbackTask(napi_env env, ClientSessionFallbackTask* task) {
  if (task == nullptr) return;
  DeleteRefIfPresent(env, &task->self_ref);
  delete task;
}

void DeleteParentWriteTask(napi_env env, ParentWriteTask* task) {
  if (task == nullptr) return;
  DeleteRefIfPresent(env, &task->self_ref);
  DeleteRefIfPresent(env, &task->req_ref);
  delete task;
}

void DeleteInvokeQueuedTask(napi_env env, InvokeQueuedTask* task) {
  if (task == nullptr) return;
  DeleteRefIfPresent(env, &task->self_ref);
  delete task;
}

napi_value RunClientSessionFallbackTask(napi_env env, napi_callback_info info) {
  void* data = nullptr;
  napi_get_cb_info(env, info, nullptr, nullptr, nullptr, &data);
  auto* task = static_cast<ClientSessionFallbackTask*>(data);
  if (env == nullptr || task == nullptr || task->self_ref == nullptr) return Undefined(env);
  napi_value self = GetRefValue(env, task->self_ref);
  TlsWrap* wrap = self != nullptr ? FindWrapBySelf(env, self) : nullptr;
  if (wrap != nullptr) {
    wrap->client_session_fallback_scheduled = false;
  }
  if (wrap == nullptr || wrap->ssl == nullptr) {
    DeleteClientSessionFallbackTask(env, task);
    return Undefined(env);
  }
  if (wrap->is_server || !wrap->established || !wrap->session_callbacks_enabled ||
      wrap->client_session_fallback_emitted) {
    DeleteClientSessionFallbackTask(env, task);
    return Undefined(env);
  }
  if (SSL_version(wrap->ssl) >= TLS1_3_VERSION || SSL_session_reused(wrap->ssl) == 1) {
    DeleteClientSessionFallbackTask(env, task);
    return Undefined(env);
  }
  SSL_SESSION* session = SSL_get_session(wrap->ssl);
  if (session != nullptr) {
    wrap->client_session_fallback_emitted = true;
    (void)NewSessionCallback(wrap->ssl, session);
  }
  DeleteClientSessionFallbackTask(env, task);
  return Undefined(env);
}

napi_value RunInvokeQueuedTask(napi_env env, napi_callback_info info) {
  size_t argc = 0;
  void* data = nullptr;
  napi_get_cb_info(env, info, &argc, nullptr, nullptr, &data);
  auto* task = static_cast<InvokeQueuedTask*>(data);
  if (task == nullptr) return Undefined(env);

  napi_value self = GetRefValue(env, task->self_ref);
  TlsWrap* wrap = self != nullptr ? FindWrapBySelf(env, self) : nullptr;
  if (wrap != nullptr) {
    InvokeQueued(wrap, task->status);
  }
  DeleteInvokeQueuedTask(env, task);
  return Undefined(env);
}

napi_value RunParentWriteTask(napi_env env, napi_callback_info info) {
  size_t argc = 0;
  void* data = nullptr;
  napi_get_cb_info(env, info, &argc, nullptr, nullptr, &data);
  auto* task = static_cast<ParentWriteTask*>(data);
  if (task == nullptr) return Undefined(env);

  napi_value self = GetRefValue(env, task->self_ref);
  napi_value req_obj = GetRefValue(env, task->req_ref);
  TlsWrap* wrap = self != nullptr ? FindWrapBySelf(env, self) : nullptr;
  if (wrap != nullptr && wrap->parent_stream_base != nullptr && req_obj != nullptr) {
    EdgeStreamBaseEmitAfterWrite(wrap->parent_stream_base, req_obj, task->status);
  }

  DeleteParentWriteTask(env, task);
  return Undefined(env);
}

void ScheduleClientSessionFallback(TlsWrap* wrap) {
  if (wrap == nullptr || wrap->env == nullptr || wrap->ssl == nullptr || wrap->is_server ||
      !wrap->established || !wrap->session_callbacks_enabled || wrap->client_session_fallback_emitted ||
      wrap->client_session_fallback_scheduled) {
    return;
  }
  if (SSL_version(wrap->ssl) >= TLS1_3_VERSION || SSL_session_reused(wrap->ssl) == 1) return;
  napi_value self = GetRefValue(wrap->env, wrap->wrapper_ref);
  if (self == nullptr) return;
  auto* task = new ClientSessionFallbackTask();
  if (napi_create_reference(wrap->env, self, 1, &task->self_ref) != napi_ok || task->self_ref == nullptr) {
    delete task;
    return;
  }
  napi_value callback = nullptr;
  if (napi_create_function(
          wrap->env, "__ubiTlsSessionFallback", NAPI_AUTO_LENGTH, RunClientSessionFallbackTask, task, &callback) !=
          napi_ok ||
      callback == nullptr) {
    DeleteClientSessionFallbackTask(wrap->env, task);
    return;
  }
  napi_value global = internal_binding::GetGlobal(wrap->env);
  napi_value process = nullptr;
  napi_value next_tick = nullptr;
  napi_valuetype next_tick_type = napi_undefined;
  if (global == nullptr ||
      napi_get_named_property(wrap->env, global, "process", &process) != napi_ok ||
      process == nullptr ||
      napi_get_named_property(wrap->env, process, "nextTick", &next_tick) != napi_ok ||
      next_tick == nullptr ||
      napi_typeof(wrap->env, next_tick, &next_tick_type) != napi_ok ||
      next_tick_type != napi_function) {
    DeleteClientSessionFallbackTask(wrap->env, task);
    return;
  }
  napi_value argv[1] = {callback};
  napi_value ignored = nullptr;
  if (napi_call_function(wrap->env, process, next_tick, 1, argv, &ignored) != napi_ok) {
    DeleteClientSessionFallbackTask(wrap->env, task);
    return;
  }
  wrap->client_session_fallback_scheduled = true;
}

bool ScheduleInvokeQueued(TlsWrap* wrap, int status) {
  if (wrap == nullptr || wrap->env == nullptr) return false;
  napi_value self = GetRefValue(wrap->env, wrap->wrapper_ref);
  if (self == nullptr) return false;

  auto* task = new InvokeQueuedTask();
  task->status = status;

  if (napi_create_reference(wrap->env, self, 1, &task->self_ref) != napi_ok || task->self_ref == nullptr) {
    DeleteInvokeQueuedTask(wrap->env, task);
    return false;
  }

  napi_value callback = nullptr;
  if (napi_create_function(wrap->env,
                           "__ubiTlsInvokeQueued",
                           NAPI_AUTO_LENGTH,
                           RunInvokeQueuedTask,
                           task,
                           &callback) != napi_ok ||
      callback == nullptr) {
    DeleteInvokeQueuedTask(wrap->env, task);
    return false;
  }

  napi_value global = internal_binding::GetGlobal(wrap->env);
  napi_value set_immediate = nullptr;
  napi_valuetype set_immediate_type = napi_undefined;
  if (global == nullptr ||
      napi_get_named_property(wrap->env, global, "setImmediate", &set_immediate) != napi_ok ||
      set_immediate == nullptr ||
      napi_typeof(wrap->env, set_immediate, &set_immediate_type) != napi_ok ||
      set_immediate_type != napi_function) {
    DeleteInvokeQueuedTask(wrap->env, task);
    return false;
  }

  napi_value argv[1] = {callback};
  napi_value ignored = nullptr;
  if (napi_call_function(wrap->env, global, set_immediate, 1, argv, &ignored) != napi_ok) {
    DeleteInvokeQueuedTask(wrap->env, task);
    return false;
  }
  return true;
}

bool ScheduleParentWriteCompletion(TlsWrap* wrap, napi_value req_obj, int status) {
  if (wrap == nullptr || wrap->env == nullptr || req_obj == nullptr) return false;
  napi_value self = GetRefValue(wrap->env, wrap->wrapper_ref);
  if (self == nullptr) return false;

  auto* task = new ParentWriteTask();
  task->status = status;

  if (napi_create_reference(wrap->env, self, 1, &task->self_ref) != napi_ok || task->self_ref == nullptr ||
      napi_create_reference(wrap->env, req_obj, 1, &task->req_ref) != napi_ok || task->req_ref == nullptr) {
    DeleteParentWriteTask(wrap->env, task);
    return false;
  }

  napi_value callback = nullptr;
  if (napi_create_function(
          wrap->env, "__ubiTlsParentWriteTask", NAPI_AUTO_LENGTH, RunParentWriteTask, task, &callback) != napi_ok ||
      callback == nullptr) {
    DeleteParentWriteTask(wrap->env, task);
    return false;
  }

  napi_value global = internal_binding::GetGlobal(wrap->env);
  napi_value set_immediate = nullptr;
  napi_valuetype type = napi_undefined;
  if (global == nullptr ||
      napi_get_named_property(wrap->env, global, "setImmediate", &set_immediate) != napi_ok ||
      set_immediate == nullptr ||
      napi_typeof(wrap->env, set_immediate, &type) != napi_ok ||
      type != napi_function) {
    DeleteParentWriteTask(wrap->env, task);
    return false;
  }

  napi_value argv[1] = {callback};
  napi_value ignored = nullptr;
  if (napi_call_function(wrap->env, global, set_immediate, 1, argv, &ignored) != napi_ok) {
    DeleteParentWriteTask(wrap->env, task);
    return false;
  }
  return true;
}

TlsWrap* GetWrapFromListener(EdgeStreamListener* listener) {
  return listener != nullptr ? static_cast<TlsWrap*>(listener->data) : nullptr;
}

bool ParentStreamOnAlloc(EdgeStreamListener* listener, size_t suggested_size, uv_buf_t* out) {
  TlsWrap* wrap = GetWrapFromListener(listener);
  if (wrap == nullptr || out == nullptr) return false;
  wrap->parent_read_buffer.resize(suggested_size);
  *out = uv_buf_init(wrap->parent_read_buffer.data(), static_cast<unsigned int>(wrap->parent_read_buffer.size()));
  return true;
}

bool ParentStreamOnRead(EdgeStreamListener* listener, ssize_t nread, const uv_buf_t* buf) {
  TlsWrap* wrap = GetWrapFromListener(listener);
  if (wrap == nullptr || wrap->ssl == nullptr) return true;

  if (nread <= 0) {
    if (nread < 0) {
      wrap->parent_terminal_read_emitted = true;
      if (nread == UV_EOF) {
        (void)ReadCleartext(wrap);
        wrap->eof = true;
      } else {
        (void)ReadCleartext(wrap);
      }
      EmitOnReadStatus(wrap, static_cast<int32_t>(nread));
    }
    return true;
  }

  if (buf == nullptr || buf->base == nullptr) return true;
  edge::crypto::EdgeBIO* enc_in = edge::crypto::EdgeBIO::FromBIO(wrap->enc_in);
  enc_in->Write(buf->base, static_cast<size_t>(nread));

  if (!wrap->hello_parser.IsEnded()) {
    size_t avail = 0;
    char* hello_data = enc_in->Peek(&avail);
    if (hello_data != nullptr && avail > 0) {
      wrap->hello_parser.Parse(wrap, reinterpret_cast<const uint8_t*>(hello_data), avail);
    }
    if (!wrap->hello_parser.IsEnded()) {
      return true;
    }
  }

  Cycle(wrap);
  return true;
}

bool ParentStreamOnAfterWrite(EdgeStreamListener* listener, napi_value req_obj, int status) {
  TlsWrap* wrap = GetWrapFromListener(listener);
  if (wrap == nullptr) return true;

  const bool is_active_parent_write = RefMatches(wrap->env, wrap->active_parent_write_req_ref, req_obj);
  if (wrap->has_active_write_issued_by_prev_listener || !is_active_parent_write) {
    (void)EdgeStreamPassAfterWrite(listener, req_obj, status);
    return true;
  }

  (void)EdgeStreamPassAfterWrite(listener, req_obj, 0);

  wrap->parent_write_in_progress = false;
  DeleteRefIfPresent(wrap->env, &wrap->active_parent_write_req_ref);

  if (wrap->pending_encrypted_writes.empty()) {
    wrap->parent_write_size = 0;
    return true;
  }

  int effective_status = (wrap->ssl == nullptr && status == 0) ? UV_ECANCELED : status;
  if (wrap->active_empty_write_req_ref != nullptr) {
    ConsumeEncryptedWriteQueue(wrap, 0, effective_status);
    wrap->parent_write_size = 0;
    napi_ref req_ref = wrap->active_empty_write_req_ref;
    wrap->active_empty_write_req_ref = nullptr;
    CompleteDetachedReq(wrap, req_ref, effective_status);
    return true;
  }

  if (effective_status == 0 && wrap->parent_write_size != 0 && wrap->enc_out != nullptr) {
    edge::crypto::EdgeBIO::FromBIO(wrap->enc_out)->Read(nullptr, wrap->parent_write_size);
  }
  ConsumeEncryptedWriteQueue(wrap, effective_status == 0 ? wrap->parent_write_size : 0, effective_status);
  wrap->parent_write_size = 0;

  if (effective_status != 0) {
    if (!wrap->shutdown_started) {
      InvokeQueued(wrap, effective_status);
    }
    return true;
  }
  (void)WritePendingCleartextInput(wrap);
  EncOut(wrap);
  MaybeStartTlsShutdown(wrap);
  return true;
}

bool ParentStreamOnAfterShutdown(EdgeStreamListener* listener, napi_value req_obj, int status) {
  TlsWrap* wrap = GetWrapFromListener(listener);
  (void)EdgeStreamPassAfterShutdown(listener, req_obj, status);
  if (wrap == nullptr) return true;
  if (!RefMatches(wrap->env, wrap->pending_shutdown_req_ref, req_obj)) {
    return true;
  }
  wrap->shutdown_started = false;
  DeleteRefIfPresent(wrap->env, &wrap->pending_shutdown_req_ref);
  return true;
}

void ParentStreamOnClose(EdgeStreamListener* listener) {
  TlsWrap* wrap = GetWrapFromListener(listener);
  if (wrap == nullptr) return;
  // Node's TLSWrap does not synthesize a terminal read from stream close.
  // EOF/error ownership stays with the parent read path; close here is only
  // lifecycle cleanup. Emitting UV_EOF from close lets an old parent handle
  // surface 'end' on a reinitialized TLSSocket, which diverges from Node.
  wrap->parent_stream_base = nullptr;
  wrap->parent_stream_listener.previous = nullptr;
  wrap->parent_write_in_progress = false;
  wrap->shutdown_started = false;
  DeleteRefIfPresent(wrap->env, &wrap->active_parent_write_req_ref);
  CleanupPendingWrites(wrap, UV_ECANCELED);
  NotifyTlsStreamClosed(wrap);
}

void QueueEncryptedWrite(TlsWrap* wrap,
                         size_t size,
                         napi_ref completion_req_ref,
                         bool force_parent_turn = false);

void QueueNewEncryptedBytes(TlsWrap* wrap) {
  const size_t pending = PendingUnqueuedEncryptedBytes(wrap);
  if (pending == 0) return;
  QueueEncryptedWrite(wrap, pending, nullptr);
}

bool HandleSslError(TlsWrap* wrap, int ssl_result, const char* fallback_code, const char* fallback_message) {
  if (wrap == nullptr || wrap->ssl == nullptr) return true;
  const int err = SSL_get_error(wrap->ssl, ssl_result);
  if (err == SSL_ERROR_WANT_READ || err == SSL_ERROR_WANT_WRITE || err == SSL_ERROR_WANT_X509_LOOKUP) {
    return false;
  }
  if (err == SSL_ERROR_ZERO_RETURN) return false;
  QueueNewEncryptedBytes(wrap);
  if (wrap->queued_encrypted_bytes != 0) {
    TryStartParentWrite(wrap);
  }
  EmitError(wrap, CreateLastOpenSslError(wrap->env, fallback_code, fallback_message));
  return true;
}

void QueueEncryptedWrite(TlsWrap* wrap,
                         size_t size,
                         napi_ref completion_req_ref,
                         bool force_parent_turn) {
  if (wrap == nullptr) return;
  if (size == 0 && !force_parent_turn) {
    if (completion_req_ref != nullptr) {
      CompleteReq(wrap, &completion_req_ref, 0);
    }
    return;
  }
  PendingEncryptedWrite pending;
  pending.size = size;
  pending.completion_req_ref = completion_req_ref;
  pending.force_parent_turn = force_parent_turn;
  wrap->pending_encrypted_writes.push_back(std::move(pending));
  wrap->queued_encrypted_bytes += size;
}

void EncOut(TlsWrap* wrap) {
  if (wrap == nullptr || wrap->ssl == nullptr || wrap->parent_write_in_progress ||
      wrap->awaiting_new_session || !wrap->hello_parser.IsEnded() ||
      wrap->has_active_write_issued_by_prev_listener) {
    return;
  }

  if (wrap->established && wrap->active_write_req_ref != nullptr) {
    wrap->write_callback_scheduled = true;
  }

  if (wrap->pending_encrypted_writes.empty()) {
    if (wrap->pending_cleartext_input.empty()) {
      if (!wrap->in_dowrite) {
        InvokeQueued(wrap, 0);
      } else {
        (void)ScheduleInvokeQueued(wrap, 0);
      }
    }
    return;
  }

  TryStartParentWrite(wrap);
}

napi_value CreateInternalWriteReq(TlsWrap* wrap) {
  return wrap != nullptr ? EdgeCreateStreamReqObject(wrap->env) : nullptr;
}

void ConsumeEncryptedWriteQueue(TlsWrap* wrap, size_t committed, int status) {
  if (wrap == nullptr) return;

  while (!wrap->pending_encrypted_writes.empty()) {
    PendingEncryptedWrite& pending = wrap->pending_encrypted_writes.front();
    if (pending.size == 0) {
      PendingEncryptedWrite finished = std::move(pending);
      wrap->pending_encrypted_writes.pop_front();
      CompleteReq(wrap, &finished.completion_req_ref, status);
      continue;
    }
    if (committed == 0) break;
    if (pending.size <= committed) {
      committed -= pending.size;
      wrap->queued_encrypted_bytes -= pending.size;
      PendingEncryptedWrite finished = std::move(pending);
      wrap->pending_encrypted_writes.pop_front();
      CompleteReq(wrap, &finished.completion_req_ref, status);
      continue;
    }
    pending.size -= committed;
    wrap->queued_encrypted_bytes -= committed;
    committed = 0;
  }
}

napi_value CreateBioWritevChunks(TlsWrap* wrap, size_t* total_out) {
  if (total_out != nullptr) *total_out = 0;
  if (wrap == nullptr || wrap->env == nullptr || wrap->enc_out == nullptr) return nullptr;

  char* data[10];
  size_t size[10];
  size_t count = sizeof(data) / sizeof(data[0]);
  const size_t total = edge::crypto::EdgeBIO::FromBIO(wrap->enc_out)->PeekMultiple(data, size, &count);
  if (total == 0 || count == 0) return nullptr;

  napi_value chunks = nullptr;
  if (napi_create_array_with_length(wrap->env, count, &chunks) != napi_ok || chunks == nullptr) {
    return nullptr;
  }

  for (size_t i = 0; i < count; ++i) {
    napi_value buffer = CreateBufferCopy(
        wrap->env,
        reinterpret_cast<const uint8_t*>(data[i]),
        size[i]);
    if (buffer == nullptr ||
        napi_set_element(wrap->env, chunks, static_cast<uint32_t>(i), buffer) != napi_ok) {
      return nullptr;
    }
  }

  if (total_out != nullptr) *total_out = total;
  return chunks;
}

void TryStartParentWrite(TlsWrap* wrap) {
  if (wrap == nullptr || wrap->env == nullptr || wrap->parent_write_in_progress ||
      wrap->parent_stream_base == nullptr ||
      wrap->has_active_write_issued_by_prev_listener || wrap->awaiting_new_session ||
      !wrap->hello_parser.IsEnded() ||
      wrap->pending_encrypted_writes.empty()) {
    return;
  }

  const PendingEncryptedWrite& pending = wrap->pending_encrypted_writes.front();
  napi_value req = CreateInternalWriteReq(wrap);
  napi_value payload = nullptr;
  napi_value chunks = nullptr;
  size_t parent_write_size = 0;
  const bool needs_force_turn = pending.force_parent_turn && pending.size == 0;
  if (req == nullptr) {
    PendingEncryptedWrite failed = std::move(wrap->pending_encrypted_writes.front());
    wrap->pending_encrypted_writes.pop_front();
    wrap->queued_encrypted_bytes -= failed.size;
    CompleteReq(wrap, &failed.completion_req_ref, UV_ENOMEM);
    InvokeQueued(wrap, UV_ENOMEM);
    return;
  }
  if (needs_force_turn) {
    payload = CreateBufferCopy(wrap->env, nullptr, 0);
    if (payload == nullptr) {
      PendingEncryptedWrite failed = std::move(wrap->pending_encrypted_writes.front());
      wrap->pending_encrypted_writes.pop_front();
      wrap->queued_encrypted_bytes -= failed.size;
      CompleteReq(wrap, &failed.completion_req_ref, UV_ENOMEM);
      InvokeQueued(wrap, UV_ENOMEM);
      return;
    }
  } else {
    chunks = CreateBioWritevChunks(wrap, &parent_write_size);
    if (chunks == nullptr || parent_write_size == 0) {
      PendingEncryptedWrite failed = std::move(wrap->pending_encrypted_writes.front());
      wrap->pending_encrypted_writes.pop_front();
      wrap->queued_encrypted_bytes -= failed.size;
      CompleteReq(wrap, &failed.completion_req_ref, UV_EPROTO);
      InvokeQueued(wrap, UV_EPROTO);
      return;
    }
  }

  if (napi_create_reference(wrap->env, req, 1, &wrap->active_parent_write_req_ref) != napi_ok ||
      wrap->active_parent_write_req_ref == nullptr) {
    PendingEncryptedWrite failed = std::move(wrap->pending_encrypted_writes.front());
    wrap->pending_encrypted_writes.pop_front();
    wrap->queued_encrypted_bytes -= failed.size;
    CompleteReq(wrap, &failed.completion_req_ref, UV_ENOMEM);
    InvokeQueued(wrap, UV_ENOMEM);
    return;
  }

  bool async = false;
  const int32_t rc = needs_force_turn
                         ? EdgeStreamBaseWriteBufferDirect(wrap->parent_stream_base, req, payload, &async)
                         : EdgeStreamBaseWritevDirect(wrap->parent_stream_base, req, chunks, &async);
  if (rc != 0) {
    DeleteRefIfPresent(wrap->env, &wrap->active_parent_write_req_ref);
    PendingEncryptedWrite failed = std::move(wrap->pending_encrypted_writes.front());
    wrap->pending_encrypted_writes.pop_front();
    wrap->queued_encrypted_bytes -= failed.size;
    CompleteReq(wrap, &failed.completion_req_ref, rc);
    InvokeQueued(wrap, rc);
    return;
  }
  wrap->parent_write_size = parent_write_size;
  wrap->parent_write_in_progress = true;
  if (!async) {
    if (!ScheduleParentWriteCompletion(wrap, req, 0)) {
      EdgeStreamBaseEmitAfterWrite(wrap->parent_stream_base, req, 0);
    }
    return;
  }
}

void MaybeStartParentShutdown(TlsWrap* wrap) {
  if (wrap == nullptr || wrap->pending_shutdown_req_ref == nullptr || wrap->shutdown_started) {
    return;
  }
  if (!wrap->established && !wrap->eof &&
      (wrap->parent_write_in_progress || !wrap->pending_encrypted_writes.empty())) {
    return;
  }
  if (wrap->parent_stream_base == nullptr) {
    InvokeReqWithStatus(wrap, &wrap->pending_shutdown_req_ref, UV_ECANCELED);
    return;
  }

  wrap->shutdown_started = true;
  napi_value req_obj = GetRefValue(wrap->env, wrap->pending_shutdown_req_ref);
  if (req_obj == nullptr) {
    DeleteRefIfPresent(wrap->env, &wrap->pending_shutdown_req_ref);
    return;
  }

  napi_value argv[1] = {req_obj};
  const int32_t rc = CallParentMethodInt(wrap, "shutdown", 1, argv, nullptr);
  if (rc != 0) {
    wrap->shutdown_started = false;
    InvokeReqWithStatus(wrap, &wrap->pending_shutdown_req_ref, rc);
    return;
  }
}

void MaybeStartTlsShutdown(TlsWrap* wrap) {
  if (wrap == nullptr || wrap->ssl == nullptr || wrap->pending_shutdown_req_ref == nullptr || wrap->shutdown_started) {
    return;
  }

  ncrypto::MarkPopErrorOnReturn mark_pop_error_on_return;
  int shutdown_rc = SSL_shutdown(wrap->ssl);
  if (shutdown_rc == 0) {
    shutdown_rc = SSL_shutdown(wrap->ssl);
  }
  QueueNewEncryptedBytes(wrap);
  EncOut(wrap);
  MaybeStartParentShutdown(wrap);
}

bool WritePendingCleartextInput(TlsWrap* wrap) {
  if (wrap == nullptr || wrap->ssl == nullptr || wrap->has_active_write_issued_by_prev_listener ||
      wrap->pending_cleartext_input.empty()) {
    return false;
  }

  ncrypto::MarkPopErrorOnReturn mark_pop_error_on_return;
  const int rc = SSL_write(wrap->ssl,
                           wrap->pending_cleartext_input.data(),
                           static_cast<int>(wrap->pending_cleartext_input.size()));
  if (rc == static_cast<int>(wrap->pending_cleartext_input.size())) {
    wrap->pending_cleartext_input.clear();
    QueueNewEncryptedBytes(wrap);
    return true;
  }

  QueueNewEncryptedBytes(wrap);
  const int err = SSL_get_error(wrap->ssl, rc);
  if (err == SSL_ERROR_SSL || err == SSL_ERROR_SYSCALL) {
    const std::string error_string = PeekLastOpenSslErrorString("TLS write failed");
    wrap->pending_cleartext_input.clear();
    wrap->write_callback_scheduled = true;
    InvokeQueued(wrap, UV_EPROTO, error_string.c_str());
    return true;
  }

  return !wrap->pending_encrypted_writes.empty();
}

bool ReadCleartext(TlsWrap* wrap) {
  if (wrap == nullptr || wrap->ssl == nullptr) return false;
  ncrypto::MarkPopErrorOnReturn mark_pop_error_on_return;
  bool made_progress = false;
  char buffer[16 * 1024];
  int last_err = SSL_ERROR_NONE;
  for (;;) {
    const int rc = SSL_read(wrap->ssl, buffer, sizeof(buffer));
    if (rc > 0) {
      EmitOnReadData(wrap, reinterpret_cast<const uint8_t*>(buffer), static_cast<size_t>(rc));
      made_progress = true;
      if (wrap->ssl == nullptr || wrap->base.destroy_notified) {
        return made_progress;
      }
      continue;
    }
    const int err = SSL_get_error(wrap->ssl, rc);
    last_err = err;
    if (err == SSL_ERROR_WANT_READ || err == SSL_ERROR_WANT_WRITE || err == SSL_ERROR_WANT_X509_LOOKUP) break;
    if (err == SSL_ERROR_ZERO_RETURN) {
      if (!wrap->eof) {
        wrap->eof = true;
        EmitOnReadStatus(wrap, UV_EOF);
      }
      break;
    }
    if (err == SSL_ERROR_SSL || err == SSL_ERROR_SYSCALL) {
      napi_value error = CreateLastOpenSslError(wrap->env, "ERR_TLS_READ", "TLS read failed");
      QueueNewEncryptedBytes(wrap);
      EncOut(wrap);
      made_progress = made_progress || wrap->parent_write_in_progress;
      EmitError(wrap, error);
      break;
    }
    break;
  }

  if (wrap->ssl != nullptr && last_err != SSL_ERROR_ZERO_RETURN) {
    QueueNewEncryptedBytes(wrap);
    EncOut(wrap);
    made_progress = made_progress || wrap->parent_write_in_progress;
  }
  return made_progress;
}

void Cycle(TlsWrap* wrap) {
  if (wrap == nullptr || wrap->ssl == nullptr) return;
  if (++wrap->cycle_depth > 1) {
    return;
  }

  for (; wrap->cycle_depth > 0; wrap->cycle_depth--) {
    (void)WritePendingCleartextInput(wrap);
    (void)ReadCleartext(wrap);
    (void)MaybeEmitClientOcspResponse(wrap, wrap->established);
    if (wrap->ssl == nullptr) {
      wrap->cycle_depth = 0;
      break;
    }
    EncOut(wrap);
    MaybeStartTlsShutdown(wrap);
  }
}

void InitSsl(TlsWrap* wrap) {
  if (wrap == nullptr || wrap->secure_context == nullptr || wrap->secure_context->ctx == nullptr) return;
  SSL_CTX_set_tlsext_status_cb(wrap->secure_context->ctx, TLSExtStatusCallback);
  SSL_CTX_set_tlsext_status_arg(wrap->secure_context->ctx, nullptr);
  wrap->ssl = SSL_new(wrap->secure_context->ctx);
  if (wrap->ssl == nullptr) return;
  SSL_CTX_sess_set_new_cb(wrap->secure_context->ctx, NewSessionCallback);
  SSL_CTX_sess_set_get_cb(wrap->secure_context->ctx, GetSessionCallback);
  wrap->enc_in = edge::crypto::EdgeBIO::New(wrap->env);
  wrap->enc_out = edge::crypto::EdgeBIO::New(wrap->env);
  if (wrap->enc_in == nullptr || wrap->enc_out == nullptr) {
    if (wrap->enc_in != nullptr) {
      BIO_free(wrap->enc_in);
      wrap->enc_in = nullptr;
    }
    if (wrap->enc_out != nullptr) {
      BIO_free(wrap->enc_out);
      wrap->enc_out = nullptr;
    }
    SSL_free(wrap->ssl);
    wrap->ssl = nullptr;
    return;
  }
  edge::crypto::EdgeBIO::FromBIO(wrap->enc_in)->set_eof_return(-1);
  edge::crypto::EdgeBIO::FromBIO(wrap->enc_out)->set_eof_return(-1);
  if (!wrap->is_server) {
    edge::crypto::EdgeBIO::FromBIO(wrap->enc_in)->set_initial(4096);
  }
  SSL_set_bio(wrap->ssl, wrap->enc_in, wrap->enc_out);
  SSL_set_app_data(wrap->ssl, wrap);
  SSL_set_info_callback(wrap->ssl, SslInfoCallback);
  SSL_set_verify(wrap->ssl, SSL_VERIFY_NONE, VerifyCallback);
#ifdef SSL_MODE_RELEASE_BUFFERS
  SSL_set_mode(wrap->ssl, SSL_MODE_RELEASE_BUFFERS);
#endif
  SSL_set_mode(wrap->ssl, SSL_MODE_AUTO_RETRY);
  SSL_set_cert_cb(wrap->ssl, CertCallback, wrap);
  if (wrap->is_server) {
    SSL_set_accept_state(wrap->ssl);
  } else {
    SSL_set_connect_state(wrap->ssl);
  }
}

napi_value TlsWrapCtor(napi_env env, napi_callback_info info) {
  napi_value self = nullptr;
  size_t argc = 0;
  napi_get_cb_info(env, info, &argc, nullptr, &self, nullptr);
  if (self == nullptr) return Undefined(env);

  auto* wrap = new TlsWrap();
  wrap->env = env;
  EdgeStreamBaseInit(&wrap->base, env, &kTlsWrapOps, kEdgeProviderTlsWrap);
  if (napi_wrap(env, self, wrap, TlsWrapFinalize, nullptr, &wrap->wrapper_ref) != napi_ok) {
    delete wrap;
    return Undefined(env);
  }
  EdgeStreamBaseSetWrapperRef(&wrap->base, wrap->wrapper_ref);
  EdgeStreamBaseSetInitialStreamProperties(&wrap->base, false, false);
  wrap->async_id = wrap->base.async_id;

  EnsureState(env).wraps.push_back(wrap);
  return self;
}

napi_value TlsWrapReadStart(napi_env env, napi_callback_info info) {
  TlsWrap* wrap = UnwrapThis(env, info, nullptr, nullptr, nullptr);
  if (wrap == nullptr) return MakeInt32(env, UV_EINVAL);
  int32_t rc = CallParentMethodInt(wrap, "readStart", 0, nullptr, nullptr);
  if (rc == UV_EALREADY) rc = 0;
  EdgeStreamBaseSetReading(&wrap->base, rc == 0);
  return MakeInt32(env, rc);
}

napi_value TlsWrapReadStop(napi_env env, napi_callback_info info) {
  TlsWrap* wrap = UnwrapThis(env, info, nullptr, nullptr, nullptr);
  if (wrap == nullptr) return MakeInt32(env, UV_EINVAL);
  int32_t rc = CallParentMethodInt(wrap, "readStop", 0, nullptr, nullptr);
  if (rc == UV_EALREADY) rc = 0;
  EdgeStreamBaseSetReading(&wrap->base, false);
  return MakeInt32(env, rc);
}

int32_t DoAppWrite(TlsWrap* wrap, napi_value req_obj, const uint8_t* data, size_t len) {
  if (wrap == nullptr || wrap->ssl == nullptr) return UV_EINVAL;
  if (wrap->active_write_req_ref != nullptr || wrap->active_empty_write_req_ref != nullptr ||
      !wrap->pending_cleartext_input.empty()) {
    return UV_EBUSY;
  }

  napi_ref req_ref = nullptr;
  if (req_obj != nullptr &&
      (napi_create_reference(wrap->env, req_obj, 1, &req_ref) != napi_ok || req_ref == nullptr)) {
    return UV_ENOMEM;
  }

  wrap->base.bytes_written += len;
  auto restore_write_state = [wrap, len]() {
    SetState(wrap->env, kEdgeBytesWritten, static_cast<int32_t>(len));
    SetState(wrap->env, kEdgeLastWriteWasAsync, 1);
  };
  restore_write_state();

  if (len == 0) {
    (void)ReadCleartext(wrap);
    if (PendingBioSize(wrap->enc_out) == 0) {
      wrap->active_empty_write_req_ref = req_ref;
      req_ref = nullptr;
      QueueEncryptedWrite(wrap, 0, nullptr, true);
    } else {
      wrap->active_write_req_ref = req_ref;
      req_ref = nullptr;
      QueueNewEncryptedBytes(wrap);
    }
    EncOut(wrap);
    if (req_ref != nullptr) {
      CompleteReq(wrap, &req_ref, UV_ECANCELED);
    }
    restore_write_state();
    return 0;
  }

  wrap->active_write_req_ref = req_ref;
  req_ref = nullptr;

  wrap->in_dowrite = true;
  const int rc = SSL_write(wrap->ssl, data, static_cast<int>(len));
  if (rc == static_cast<int>(len)) {
    QueueNewEncryptedBytes(wrap);
    EncOut(wrap);
    wrap->in_dowrite = false;
    restore_write_state();
    return 0;
  }

  QueueNewEncryptedBytes(wrap);
  const int err = SSL_get_error(wrap->ssl, rc);
  if (err == SSL_ERROR_SSL || err == SSL_ERROR_SYSCALL) {
    wrap->in_dowrite = false;
    DeleteRefIfPresent(wrap->env, &wrap->active_write_req_ref);
    restore_write_state();
    return UV_EPROTO;
  }

  wrap->pending_cleartext_input.assign(data, data + len);
  EncOut(wrap);
  wrap->in_dowrite = false;
  restore_write_state();
  return 0;
}

int TlsWrapStreamBaseWriteBuffer(EdgeStreamBase* base,
                                 napi_value req_obj,
                                 napi_value payload,
                                 bool* async_out) {
  if (async_out != nullptr) *async_out = false;
  if (base == nullptr || base->env == nullptr) return UV_EINVAL;
  auto* wrap = reinterpret_cast<TlsWrap*>(
      reinterpret_cast<char*>(base) - offsetof(TlsWrap, base));
  if (wrap == nullptr || wrap->ssl == nullptr) return UV_EINVAL;

  const uint8_t* data = nullptr;
  size_t len = 0;
  bool refable = false;
  std::string temp_utf8;
  if (!EdgeStreamBaseExtractByteSpan(base->env, payload, &data, &len, &refable, &temp_utf8) ||
      (len > 0 && data == nullptr)) {
    return UV_EINVAL;
  }

  const int32_t rc = DoAppWrite(wrap, req_obj, data, len);
  if (async_out != nullptr && rc == 0) *async_out = true;
  return rc;
}

napi_value TlsWrapWriteBuffer(napi_env env, napi_callback_info info) {
  size_t argc = 2;
  napi_value argv[2] = {nullptr, nullptr};
  TlsWrap* wrap = UnwrapThis(env, info, &argc, argv, nullptr);
  if (wrap == nullptr || argc < 2) return MakeInt32(env, UV_EINVAL);
  const uint8_t* data = nullptr;
  size_t len = 0;
  bool refable = false;
  std::string temp_utf8;
  if (!EdgeStreamBaseExtractByteSpan(env, argv[1], &data, &len, &refable, &temp_utf8) || data == nullptr) {
    return MakeInt32(env, UV_EINVAL);
  }
  return MakeInt32(env, DoAppWrite(wrap, argv[0], data, len));
}

napi_value TlsWrapWriteString(napi_env env, napi_callback_info info, const char* encoding_name) {
  size_t argc = 2;
  napi_value argv[2] = {nullptr, nullptr};
  TlsWrap* wrap = UnwrapThis(env, info, &argc, argv, nullptr);
  if (wrap == nullptr || argc < 2) return MakeInt32(env, UV_EINVAL);
  napi_value payload = argv[1];
  if (encoding_name != nullptr) {
    napi_value encoding = nullptr;
    if (napi_create_string_utf8(env, encoding_name, NAPI_AUTO_LENGTH, &encoding) == napi_ok && encoding != nullptr) {
      payload = EdgeStreamBufferFromWithEncoding(env, argv[1], encoding);
    }
  }
  const uint8_t* data = nullptr;
  size_t len = 0;
  bool refable = false;
  std::string temp_utf8;
  if (!EdgeStreamBaseExtractByteSpan(env, payload, &data, &len, &refable, &temp_utf8) || data == nullptr) {
    return MakeInt32(env, UV_EINVAL);
  }
  return MakeInt32(env, DoAppWrite(wrap, argv[0], data, len));
}

napi_value TlsWrapWriteLatin1String(napi_env env, napi_callback_info info) {
  return TlsWrapWriteString(env, info, "latin1");
}

napi_value TlsWrapWriteUtf8String(napi_env env, napi_callback_info info) {
  return TlsWrapWriteString(env, info, "utf8");
}

napi_value TlsWrapWriteAsciiString(napi_env env, napi_callback_info info) {
  return TlsWrapWriteString(env, info, "ascii");
}

napi_value TlsWrapWriteUcs2String(napi_env env, napi_callback_info info) {
  return TlsWrapWriteString(env, info, "ucs2");
}

napi_value TlsWrapWritev(napi_env env, napi_callback_info info) {
  size_t argc = 3;
  napi_value argv[3] = {nullptr, nullptr, nullptr};
  TlsWrap* wrap = UnwrapThis(env, info, &argc, argv, nullptr);
  if (wrap == nullptr || argc < 2) return MakeInt32(env, UV_EINVAL);
  bool all_buffers = false;
  if (argc >= 3 && argv[2] != nullptr) napi_get_value_bool(env, argv[2], &all_buffers);
  uint32_t raw_len = 0;
  if (napi_get_array_length(env, argv[1], &raw_len) != napi_ok) return MakeInt32(env, UV_EINVAL);
  const uint32_t count = all_buffers ? raw_len : (raw_len / 2);
  std::vector<uint8_t> combined;
  for (uint32_t i = 0; i < count; ++i) {
    napi_value chunk = nullptr;
    napi_get_element(env, argv[1], all_buffers ? i : (i * 2), &chunk);
    const uint8_t* data = nullptr;
    size_t len = 0;
    bool refable = false;
    std::string temp_utf8;
    if (!EdgeStreamBaseExtractByteSpan(env, chunk, &data, &len, &refable, &temp_utf8) || data == nullptr) {
      return MakeInt32(env, UV_EINVAL);
    }
    combined.insert(combined.end(), data, data + len);
  }
  return MakeInt32(env, DoAppWrite(wrap, argv[0], combined.data(), combined.size()));
}

napi_value TlsWrapShutdown(napi_env env, napi_callback_info info) {
  size_t argc = 1;
  napi_value argv[1] = {nullptr};
  TlsWrap* wrap = UnwrapThis(env, info, &argc, argv, nullptr);
  if (wrap == nullptr || argc < 1) return MakeInt32(env, UV_EINVAL);
  if (wrap->shutdown_started || wrap->pending_shutdown_req_ref != nullptr) {
    return MakeInt32(env, 0);
  }
  if (wrap->ssl == nullptr) {
    wrap->shutdown_started = true;
    const int32_t rc = CallParentMethodInt(wrap, "shutdown", 1, argv, nullptr);
    if (rc != 0) wrap->shutdown_started = false;
    return MakeInt32(env, rc);
  }

  if (napi_create_reference(env, argv[0], 1, &wrap->pending_shutdown_req_ref) != napi_ok) {
    return MakeInt32(env, UV_ENOMEM);
  }
  MaybeStartTlsShutdown(wrap);
  return MakeInt32(env, 0);
}

napi_value TlsWrapClose(napi_env env, napi_callback_info info) {
  size_t argc = 1;
  napi_value argv[1] = {nullptr};
  TlsWrap* wrap = UnwrapThis(env, info, &argc, argv, nullptr);
  if (wrap == nullptr) return Undefined(env);
  DestroySsl(wrap);
  NotifyTlsStreamClosed(wrap);
  (void)CallParentMethodInt(wrap, "close", argc, argv, nullptr);
  return Undefined(env);
}

napi_value TlsWrapRef(napi_env env, napi_callback_info info) {
  TlsWrap* wrap = UnwrapThis(env, info, nullptr, nullptr, nullptr);
  if (wrap != nullptr) {
    wrap->refed = true;
    SyncKeepaliveRef(wrap);
    (void)CallParentMethodInt(wrap, "ref", 0, nullptr, nullptr);
  }
  return Undefined(env);
}

napi_value TlsWrapUnref(napi_env env, napi_callback_info info) {
  TlsWrap* wrap = UnwrapThis(env, info, nullptr, nullptr, nullptr);
  if (wrap != nullptr) {
    wrap->refed = false;
    SyncKeepaliveRef(wrap);
    (void)CallParentMethodInt(wrap, "unref", 0, nullptr, nullptr);
  }
  return Undefined(env);
}

napi_value TlsWrapGetAsyncId(napi_env env, napi_callback_info info) {
  TlsWrap* wrap = UnwrapThis(env, info, nullptr, nullptr, nullptr);
  return EdgeStreamBaseGetAsyncId(wrap != nullptr ? &wrap->base : nullptr);
}

napi_value TlsWrapGetProviderType(napi_env env, napi_callback_info info) {
  TlsWrap* wrap = UnwrapThis(env, info, nullptr, nullptr, nullptr);
  return EdgeStreamBaseGetProviderType(wrap != nullptr ? &wrap->base : nullptr);
}

napi_value TlsWrapAsyncReset(napi_env env, napi_callback_info info) {
  TlsWrap* wrap = UnwrapThis(env, info, nullptr, nullptr, nullptr);
  if (wrap != nullptr) {
    (void)EdgeStreamBaseAsyncReset(&wrap->base);
    wrap->async_id = wrap->base.async_id;
  }
  return Undefined(env);
}

napi_value TlsWrapUseUserBuffer(napi_env env, napi_callback_info info) {
  size_t argc = 1;
  napi_value argv[1] = {nullptr};
  TlsWrap* wrap = UnwrapThis(env, info, &argc, argv, nullptr);
  if (wrap == nullptr || argc < 1) return MakeInt32(env, UV_EINVAL);
  return EdgeStreamBaseUseUserBuffer(&wrap->base, argv[0]);
}

napi_value TlsWrapGetOnRead(napi_env env, napi_callback_info info) {
  TlsWrap* wrap = UnwrapThis(env, info, nullptr, nullptr, nullptr);
  return EdgeStreamBaseGetOnRead(wrap != nullptr ? &wrap->base : nullptr);
}

napi_value TlsWrapSetOnRead(napi_env env, napi_callback_info info) {
  size_t argc = 1;
  napi_value argv[1] = {nullptr};
  TlsWrap* wrap = UnwrapThis(env, info, &argc, argv, nullptr);
  if (wrap == nullptr || argc < 1) return Undefined(env);
  return EdgeStreamBaseSetOnRead(&wrap->base, argv[0]);
}

napi_value TlsWrapExternalStreamGetter(napi_env env, napi_callback_info info) {
  TlsWrap* wrap = UnwrapThis(env, info, nullptr, nullptr, nullptr);
  return EdgeStreamBaseGetExternal(wrap != nullptr ? &wrap->base : nullptr);
}

napi_value TlsWrapBytesReadGetter(napi_env env, napi_callback_info info) {
  TlsWrap* wrap = UnwrapThis(env, info, nullptr, nullptr, nullptr);
  return EdgeStreamBaseGetBytesRead(wrap != nullptr ? &wrap->base : nullptr);
}

napi_value TlsWrapBytesWrittenGetter(napi_env env, napi_callback_info info) {
  TlsWrap* wrap = UnwrapThis(env, info, nullptr, nullptr, nullptr);
  return EdgeStreamBaseGetBytesWritten(wrap != nullptr ? &wrap->base : nullptr);
}

napi_value TlsWrapFdGetter(napi_env env, napi_callback_info info) {
  napi_value self = nullptr;
  size_t argc = 0;
  napi_get_cb_info(env, info, &argc, nullptr, &self, nullptr);
  TlsWrap* wrap = UnwrapTlsWrap(env, self);
  napi_value parent = wrap != nullptr ? GetRefValue(env, wrap->parent_ref) : nullptr;
  napi_value out = GetNamedValue(env, parent, "fd");
  return out != nullptr ? out : MakeInt32(env, -1);
}

napi_value TlsWrapSetVerifyMode(napi_env env, napi_callback_info info) {
  size_t argc = 2;
  napi_value argv[2] = {nullptr, nullptr};
  TlsWrap* wrap = UnwrapThis(env, info, &argc, argv, nullptr);
  if (wrap == nullptr || wrap->ssl == nullptr || argc < 2) return Undefined(env);
  bool request_cert = false;
  bool reject_unauthorized = false;
  napi_get_value_bool(env, argv[0], &request_cert);
  napi_get_value_bool(env, argv[1], &reject_unauthorized);
  wrap->request_cert = request_cert;
  wrap->reject_unauthorized = reject_unauthorized;

  int verify_mode = SSL_VERIFY_NONE;
  if (wrap->is_server) {
    if (request_cert) {
      verify_mode = SSL_VERIFY_PEER;
      if (reject_unauthorized) verify_mode |= SSL_VERIFY_FAIL_IF_NO_PEER_CERT;
    }
  }
  SSL_set_verify(wrap->ssl, verify_mode, VerifyCallback);
  return Undefined(env);
}

napi_value TlsWrapEnableTrace(napi_env env, napi_callback_info info) {
  (void)env;
  (void)info;
  return Undefined(env);
}

napi_value TlsWrapEnableSessionCallbacks(napi_env env, napi_callback_info info) {
  TlsWrap* wrap = UnwrapThis(env, info, nullptr, nullptr, nullptr);
  if (wrap == nullptr || wrap->ssl == nullptr || wrap->session_callbacks_enabled) return Undefined(env);
  wrap->session_callbacks_enabled = true;
  if (wrap->is_server) {
    wrap->hello_parser.Start(OnClientHello);
  } else if (wrap->established && wrap->client_session_event_count == 0 &&
             SSL_version(wrap->ssl) < TLS1_3_VERSION &&
             SSL_session_reused(wrap->ssl) != 1) {
    SSL_SESSION* session = SSL_get_session(wrap->ssl);
    if (session != nullptr) {
      (void)NewSessionCallback(wrap->ssl, session);
    }
  }
  return Undefined(env);
}

napi_value TlsWrapEnableCertCb(napi_env env, napi_callback_info info) {
  TlsWrap* wrap = UnwrapThis(env, info, nullptr, nullptr, nullptr);
  if (wrap != nullptr) {
    wrap->waiting_cert_cb = true;
    wrap->cert_cb = ResumeCertCallback;
    wrap->cert_cb_arg = wrap;
  }
  return Undefined(env);
}

napi_value TlsWrapEnableALPNCb(napi_env env, napi_callback_info info) {
  TlsWrap* wrap = UnwrapThis(env, info, nullptr, nullptr, nullptr);
  if (wrap != nullptr && wrap->ssl != nullptr) {
    wrap->alpn_callback_enabled = true;
    SSL_CTX_set_alpn_select_cb(SSL_get_SSL_CTX(wrap->ssl), SelectALPNCallback, nullptr);
  }
  return Undefined(env);
}

napi_value TlsWrapEnablePskCallback(napi_env env, napi_callback_info info) {
  TlsWrap* wrap = UnwrapThis(env, info, nullptr, nullptr, nullptr);
  if (wrap != nullptr && wrap->ssl != nullptr) {
    SSL_set_psk_server_callback(wrap->ssl, PskServerCallback);
    SSL_set_psk_client_callback(wrap->ssl, PskClientCallback);
  }
  return Undefined(env);
}

napi_value TlsWrapSetPskIdentityHint(napi_env env, napi_callback_info info) {
  size_t argc = 1;
  napi_value argv[1] = {nullptr};
  TlsWrap* wrap = UnwrapThis(env, info, &argc, argv, nullptr);
  if (wrap == nullptr || wrap->ssl == nullptr || argc < 1) return Undefined(env);
  const std::string hint = ValueToUtf8(env, argv[0]);
  if (!hint.empty() && SSL_use_psk_identity_hint(wrap->ssl, hint.c_str()) != 1) {
    EmitError(wrap,
              CreateErrorWithCode(env,
                                  "ERR_TLS_PSK_SET_IDENTITY_HINT_FAILED",
                                  "Failed to set PSK identity hint"));
  }
  return Undefined(env);
}

napi_value TlsWrapEnableKeylogCallback(napi_env env, napi_callback_info info) {
  TlsWrap* wrap = UnwrapThis(env, info, nullptr, nullptr, nullptr);
  if (wrap != nullptr && wrap->ssl != nullptr) {
    SSL_CTX_set_keylog_callback(SSL_get_SSL_CTX(wrap->ssl), KeylogCallback);
  }
  return Undefined(env);
}

napi_value TlsWrapWritesIssuedByPrevListenerDone(napi_env env, napi_callback_info info) {
  TlsWrap* wrap = UnwrapThis(env, info, nullptr, nullptr, nullptr);
  if (wrap != nullptr) {
    wrap->has_active_write_issued_by_prev_listener = false;
    EncOut(wrap);
  }
  return Undefined(env);
}

napi_value TlsWrapSetALPNProtocols(napi_env env, napi_callback_info info) {
  size_t argc = 1;
  napi_value argv[1] = {nullptr};
  TlsWrap* wrap = UnwrapThis(env, info, &argc, argv, nullptr);
  if (wrap == nullptr || wrap->ssl == nullptr || argc < 1) return Undefined(env);
  const uint8_t* data = nullptr;
  size_t len = 0;
  bool refable = false;
  std::string temp_utf8;
  if (!EdgeStreamBaseExtractByteSpan(env, argv[0], &data, &len, &refable, &temp_utf8) || data == nullptr) {
    napi_throw_type_error(env, nullptr, "Must give a Buffer as first argument");
    return nullptr;
  }
  if (wrap->is_server) {
    wrap->alpn_protos.assign(data, data + len);
    SSL_CTX_set_alpn_select_cb(SSL_get_SSL_CTX(wrap->ssl), SelectALPNCallback, nullptr);
  } else {
    (void)SSL_set_alpn_protos(wrap->ssl, data, static_cast<unsigned int>(len));
  }
  return Undefined(env);
}

napi_value TlsWrapRequestOCSP(napi_env env, napi_callback_info info) {
  TlsWrap* wrap = UnwrapThis(env, info, nullptr, nullptr, nullptr);
  if (wrap != nullptr && wrap->ssl != nullptr && !wrap->is_server) {
    SSL_set_tlsext_status_type(wrap->ssl, TLSEXT_STATUSTYPE_ocsp);
  }
  return Undefined(env);
}

napi_value TlsWrapStart(napi_env env, napi_callback_info info) {
  TlsWrap* wrap = UnwrapThis(env, info, nullptr, nullptr, nullptr);
  if (wrap == nullptr) return Undefined(env);
  wrap->started = true;
  EnsureKeepaliveHandle(wrap);
  SyncKeepaliveRef(wrap);
  if (wrap->is_server && !wrap->session_callbacks_enabled) {
    napi_value self = GetRefValue(env, wrap->wrapper_ref);
    if (self != nullptr &&
        (IsFunction(env, GetNamedValue(env, self, "onclienthello")) ||
         IsFunction(env, GetNamedValue(env, self, "onnewsession")))) {
      wrap->session_callbacks_enabled = true;
      wrap->hello_parser.Start(OnClientHello);
    }
  }
  (void)ReadCleartext(wrap);
  EncOut(wrap);
  return Undefined(env);
}

napi_value TlsWrapRenegotiate(napi_env env, napi_callback_info info) {
  TlsWrap* wrap = UnwrapThis(env, info, nullptr, nullptr, nullptr);
  if (wrap == nullptr || wrap->ssl == nullptr) return Undefined(env);
  ncrypto::ClearErrorOnReturn clear_error_on_return;
#ifndef OPENSSL_IS_BORINGSSL
  if (SSL_renegotiate(wrap->ssl) != 1) {
    napi_throw(env, CreateLastOpenSslError(env, nullptr, "TLS renegotiation failed"));
    return nullptr;
  }
#endif
  return Undefined(env);
}

napi_value TlsWrapSetServername(napi_env env, napi_callback_info info) {
  size_t argc = 1;
  napi_value argv[1] = {nullptr};
  TlsWrap* wrap = UnwrapThis(env, info, &argc, argv, nullptr);
  if (wrap == nullptr || wrap->ssl == nullptr || argc < 1) return Undefined(env);
  const std::string servername = ValueToUtf8(env, argv[0]);
  if (!servername.empty()) {
    (void)SSL_set_tlsext_host_name(wrap->ssl, servername.c_str());
  }
  return Undefined(env);
}

napi_value TlsWrapGetServername(napi_env env, napi_callback_info info) {
  TlsWrap* wrap = UnwrapThis(env, info, nullptr, nullptr, nullptr);
  if (wrap == nullptr || wrap->ssl == nullptr) return MakeBool(env, false);
  const char* servername = SSL_get_servername(wrap->ssl, TLSEXT_NAMETYPE_host_name);
  if (servername == nullptr) return MakeBool(env, false);
  napi_value out = nullptr;
  napi_create_string_utf8(env, servername, NAPI_AUTO_LENGTH, &out);
  return out != nullptr ? out : MakeBool(env, false);
}

bool LoadSessionBytes(TlsWrap* wrap, const uint8_t* data, size_t len) {
  if (wrap == nullptr || wrap->ssl == nullptr || data == nullptr || len == 0) return false;
  const unsigned char* ptr = data;
  SSL_SESSION* session = d2i_SSL_SESSION(nullptr, &ptr, static_cast<long>(len));
  if (session == nullptr) return false;
  const int rc = SSL_set_session(wrap->ssl, session);
  SSL_SESSION_free(session);
  return rc == 1;
}

napi_value TlsWrapSetSession(napi_env env, napi_callback_info info) {
  size_t argc = 1;
  napi_value argv[1] = {nullptr};
  TlsWrap* wrap = UnwrapThis(env, info, &argc, argv, nullptr);
  if (wrap == nullptr || argc < 1) return Undefined(env);
  const uint8_t* data = nullptr;
  size_t len = 0;
  bool refable = false;
  std::string temp_utf8;
  if (EdgeStreamBaseExtractByteSpan(env, argv[0], &data, &len, &refable, &temp_utf8) && data != nullptr) {
    wrap->pending_session.assign(data, data + len);
    if (wrap->is_server) {
      const unsigned char* ptr = wrap->pending_session.data();
      SSL_SESSION* session = d2i_SSL_SESSION(nullptr, &ptr, static_cast<long>(wrap->pending_session.size()));
      if (wrap->next_session != nullptr) SSL_SESSION_free(wrap->next_session);
      wrap->next_session = session;
    } else if (wrap->ssl != nullptr) {
      (void)LoadSessionBytes(wrap, wrap->pending_session.data(), wrap->pending_session.size());
    }
  }
  return Undefined(env);
}

napi_value TlsWrapLoadSession(napi_env env, napi_callback_info info) {
  return TlsWrapSetSession(env, info);
}

napi_value TlsWrapGetSession(napi_env env, napi_callback_info info) {
  TlsWrap* wrap = UnwrapThis(env, info, nullptr, nullptr, nullptr);
  if (wrap == nullptr || wrap->ssl == nullptr) return Undefined(env);
  SSL_SESSION* session = SSL_get_session(wrap->ssl);
  if (session == nullptr) return Undefined(env);
  const int size = i2d_SSL_SESSION(session, nullptr);
  if (size <= 0) return Undefined(env);
  std::vector<uint8_t> out(static_cast<size_t>(size));
  unsigned char* ptr = out.data();
  if (i2d_SSL_SESSION(session, &ptr) != size) return Undefined(env);
  napi_value buffer = CreateBufferCopy(env, out.data(), out.size());
  return buffer != nullptr ? buffer : Undefined(env);
}

napi_value TlsWrapExportKeyingMaterial(napi_env env, napi_callback_info info) {
  size_t argc = 3;
  napi_value argv[3] = {nullptr, nullptr, nullptr};
  TlsWrap* wrap = UnwrapThis(env, info, &argc, argv, nullptr);
  if (wrap == nullptr || wrap->ssl == nullptr || argc < 2) return Undefined(env);
  uint32_t length = 0;
  napi_get_value_uint32(env, argv[0], &length);
  const std::string label = ValueToUtf8(env, argv[1]);
  std::vector<uint8_t> context_bytes;
  const uint8_t* context_data = nullptr;
  size_t context_len = 0;
  if (argc >= 3 && argv[2] != nullptr) {
    bool refable = false;
    std::string temp_utf8;
    if (EdgeStreamBaseExtractByteSpan(env, argv[2], &context_data, &context_len, &refable, &temp_utf8) &&
        context_data != nullptr) {
      context_bytes.assign(context_data, context_data + context_len);
      context_data = context_bytes.data();
    }
  }
  std::vector<uint8_t> out(length, 0);
  if (SSL_export_keying_material(wrap->ssl,
                                 out.data(),
                                 out.size(),
                                 label.c_str(),
                                 label.size(),
                                 context_bytes.empty() ? nullptr : context_data,
                                 context_len,
                                 context_bytes.empty() ? 0 : 1) != 1) {
    EmitError(wrap, CreateLastOpenSslError(env, "ERR_TLS_EXPORT_KEYING_MATERIAL", "Key export failed"));
    return Undefined(env);
  }
  napi_value buffer = CreateBufferCopy(env, out.data(), out.size());
  return buffer != nullptr ? buffer : Undefined(env);
}

napi_value TlsWrapSetMaxSendFragment(napi_env env, napi_callback_info info) {
  size_t argc = 1;
  napi_value argv[1] = {nullptr};
  TlsWrap* wrap = UnwrapThis(env, info, &argc, argv, nullptr);
  if (wrap == nullptr || wrap->ssl == nullptr || argc < 1) return MakeInt32(env, 0);
#ifdef SSL_set_max_send_fragment
  uint32_t value = 0;
  napi_get_value_uint32(env, argv[0], &value);
  return MakeInt32(env, SSL_set_max_send_fragment(wrap->ssl, value) == 1 ? 1 : 0);
#else
  return MakeInt32(env, 1);
#endif
}

napi_value TlsWrapGetALPNNegotiatedProtocol(napi_env env, napi_callback_info info) {
  TlsWrap* wrap = UnwrapThis(env, info, nullptr, nullptr, nullptr);
  if (wrap == nullptr || wrap->ssl == nullptr) return MakeBool(env, false);
  const unsigned char* data = nullptr;
  unsigned int len = 0;
  SSL_get0_alpn_selected(wrap->ssl, &data, &len);
  if (data == nullptr || len == 0) return MakeBool(env, false);
  napi_value out = nullptr;
  napi_create_string_utf8(env, reinterpret_cast<const char*>(data), len, &out);
  return out != nullptr ? out : MakeBool(env, false);
}

napi_value TlsWrapGetCipher(napi_env env, napi_callback_info info) {
  TlsWrap* wrap = UnwrapThis(env, info, nullptr, nullptr, nullptr);
  if (wrap == nullptr || wrap->ssl == nullptr) return Undefined(env);
  const SSL_CIPHER* cipher = SSL_get_current_cipher(wrap->ssl);
  if (cipher == nullptr) return Undefined(env);
  napi_value out = nullptr;
  napi_create_object(env, &out);
  napi_value name = nullptr;
  napi_value standard_name = nullptr;
  napi_value version = nullptr;
  const char* cipher_name = SSL_CIPHER_get_name(cipher);
  const char* cipher_standard_name = SSL_CIPHER_standard_name(cipher);
  const char* cipher_version = SSL_CIPHER_get_version(cipher);
  if (cipher_name != nullptr) napi_create_string_utf8(env, cipher_name, NAPI_AUTO_LENGTH, &name);
  if (cipher_standard_name != nullptr) {
    napi_create_string_utf8(env, cipher_standard_name, NAPI_AUTO_LENGTH, &standard_name);
  } else if (cipher_name != nullptr) {
    napi_create_string_utf8(env, cipher_name, NAPI_AUTO_LENGTH, &standard_name);
  }
  if (cipher_version != nullptr) napi_create_string_utf8(env, cipher_version, NAPI_AUTO_LENGTH, &version);
  if (name != nullptr) napi_set_named_property(env, out, "name", name);
  if (standard_name != nullptr) napi_set_named_property(env, out, "standardName", standard_name);
  if (version != nullptr) napi_set_named_property(env, out, "version", version);
  return out != nullptr ? out : Undefined(env);
}

napi_value TlsWrapGetSharedSigalgs(napi_env env, napi_callback_info info) {
  TlsWrap* wrap = UnwrapThis(env, info, nullptr, nullptr, nullptr);
  napi_value out = nullptr;
  napi_create_array(env, &out);
  if (wrap == nullptr || wrap->ssl == nullptr) return out != nullptr ? out : Undefined(env);
  int nsig = SSL_get_shared_sigalgs(wrap->ssl, 0, nullptr, nullptr, nullptr, nullptr, nullptr);
  for (int i = 0; i < nsig; ++i) {
    int sign_nid = NID_undef;
    int hash_nid = NID_undef;
    if (SSL_get_shared_sigalgs(wrap->ssl, i, &sign_nid, &hash_nid, nullptr, nullptr, nullptr) <= 0) continue;

    std::string sig_with_md;
    switch (sign_nid) {
      case EVP_PKEY_RSA:
        sig_with_md = "RSA+";
        break;
      case EVP_PKEY_RSA_PSS:
        sig_with_md = "RSA-PSS+";
        break;
      case EVP_PKEY_DSA:
        sig_with_md = "DSA+";
        break;
      case EVP_PKEY_EC:
        sig_with_md = "ECDSA+";
        break;
      case NID_ED25519:
        sig_with_md = "Ed25519+";
        break;
      case NID_ED448:
        sig_with_md = "Ed448+";
        break;
#ifndef OPENSSL_NO_GOST
      case NID_id_GostR3410_2001:
        sig_with_md = "gost2001+";
        break;
      case NID_id_GostR3410_2012_256:
        sig_with_md = "gost2012_256+";
        break;
      case NID_id_GostR3410_2012_512:
        sig_with_md = "gost2012_512+";
        break;
#endif
      default: {
        const char* sign_name = OBJ_nid2sn(sign_nid);
        sig_with_md = sign_name != nullptr ? std::string(sign_name) + "+" : "UNDEF+";
        break;
      }
    }

    const char* hash_name = OBJ_nid2sn(hash_nid);
    sig_with_md += hash_name != nullptr ? hash_name : "UNDEF";

    napi_value value = nullptr;
    napi_create_string_utf8(env, sig_with_md.c_str(), NAPI_AUTO_LENGTH, &value);
    napi_set_element(env, out, i, value);
  }
  return out != nullptr ? out : Undefined(env);
}

napi_value TlsWrapGetEphemeralKeyInfo(napi_env env, napi_callback_info info) {
  TlsWrap* wrap = UnwrapThis(env, info, nullptr, nullptr, nullptr);
  napi_value out = nullptr;
  napi_create_object(env, &out);
  if (wrap == nullptr || wrap->ssl == nullptr) return out != nullptr ? out : Undefined(env);
  if (wrap->is_server) return Null(env);
  EVP_PKEY* key = nullptr;
  if (SSL_get_peer_tmp_key(wrap->ssl, &key) != 1 || key == nullptr) return out != nullptr ? out : Undefined(env);
  const int key_id = EVP_PKEY_id(key);
  const int bits = EVP_PKEY_bits(key);
  const char* key_type = nullptr;
  const char* key_name = nullptr;
  if (key_id == EVP_PKEY_DH) {
    key_type = "DH";
  } else if (key_id == EVP_PKEY_EC || key_id == EVP_PKEY_X25519 || key_id == EVP_PKEY_X448) {
    key_type = "ECDH";
    if (key_id == EVP_PKEY_EC) {
      EC_KEY* ec = EVP_PKEY_get1_EC_KEY(key);
      if (ec != nullptr) {
        const EC_GROUP* group = EC_KEY_get0_group(ec);
        if (group != nullptr) {
          key_name = OBJ_nid2sn(EC_GROUP_get_curve_name(group));
        }
        EC_KEY_free(ec);
      }
    } else {
      key_name = OBJ_nid2sn(key_id);
    }
  }
  if (key_type != nullptr) {
    napi_value type = nullptr;
    napi_create_string_utf8(env, key_type, NAPI_AUTO_LENGTH, &type);
    if (type != nullptr) napi_set_named_property(env, out, "type", type);
  }
  if (key_name != nullptr && key_name[0] != '\0') {
    napi_value name = nullptr;
    napi_create_string_utf8(env, key_name, NAPI_AUTO_LENGTH, &name);
    if (name != nullptr) napi_set_named_property(env, out, "name", name);
  }
  if (bits > 0) {
    napi_set_named_property(env, out, "size", MakeInt32(env, bits));
  }
  EVP_PKEY_free(key);
  return out != nullptr ? out : Undefined(env);
}

napi_value TlsWrapGetFinished(napi_env env, napi_callback_info info) {
  TlsWrap* wrap = UnwrapThis(env, info, nullptr, nullptr, nullptr);
  if (wrap == nullptr || wrap->ssl == nullptr) return Undefined(env);
  char dummy[1];
  const size_t len = SSL_get_finished(wrap->ssl, dummy, sizeof(dummy));
  if (len == 0) return Undefined(env);
  std::vector<uint8_t> out(len);
  (void)SSL_get_finished(wrap->ssl, out.data(), out.size());
  napi_value buffer = CreateBufferCopy(env, out.data(), out.size());
  return buffer != nullptr ? buffer : Undefined(env);
}

napi_value TlsWrapGetPeerFinished(napi_env env, napi_callback_info info) {
  TlsWrap* wrap = UnwrapThis(env, info, nullptr, nullptr, nullptr);
  if (wrap == nullptr || wrap->ssl == nullptr) return Undefined(env);
  char dummy[1];
  const size_t len = SSL_get_peer_finished(wrap->ssl, dummy, sizeof(dummy));
  if (len == 0) return Undefined(env);
  std::vector<uint8_t> out(len);
  (void)SSL_get_peer_finished(wrap->ssl, out.data(), out.size());
  napi_value buffer = CreateBufferCopy(env, out.data(), out.size());
  return buffer != nullptr ? buffer : Undefined(env);
}

napi_value TlsWrapGetProtocol(napi_env env, napi_callback_info info) {
  TlsWrap* wrap = UnwrapThis(env, info, nullptr, nullptr, nullptr);
  if (wrap == nullptr || wrap->ssl == nullptr) return Undefined(env);
  const char* version = SSL_get_version(wrap->ssl);
  if (version == nullptr) return Undefined(env);
  napi_value out = nullptr;
  napi_create_string_utf8(env, version, NAPI_AUTO_LENGTH, &out);
  return out != nullptr ? out : Undefined(env);
}

napi_value TlsWrapGetTLSTicket(napi_env env, napi_callback_info info) {
  TlsWrap* wrap = UnwrapThis(env, info, nullptr, nullptr, nullptr);
  if (wrap == nullptr || wrap->ssl == nullptr) return Undefined(env);

  SSL_SESSION* session = SSL_get_session(wrap->ssl);
  if (session == nullptr) return Undefined(env);

  const unsigned char* ticket = nullptr;
  size_t length = 0;
  SSL_SESSION_get0_ticket(session, &ticket, &length);
  if (ticket == nullptr) return Undefined(env);

  napi_value out = nullptr;
  if (napi_create_buffer_copy(env, length, ticket, nullptr, &out) != napi_ok || out == nullptr) {
    return Undefined(env);
  }
  return out;
}

napi_value TlsWrapIsSessionReused(napi_env env, napi_callback_info info) {
  TlsWrap* wrap = UnwrapThis(env, info, nullptr, nullptr, nullptr);
  return MakeBool(env, wrap != nullptr && wrap->ssl != nullptr && SSL_session_reused(wrap->ssl) == 1);
}

bool AppendX509NameEntry(napi_env env, napi_value target, int nid, const std::string& value) {
  const char* key = OBJ_nid2sn(nid);
  if (key == nullptr) return true;
  napi_value current = GetNamedValue(env, target, key);
  napi_value str = nullptr;
  if (napi_create_string_utf8(env, value.c_str(), value.size(), &str) != napi_ok || str == nullptr) return false;
  if (current == nullptr || current == Undefined(env) || current == Null(env)) {
    return napi_set_named_property(env, target, key, str) == napi_ok;
  }
  bool is_array = false;
  if (napi_is_array(env, current, &is_array) == napi_ok && is_array) {
    uint32_t length = 0;
    napi_get_array_length(env, current, &length);
    napi_set_element(env, current, length, str);
    return true;
  }
  napi_value arr = nullptr;
  if (napi_create_array_with_length(env, 2, &arr) != napi_ok || arr == nullptr) return false;
  napi_set_element(env, arr, 0, current);
  napi_set_element(env, arr, 1, str);
  return napi_set_named_property(env, target, key, arr) == napi_ok;
}

napi_value CreateX509NameObject(napi_env env, X509_NAME* name) {
  napi_value out = nullptr;
  napi_create_object(env, &out);
  if (out == nullptr || name == nullptr) return out;
  const int count = X509_NAME_entry_count(name);
  for (int i = 0; i < count; ++i) {
    X509_NAME_ENTRY* entry = X509_NAME_get_entry(name, i);
    if (entry == nullptr) continue;
    ASN1_OBJECT* object = X509_NAME_ENTRY_get_object(entry);
    ASN1_STRING* data = X509_NAME_ENTRY_get_data(entry);
    unsigned char* utf8 = nullptr;
    const int utf8_len = ASN1_STRING_to_UTF8(&utf8, data);
    if (utf8_len < 0 || utf8 == nullptr) continue;
    std::string value(reinterpret_cast<char*>(utf8), static_cast<size_t>(utf8_len));
    OPENSSL_free(utf8);
    (void)AppendX509NameEntry(env, out, OBJ_obj2nid(object), value);
  }
  return out;
}

std::string GetSubjectAltNameString(X509* cert) {
  std::string out;
  if (cert == nullptr) return out;
  GENERAL_NAMES* names =
      static_cast<GENERAL_NAMES*>(X509_get_ext_d2i(cert, NID_subject_alt_name, nullptr, nullptr));
  if (names == nullptr) return out;
  const int count = sk_GENERAL_NAME_num(names);
  for (int i = 0; i < count; ++i) {
    const GENERAL_NAME* name = sk_GENERAL_NAME_value(names, i);
    if (name == nullptr) continue;
    if (!out.empty()) out.append(", ");
    if (name->type == GEN_DNS) {
      const auto* dns = ASN1_STRING_get0_data(name->d.dNSName);
      const int dns_len = ASN1_STRING_length(name->d.dNSName);
      out.append("DNS:");
      out.append(reinterpret_cast<const char*>(dns), static_cast<size_t>(dns_len));
    } else if (name->type == GEN_IPADD) {
      out.append("IP Address:");
      const unsigned char* ip = ASN1_STRING_get0_data(name->d.iPAddress);
      const int ip_len = ASN1_STRING_length(name->d.iPAddress);
      char buf[INET6_ADDRSTRLEN] = {0};
      if (ip_len == 4) {
        uv_inet_ntop(AF_INET, ip, buf, sizeof(buf));
      } else if (ip_len == 16) {
        uv_inet_ntop(AF_INET6, ip, buf, sizeof(buf));
      }
      out.append(buf);
    } else {
      out.pop_back();
      out.pop_back();
    }
  }
  GENERAL_NAMES_free(names);
  return out;
}

napi_value CreateLegacyCertObject(napi_env env, X509* cert) {
  if (cert == nullptr) return Undefined(env);
  napi_value crypto_binding = ResolveInternalBinding(env, "crypto");
  if (crypto_binding == nullptr) return Undefined(env);

  napi_value parse_x509 = nullptr;
  if (napi_get_named_property(env, crypto_binding, "parseX509", &parse_x509) != napi_ok || parse_x509 == nullptr) {
    return Undefined(env);
  }

  const int der_len = i2d_X509(cert, nullptr);
  if (der_len <= 0) return Undefined(env);
  std::vector<uint8_t> der(static_cast<size_t>(der_len));
  unsigned char* ptr = der.data();
  if (i2d_X509(cert, &ptr) != der_len) return Undefined(env);

  napi_value raw = CreateBufferCopy(env, der.data(), der.size());
  if (raw == nullptr) return Undefined(env);
  napi_value handle_argv[1] = {raw};
  napi_value handle = nullptr;
  if (napi_call_function(env, crypto_binding, parse_x509, 1, handle_argv, &handle) != napi_ok || handle == nullptr) {
    return Undefined(env);
  }

  napi_value to_legacy = nullptr;
  if (napi_get_named_property(env, handle, "toLegacy", &to_legacy) != napi_ok || to_legacy == nullptr) {
    return Undefined(env);
  }
  napi_value out = nullptr;
  if (napi_call_function(env, handle, to_legacy, 0, nullptr, &out) != napi_ok || out == nullptr) {
    return Undefined(env);
  }

  if (X509_check_issued(cert, cert) == X509_V_OK) {
    napi_set_named_property(env, out, "issuerCertificate", out);
  }

  return out;
}

napi_value BuildDetailedPeerCertificateObject(napi_env env, SSL* ssl, bool is_server) {
  if (ssl == nullptr) return Undefined(env);

  ncrypto::X509Pointer cert;
  if (is_server) {
    cert.reset(SSL_get_peer_certificate(ssl));
  }

  STACK_OF(X509)* ssl_certs = SSL_get_peer_cert_chain(ssl);
  if (!cert && (ssl_certs == nullptr || sk_X509_num(ssl_certs) == 0)) {
    return Undefined(env);
  }

  std::vector<ncrypto::X509Pointer> peer_certs;
  if (!cert) {
    cert.reset(X509_dup(sk_X509_value(ssl_certs, 0)));
    if (!cert) return Undefined(env);
    for (int i = 1; i < sk_X509_num(ssl_certs); ++i) {
      ncrypto::X509Pointer dup(X509_dup(sk_X509_value(ssl_certs, i)));
      if (!dup) return Undefined(env);
      peer_certs.push_back(std::move(dup));
    }
  } else if (ssl_certs != nullptr) {
    for (int i = 0; i < sk_X509_num(ssl_certs); ++i) {
      ncrypto::X509Pointer dup(X509_dup(sk_X509_value(ssl_certs, i)));
      if (!dup) return Undefined(env);
      peer_certs.push_back(std::move(dup));
    }
  }

  napi_value result = CreateLegacyCertObject(env, cert.get());
  if (result == nullptr || internal_binding::IsUndefined(env, result)) return Undefined(env);

  napi_value issuer_object = result;
  for (;;) {
    size_t match_index = peer_certs.size();
    for (size_t i = 0; i < peer_certs.size(); ++i) {
      if (cert.view().isIssuedBy(peer_certs[i].view())) {
        match_index = i;
        break;
      }
    }

    if (match_index == peer_certs.size()) break;

    napi_value next = CreateLegacyCertObject(env, peer_certs[match_index].get());
    if (next == nullptr || internal_binding::IsUndefined(env, next)) return Undefined(env);
    napi_set_named_property(env, issuer_object, "issuerCertificate", next);
    issuer_object = next;
    cert = std::move(peer_certs[match_index]);
    peer_certs.erase(peer_certs.begin() + static_cast<std::ptrdiff_t>(match_index));
  }

  while (!cert.view().isIssuedBy(cert.view())) {
    X509* prev = cert.get();
    auto issuer = ncrypto::X509Pointer::IssuerFrom(SSL_get_SSL_CTX(ssl), cert.view());
    if (!issuer) break;
    napi_value next = CreateLegacyCertObject(env, issuer.get());
    if (next == nullptr || internal_binding::IsUndefined(env, next)) return Undefined(env);
    napi_set_named_property(env, issuer_object, "issuerCertificate", next);
    issuer_object = next;
    if (issuer.get() == prev) break;
    cert = std::move(issuer);
  }

  return result;
}

napi_value TlsWrapVerifyError(napi_env env, napi_callback_info info) {
  TlsWrap* wrap = UnwrapThis(env, info, nullptr, nullptr, nullptr);
  if (wrap == nullptr || wrap->ssl == nullptr) return Null(env);
  ncrypto::SSLPointer ssl_view(wrap->ssl);
  long verify_error = static_cast<long>(
      ssl_view.verifyPeerCertificate().value_or(X509_V_ERR_UNABLE_TO_GET_ISSUER_CERT));
  ssl_view.release();
  if (verify_error == X509_V_OK) return Null(env);
  const char* code = ncrypto::X509Pointer::ErrorCode(static_cast<int32_t>(verify_error));
  const char* reason = X509_verify_cert_error_string(verify_error);
  return CreateErrorWithCode(env, code != nullptr ? code : "ERR_TLS_CERT", reason != nullptr ? reason
                                                                                              : "Certificate verification failed");
}

napi_value TlsWrapGetPeerCertificate(napi_env env, napi_callback_info info) {
  size_t argc = 1;
  napi_value argv[1] = {nullptr};
  TlsWrap* wrap = UnwrapThis(env, info, &argc, argv, nullptr);
  if (wrap == nullptr || wrap->ssl == nullptr) return Undefined(env);
  bool detailed = false;
  if (argc >= 1 && argv[0] != nullptr) {
    napi_get_value_bool(env, argv[0], &detailed);
  }
  if (detailed) {
    return BuildDetailedPeerCertificateObject(env, wrap->ssl, wrap->is_server);
  }

  X509* cert = SSL_get_peer_certificate(wrap->ssl);
  if (cert == nullptr) {
    STACK_OF(X509)* chain = SSL_get_peer_cert_chain(wrap->ssl);
    if (chain == nullptr || sk_X509_num(chain) == 0) return Undefined(env);
    cert = X509_dup(sk_X509_value(chain, 0));
  }
  if (cert == nullptr) return Undefined(env);
  napi_value out = CreateLegacyCertObject(env, cert);
  X509_free(cert);
  return out != nullptr ? out : Undefined(env);
}

napi_value TlsWrapGetCertificate(napi_env env, napi_callback_info info) {
  TlsWrap* wrap = UnwrapThis(env, info, nullptr, nullptr, nullptr);
  if (wrap == nullptr || wrap->ssl == nullptr) return Undefined(env);
  X509* cert = SSL_get_certificate(wrap->ssl);
  return CreateLegacyCertObject(env, cert);
}

napi_value TlsWrapGetPeerX509Certificate(napi_env env, napi_callback_info info) {
  TlsWrap* wrap = UnwrapThis(env, info, nullptr, nullptr, nullptr);
  if (wrap == nullptr || wrap->ssl == nullptr) return Undefined(env);
  X509* cert = SSL_get_peer_certificate(wrap->ssl);
  if (cert == nullptr) return Undefined(env);

  napi_value crypto_binding = ResolveInternalBinding(env, "crypto");
  if (crypto_binding == nullptr) {
    X509_free(cert);
    return Undefined(env);
  }

  napi_value parse_x509 = nullptr;
  if (napi_get_named_property(env, crypto_binding, "parseX509", &parse_x509) != napi_ok || parse_x509 == nullptr) {
    X509_free(cert);
    return Undefined(env);
  }

  const int der_len = i2d_X509(cert, nullptr);
  if (der_len <= 0) {
    X509_free(cert);
    return Undefined(env);
  }
  std::vector<uint8_t> der(static_cast<size_t>(der_len));
  unsigned char* ptr = der.data();
  if (i2d_X509(cert, &ptr) != der_len) {
    X509_free(cert);
    return Undefined(env);
  }

  std::vector<uint8_t> issuer_der;
  STACK_OF(X509)* chain = SSL_get_peer_cert_chain(wrap->ssl);
  if (chain == nullptr) {
#if OPENSSL_VERSION_MAJOR >= 3
    chain = SSL_get0_verified_chain(wrap->ssl);
#endif
  }
  if (chain != nullptr) {
    const int count = sk_X509_num(chain);
    for (int i = 0; i < count; ++i) {
      X509* candidate = sk_X509_value(chain, i);
      if (candidate == nullptr) continue;
      if (X509_NAME_cmp(X509_get_subject_name(candidate), X509_get_issuer_name(cert)) != 0) continue;
      const int issuer_len = i2d_X509(candidate, nullptr);
      if (issuer_len <= 0) continue;
      issuer_der.resize(static_cast<size_t>(issuer_len));
      unsigned char* issuer_ptr = issuer_der.data();
      if (i2d_X509(candidate, &issuer_ptr) != issuer_len) {
        issuer_der.clear();
      }
      break;
    }
  }
  X509_free(cert);

  napi_value raw = CreateBufferCopy(env, der.data(), der.size());
  if (raw == nullptr) return Undefined(env);
  napi_value handle_argv[2] = {raw, Undefined(env)};
  size_t handle_argc = 1;
  if (!issuer_der.empty()) {
    handle_argv[1] = CreateBufferCopy(env, issuer_der.data(), issuer_der.size());
    if (handle_argv[1] != nullptr) {
      handle_argc = 2;
    }
  }
  napi_value out = nullptr;
  if (napi_call_function(env, crypto_binding, parse_x509, handle_argc, handle_argv, &out) != napi_ok || out == nullptr) {
    return Undefined(env);
  }
  return out;
}

napi_value TlsWrapGetX509Certificate(napi_env env, napi_callback_info info) {
  TlsWrap* wrap = UnwrapThis(env, info, nullptr, nullptr, nullptr);
  if (wrap == nullptr || wrap->ssl == nullptr) return Undefined(env);
  X509* cert = SSL_get_certificate(wrap->ssl);
  if (cert == nullptr) return Undefined(env);

  napi_value crypto_binding = ResolveInternalBinding(env, "crypto");
  if (crypto_binding == nullptr) return Undefined(env);

  napi_value parse_x509 = nullptr;
  if (napi_get_named_property(env, crypto_binding, "parseX509", &parse_x509) != napi_ok || parse_x509 == nullptr) {
    return Undefined(env);
  }

  const int der_len = i2d_X509(cert, nullptr);
  if (der_len <= 0) return Undefined(env);
  std::vector<uint8_t> der(static_cast<size_t>(der_len));
  unsigned char* ptr = der.data();
  if (i2d_X509(cert, &ptr) != der_len) return Undefined(env);

  napi_value raw = CreateBufferCopy(env, der.data(), der.size());
  if (raw == nullptr) return Undefined(env);
  napi_value handle_argv[1] = {raw};
  napi_value out = nullptr;
  if (napi_call_function(env, crypto_binding, parse_x509, 1, handle_argv, &out) != napi_ok || out == nullptr) {
    return Undefined(env);
  }
  return out;
}

napi_value TlsWrapSetKeyCert(napi_env env, napi_callback_info info) {
  size_t argc = 1;
  napi_value argv[1] = {nullptr};
  TlsWrap* wrap = UnwrapThis(env, info, &argc, argv, nullptr);
  if (wrap == nullptr || wrap->ssl == nullptr || wrap->is_server == false || argc < 1) return Undefined(env);
  edge::crypto::SecureContextHolder* holder = nullptr;
  if (internal_binding::EdgeCryptoGetSecureContextHolderFromObject(env, argv[0], &holder) && holder != nullptr) {
    if (!SetSecureContextOnSslAndRetain(wrap, holder, argv[0])) {
      EmitError(wrap, CreateLastOpenSslError(env, "ERR_TLS_INVALID_CONTEXT", "Failed to update secure context"));
    }
  } else {
    napi_throw_type_error(env, nullptr, "Must give a SecureContext as first argument");
    return nullptr;
  }
  return Undefined(env);
}

napi_value TlsWrapDestroySSL(napi_env env, napi_callback_info info) {
  TlsWrap* wrap = UnwrapThis(env, info, nullptr, nullptr, nullptr);
  DestroySsl(wrap);
  return Undefined(env);
}

napi_value TlsWrapEndParser(napi_env env, napi_callback_info info) {
  TlsWrap* wrap = UnwrapThis(env, info, nullptr, nullptr, nullptr);
  if (wrap == nullptr) return Undefined(env);
  wrap->hello_parser.End();
  Cycle(wrap);
  return Undefined(env);
}

napi_value TlsWrapSetOCSPResponse(napi_env env, napi_callback_info info) {
  size_t argc = 1;
  napi_value argv[1] = {nullptr};
  TlsWrap* wrap = UnwrapThis(env, info, &argc, argv, nullptr);
  if (wrap == nullptr || argc < 1 || argv[0] == nullptr) return Undefined(env);
  const uint8_t* data = nullptr;
  size_t len = 0;
  if (!GetArrayBufferViewSpan(env, argv[0], &data, &len) || data == nullptr) {
    napi_throw_type_error(env, nullptr, "OCSP response must be a Buffer");
    return nullptr;
  }
  wrap->ocsp_response.assign(data, data + len);
  return Undefined(env);
}

napi_value TlsWrapCertCbDone(napi_env env, napi_callback_info info) {
  TlsWrap* wrap = UnwrapThis(env, info, nullptr, nullptr, nullptr);
  if (wrap == nullptr || wrap->ssl == nullptr) return Undefined(env);
  napi_value self = GetRefValue(env, wrap->wrapper_ref);
  napi_value sni_context = GetNamedValue(env, self, "sni_context");
  edge::crypto::SecureContextHolder* holder = nullptr;
  napi_valuetype sni_type = napi_undefined;
  if (sni_context != nullptr &&
      napi_typeof(env, sni_context, &sni_type) == napi_ok &&
      sni_type == napi_object) {
    if (internal_binding::EdgeCryptoGetSecureContextHolderFromObject(env, sni_context, &holder) && holder != nullptr) {
      if (!SetSecureContextOnSslAndRetain(wrap, holder, sni_context)) {
        EmitError(wrap, CreateLastOpenSslError(env, "ERR_TLS_INVALID_CONTEXT", "Failed to set SNI context"));
        return Undefined(env);
      }
    } else {
      napi_value code = nullptr;
      napi_value message = nullptr;
      napi_value error = nullptr;
      napi_create_string_utf8(env, "ERR_TLS_INVALID_CONTEXT", NAPI_AUTO_LENGTH, &code);
      napi_create_string_utf8(env, "Invalid SNI context", NAPI_AUTO_LENGTH, &message);
      napi_create_type_error(env, code, message, &error);
      if (error != nullptr && code != nullptr) {
        napi_set_named_property(env, error, "code", code);
      }
      EmitError(wrap, error);
      return Undefined(env);
    }
  }
  TlsCertCb cb = wrap->cert_cb;
  void* cb_arg = wrap->cert_cb_arg;
  wrap->cert_cb_running = false;
  wrap->waiting_cert_cb = false;
  wrap->cert_cb = nullptr;
  wrap->cert_cb_arg = nullptr;
  if (cb != nullptr) {
    cb(cb_arg);
  } else {
    Cycle(wrap);
  }
  return Undefined(env);
}

napi_value TlsWrapNewSessionDone(napi_env env, napi_callback_info info) {
  TlsWrap* wrap = UnwrapThis(env, info, nullptr, nullptr, nullptr);
  if (wrap == nullptr) return Undefined(env);
  wrap->awaiting_new_session = false;
  Cycle(wrap);
  return Undefined(env);
}

napi_value TlsWrapReceive(napi_env env, napi_callback_info info) {
  size_t argc = 1;
  napi_value argv[1] = {nullptr};
  TlsWrap* wrap = UnwrapThis(env, info, &argc, argv, nullptr);
  if (wrap == nullptr || wrap->ssl == nullptr || argc < 1) return Undefined(env);
  const uint8_t* data = nullptr;
  size_t len = 0;
  size_t offset = 0;
  if (!GetArrayBufferBytes(env, argv[0], &data, &len, &offset) || data == nullptr) return Undefined(env);
  InjectParentStreamBytes(wrap, data + offset, len - offset);
  return Undefined(env);
}

napi_value TlsWrapGetWriteQueueSize(napi_env env, napi_callback_info info) {
  napi_value self = nullptr;
  size_t argc = 0;
  napi_get_cb_info(env, info, &argc, nullptr, &self, nullptr);
  TlsWrap* wrap = UnwrapTlsWrap(env, self);

  uint32_t size = 0;
  if (wrap != nullptr) {
    for (const auto& pending : wrap->pending_encrypted_writes) {
      size += static_cast<uint32_t>(pending.size);
    }
  }

  napi_value out = nullptr;
  napi_create_uint32(env, size, &out);
  return out;
}

napi_value TlsWrapWrap(napi_env env, napi_callback_info info) {
  size_t argc = 4;
  napi_value argv[4] = {nullptr, nullptr, nullptr, nullptr};
  napi_get_cb_info(env, info, &argc, argv, nullptr, nullptr);
  if (argc < 2 || argv[0] == nullptr || argv[1] == nullptr) return Undefined(env);

  edge::crypto::SecureContextHolder* holder = nullptr;
  if (!internal_binding::EdgeCryptoGetSecureContextHolderFromObject(env, argv[1], &holder) || holder == nullptr) {
    napi_throw_type_error(env, nullptr, "SecureContext required");
    return nullptr;
  }

  napi_value ctor = GetRefValue(env, EnsureState(env).tls_wrap_ctor_ref);
  napi_value out = nullptr;
  if (ctor == nullptr || napi_new_instance(env, ctor, 0, nullptr, &out) != napi_ok || out == nullptr) {
    return Undefined(env);
  }

  TlsWrap* wrap = UnwrapTlsWrap(env, out);
  if (wrap == nullptr) return Undefined(env);
  EdgeStreamBase* parent_stream_base = EdgeStreamBaseFromValue(env, argv[0]);
  if (parent_stream_base == nullptr) {
    napi_throw_type_error(env, nullptr, "handle must be a StreamBase");
    return nullptr;
  }
  napi_create_reference(env, argv[0], 1, &wrap->parent_ref);
  napi_create_reference(env, argv[1], 1, &wrap->context_ref);
  wrap->parent_stream_base = parent_stream_base;
  wrap->secure_context = holder;
  if (argc >= 3 && argv[2] != nullptr) {
    bool is_server = false;
    napi_get_value_bool(env, argv[2], &is_server);
    wrap->is_server = is_server;
  }
  if (argc >= 4 && argv[3] != nullptr) {
    bool has_active = false;
    napi_get_value_bool(env, argv[3], &has_active);
    wrap->has_active_write_issued_by_prev_listener = has_active;
  }
  wrap->keepalive_needed = EdgeStreamBaseGetLibuvStream(env, argv[0]) == nullptr;

  InitSsl(wrap);
  if (!wrap->pending_session.empty()) {
    (void)LoadSessionBytes(wrap, wrap->pending_session.data(), wrap->pending_session.size());
  }

  wrap->parent_stream_listener.data = wrap;
  wrap->parent_stream_listener.on_alloc = ParentStreamOnAlloc;
  wrap->parent_stream_listener.on_read = ParentStreamOnRead;
  wrap->parent_stream_listener.on_after_write = ParentStreamOnAfterWrite;
  wrap->parent_stream_listener.on_after_shutdown = ParentStreamOnAfterShutdown;
  wrap->parent_stream_listener.on_close = ParentStreamOnClose;
  (void)EdgeStreamBasePushListener(parent_stream_base, &wrap->parent_stream_listener);

  napi_value parent_reading = GetNamedValue(env, argv[0], "reading");
  if (parent_reading != nullptr) {
    bool reading = false;
    if (napi_get_value_bool(env, parent_reading, &reading) == napi_ok) {
      EdgeStreamBaseSetReading(&wrap->base, reading);
    }
  }

  return out;
}

napi_value EdgeInstallTlsWrapBindingInternal(napi_env env) {
  TlsBindingState& state = EnsureState(env);
  napi_value cached = GetRefValue(env, state.binding_ref);
  if (cached != nullptr) return cached;

  napi_value binding = nullptr;
  if (napi_create_object(env, &binding) != napi_ok || binding == nullptr) return nullptr;

  constexpr napi_property_attributes kMutableMethod =
      static_cast<napi_property_attributes>(napi_writable | napi_configurable);
  napi_property_descriptor tls_wrap_props[] = {
      {"readStart", nullptr, TlsWrapReadStart, nullptr, nullptr, nullptr, napi_default_method, nullptr},
      {"readStop", nullptr, TlsWrapReadStop, nullptr, nullptr, nullptr, napi_default_method, nullptr},
      {"writeBuffer", nullptr, TlsWrapWriteBuffer, nullptr, nullptr, nullptr, napi_default_method, nullptr},
      {"writev", nullptr, TlsWrapWritev, nullptr, nullptr, nullptr, napi_default_method, nullptr},
      {"writeLatin1String", nullptr, TlsWrapWriteLatin1String, nullptr, nullptr, nullptr, napi_default_method,
       nullptr},
      {"writeUtf8String", nullptr, TlsWrapWriteUtf8String, nullptr, nullptr, nullptr, napi_default_method, nullptr},
      {"writeAsciiString", nullptr, TlsWrapWriteAsciiString, nullptr, nullptr, nullptr, napi_default_method, nullptr},
      {"writeUcs2String", nullptr, TlsWrapWriteUcs2String, nullptr, nullptr, nullptr, napi_default_method, nullptr},
      {"shutdown", nullptr, TlsWrapShutdown, nullptr, nullptr, nullptr, napi_default_method, nullptr},
      {"close", nullptr, TlsWrapClose, nullptr, nullptr, nullptr, kMutableMethod, nullptr},
      {"ref", nullptr, TlsWrapRef, nullptr, nullptr, nullptr, kMutableMethod, nullptr},
      {"unref", nullptr, TlsWrapUnref, nullptr, nullptr, nullptr, kMutableMethod, nullptr},
      {"getAsyncId", nullptr, TlsWrapGetAsyncId, nullptr, nullptr, nullptr, napi_default_method, nullptr},
      {"getProviderType", nullptr, TlsWrapGetProviderType, nullptr, nullptr, nullptr, napi_default_method, nullptr},
      {"asyncReset", nullptr, TlsWrapAsyncReset, nullptr, nullptr, nullptr, napi_default_method, nullptr},
      {"useUserBuffer", nullptr, TlsWrapUseUserBuffer, nullptr, nullptr, nullptr, napi_default_method, nullptr},
      {"onread", nullptr, nullptr, TlsWrapGetOnRead, TlsWrapSetOnRead, nullptr, napi_default, nullptr},
      {"setVerifyMode", nullptr, TlsWrapSetVerifyMode, nullptr, nullptr, nullptr, napi_default_method, nullptr},
      {"enableTrace", nullptr, TlsWrapEnableTrace, nullptr, nullptr, nullptr, napi_default_method, nullptr},
      {"enableSessionCallbacks", nullptr, TlsWrapEnableSessionCallbacks, nullptr, nullptr, nullptr, napi_default,
       nullptr},
      {"enableCertCb", nullptr, TlsWrapEnableCertCb, nullptr, nullptr, nullptr, napi_default_method, nullptr},
      {"enableALPNCb", nullptr, TlsWrapEnableALPNCb, nullptr, nullptr, nullptr, napi_default_method, nullptr},
      {"enablePskCallback", nullptr, TlsWrapEnablePskCallback, nullptr, nullptr, nullptr, napi_default_method,
       nullptr},
      {"setPskIdentityHint", nullptr, TlsWrapSetPskIdentityHint, nullptr, nullptr, nullptr, napi_default_method,
       nullptr},
      {"enableKeylogCallback", nullptr, TlsWrapEnableKeylogCallback, nullptr, nullptr, nullptr, napi_default,
       nullptr},
      {"writesIssuedByPrevListenerDone", nullptr, TlsWrapWritesIssuedByPrevListenerDone, nullptr, nullptr, nullptr,
       napi_default, nullptr},
      {"setALPNProtocols", nullptr, TlsWrapSetALPNProtocols, nullptr, nullptr, nullptr, napi_default_method, nullptr},
      {"requestOCSP", nullptr, TlsWrapRequestOCSP, nullptr, nullptr, nullptr, napi_default_method, nullptr},
      {"start", nullptr, TlsWrapStart, nullptr, nullptr, nullptr, napi_default_method, nullptr},
      {"renegotiate", nullptr, TlsWrapRenegotiate, nullptr, nullptr, nullptr, napi_default_method, nullptr},
      {"setServername", nullptr, TlsWrapSetServername, nullptr, nullptr, nullptr, napi_default_method, nullptr},
      {"getServername", nullptr, TlsWrapGetServername, nullptr, nullptr, nullptr, napi_default_method, nullptr},
      {"setSession", nullptr, TlsWrapSetSession, nullptr, nullptr, nullptr, napi_default_method, nullptr},
      {"getSession", nullptr, TlsWrapGetSession, nullptr, nullptr, nullptr, napi_default_method, nullptr},
      {"getCipher", nullptr, TlsWrapGetCipher, nullptr, nullptr, nullptr, napi_default_method, nullptr},
      {"getSharedSigalgs", nullptr, TlsWrapGetSharedSigalgs, nullptr, nullptr, nullptr, napi_default_method, nullptr},
      {"getEphemeralKeyInfo", nullptr, TlsWrapGetEphemeralKeyInfo, nullptr, nullptr, nullptr, napi_default_method,
       nullptr},
      {"getFinished", nullptr, TlsWrapGetFinished, nullptr, nullptr, nullptr, napi_default_method, nullptr},
      {"getPeerFinished", nullptr, TlsWrapGetPeerFinished, nullptr, nullptr, nullptr, napi_default_method, nullptr},
      {"getProtocol", nullptr, TlsWrapGetProtocol, nullptr, nullptr, nullptr, napi_default_method, nullptr},
      {"getTLSTicket", nullptr, TlsWrapGetTLSTicket, nullptr, nullptr, nullptr, napi_default_method, nullptr},
      {"isSessionReused", nullptr, TlsWrapIsSessionReused, nullptr, nullptr, nullptr, napi_default_method, nullptr},
      {"getPeerX509Certificate", nullptr, TlsWrapGetPeerX509Certificate, nullptr, nullptr, nullptr,
       napi_default_method, nullptr},
      {"getX509Certificate", nullptr, TlsWrapGetX509Certificate, nullptr, nullptr, nullptr, napi_default_method,
       nullptr},
      {"exportKeyingMaterial", nullptr, TlsWrapExportKeyingMaterial, nullptr, nullptr, nullptr, napi_default,
       nullptr},
      {"setMaxSendFragment", nullptr, TlsWrapSetMaxSendFragment, nullptr, nullptr, nullptr, napi_default_method,
       nullptr},
      {"getALPNNegotiatedProtocol", nullptr, TlsWrapGetALPNNegotiatedProtocol, nullptr, nullptr, nullptr,
       napi_default, nullptr},
      {"verifyError", nullptr, TlsWrapVerifyError, nullptr, nullptr, nullptr, napi_default_method, nullptr},
      {"getPeerCertificate", nullptr, TlsWrapGetPeerCertificate, nullptr, nullptr, nullptr, napi_default_method,
       nullptr},
      {"getCertificate", nullptr, TlsWrapGetCertificate, nullptr, nullptr, nullptr, napi_default_method, nullptr},
      {"setKeyCert", nullptr, TlsWrapSetKeyCert, nullptr, nullptr, nullptr, napi_default_method, nullptr},
      {"destroySSL", nullptr, TlsWrapDestroySSL, nullptr, nullptr, nullptr, napi_default_method, nullptr},
      {"loadSession", nullptr, TlsWrapLoadSession, nullptr, nullptr, nullptr, napi_default_method, nullptr},
      {"endParser", nullptr, TlsWrapEndParser, nullptr, nullptr, nullptr, napi_default_method, nullptr},
      {"setOCSPResponse", nullptr, TlsWrapSetOCSPResponse, nullptr, nullptr, nullptr, napi_default_method, nullptr},
      {"certCbDone", nullptr, TlsWrapCertCbDone, nullptr, nullptr, nullptr, napi_default_method, nullptr},
      {"newSessionDone", nullptr, TlsWrapNewSessionDone, nullptr, nullptr, nullptr, napi_default_method, nullptr},
      {"receive", nullptr, TlsWrapReceive, nullptr, nullptr, nullptr, napi_default_method, nullptr},
      {"writeQueueSize", nullptr, nullptr, TlsWrapGetWriteQueueSize, nullptr, nullptr, napi_default, nullptr},
      {"bytesRead", nullptr, nullptr, TlsWrapBytesReadGetter, nullptr, nullptr, napi_default, nullptr},
      {"bytesWritten", nullptr, nullptr, TlsWrapBytesWrittenGetter, nullptr, nullptr, napi_default, nullptr},
      {"fd", nullptr, nullptr, TlsWrapFdGetter, nullptr, nullptr, napi_default, nullptr},
      {"_externalStream", nullptr, nullptr, TlsWrapExternalStreamGetter, nullptr, nullptr, napi_default, nullptr},
  };

  napi_value tls_wrap_ctor = nullptr;
  if (napi_define_class(env,
                        "TLSWrap",
                        NAPI_AUTO_LENGTH,
                        TlsWrapCtor,
                        nullptr,
                        sizeof(tls_wrap_props) / sizeof(tls_wrap_props[0]),
                        tls_wrap_props,
                        &tls_wrap_ctor) != napi_ok ||
      tls_wrap_ctor == nullptr) {
    return nullptr;
  }

  DeleteRefIfPresent(env, &state.tls_wrap_ctor_ref);
  napi_create_reference(env, tls_wrap_ctor, 1, &state.tls_wrap_ctor_ref);

  napi_value wrap_fn = nullptr;
  if (napi_create_function(env, "wrap", NAPI_AUTO_LENGTH, TlsWrapWrap, nullptr, &wrap_fn) != napi_ok ||
      wrap_fn == nullptr) {
    return nullptr;
  }

  napi_set_named_property(env, binding, "TLSWrap", tls_wrap_ctor);
  napi_set_named_property(env, binding, "wrap", wrap_fn);

  DeleteRefIfPresent(env, &state.binding_ref);
  napi_create_reference(env, binding, 1, &state.binding_ref);
  return binding;
}

}  // namespace

napi_value EdgeInstallTlsWrapBinding(napi_env env) {
  if (env == nullptr) return nullptr;
  return EdgeInstallTlsWrapBindingInternal(env);
}
