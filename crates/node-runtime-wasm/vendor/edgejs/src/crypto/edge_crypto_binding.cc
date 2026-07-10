#include "crypto/edge_crypto_binding.h"
#include "crypto/edge_secure_context_bridge.h"
#include "edge_environment.h"
#include "edge_option_helpers.h"

#include <algorithm>
#include <atomic>
#include <cctype>
#include <cstdio>
#include <cstdlib>
#include <cstring>
#include <memory>
#include <mutex>
#include <set>
#include <string>
#include <unordered_set>
#include <vector>

#include "ncrypto.h"
#include <openssl/crypto.h>
#include <openssl/core_names.h>
#include <openssl/dsa.h>
#include <openssl/err.h>
#include <openssl/evp.h>
#include <openssl/hmac.h>
#include <openssl/ecdsa.h>
#include <openssl/params.h>
#include <openssl/pem.h>
#include <openssl/pkcs12.h>
#include <openssl/rsa.h>
#include <openssl/ssl.h>
#include <openssl/x509.h>
#include <openssl/x509err.h>
#include <openssl/x509v3.h>
#include <uv.h>

namespace edge::crypto {
namespace {

static const char* const kBundledRootCerts[] = {
#ifndef NODE_WANT_INTERNALS
#define NODE_WANT_INTERNALS 1
#define EDGE_UNDEF_NODE_WANT_INTERNALS 1
#endif
#include "node_root_certs.h"
#ifdef EDGE_UNDEF_NODE_WANT_INTERNALS
#undef EDGE_UNDEF_NODE_WANT_INTERNALS
#undef NODE_WANT_INTERNALS
#endif
};

struct X509Less {
  bool operator()(const X509* lhs, const X509* rhs) const noexcept {
    return X509_cmp(const_cast<X509*>(lhs), const_cast<X509*>(rhs)) < 0;
  }
};

using X509Set = std::set<X509*, X509Less>;

std::once_flag g_bundled_root_certs_once;
std::once_flag g_extra_root_certs_once;
std::once_flag g_system_root_certs_once;
std::vector<X509*> g_bundled_root_certs;
std::vector<X509*> g_extra_root_certs;
std::vector<X509*> g_system_root_certs;
std::mutex g_user_root_certs_mutex;
std::unique_ptr<X509Set> g_user_root_certs;

std::atomic<bool> g_tried_cert_loading_off_thread{false};
std::atomic<bool> g_cert_loading_thread_started{false};
std::mutex g_cert_loading_thread_mutex;
uv_thread_t g_cert_loading_thread;
std::once_flag g_cert_loading_cleanup_once;

struct RootCertLoadPlan {
  bool load_bundled = false;
  bool load_extra = false;
  bool load_system = false;
};

RootCertLoadPlan g_root_cert_load_plan;

bool AddCertificateToStore(X509_STORE* store, X509* cert);
EVP_PKEY* ParsePrivateKeyWithPassphraseImpl(const uint8_t* data,
                                            size_t len,
                                            const uint8_t* passphrase,
                                            size_t passphrase_len,
                                            bool has_passphrase);

bool EnsureTicketKeys(SecureContextHolder* holder) {
  if (holder == nullptr) return false;
  if (holder->ticket_keys.size() == 48) return true;
  holder->ticket_keys.resize(48);
  if (RAND_bytes(holder->ticket_keys.data(), static_cast<int>(holder->ticket_keys.size())) != 1) {
    holder->ticket_keys.clear();
    return false;
  }
  return true;
}

int TicketCompatibilityCallback(SSL* ssl,
                                unsigned char* name,
                                unsigned char* iv,
                                EVP_CIPHER_CTX* ectx,
                                HMAC_CTX* hctx,
                                int enc) {
  auto* holder = static_cast<SecureContextHolder*>(SSL_CTX_get_app_data(SSL_get_SSL_CTX(ssl)));
  if (holder == nullptr || !EnsureTicketKeys(holder)) {
    return -1;
  }

  const unsigned char* key_name = holder->ticket_keys.data();
  const unsigned char* hmac_key = holder->ticket_keys.data() + 16;
  const unsigned char* aes_key = holder->ticket_keys.data() + 32;

  if (enc) {
    std::memcpy(name, key_name, 16);
    if (RAND_bytes(iv, 16) != 1 ||
        EVP_EncryptInit_ex(ectx, EVP_aes_128_cbc(), nullptr, aes_key, iv) <= 0 ||
        HMAC_Init_ex(hctx, hmac_key, 16, EVP_sha256(), nullptr) <= 0) {
      return -1;
    }
    return 1;
  }

  if (std::memcmp(name, key_name, 16) != 0) {
    return 0;
  }

  if (EVP_DecryptInit_ex(ectx, EVP_aes_128_cbc(), nullptr, aes_key, iv) <= 0 ||
      HMAC_Init_ex(hctx, hmac_key, 16, EVP_sha256(), nullptr) <= 0) {
    return -1;
  }

  return 1;
}

void EnsureTicketCallback(SecureContextHolder* holder) {
  if (holder == nullptr || holder->ctx == nullptr || holder->ticket_callback_installed) return;
  if (!EnsureTicketKeys(holder)) return;
  SSL_CTX_set_app_data(holder->ctx, holder);
  SSL_CTX_set_tlsext_ticket_key_cb(holder->ctx, TicketCompatibilityCallback);
  holder->ticket_callback_installed = true;
}

SSL_CTX* CreateConfiguredSecureContext(const SSL_METHOD* method) {
  SSL_CTX* ctx = SSL_CTX_new(method);
  if (ctx == nullptr) return nullptr;
#if OPENSSL_VERSION_NUMBER >= 0x10100000L
  // Match Node's effective TLS defaults more closely on distros whose shared
  // OpenSSL raises the system security level above OpenSSL's upstream default.
  SSL_CTX_set_security_level(ctx, 1);
#endif
  SSL_CTX_set_options(ctx, SSL_OP_NO_SSLv2);
  SSL_CTX_set_options(ctx, SSL_OP_NO_SSLv3);
#if OPENSSL_VERSION_MAJOR >= 3
  SSL_CTX_set_options(ctx, SSL_OP_ALLOW_CLIENT_RENEGOTIATION);
#endif
  SSL_CTX_clear_mode(ctx, SSL_MODE_NO_AUTO_CHAIN);
#ifdef SSL_SESS_CACHE_NO_INTERNAL
  SSL_CTX_set_session_cache_mode(
      ctx,
      SSL_SESS_CACHE_CLIENT | SSL_SESS_CACHE_SERVER | SSL_SESS_CACHE_NO_INTERNAL | SSL_SESS_CACHE_NO_AUTO_CLEAR);
#else
  SSL_CTX_set_session_cache_mode(ctx, SSL_SESS_CACHE_CLIENT | SSL_SESS_CACHE_SERVER | SSL_SESS_CACHE_NO_AUTO_CLEAR);
#endif
  return ctx;
}
bool AppendUniquePemString(std::vector<std::string>* out, const std::string& pem);
bool GetByteStringArray(napi_env env, napi_value value, std::vector<std::string>* out);
bool GetEffectiveCaOptions(napi_env env, bool* use_openssl_ca, bool* use_system_ca);
std::vector<X509*>& GetBundledRootCertificatesParsed();
std::vector<X509*>& GetExtraRootCertificatesParsed();
std::vector<X509*>& GetSystemRootCertificatesParsed();
void EnsureRootCertThreadCleanupHook(napi_env env);
void CleanupRootCertLoading(void* data);


std::string ValueToUtf8(napi_env env, napi_value value) {
  size_t len = 0;
  if (napi_get_value_string_utf8(env, value, nullptr, 0, &len) != napi_ok) return "";
  std::string out(len + 1, '\0');
  size_t copied = 0;
  if (napi_get_value_string_utf8(env, value, out.data(), out.size(), &copied) != napi_ok) return "";
  out.resize(copied);
  return out;
}

size_t TypedArrayBytesPerElement(napi_typedarray_type type);

bool GetBufferBytes(napi_env env, napi_value value, uint8_t** data, size_t* len) {
  static uint8_t kEmptyBufferSentinel = 0;
  if (value == nullptr || data == nullptr || len == nullptr) return false;

  bool is_buffer = false;
  if (napi_is_buffer(env, value, &is_buffer) == napi_ok && is_buffer) {
    if (napi_get_buffer_info(env, value, reinterpret_cast<void**>(data), len) != napi_ok) {
      return false;
    }
    if (*len == 0 && *data == nullptr) {
      *data = &kEmptyBufferSentinel;
    }
    return true;
  }

  bool is_arraybuffer = false;
  if (napi_is_arraybuffer(env, value, &is_arraybuffer) == napi_ok && is_arraybuffer) {
    void* raw = nullptr;
    size_t byte_len = 0;
    if (napi_get_arraybuffer_info(env, value, &raw, &byte_len) != napi_ok) return false;
    if (raw == nullptr && byte_len != 0) return false;
    *data = raw != nullptr ? static_cast<uint8_t*>(raw) : &kEmptyBufferSentinel;
    *len = byte_len;
    return true;
  }

  bool is_typedarray = false;
  if (napi_is_typedarray(env, value, &is_typedarray) == napi_ok && is_typedarray) {
    napi_typedarray_type ta_type = napi_uint8_array;
    size_t element_len = 0;
    void* raw = nullptr;
    napi_value ab = nullptr;
    size_t byte_offset = 0;
    if (napi_get_typedarray_info(env, value, &ta_type, &element_len, &raw, &ab, &byte_offset) != napi_ok) {
      return false;
    }
    const size_t byte_len = element_len * TypedArrayBytesPerElement(ta_type);
    if (raw == nullptr && byte_len != 0) return false;
    *data = raw != nullptr ? static_cast<uint8_t*>(raw) : &kEmptyBufferSentinel;
    *len = byte_len;
    return true;
  }

  bool is_dataview = false;
  if (napi_is_dataview(env, value, &is_dataview) == napi_ok && is_dataview) {
    size_t byte_len = 0;
    void* raw = nullptr;
    napi_value ab = nullptr;
    size_t byte_offset = 0;
    if (napi_get_dataview_info(env, value, &byte_len, &raw, &ab, &byte_offset) != napi_ok) return false;
    if (raw == nullptr && byte_len != 0) return false;
    *data = raw != nullptr ? static_cast<uint8_t*>(raw) : &kEmptyBufferSentinel;
    *len = byte_len;
    return true;
  }

  return false;
}

bool GetBufferOrStringBytes(napi_env env,
                            napi_value value,
                            std::vector<uint8_t>* owned,
                            uint8_t** data,
                            size_t* len) {
  static uint8_t kEmptyBufferSentinel = 0;
  if (owned == nullptr || data == nullptr || len == nullptr) return false;
  if (GetBufferBytes(env, value, data, len)) return true;

  napi_valuetype type = napi_undefined;
  if (napi_typeof(env, value, &type) != napi_ok || type != napi_string) return false;
  std::string text = ValueToUtf8(env, value);
  owned->assign(text.begin(), text.end());
  if (owned->empty()) {
    *data = &kEmptyBufferSentinel;
    *len = 0;
  } else {
    *data = owned->data();
    *len = owned->size();
  }
  return true;
}

bool GetKeyBytes(napi_env env,
                 napi_value value,
                 std::vector<uint8_t>* owned,
                 uint8_t** data,
                 size_t* len) {
  if (GetBufferOrStringBytes(env, value, owned, data, len)) return true;
  if (value == nullptr) return false;

  napi_value export_fn = nullptr;
  napi_valuetype type = napi_undefined;
  if (napi_get_named_property(env, value, "export", &export_fn) != napi_ok ||
      export_fn == nullptr ||
      napi_typeof(env, export_fn, &type) != napi_ok ||
      type != napi_function) {
    return false;
  }

  napi_value exported = nullptr;
  if (napi_call_function(env, value, export_fn, 0, nullptr, &exported) != napi_ok || exported == nullptr) {
    return false;
  }
  return GetBufferOrStringBytes(env, exported, owned, data, len);
}

bool IsNullOrUndefined(napi_env env, napi_value value) {
  napi_valuetype type = napi_undefined;
  if (napi_typeof(env, value, &type) != napi_ok) return false;
  return type == napi_undefined || type == napi_null;
}

bool ReadPassphrase(napi_env env, napi_value value, std::string* out, bool* provided) {
  if (out == nullptr || provided == nullptr) return false;
  *provided = false;
  out->clear();
  if (value == nullptr || IsNullOrUndefined(env, value)) return true;

  uint8_t* bytes = nullptr;
  size_t byte_len = 0;
  if (GetBufferBytes(env, value, &bytes, &byte_len)) {
    out->assign(reinterpret_cast<const char*>(bytes), byte_len);
    *provided = true;
    return true;
  }

  napi_valuetype type = napi_undefined;
  if (napi_typeof(env, value, &type) != napi_ok || type != napi_string) return false;
  *out = ValueToUtf8(env, value);
  *provided = true;
  return true;
}

size_t TypedArrayBytesPerElement(napi_typedarray_type type) {
  switch (type) {
    case napi_int8_array:
    case napi_uint8_array:
    case napi_uint8_clamped_array:
      return 1;
    case napi_int16_array:
    case napi_uint16_array:
    case napi_float16_array:
      return 2;
    case napi_int32_array:
    case napi_uint32_array:
    case napi_float32_array:
      return 4;
    case napi_float64_array:
    case napi_bigint64_array:
    case napi_biguint64_array:
      return 8;
    default:
      return 1;
  }
}

std::vector<std::string> GetStringArrayProperty(napi_env env, napi_value obj, const char* name) {
  std::vector<std::string> out;
  if (obj == nullptr || name == nullptr) return out;

  napi_value array_value = nullptr;
  if (napi_get_named_property(env, obj, name, &array_value) != napi_ok || array_value == nullptr) return out;

  bool is_array = false;
  if (napi_is_array(env, array_value, &is_array) != napi_ok || !is_array) return out;

  uint32_t length = 0;
  if (napi_get_array_length(env, array_value, &length) != napi_ok) return out;
  out.reserve(length);
  for (uint32_t i = 0; i < length; ++i) {
    napi_value entry = nullptr;
    if (napi_get_element(env, array_value, i, &entry) != napi_ok || entry == nullptr) {
      out.emplace_back();
      continue;
    }
    out.push_back(ValueToUtf8(env, entry));
  }
  return out;
}

bool IsCryptoDebugEnabled() {
  const char* value = std::getenv("NODE_DEBUG_NATIVE");
  if (value == nullptr) return false;
  return std::strstr(value, "crypto") != nullptr || std::strstr(value, "CRYPTO") != nullptr;
}

void DebugCryptoLog(const char* message) {
  if (message == nullptr || !IsCryptoDebugEnabled()) return;
  std::fputs(message, stderr);
}

bool AppendUniquePemString(std::vector<std::string>* out, const std::string& pem) {
  if (out == nullptr) return false;
  if (std::find(out->begin(), out->end(), pem) != out->end()) return false;
  out->push_back(pem);
  return true;
}

bool LooksLikePemCertificateText(const std::string& text) {
  return text.find("-----BEGIN CERTIFICATE-----") != std::string::npos;
}

bool IsPlainTextBlob(const std::string& text) {
  for (unsigned char ch : text) {
    if (ch == '\n' || ch == '\r' || ch == '\t') continue;
    if (ch < 0x20 || ch > 0x7e) return false;
  }
  return true;
}

bool GetByteStringArray(napi_env env, napi_value value, std::vector<std::string>* out) {
  if (out == nullptr) return false;
  bool is_array = false;
  if (napi_is_array(env, value, &is_array) != napi_ok || !is_array) return false;

  uint32_t length = 0;
  if (napi_get_array_length(env, value, &length) != napi_ok) return false;
  out->clear();
  out->reserve(length);
  for (uint32_t i = 0; i < length; ++i) {
    napi_value entry = nullptr;
    if (napi_get_element(env, value, i, &entry) != napi_ok || entry == nullptr) return false;

    std::vector<uint8_t> owned;
    uint8_t* bytes = nullptr;
    size_t byte_len = 0;
    if (!GetBufferOrStringBytes(env, entry, &owned, &bytes, &byte_len)) return false;
    out->emplace_back(reinterpret_cast<const char*>(bytes), byte_len);
  }
  return true;
}

unsigned long LoadCertsFromBio(std::vector<X509*>* certs, BIO* bio) {
  if (certs == nullptr || bio == nullptr) return 0;
  ERR_set_mark();
  while (X509* x509 = PEM_read_bio_X509(bio, nullptr, nullptr, nullptr)) {
    certs->push_back(x509);
  }
  const unsigned long err = ERR_peek_last_error();
  if (err != 0 &&
      !(ERR_GET_LIB(err) == ERR_LIB_PEM &&
        ERR_GET_REASON(err) == PEM_R_NO_START_LINE)) {
    ERR_pop_to_mark();
    return err;
  }
  ERR_clear_error();
  ERR_pop_to_mark();
  return 0;
}

unsigned long LoadCertsFromFile(std::vector<X509*>* certs, const char* path) {
  if (certs == nullptr || path == nullptr || path[0] == '\0') return 0;
  BIO* bio = BIO_new_file(path, "r");
  if (bio == nullptr) return ERR_get_error();
  const unsigned long err = LoadCertsFromBio(certs, bio);
  BIO_free(bio);
  return err;
}

std::string X509ToPemString(X509* cert) {
  if (cert == nullptr) return "";
  BIO* bio = BIO_new(BIO_s_mem());
  if (bio == nullptr) return "";
  std::string out;
  if (PEM_write_bio_X509(bio, cert) == 1) {
    char* data = nullptr;
    const long size = BIO_get_mem_data(bio, &data);
    if (size > 0 && data != nullptr) out.assign(data, static_cast<size_t>(size));
  }
  BIO_free(bio);
  return out;
}

bool AddCertificateToStore(X509_STORE* store, X509* cert) {
  if (store == nullptr || cert == nullptr) return false;
  if (X509_STORE_add_cert(store, cert) == 1) return true;
  const unsigned long err = ERR_peek_last_error();
  if (err != 0 &&
      ERR_GET_LIB(err) == ERR_LIB_X509 &&
      ERR_GET_REASON(err) == X509_R_CERT_ALREADY_IN_HASH_TABLE) {
    ERR_clear_error();
    return true;
  }
  return false;
}

void FreeCertificates(std::vector<X509*>* certs) {
  if (certs == nullptr) return;
  for (X509* cert : *certs) {
    X509_free(cert);
  }
  certs->clear();
}

std::vector<X509*>& GetBundledRootCertificatesParsed() {
  std::call_once(g_bundled_root_certs_once, []() {
    g_bundled_root_certs.reserve(sizeof(kBundledRootCerts) / sizeof(kBundledRootCerts[0]));
    for (const char* pem : kBundledRootCerts) {
      BIO* bio = BIO_new_mem_buf(pem, -1);
      if (bio == nullptr) continue;
      X509* cert = PEM_read_bio_X509(bio, nullptr, nullptr, nullptr);
      BIO_free(bio);
      if (cert != nullptr) g_bundled_root_certs.push_back(cert);
    }
  });
  return g_bundled_root_certs;
}

std::vector<X509*>& GetExtraRootCertificatesParsed() {
  std::call_once(g_extra_root_certs_once, []() {
    const char* extra_root_certs_file = std::getenv("NODE_EXTRA_CA_CERTS");
    if (extra_root_certs_file == nullptr || extra_root_certs_file[0] == '\0') return;

    const unsigned long err = LoadCertsFromFile(&g_extra_root_certs, extra_root_certs_file);
    if (err != 0) {
      char buf[256];
      ERR_error_string_n(err, buf, sizeof(buf));
      std::fprintf(stderr,
                   "Warning: Ignoring extra certs from `%s`, load failed: %s\n",
                   extra_root_certs_file,
                   buf);
      FreeCertificates(&g_extra_root_certs);
    }
  });
  return g_extra_root_certs;
}

std::vector<X509*>& GetSystemRootCertificatesParsed() {
  std::call_once(g_system_root_certs_once, []() {
    // Edge does not yet load platform trust stores natively.
  });
  return g_system_root_certs;
}

napi_value CreateStringArray(napi_env env, const std::vector<std::string>& values) {
  napi_value out = nullptr;
  if (napi_create_array_with_length(env, values.size(), &out) != napi_ok || out == nullptr) return nullptr;
  for (size_t i = 0; i < values.size(); ++i) {
    napi_value entry = nullptr;
    if (napi_create_string_utf8(env, values[i].c_str(), values[i].size(), &entry) == napi_ok && entry != nullptr) {
      napi_set_element(env, out, static_cast<uint32_t>(i), entry);
    }
  }
  return out;
}

napi_value CreatePemArrayFromX509Vector(napi_env env, const std::vector<X509*>& certs) {
  std::vector<std::string> pems;
  pems.reserve(certs.size());
  for (X509* cert : certs) {
    if (cert == nullptr) continue;
    AppendUniquePemString(&pems, X509ToPemString(cert));
  }
  return CreateStringArray(env, pems);
}

bool GetEffectiveCaOptions(napi_env env, bool* use_openssl_ca, bool* use_system_ca) {
  if (use_openssl_ca == nullptr || use_system_ca == nullptr) return false;
#ifdef NODE_OPENSSL_CERT_STORE
  *use_openssl_ca = true;
#else
  *use_openssl_ca = false;
#endif
  *use_system_ca = false;

  napi_value global = nullptr;
  napi_value process = nullptr;
  if (napi_get_global(env, &global) != napi_ok || global == nullptr ||
      napi_get_named_property(env, global, "process", &process) != napi_ok || process == nullptr) {
    return false;
  }

  std::vector<std::string> raw_exec_argv = GetStringArrayProperty(env, process, "execArgv");
  const edge_options::EffectiveCliState state = edge_options::BuildEffectiveCliState(raw_exec_argv);
  const std::vector<std::string>& tokens = state.ok ? state.effective_tokens : raw_exec_argv;

  const char* env_use_system_ca = std::getenv("NODE_USE_SYSTEM_CA");
  *use_system_ca = env_use_system_ca != nullptr && std::strcmp(env_use_system_ca, "1") == 0;

  for (const auto& token : tokens) {
    if (token == "--use-openssl-ca") {
      *use_openssl_ca = true;
      continue;
    }
    if (token == "--use-bundled-ca") {
      *use_openssl_ca = false;
      continue;
    }
    if (token == "--use-system-ca") {
      *use_system_ca = true;
      continue;
    }
    if (token == "--no-use-system-ca") {
      *use_system_ca = false;
      continue;
    }
  }
  return true;
}

bool AddRootCertsToContextStore(napi_env env, SSL_CTX* ctx) {
  if (ctx == nullptr) return false;
  X509_STORE* store = SSL_CTX_get_cert_store(ctx);
  if (store == nullptr) return false;

  std::lock_guard<std::mutex> lock(g_user_root_certs_mutex);
  if (g_user_root_certs != nullptr) {
    for (X509* cert : *g_user_root_certs) {
      if (!AddCertificateToStore(store, cert)) return false;
    }
    return true;
  }

  bool use_openssl_ca = false;
  bool use_system_ca = false;
  (void)GetEffectiveCaOptions(env, &use_openssl_ca, &use_system_ca);

  if (use_openssl_ca) {
    if (X509_STORE_set_default_paths(store) != 1) return false;
  } else {
    for (X509* cert : GetBundledRootCertificatesParsed()) {
      if (!AddCertificateToStore(store, cert)) return false;
    }
    if (use_system_ca) {
      for (X509* cert : GetSystemRootCertificatesParsed()) {
        if (!AddCertificateToStore(store, cert)) return false;
      }
    }
  }

  for (X509* cert : GetExtraRootCertificatesParsed()) {
    if (!AddCertificateToStore(store, cert)) return false;
  }
  return true;
}

void LoadCACertificatesThread(void* data) {
  const RootCertLoadPlan plan = *static_cast<RootCertLoadPlan*>(data);
  if (plan.load_bundled) {
    DebugCryptoLog("Started loading bundled root certificates off-thread\n");
    (void)GetBundledRootCertificatesParsed();
  }
  if (plan.load_extra) {
    DebugCryptoLog("Started loading extra root certificates off-thread\n");
    (void)GetExtraRootCertificatesParsed();
  }
  if (plan.load_system) {
    DebugCryptoLog("Started loading system root certificates off-thread\n");
    (void)GetSystemRootCertificatesParsed();
  }
}

void CleanupRootCertLoading(void* data) {
  (void)data;
  std::lock_guard<std::mutex> lock(g_cert_loading_thread_mutex);
  if (g_cert_loading_thread_started.exchange(false)) {
    uv_thread_join(&g_cert_loading_thread);
  }
}

void EnsureRootCertThreadCleanupHook(napi_env env) {
  if (env == nullptr) return;
  std::call_once(g_cert_loading_cleanup_once, []() {
    std::atexit([]() { CleanupRootCertLoading(nullptr); });
  });
}

bool GetAnyBufferSourceBytesImpl(napi_env env, napi_value value, uint8_t** data, size_t* len) {
  static uint8_t kEmptyBufferSentinel = 0;
  if (GetBufferBytes(env, value, data, len)) return true;

  bool is_arraybuffer = false;
  if (napi_is_arraybuffer(env, value, &is_arraybuffer) == napi_ok && is_arraybuffer) {
    void* raw = nullptr;
    size_t byte_len = 0;
    if (napi_get_arraybuffer_info(env, value, &raw, &byte_len) != napi_ok) return false;
    if (raw == nullptr && byte_len != 0) return false;
    *data = raw != nullptr ? static_cast<uint8_t*>(raw) : &kEmptyBufferSentinel;
    *len = byte_len;
    return true;
  }

  bool is_sharedarraybuffer = false;
  if (node_api_is_sharedarraybuffer(env, value, &is_sharedarraybuffer) == napi_ok && is_sharedarraybuffer) {
    void* raw = nullptr;
    size_t byte_len = 0;
    if (napi_get_arraybuffer_info(env, value, &raw, &byte_len) != napi_ok) return false;
    if (raw == nullptr && byte_len != 0) return false;
    *data = raw != nullptr ? static_cast<uint8_t*>(raw) : &kEmptyBufferSentinel;
    *len = byte_len;
    return true;
  }

  bool is_typedarray = false;
  if (napi_is_typedarray(env, value, &is_typedarray) == napi_ok && is_typedarray) {
    napi_typedarray_type ta_type = napi_uint8_array;
    size_t element_len = 0;
    void* raw = nullptr;
    napi_value ab = nullptr;
    size_t byte_offset = 0;
    if (napi_get_typedarray_info(env, value, &ta_type, &element_len, &raw, &ab, &byte_offset) != napi_ok) {
      return false;
    }
    const size_t byte_len = element_len * TypedArrayBytesPerElement(ta_type);
    if (raw == nullptr && byte_len != 0) return false;
    *data = raw != nullptr ? static_cast<uint8_t*>(raw) : &kEmptyBufferSentinel;
    *len = byte_len;
    return true;
  }

  bool is_dataview = false;
  if (napi_is_dataview(env, value, &is_dataview) == napi_ok && is_dataview) {
    size_t byte_len = 0;
    void* raw = nullptr;
    napi_value ab = nullptr;
    size_t byte_offset = 0;
    if (napi_get_dataview_info(env, value, &byte_len, &raw, &ab, &byte_offset) != napi_ok) return false;
    if (raw == nullptr && byte_len != 0) return false;
    *data = raw != nullptr ? static_cast<uint8_t*>(raw) : &kEmptyBufferSentinel;
    *len = byte_len;
    return true;
  }

  return false;
}

napi_value MakeError(napi_env env, const char* code, const char* message) {
  enum class ErrorCtorKind {
    kError,
    kTypeError,
    kRangeError,
  };
  auto classify = [](const char* err_code) -> ErrorCtorKind {
    if (err_code == nullptr) return ErrorCtorKind::kError;
    if (std::strcmp(err_code, "ERR_INVALID_ARG_TYPE") == 0 ||
        std::strcmp(err_code, "ERR_INVALID_ARG_VALUE") == 0 ||
        std::strcmp(err_code, "ERR_CRYPTO_INVALID_DIGEST") == 0 ||
        std::strcmp(err_code, "ERR_CRYPTO_INVALID_KEY_OBJECT_TYPE") == 0 ||
        std::strcmp(err_code, "ERR_CRYPTO_INVALID_AUTH_TAG") == 0) {
      return ErrorCtorKind::kTypeError;
    }
    if (std::strcmp(err_code, "ERR_OUT_OF_RANGE") == 0 ||
        std::strcmp(err_code, "ERR_CRYPTO_INVALID_KEYLEN") == 0) {
      return ErrorCtorKind::kRangeError;
    }
    return ErrorCtorKind::kError;
  };

  napi_value code_v = nullptr;
  napi_value msg_v = nullptr;
  napi_value err_v = nullptr;
  if (code != nullptr) {
    napi_create_string_utf8(env, code, NAPI_AUTO_LENGTH, &code_v);
  }
  napi_create_string_utf8(env, message, NAPI_AUTO_LENGTH, &msg_v);
  switch (classify(code)) {
    case ErrorCtorKind::kTypeError:
      napi_create_type_error(env, code_v, msg_v, &err_v);
      break;
    case ErrorCtorKind::kRangeError:
      napi_create_range_error(env, code_v, msg_v, &err_v);
      break;
    default:
      napi_create_error(env, code_v, msg_v, &err_v);
      break;
  }
  if (err_v != nullptr && code_v != nullptr) napi_set_named_property(env, err_v, "code", code_v);
  return err_v;
}

void ThrowError(napi_env env, const char* code, const char* message);

void SetOptionalErrorStringProperty(napi_env env, napi_value err, const char* name, const char* value) {
  if (err == nullptr || name == nullptr || value == nullptr || value[0] == '\0') return;
  napi_value v = nullptr;
  if (napi_create_string_utf8(env, value, NAPI_AUTO_LENGTH, &v) != napi_ok || v == nullptr) return;

  napi_value global = nullptr;
  napi_value reflect = nullptr;
  napi_value set_fn = nullptr;
  napi_valuetype set_fn_type = napi_undefined;
  napi_value key = nullptr;
  if (napi_get_global(env, &global) == napi_ok &&
      global != nullptr &&
      napi_get_named_property(env, global, "Reflect", &reflect) == napi_ok &&
      reflect != nullptr &&
      napi_get_named_property(env, reflect, "set", &set_fn) == napi_ok &&
      set_fn != nullptr &&
      napi_typeof(env, set_fn, &set_fn_type) == napi_ok &&
      set_fn_type == napi_function &&
      napi_create_string_utf8(env, name, NAPI_AUTO_LENGTH, &key) == napi_ok &&
      key != nullptr) {
    napi_value argv[3] = {err, key, v};
    napi_value ignored = nullptr;
    if (napi_call_function(env, reflect, set_fn, 3, argv, &ignored) == napi_ok) {
      return;
    }
  }

  napi_set_named_property(env, err, name, v);
}

void ThrowOpenSslError(napi_env env, const char* code, unsigned long err, const char* fallback_message) {
  if (err == 0) {
    ThrowError(env, code, fallback_message);
    return;
  }

  const char* effective_code = code;
  const char* reason = ERR_reason_error_string(err);
  if (effective_code == nullptr) {
    if (ERR_GET_LIB(err) == ERR_LIB_PEM && reason != nullptr) {
      if (std::strstr(reason, "ASN1 lib") != nullptr) {
        effective_code = "ERR_OSSL_PEM_ASN1_LIB";
      } else if (std::strstr(reason, "no start line") != nullptr) {
        effective_code = "ERR_OSSL_PEM_NO_START_LINE";
      }
    }
    if (effective_code == nullptr) {
      effective_code = "ERR_CRYPTO_OPERATION_FAILED";
    }
  }
  if (effective_code != nullptr &&
      std::strcmp(effective_code, "ERR_CRYPTO_OPERATION_FAILED") == 0 &&
      reason != nullptr &&
      std::strstr(reason, "invalid digest") != nullptr) {
    effective_code = "ERR_OSSL_INVALID_DIGEST";
  }
  if (effective_code != nullptr &&
      std::strcmp(effective_code, "ERR_CRYPTO_OPERATION_FAILED") == 0 &&
      reason != nullptr &&
      std::strstr(reason, "pss saltlen too small") != nullptr) {
    effective_code = "ERR_OSSL_PSS_SALTLEN_TOO_SMALL";
  }
  if (effective_code != nullptr &&
      std::strcmp(effective_code, "ERR_CRYPTO_OPERATION_FAILED") == 0 &&
      reason != nullptr &&
      std::strstr(reason, "bad decrypt") != nullptr) {
    effective_code = "ERR_OSSL_BAD_DECRYPT";
  }
  if (effective_code != nullptr &&
      std::strcmp(effective_code, "ERR_CRYPTO_OPERATION_FAILED") == 0 &&
      reason != nullptr &&
      std::strstr(reason, "operation not supported for this keytype") != nullptr) {
    effective_code = "ERR_OSSL_EVP_OPERATION_NOT_SUPPORTED_FOR_THIS_KEYTYPE";
  }
  if (effective_code != nullptr &&
      std::strcmp(effective_code, "ERR_CRYPTO_OPERATION_FAILED") == 0 &&
      reason != nullptr &&
      std::strstr(reason, "wrong final block length") != nullptr) {
    effective_code = "ERR_OSSL_WRONG_FINAL_BLOCK_LENGTH";
  }
  if (effective_code != nullptr &&
      std::strcmp(effective_code, "ERR_CRYPTO_OPERATION_FAILED") == 0 &&
      reason != nullptr &&
      std::strstr(reason, "data not multiple of block length") != nullptr) {
    effective_code = "ERR_OSSL_DATA_NOT_MULTIPLE_OF_BLOCK_LENGTH";
  }
  if (effective_code != nullptr &&
      std::strcmp(effective_code, "ERR_CRYPTO_OPERATION_FAILED") == 0 &&
      reason != nullptr &&
      std::strstr(reason, "oaep decoding error") != nullptr) {
    effective_code = "ERR_OSSL_RSA_OAEP_DECODING_ERROR";
  }
  if (effective_code != nullptr &&
      std::strcmp(effective_code, "ERR_CRYPTO_OPERATION_FAILED") == 0 &&
      reason != nullptr &&
      std::strstr(reason, "digest too big for rsa key") != nullptr) {
    effective_code = "ERR_OSSL_RSA_DIGEST_TOO_BIG_FOR_RSA_KEY";
  }
  if (effective_code != nullptr &&
      std::strcmp(effective_code, "ERR_CRYPTO_OPERATION_FAILED") == 0 &&
      reason != nullptr &&
      std::strstr(reason, "bignum too long") != nullptr) {
    effective_code = "ERR_OSSL_BN_BIGNUM_TOO_LONG";
  }
  if (effective_code != nullptr &&
      std::strcmp(effective_code, "ERR_CRYPTO_OPERATION_FAILED") == 0 &&
      reason != nullptr &&
      std::strstr(reason, "illegal or unsupported padding mode") != nullptr) {
#if OPENSSL_VERSION_NUMBER >= 0x30000000L
    effective_code = "ERR_OSSL_ILLEGAL_OR_UNSUPPORTED_PADDING_MODE";
#else
    effective_code = "ERR_OSSL_RSA_ILLEGAL_OR_UNSUPPORTED_PADDING_MODE";
#endif
  }
  if (effective_code != nullptr &&
      std::strcmp(effective_code, "ERR_CRYPTO_OPERATION_FAILED") == 0 &&
      reason != nullptr &&
      std::strstr(reason, "interrupted or cancelled") != nullptr) {
    effective_code = "ERR_OSSL_CRYPTO_INTERRUPTED_OR_CANCELLED";
  }

  char buf[256];
  ERR_error_string_n(err, buf, sizeof(buf));
  napi_value error = MakeError(env, effective_code, buf);
  SetOptionalErrorStringProperty(env, error, "library", ERR_lib_error_string(err));
#if OPENSSL_VERSION_NUMBER < 0x30000000L
  SetOptionalErrorStringProperty(env, error, "function", ERR_func_error_string(err));
#endif
  SetOptionalErrorStringProperty(env, error, "reason", reason);
  bool pending = false;
  if (napi_is_exception_pending(env, &pending) == napi_ok && pending) {
    return;
  }
  if (error != nullptr) {
    napi_throw(env, error);
  } else {
    ThrowError(env, code, buf);
  }
}

void ThrowError(napi_env env, const char* code, const char* message) {
  napi_value err = MakeError(env, code, message);
  if (err != nullptr) napi_throw(env, err);
}

unsigned long ConsumeOpenSslError() {
  const unsigned long first = ERR_get_error();
  while (ERR_get_error() != 0) {
  }
  return first;
}

void ThrowLastOpenSslError(napi_env env, const char* fallback_code, const char* fallback_message) {
  ThrowOpenSslError(env, fallback_code, ConsumeOpenSslError(), fallback_message);
}

ncrypto::Digest ResolveDigest(const std::string& name) {
  if (name == "RSA-SHA1") return ncrypto::Digest::SHA1;
  return ncrypto::Digest::FromName(name.c_str());
}

ncrypto::Cipher ResolveCipher(const std::string& name) {
  return ncrypto::Cipher::FromName(name.c_str());
}

std::string CanonicalizeDigestName(const std::string& in) {
  std::string out;
  out.reserve(in.size());
  for (char ch : in) {
    if (ch == '-') continue;
    out.push_back(static_cast<char>(std::tolower(static_cast<unsigned char>(ch))));
  }
  return out;
}

void SecureContextFinalizer(node_api_basic_env env, void* data, void* hint) {
  (void)env;
  (void)hint;
  auto* holder = reinterpret_cast<SecureContextHolder*>(data);
  delete holder;
}

void ResetStoredCertificate(X509** slot, X509* cert) {
  if (slot == nullptr) return;
  if (*slot != nullptr) {
    X509_free(*slot);
    *slot = nullptr;
  }
  if (cert != nullptr) {
    *slot = X509_dup(cert);
  }
}

void UpdateIssuerFromStore(SecureContextHolder* holder) {
  if (holder == nullptr || holder->ctx == nullptr || holder->cert == nullptr) return;

  ResetStoredCertificate(&holder->issuer, nullptr);

  X509_STORE* store = SSL_CTX_get_cert_store(holder->ctx);
  if (store == nullptr) return;

#if OPENSSL_VERSION_NUMBER >= 0x10002000L
  X509_STORE_CTX* store_ctx = X509_STORE_CTX_new();
  if (store_ctx == nullptr) return;
  if (X509_STORE_CTX_init(store_ctx, store, holder->cert, nullptr) != 1) {
    X509_STORE_CTX_free(store_ctx);
    return;
  }

  X509* issuer = nullptr;
  if (X509_STORE_CTX_get1_issuer(&issuer, store_ctx, holder->cert) == 1 && issuer != nullptr) {
    ResetStoredCertificate(&holder->issuer, issuer);
    X509_free(issuer);
  }
  X509_STORE_CTX_free(store_ctx);
#endif
}

int UseCertificateChain(SecureContextHolder* holder, const uint8_t* data, size_t len) {
  if (holder == nullptr || holder->ctx == nullptr || data == nullptr || len == 0) return 0;

  BIO* bio = BIO_new_mem_buf(data, static_cast<int>(len));
  if (bio == nullptr) return 0;

  ncrypto::X509Pointer cert(PEM_read_bio_X509_AUX(bio, nullptr, nullptr, nullptr));
  if (!cert) {
    BIO_free(bio);
    return 0;
  }

  ncrypto::StackOfX509 extra_certs(sk_X509_new_null());
  if (!extra_certs) {
    BIO_free(bio);
    return 0;
  }

  ncrypto::X509Pointer extra;
  while ((extra = ncrypto::X509Pointer(PEM_read_bio_X509(bio, nullptr, nullptr, nullptr)))) {
    if (!sk_X509_push(extra_certs.get(), extra.get())) {
      BIO_free(bio);
      return 0;
    }
    extra.release();
  }

  BIO_free(bio);

  const unsigned long err = ERR_peek_last_error();
  if (err != 0) {
    if (ERR_GET_LIB(err) == ERR_LIB_PEM &&
        ERR_GET_REASON(err) == PEM_R_NO_START_LINE) {
      ERR_clear_error();
    } else {
      return 0;
    }
  }

  X509* issuer = nullptr;
  int ok = SSL_CTX_use_certificate(holder->ctx, cert.get());
  if (ok == 1) {
    SSL_CTX_clear_extra_chain_certs(holder->ctx);

    const int count = sk_X509_num(extra_certs.get());
    for (int i = 0; i < count; ++i) {
      X509* ca = sk_X509_value(extra_certs.get(), i);
      if (ca == nullptr) continue;
      if (!SSL_CTX_add1_chain_cert(holder->ctx, ca)) {
        ok = 0;
        issuer = nullptr;
        break;
      }
      if (issuer == nullptr && X509_check_issued(ca, cert.get()) == X509_V_OK) {
        issuer = ca;
      }
    }
  }

  if (ok == 1) {
    ResetStoredCertificate(&holder->cert, cert.get());
    if (issuer != nullptr) {
      ResetStoredCertificate(&holder->issuer, issuer);
    } else {
      UpdateIssuerFromStore(holder);
    }
  }

  return ok;
}

X509* ParseX509(const uint8_t* data, size_t len);
X509_CRL* ParseX509Crl(const uint8_t* data, size_t len);

napi_value CreateBufferCopy(napi_env env, const uint8_t* data, size_t len) {
  napi_value out = nullptr;
  void* copied = nullptr;
  if (napi_create_buffer_copy(env, len, len > 0 ? data : nullptr, &copied, &out) != napi_ok || out == nullptr) {
    return nullptr;
  }
  return out;
}

napi_value CreateBufferCopy(napi_env env, const ncrypto::DataPointer& dp) {
  return CreateBufferCopy(env, dp.get<uint8_t>(), dp.size());
}

napi_value CreateX509DerBuffer(napi_env env, X509* cert) {
  if (cert == nullptr) return nullptr;
  const int size = i2d_X509(cert, nullptr);
  if (size <= 0) return nullptr;
  std::vector<uint8_t> out(static_cast<size_t>(size));
  unsigned char* write_ptr = out.data();
  if (i2d_X509(cert, &write_ptr) != size) return nullptr;
  return CreateBufferCopy(env, out.data(), out.size());
}

napi_value CryptoHashOneShot(napi_env env, napi_callback_info info) {
  size_t argc = 2;
  napi_value argv[2] = {nullptr, nullptr};
  if (napi_get_cb_info(env, info, &argc, argv, nullptr, nullptr) != napi_ok || argc < 2) return nullptr;
  const std::string algo = ValueToUtf8(env, argv[0]);
  uint8_t* in = nullptr;
  size_t in_len = 0;
  if (!GetBufferBytes(env, argv[1], &in, &in_len)) {
    ThrowError(env, "ERR_INVALID_ARG_TYPE", "hash data must be a Buffer");
    return nullptr;
  }
  const ncrypto::Digest md = ResolveDigest(algo);
  if (!md) {
    ThrowError(env, "ERR_CRYPTO_HASH_UNKNOWN", "Unknown hash");
    return nullptr;
  }
  auto out = ncrypto::hashDigest({in, in_len}, md.get());
  if (!out) {
    const std::string canonical = CanonicalizeDigestName(algo);
    size_t default_xof_len = 0;
    if (canonical == "shake128") {
      default_xof_len = 16;
    } else if (canonical == "shake256") {
      default_xof_len = 32;
    }
    if (default_xof_len > 0) {
      out = ncrypto::xofHashDigest({in, in_len}, md.get(), default_xof_len);
    }
  }
  if (!out) {
    ThrowError(env, "ERR_CRYPTO_OPERATION_FAILED", "Hash operation failed");
    return nullptr;
  }
  return CreateBufferCopy(env, out);
}

napi_value CryptoHashOneShotXof(napi_env env, napi_callback_info info) {
  size_t argc = 3;
  napi_value argv[3] = {nullptr, nullptr, nullptr};
  if (napi_get_cb_info(env, info, &argc, argv, nullptr, nullptr) != napi_ok || argc < 3) return nullptr;
  const std::string algo = ValueToUtf8(env, argv[0]);
  uint8_t* in = nullptr;
  size_t in_len = 0;
  if (!GetBufferBytes(env, argv[1], &in, &in_len)) {
    ThrowError(env, "ERR_INVALID_ARG_TYPE", "hash data must be a Buffer");
    return nullptr;
  }
  int32_t out_len_i32 = 0;
  if (napi_get_value_int32(env, argv[2], &out_len_i32) != napi_ok || out_len_i32 < 0) {
    ThrowError(env, "ERR_INVALID_ARG_VALUE", "Invalid output length");
    return nullptr;
  }
  const ncrypto::Digest md = ResolveDigest(algo);
  if (!md) {
    ThrowError(env, "ERR_CRYPTO_HASH_UNKNOWN", "Unknown hash");
    return nullptr;
  }
  const bool is_xof = (EVP_MD_flags(md.get()) & EVP_MD_FLAG_XOF) != 0;
  if (!is_xof) {
    const size_t digest_size = md.size();
    if (static_cast<size_t>(out_len_i32) != digest_size) {
      const std::string message =
          "Output length " + std::to_string(out_len_i32) + " is invalid for " + algo +
          ", which does not support XOF";
      ThrowError(env, "ERR_CRYPTO_OPERATION_FAILED", message.c_str());
      return nullptr;
    }
    auto out = ncrypto::hashDigest({in, in_len}, md.get());
    if (!out) {
      ThrowError(env, "ERR_CRYPTO_OPERATION_FAILED", "Hash operation failed");
      return nullptr;
    }
    return CreateBufferCopy(env, out);
  }
  if (out_len_i32 == 0) {
    return CreateBufferCopy(env, nullptr, 0);
  }

  auto out = ncrypto::xofHashDigest({in, in_len}, md.get(), static_cast<size_t>(out_len_i32));
  if (!out) {
    ThrowError(env, "ERR_CRYPTO_OPERATION_FAILED", "Hash operation failed");
    return nullptr;
  }
  return CreateBufferCopy(env, out);
}

napi_value CryptoHmacOneShot(napi_env env, napi_callback_info info) {
  size_t argc = 3;
  napi_value argv[3] = {nullptr, nullptr, nullptr};
  if (napi_get_cb_info(env, info, &argc, argv, nullptr, nullptr) != napi_ok || argc < 3) return nullptr;
  const std::string algo = ValueToUtf8(env, argv[0]);
  uint8_t* key = nullptr;
  size_t key_len = 0;
  uint8_t* in = nullptr;
  size_t in_len = 0;
  if (!GetBufferBytes(env, argv[1], &key, &key_len) || !GetBufferBytes(env, argv[2], &in, &in_len)) {
    ThrowError(env, "ERR_INVALID_ARG_TYPE", "hmac key/data must be Buffers");
    return nullptr;
  }
  const ncrypto::Digest md = ResolveDigest(algo);
  if (!md) {
    const std::string message = "Invalid digest: " + algo;
    ThrowError(env, "ERR_CRYPTO_INVALID_DIGEST", message.c_str());
    return nullptr;
  }
  auto hmac = ncrypto::HMACCtxPointer::New();
  if (!hmac || !hmac.init({key, key_len}, md) || !hmac.update({in, in_len})) {
    ThrowError(env, "ERR_CRYPTO_OPERATION_FAILED", "HMAC operation failed");
    return nullptr;
  }
  auto out = hmac.digest();
  if (!out) {
    ThrowError(env, "ERR_CRYPTO_OPERATION_FAILED", "HMAC operation failed");
    return nullptr;
  }
  return CreateBufferCopy(env, out);
}

napi_value CryptoRandomFillSync(napi_env env, napi_callback_info info) {
  size_t argc = 3;
  napi_value argv[3] = {nullptr, nullptr, nullptr};
  if (napi_get_cb_info(env, info, &argc, argv, nullptr, nullptr) != napi_ok || argc < 1) return nullptr;
  uint8_t* data = nullptr;
  size_t len = 0;
  if (!GetAnyBufferSourceBytesImpl(env, argv[0], &data, &len)) {
    ThrowError(env, "ERR_INVALID_ARG_TYPE", "buffer must be an ArrayBuffer or ArrayBufferView");
    return nullptr;
  }
  int32_t offset = 0;
  int32_t size = static_cast<int32_t>(len);
  if (argc >= 2 && argv[1] != nullptr) napi_get_value_int32(env, argv[1], &offset);
  if (argc >= 3 && argv[2] != nullptr) napi_get_value_int32(env, argv[2], &size);
  if (offset < 0 || size < 0 || static_cast<size_t>(offset + size) > len) {
    ThrowError(env, "ERR_OUT_OF_RANGE", "offset/size out of range");
    return nullptr;
  }
  if (!ncrypto::CSPRNG(data + offset, static_cast<size_t>(size))) {
    ThrowError(env, "ERR_CRYPTO_OPERATION_FAILED", "CSPRNG failed");
    return nullptr;
  }
  return argv[0];
}

napi_value CryptoRandomBytes(napi_env env, napi_callback_info info) {
  size_t argc = 1;
  napi_value argv[1] = {nullptr};
  if (napi_get_cb_info(env, info, &argc, argv, nullptr, nullptr) != napi_ok || argc < 1) return nullptr;
  int32_t n = 0;
  if (napi_get_value_int32(env, argv[0], &n) != napi_ok || n < 0) {
    ThrowError(env, "ERR_INVALID_ARG_TYPE", "size must be a number >= 0");
    return nullptr;
  }
  napi_value out = nullptr;
  napi_value ab = nullptr;
  void* out_data_raw = nullptr;
  if (napi_create_arraybuffer(env, static_cast<size_t>(n), &out_data_raw, &ab) != napi_ok || ab == nullptr) {
    return nullptr;
  }
  auto* out_data = reinterpret_cast<uint8_t*>(out_data_raw);
  if (n > 0 && !ncrypto::CSPRNG(out_data, static_cast<size_t>(n))) {
    ThrowError(env, "ERR_CRYPTO_OPERATION_FAILED", "CSPRNG failed");
    return nullptr;
  }
  if (napi_create_typedarray(env, napi_uint8_array, static_cast<size_t>(n), ab, 0, &out) != napi_ok) return nullptr;
  return out;
}

napi_value CryptoPbkdf2Sync(napi_env env, napi_callback_info info) {
  size_t argc = 5;
  napi_value argv[5] = {nullptr, nullptr, nullptr, nullptr, nullptr};
  if (napi_get_cb_info(env, info, &argc, argv, nullptr, nullptr) != napi_ok || argc < 5) return nullptr;
  uint8_t* pass = nullptr;
  size_t pass_len = 0;
  uint8_t* salt = nullptr;
  size_t salt_len = 0;
  if (!GetBufferBytes(env, argv[0], &pass, &pass_len) || !GetBufferBytes(env, argv[1], &salt, &salt_len)) {
    ThrowError(env, "ERR_INVALID_ARG_TYPE", "password/salt must be Buffers");
    return nullptr;
  }
  int32_t iter = 0;
  int32_t keylen = 0;
  napi_get_value_int32(env, argv[2], &iter);
  napi_get_value_int32(env, argv[3], &keylen);
  const std::string digest = ValueToUtf8(env, argv[4]);
  const ncrypto::Digest md = ResolveDigest(digest);
  if (!md) {
    const std::string message = "Invalid digest: " + digest;
    ThrowError(env, "ERR_CRYPTO_INVALID_DIGEST", message.c_str());
    return nullptr;
  }
  if (iter <= 0 || keylen < 0) {
    ThrowError(env, "ERR_INVALID_ARG_VALUE", "Invalid pbkdf2 arguments");
    return nullptr;
  }
  auto out = ncrypto::pbkdf2(md, {reinterpret_cast<char*>(pass), pass_len}, {salt, salt_len},
                             static_cast<uint32_t>(iter), static_cast<size_t>(keylen));
  if (!out) {
    ThrowError(env, "ERR_CRYPTO_OPERATION_FAILED", "PBKDF2 failed");
    return nullptr;
  }
  return CreateBufferCopy(env, out);
}

napi_value CryptoScryptSync(napi_env env, napi_callback_info info) {
  size_t argc = 7;
  napi_value argv[7] = {nullptr, nullptr, nullptr, nullptr, nullptr, nullptr, nullptr};
  if (napi_get_cb_info(env, info, &argc, argv, nullptr, nullptr) != napi_ok || argc < 3) return nullptr;
  uint8_t* pass = nullptr;
  size_t pass_len = 0;
  uint8_t* salt = nullptr;
  size_t salt_len = 0;
  if (!GetBufferBytes(env, argv[0], &pass, &pass_len) || !GetBufferBytes(env, argv[1], &salt, &salt_len)) {
    ThrowError(env, "ERR_INVALID_ARG_TYPE", "password/salt must be Buffers");
    return nullptr;
  }
  int32_t keylen = 0;
  napi_get_value_int32(env, argv[2], &keylen);
  if (keylen < 0) {
    ThrowError(env, "ERR_INVALID_ARG_VALUE", "Invalid key length");
    return nullptr;
  }
  uint64_t N = 16384;
  uint64_t r = 8;
  uint64_t p = 1;
  uint64_t maxmem = 0;
  if (argc >= 4 && argv[3] != nullptr) {
    double v = 0;
    if (napi_get_value_double(env, argv[3], &v) == napi_ok && v > 0) N = static_cast<uint64_t>(v);
  }
  if (argc >= 5 && argv[4] != nullptr) {
    double v = 0;
    if (napi_get_value_double(env, argv[4], &v) == napi_ok && v > 0) r = static_cast<uint64_t>(v);
  }
  if (argc >= 6 && argv[5] != nullptr) {
    double v = 0;
    if (napi_get_value_double(env, argv[5], &v) == napi_ok && v > 0) p = static_cast<uint64_t>(v);
  }
  if (argc >= 7 && argv[6] != nullptr) {
    double v = 0;
    if (napi_get_value_double(env, argv[6], &v) == napi_ok && v > 0) maxmem = static_cast<uint64_t>(v);
  }
  if (!ncrypto::checkScryptParams(N, r, p, maxmem)) {
    ThrowError(env, "ERR_CRYPTO_INVALID_SCRYPT_PARAMS", "Invalid scrypt params: memory limit exceeded");
    return nullptr;
  }
  auto out = ncrypto::scrypt({reinterpret_cast<char*>(pass), pass_len}, {salt, salt_len},
                             N, r, p, maxmem, static_cast<size_t>(keylen));
  if (!out) {
    // Keep behavior aligned with previous bridge for platforms/OpenSSL builds
    // where ncrypto::scrypt can reject params that EVP_PBE_scrypt accepts.
    std::vector<uint8_t> fallback(static_cast<size_t>(keylen));
    if (EVP_PBE_scrypt(reinterpret_cast<const char*>(pass), pass_len,
                       salt, salt_len, N, r, p, maxmem, fallback.data(), fallback.size()) != 1) {
      ThrowLastOpenSslError(env, "ERR_CRYPTO_INVALID_SCRYPT_PARAMS", "Invalid scrypt params: memory limit exceeded");
      return nullptr;
    }
    return CreateBufferCopy(env, fallback.data(), fallback.size());
  }
  return CreateBufferCopy(env, out);
}

napi_value CryptoHkdfSync(napi_env env, napi_callback_info info) {
  size_t argc = 5;
  napi_value argv[5] = {nullptr, nullptr, nullptr, nullptr, nullptr};
  if (napi_get_cb_info(env, info, &argc, argv, nullptr, nullptr) != napi_ok || argc < 5) return nullptr;
  const std::string digest = ValueToUtf8(env, argv[0]);
  std::vector<uint8_t> key_owned;
  uint8_t *ikm = nullptr, *salt = nullptr, *info_bytes = nullptr;
  size_t ikm_len = 0, salt_len = 0, info_len = 0;
  if (!GetKeyBytes(env, argv[1], &key_owned, &ikm, &ikm_len) ||
      !GetAnyBufferSourceBytesImpl(env, argv[2], &salt, &salt_len) ||
      !GetAnyBufferSourceBytesImpl(env, argv[3], &info_bytes, &info_len)) {
    ThrowError(env, "ERR_INVALID_ARG_TYPE", "hkdf input/salt/info must be Buffers or strings");
    return nullptr;
  }
  int32_t keylen = 0;
  napi_get_value_int32(env, argv[4], &keylen);
  if (keylen < 0) {
    ThrowError(env, "ERR_INVALID_ARG_VALUE", "Invalid key length");
    return nullptr;
  }
  const ncrypto::Digest md = ResolveDigest(digest);
  if (!md) {
    const std::string message = "Invalid digest: " + digest;
    ThrowError(env, "ERR_CRYPTO_INVALID_DIGEST", message.c_str());
    return nullptr;
  }
  if (!ncrypto::checkHkdfLength(md, static_cast<size_t>(keylen))) {
    ThrowError(env, "ERR_CRYPTO_INVALID_KEYLEN", "Invalid key length");
    return nullptr;
  }
  auto out = ncrypto::hkdf(md, {ikm, ikm_len}, {info_bytes, info_len}, {salt, salt_len},
                           static_cast<size_t>(keylen));
  if (!out) {
    ThrowError(env, "ERR_CRYPTO_OPERATION_FAILED", "hkdf failed");
    return nullptr;
  }
  return CreateBufferCopy(env, out);
}

napi_value CryptoCipherTransform(napi_env env, napi_callback_info info) {
  size_t argc = 7;
  napi_value argv[7] = {nullptr, nullptr, nullptr, nullptr, nullptr, nullptr, nullptr};
  if (napi_get_cb_info(env, info, &argc, argv, nullptr, nullptr) != napi_ok || argc < 6) return nullptr;
  const std::string algo = ValueToUtf8(env, argv[0]);
  uint8_t *key = nullptr, *iv = nullptr, *input = nullptr;
  size_t key_len = 0, iv_len = 0, in_len = 0;
  if (!GetBufferBytes(env, argv[1], &key, &key_len)) {
    ThrowError(env, "ERR_INVALID_ARG_TYPE", "key must be a Buffer");
    return nullptr;
  }

  bool iv_is_null = false;
  napi_valuetype iv_type = napi_undefined;
  if (napi_typeof(env, argv[2], &iv_type) == napi_ok && iv_type == napi_null) {
    iv_is_null = true;
  } else if (!GetBufferBytes(env, argv[2], &iv, &iv_len)) {
    ThrowError(env, "ERR_INVALID_ARG_TYPE", "iv must be a Buffer or null");
    return nullptr;
  }
  if (!GetBufferBytes(env, argv[3], &input, &in_len)) {
    ThrowError(env, "ERR_INVALID_ARG_TYPE", "input must be a Buffer");
    return nullptr;
  }
  bool decrypt = false;
  napi_get_value_bool(env, argv[4], &decrypt);
  bool auto_padding = true;
  if (argc >= 7 && argv[6] != nullptr) napi_get_value_bool(env, argv[6], &auto_padding);

  const ncrypto::Cipher cipher = ResolveCipher(algo);
  if (!cipher) {
    ThrowError(env, "ERR_CRYPTO_UNKNOWN_CIPHER", "Unknown cipher");
    return nullptr;
  }
  if (cipher.getKeyLength() > 0 && key_len != static_cast<size_t>(cipher.getKeyLength())) {
    ThrowError(env, "ERR_CRYPTO_INVALID_KEYLEN", "Invalid key length");
    return nullptr;
  }
  if (!iv_is_null && cipher.getIvLength() >= 0 && iv_len != static_cast<size_t>(cipher.getIvLength())) {
    ThrowError(env, "ERR_CRYPTO_INVALID_IV", "Invalid IV length");
    return nullptr;
  }

  auto ctx = ncrypto::CipherCtxPointer::New();
  if (!ctx || !ctx.init(cipher, !decrypt, key, iv_is_null ? nullptr : iv)) {
    ThrowLastOpenSslError(env, "ERR_CRYPTO_OPERATION_FAILED", "cipher initialization failed");
    return nullptr;
  }
  if (!ctx.setPadding(auto_padding)) {
    ThrowLastOpenSslError(env, "ERR_CRYPTO_OPERATION_FAILED", "cipher initialization failed");
    return nullptr;
  }

  std::vector<uint8_t> out(in_len + std::max(32, ctx.getBlockSize() + 16));
  int out1 = 0;
  int out2 = 0;
  if (!ctx.update({input, in_len}, out.data(), &out1, false) ||
      !ctx.update({nullptr, 0}, out.data() + out1, &out2, true)) {
    ThrowLastOpenSslError(env, "ERR_CRYPTO_OPERATION_FAILED", "cipher operation failed");
    return nullptr;
  }
  return CreateBufferCopy(env, out.data(), static_cast<size_t>(out1 + out2));
}

napi_value CryptoGetHashes(napi_env env, napi_callback_info info) {
  std::unordered_set<std::string> unique_hashes;
#if OPENSSL_VERSION_NUMBER >= 0x30000000L
  EVP_MD_do_all_provided(
      nullptr,
      [](EVP_MD* md, void* arg) {
        if (md == nullptr || arg == nullptr) return;
        const char* name = EVP_MD_get0_name(md);
        if (name == nullptr) return;
        auto* out = static_cast<std::unordered_set<std::string>*>(arg);
        const std::string raw_name(name);
        const std::string canonical = CanonicalizeDigestName(raw_name);
        // Keep only names we can resolve through the same digest path used by
        // hashing APIs, so getHashes() round-trips with createHash()/hash().
        if (!canonical.empty() && ResolveDigest(canonical)) {
          out->emplace(canonical);
        }
      },
      &unique_hashes);
#endif
  static const char* kFallbackCandidates[] = {
      "sha1",      "sha224",    "sha256",    "sha384",    "sha512",      "shake128",   "shake256",
      "md5",       "ripemd160", "sha3-224",  "sha3-256",  "sha3-384",    "sha3-512",   "blake2b512",
      "blake2s256"};
  for (const char* candidate : kFallbackCandidates) {
    if (ResolveDigest(candidate)) unique_hashes.emplace(candidate);
  }
  if (ResolveDigest("sha1")) unique_hashes.emplace("RSA-SHA1");

  std::vector<std::string> hashes(unique_hashes.begin(), unique_hashes.end());
  std::sort(hashes.begin(), hashes.end());
  napi_value arr = nullptr;
  napi_create_array_with_length(env, hashes.size(), &arr);
  for (uint32_t i = 0; i < hashes.size(); i++) {
    napi_value s = nullptr;
    napi_create_string_utf8(env, hashes[i].c_str(), NAPI_AUTO_LENGTH, &s);
    napi_set_element(env, arr, i, s);
  }
  return arr;
}

napi_value CryptoGetCiphers(napi_env env, napi_callback_info info) {
  std::vector<std::string> names;
  ncrypto::Cipher::ForEach([&names](const char* name) { names.emplace_back(name); });
  std::sort(names.begin(), names.end());
  names.erase(std::unique(names.begin(), names.end()), names.end());
  napi_value arr = nullptr;
  napi_create_array_with_length(env, names.size(), &arr);
  for (uint32_t i = 0; i < names.size(); i++) {
    napi_value s = nullptr;
    napi_create_string_utf8(env, names[i].c_str(), NAPI_AUTO_LENGTH, &s);
    napi_set_element(env, arr, i, s);
  }
  return arr;
}

napi_value CryptoGetSSLCiphers(napi_env env, napi_callback_info info) {
  (void)info;
  std::vector<std::string> names;
  ncrypto::SSLCtxPointer ctx = ncrypto::SSLCtxPointer::New();
  if (ctx) {
    ncrypto::SSLPointer ssl = ncrypto::SSLPointer::New(ctx);
    if (ssl) {
      ssl.getCiphers([&names](const char* name) {
        if (name != nullptr) names.emplace_back(name);
      });
    }
  }

  napi_value arr = nullptr;
  napi_create_array_with_length(env, names.size(), &arr);
  for (uint32_t i = 0; i < names.size(); i++) {
    napi_value s = nullptr;
    napi_create_string_utf8(env, names[i].c_str(), NAPI_AUTO_LENGTH, &s);
    napi_set_element(env, arr, i, s);
  }
  return arr;
}

napi_value CryptoGetCurves(napi_env env, napi_callback_info info) {
  std::vector<std::string> curves;
  ncrypto::Ec::GetCurves([&curves](const char* name) {
    if (name != nullptr) curves.emplace_back(name);
    return true;
  });
  std::sort(curves.begin(), curves.end());
  curves.erase(std::unique(curves.begin(), curves.end()), curves.end());
  napi_value arr = nullptr;
  napi_create_array_with_length(env, curves.size(), &arr);
  for (uint32_t i = 0; i < curves.size(); i++) {
    napi_value s = nullptr;
    napi_create_string_utf8(env, curves[i].c_str(), NAPI_AUTO_LENGTH, &s);
    napi_set_element(env, arr, i, s);
  }
  return arr;
}

napi_value CryptoGetCipherInfo(napi_env env, napi_callback_info info) {
  size_t argc = 4;
  napi_value argv[4] = {nullptr, nullptr, nullptr, nullptr};
  if (napi_get_cb_info(env, info, &argc, argv, nullptr, nullptr) != napi_ok || argc < 1) return nullptr;

  napi_value undefined = nullptr;
  napi_get_undefined(env, &undefined);

  // Node native signature:
  //   getCipherInfo(infoObject, nameOrNid, keyLength?, ivLength?)
  // Keep backward compatibility for legacy single-arg call by treating argv[0]
  // as the algorithm when no info object is provided.
  bool node_signature = false;
  if (argc >= 2 && argv[0] != nullptr && argv[1] != nullptr) {
    napi_valuetype info_t = napi_undefined;
    napi_valuetype alg_t = napi_undefined;
    const bool info_ok = napi_typeof(env, argv[0], &info_t) == napi_ok;
    const bool alg_ok = napi_typeof(env, argv[1], &alg_t) == napi_ok;
    node_signature = info_ok && alg_ok && info_t == napi_object &&
                     (alg_t == napi_string || alg_t == napi_number);
  }

  napi_value out = nullptr;
  if (node_signature) {
    out = argv[0];
  } else {
    napi_create_object(env, &out);
  }
  if (out == nullptr) return undefined;

  napi_value name_or_nid = node_signature ? argv[1] : argv[0];
  napi_valuetype alg_t = napi_undefined;
  if (name_or_nid == nullptr || napi_typeof(env, name_or_nid, &alg_t) != napi_ok) {
    return undefined;
  }
  const ncrypto::Cipher cipher = [&]() -> ncrypto::Cipher {
    if (alg_t == napi_string) {
      const std::string algo = ValueToUtf8(env, name_or_nid);
      return ResolveCipher(algo);
    }
    if (alg_t == napi_number) {
      int32_t nid = 0;
      if (napi_get_value_int32(env, name_or_nid, &nid) != napi_ok) return {};
      return ncrypto::Cipher::FromNid(nid);
    }
    return {};
  }();

  if (!cipher) return undefined;

  int iv_length = cipher.getIvLength();
  int key_length = cipher.getKeyLength();
  const int block_length = cipher.getBlockSize();

  if (node_signature) {
    const bool has_key_len =
        argc >= 3 && argv[2] != nullptr && [&]() {
          napi_valuetype t = napi_undefined;
          return napi_typeof(env, argv[2], &t) == napi_ok && t == napi_number;
        }();
    const bool has_iv_len =
        argc >= 4 && argv[3] != nullptr && [&]() {
          napi_valuetype t = napi_undefined;
          return napi_typeof(env, argv[3], &t) == napi_ok && t == napi_number;
        }();

    if (has_key_len || has_iv_len) {
      ncrypto::CipherCtxPointer ctx = ncrypto::CipherCtxPointer::New();
      if (!ctx || !ctx.init(cipher, true)) return undefined;

      if (has_key_len) {
        int32_t requested_key_len = 0;
        if (napi_get_value_int32(env, argv[2], &requested_key_len) != napi_ok) return undefined;
        if (!ctx.setKeyLength(static_cast<size_t>(requested_key_len))) return undefined;
        key_length = requested_key_len;
      }

      if (has_iv_len) {
        int32_t requested_iv_len = 0;
        if (napi_get_value_int32(env, argv[3], &requested_iv_len) != napi_ok) return undefined;
        if (cipher.isCcmMode()) {
          if (requested_iv_len < 7 || requested_iv_len > 13) return undefined;
        } else if (cipher.isGcmMode()) {
          // Node accepts any iv length for GCM here and defers validation.
        } else if (cipher.isOcbMode()) {
          if (!ctx.setIvLength(static_cast<size_t>(requested_iv_len))) return undefined;
        } else if (requested_iv_len != iv_length) {
          return undefined;
        }
        iv_length = requested_iv_len;
      }
    }
  }

  const std::string_view mode = cipher.getModeLabel();
  if (!mode.empty()) {
    napi_value mode_v = nullptr;
    napi_create_string_utf8(env, mode.data(), mode.size(), &mode_v);
    if (mode_v != nullptr) napi_set_named_property(env, out, "mode", mode_v);
  }

  if (const char* cipher_name = cipher.getName(); cipher_name != nullptr && cipher_name[0] != '\0') {
    napi_value name_v = nullptr;
    napi_create_string_utf8(env, cipher_name, NAPI_AUTO_LENGTH, &name_v);
    if (name_v != nullptr) napi_set_named_property(env, out, "name", name_v);
  }

  napi_value nid_v = nullptr;
  napi_create_int32(env, cipher.getNid(), &nid_v);
  if (nid_v != nullptr) napi_set_named_property(env, out, "nid", nid_v);

  if (!cipher.isStreamMode()) {
    napi_value block_v = nullptr;
    napi_create_int32(env, block_length, &block_v);
    if (block_v != nullptr) napi_set_named_property(env, out, "blockSize", block_v);
  }

  if (iv_length != 0) {
    napi_value iv_v = nullptr;
    napi_create_int32(env, iv_length, &iv_v);
    if (iv_v != nullptr) napi_set_named_property(env, out, "ivLength", iv_v);
  }

  napi_value key_v = nullptr;
  napi_create_int32(env, key_length, &key_v);
  if (key_v != nullptr) napi_set_named_property(env, out, "keyLength", key_v);

  return out;
}

napi_value CryptoParsePfx(napi_env env, napi_callback_info info) {
  size_t argc = 2;
  napi_value argv[2] = {nullptr, nullptr};
  if (napi_get_cb_info(env, info, &argc, argv, nullptr, nullptr) != napi_ok || argc < 1) return nullptr;
  uint8_t* pfx = nullptr;
  size_t pfx_len = 0;
  if (!GetBufferBytes(env, argv[0], &pfx, &pfx_len)) return nullptr;
  std::string pass;
  bool has_pass = false;
  if (argc >= 2 && !ReadPassphrase(env, argv[1], &pass, &has_pass)) {
    ThrowError(env, "ERR_INVALID_ARG_TYPE", "passphrase must be a string or Buffer");
    return nullptr;
  }
  const unsigned char* p = pfx;
  PKCS12* pkcs12 = d2i_PKCS12(nullptr, &p, static_cast<long>(pfx_len));
  if (pkcs12 == nullptr) {
    ThrowError(env, "ERR_CRYPTO_PFX", "not enough data");
    return nullptr;
  }
  EVP_PKEY* pkey = nullptr;
  X509* cert = nullptr;
  STACK_OF(X509)* ca = nullptr;
  const int ok = PKCS12_parse(pkcs12, has_pass ? pass.c_str() : nullptr, &pkey, &cert, &ca);
  PKCS12_free(pkcs12);
  if (pkey) EVP_PKEY_free(pkey);
  if (cert) X509_free(cert);
  if (ca) sk_X509_pop_free(ca, X509_free);
  if (ok != 1) {
    ThrowError(env, "ERR_CRYPTO_PFX", "mac verify failure");
    return nullptr;
  }
  napi_value true_v = nullptr;
  napi_get_boolean(env, true, &true_v);
  return true_v;
}

napi_value CryptoParseCrl(napi_env env, napi_callback_info info) {
  size_t argc = 1;
  napi_value argv[1] = {nullptr};
  if (napi_get_cb_info(env, info, &argc, argv, nullptr, nullptr) != napi_ok || argc < 1) return nullptr;
  uint8_t* crl_data = nullptr;
  size_t crl_len = 0;
  if (!GetBufferBytes(env, argv[0], &crl_data, &crl_len)) return nullptr;
  BIO* bio = BIO_new_mem_buf(crl_data, static_cast<int>(crl_len));
  X509_CRL* crl = bio ? PEM_read_bio_X509_CRL(bio, nullptr, nullptr, nullptr) : nullptr;
  if (crl == nullptr && bio != nullptr) {
    (void)BIO_reset(bio);
    crl = d2i_X509_CRL_bio(bio, nullptr);
  }
  if (crl) X509_CRL_free(crl);
  if (bio) BIO_free(bio);
  if (crl == nullptr) {
    ThrowError(env, "ERR_CRYPTO_CRL", "Failed to parse CRL");
    return nullptr;
  }
  napi_value true_v = nullptr;
  napi_get_boolean(env, true, &true_v);
  return true_v;
}

napi_value CryptoSecureContextCreate(napi_env env, napi_callback_info info) {
  (void)info;
  SSL_CTX* ctx = CreateConfiguredSecureContext(TLS_method());
  if (ctx == nullptr) {
    ThrowLastOpenSslError(env, "ERR_TLS_INVALID_CONTEXT", "Failed to create secure context");
    return nullptr;
  }
  auto* holder = new SecureContextHolder(ctx);
  EnsureTicketCallback(holder);
  napi_value out = nullptr;
  if (napi_create_external(env, holder, SecureContextFinalizer, nullptr, &out) != napi_ok || out == nullptr) {
    delete holder;
    return nullptr;
  }
  return out;
}

napi_value CryptoSecureContextInit(napi_env env, napi_callback_info info) {
  size_t argc = 4;
  napi_value argv[4] = {nullptr, nullptr, nullptr, nullptr};
  if (napi_get_cb_info(env, info, &argc, argv, nullptr, nullptr) != napi_ok || argc < 1) return nullptr;
  SecureContextHolder* holder = nullptr;
  if (!GetSecureContextHolder(env, argv[0], &holder) || holder == nullptr || holder->ctx == nullptr) {
    ThrowError(env, "ERR_INVALID_ARG_TYPE", "context must be a secure context handle");
    return nullptr;
  }
  int32_t min_version = 0;
  int32_t max_version = 0;
  if (argc >= 3 && argv[2] != nullptr) napi_get_value_int32(env, argv[2], &min_version);
  if (argc >= 4 && argv[3] != nullptr) napi_get_value_int32(env, argv[3], &max_version);
  if (max_version == 0) max_version = TLS1_3_VERSION;
  const SSL_METHOD* method = TLS_method();

  napi_valuetype protocol_type = napi_undefined;
  if (argc >= 2 && napi_typeof(env, argv[1], &protocol_type) == napi_ok && protocol_type == napi_string) {
    const std::string ssl_method = ValueToUtf8(env, argv[1]);
    if (ssl_method == "SSLv2_method" || ssl_method == "SSLv2_server_method" ||
        ssl_method == "SSLv2_client_method") {
      ThrowError(env, "ERR_TLS_INVALID_PROTOCOL_METHOD", "SSLv2 methods disabled");
      return nullptr;
    } else if (ssl_method == "SSLv3_method" || ssl_method == "SSLv3_server_method" ||
               ssl_method == "SSLv3_client_method") {
      ThrowError(env, "ERR_TLS_INVALID_PROTOCOL_METHOD", "SSLv3 methods disabled");
      return nullptr;
    } else if (ssl_method == "SSLv23_method") {
      max_version = TLS1_2_VERSION;
    } else if (ssl_method == "SSLv23_server_method") {
      max_version = TLS1_2_VERSION;
      method = TLS_server_method();
    } else if (ssl_method == "SSLv23_client_method") {
      max_version = TLS1_2_VERSION;
      method = TLS_client_method();
    } else if (ssl_method == "TLS_method") {
      min_version = 0;
      max_version = TLS1_3_VERSION;
    } else if (ssl_method == "TLS_server_method") {
      min_version = 0;
      max_version = TLS1_3_VERSION;
      method = TLS_server_method();
    } else if (ssl_method == "TLS_client_method") {
      min_version = 0;
      max_version = TLS1_3_VERSION;
      method = TLS_client_method();
    } else if (ssl_method == "TLSv1_method") {
      min_version = TLS1_VERSION;
      max_version = TLS1_VERSION;
    } else if (ssl_method == "TLSv1_server_method") {
      min_version = TLS1_VERSION;
      max_version = TLS1_VERSION;
      method = TLS_server_method();
    } else if (ssl_method == "TLSv1_client_method") {
      min_version = TLS1_VERSION;
      max_version = TLS1_VERSION;
      method = TLS_client_method();
    } else if (ssl_method == "TLSv1_1_method") {
      min_version = TLS1_1_VERSION;
      max_version = TLS1_1_VERSION;
    } else if (ssl_method == "TLSv1_1_server_method") {
      min_version = TLS1_1_VERSION;
      max_version = TLS1_1_VERSION;
      method = TLS_server_method();
    } else if (ssl_method == "TLSv1_1_client_method") {
      min_version = TLS1_1_VERSION;
      max_version = TLS1_1_VERSION;
      method = TLS_client_method();
    } else if (ssl_method == "TLSv1_2_method") {
      min_version = TLS1_2_VERSION;
      max_version = TLS1_2_VERSION;
    } else if (ssl_method == "TLSv1_2_server_method") {
      min_version = TLS1_2_VERSION;
      max_version = TLS1_2_VERSION;
      method = TLS_server_method();
    } else if (ssl_method == "TLSv1_2_client_method") {
      min_version = TLS1_2_VERSION;
      max_version = TLS1_2_VERSION;
      method = TLS_client_method();
    } else {
      const std::string message = std::string("Unknown method: ") + ssl_method;
      ThrowError(env, "ERR_TLS_INVALID_PROTOCOL_METHOD", message.c_str());
      return nullptr;
    }
  }

  SSL_CTX* ctx = CreateConfiguredSecureContext(method);
  if (ctx == nullptr) {
    ThrowLastOpenSslError(env, "ERR_TLS_INVALID_CONTEXT", "Failed to create secure context");
    return nullptr;
  }
  if (SSL_CTX_set_min_proto_version(ctx, min_version) != 1) {
    SSL_CTX_free(ctx);
    ThrowLastOpenSslError(env, "ERR_TLS_INVALID_PROTOCOL_VERSION", "Failed to set min protocol version");
    return nullptr;
  }
  if (SSL_CTX_set_max_proto_version(ctx, max_version) != 1) {
    SSL_CTX_free(ctx);
    ThrowLastOpenSslError(env, "ERR_TLS_INVALID_PROTOCOL_VERSION", "Failed to set max protocol version");
    return nullptr;
  }

  ResetStoredCertificate(&holder->cert, nullptr);
  ResetStoredCertificate(&holder->issuer, nullptr);
  SSL_CTX* old_ctx = holder->ctx;
  holder->ctx = ctx;
  holder->ticket_callback_installed = false;
  EnsureTicketCallback(holder);
  if (old_ctx != nullptr) {
    ClearSecureContextAppData(old_ctx);
    SSL_CTX_free(old_ctx);
  }
  napi_value true_v = nullptr;
  napi_get_boolean(env, true, &true_v);
  return true_v;
}

napi_value CryptoSecureContextSetMinProto(napi_env env, napi_callback_info info) {
  size_t argc = 2;
  napi_value argv[2] = {nullptr, nullptr};
  if (napi_get_cb_info(env, info, &argc, argv, nullptr, nullptr) != napi_ok || argc < 2) return nullptr;
  SecureContextHolder* holder = nullptr;
  if (!GetSecureContextHolder(env, argv[0], &holder) || holder == nullptr || holder->ctx == nullptr) {
    ThrowError(env, "ERR_INVALID_ARG_TYPE", "context must be a secure context handle");
    return nullptr;
  }
  int32_t min_version = 0;
  if (napi_get_value_int32(env, argv[1], &min_version) != napi_ok) {
    ThrowError(env, "ERR_INVALID_ARG_TYPE", "min protocol version must be an integer");
    return nullptr;
  }
  if (SSL_CTX_set_min_proto_version(holder->ctx, min_version) != 1) {
    ThrowLastOpenSslError(env, "ERR_TLS_INVALID_PROTOCOL_VERSION", "Failed to set min protocol version");
    return nullptr;
  }
  napi_value true_v = nullptr;
  napi_get_boolean(env, true, &true_v);
  return true_v;
}

napi_value CryptoSecureContextSetMaxProto(napi_env env, napi_callback_info info) {
  size_t argc = 2;
  napi_value argv[2] = {nullptr, nullptr};
  if (napi_get_cb_info(env, info, &argc, argv, nullptr, nullptr) != napi_ok || argc < 2) return nullptr;
  SecureContextHolder* holder = nullptr;
  if (!GetSecureContextHolder(env, argv[0], &holder) || holder == nullptr || holder->ctx == nullptr) {
    ThrowError(env, "ERR_INVALID_ARG_TYPE", "context must be a secure context handle");
    return nullptr;
  }
  int32_t max_version = 0;
  if (napi_get_value_int32(env, argv[1], &max_version) != napi_ok) {
    ThrowError(env, "ERR_INVALID_ARG_TYPE", "max protocol version must be an integer");
    return nullptr;
  }
  if (SSL_CTX_set_max_proto_version(holder->ctx, max_version) != 1) {
    ThrowLastOpenSslError(env, "ERR_TLS_INVALID_PROTOCOL_VERSION", "Failed to set max protocol version");
    return nullptr;
  }
  napi_value true_v = nullptr;
  napi_get_boolean(env, true, &true_v);
  return true_v;
}

napi_value CryptoSecureContextGetMinProto(napi_env env, napi_callback_info info) {
  size_t argc = 1;
  napi_value argv[1] = {nullptr};
  if (napi_get_cb_info(env, info, &argc, argv, nullptr, nullptr) != napi_ok || argc < 1) return nullptr;
  SecureContextHolder* holder = nullptr;
  if (!GetSecureContextHolder(env, argv[0], &holder) || holder == nullptr || holder->ctx == nullptr) {
    ThrowError(env, "ERR_INVALID_ARG_TYPE", "context must be a secure context handle");
    return nullptr;
  }
  napi_value out = nullptr;
  napi_create_int32(env, SSL_CTX_get_min_proto_version(holder->ctx), &out);
  return out;
}

napi_value CryptoSecureContextGetMaxProto(napi_env env, napi_callback_info info) {
  size_t argc = 1;
  napi_value argv[1] = {nullptr};
  if (napi_get_cb_info(env, info, &argc, argv, nullptr, nullptr) != napi_ok || argc < 1) return nullptr;
  SecureContextHolder* holder = nullptr;
  if (!GetSecureContextHolder(env, argv[0], &holder) || holder == nullptr || holder->ctx == nullptr) {
    ThrowError(env, "ERR_INVALID_ARG_TYPE", "context must be a secure context handle");
    return nullptr;
  }
  napi_value out = nullptr;
  napi_create_int32(env, SSL_CTX_get_max_proto_version(holder->ctx), &out);
  return out;
}

napi_value CryptoSecureContextSetOptions(napi_env env, napi_callback_info info) {
  size_t argc = 2;
  napi_value argv[2] = {nullptr, nullptr};
  if (napi_get_cb_info(env, info, &argc, argv, nullptr, nullptr) != napi_ok || argc < 2) return nullptr;
  SecureContextHolder* holder = nullptr;
  if (!GetSecureContextHolder(env, argv[0], &holder) || holder == nullptr || holder->ctx == nullptr) {
    ThrowError(env, "ERR_INVALID_ARG_TYPE", "context must be a secure context handle");
    return nullptr;
  }
  int64_t options = 0;
  if (napi_get_value_int64(env, argv[1], &options) != napi_ok) {
    ThrowError(env, "ERR_INVALID_ARG_TYPE", "secure options must be an integer");
    return nullptr;
  }
  SSL_CTX_set_options(holder->ctx, static_cast<uint64_t>(options));
  napi_value true_v = nullptr;
  napi_get_boolean(env, true, &true_v);
  return true_v;
}

napi_value CryptoSecureContextSetCiphers(napi_env env, napi_callback_info info) {
  size_t argc = 2;
  napi_value argv[2] = {nullptr, nullptr};
  if (napi_get_cb_info(env, info, &argc, argv, nullptr, nullptr) != napi_ok || argc < 2) return nullptr;
  SecureContextHolder* holder = nullptr;
  if (!GetSecureContextHolder(env, argv[0], &holder) || holder == nullptr || holder->ctx == nullptr) {
    ThrowError(env, "ERR_INVALID_ARG_TYPE", "context must be a secure context handle");
    return nullptr;
  }
  const std::string ciphers = ValueToUtf8(env, argv[1]);
  if (SSL_CTX_set_cipher_list(holder->ctx, ciphers.c_str()) != 1) {
    const unsigned long err = ERR_get_error();
    if (ciphers.empty() && ERR_GET_REASON(err) == SSL_R_NO_CIPHER_MATCH) {
      napi_value true_v = nullptr;
      napi_get_boolean(env, true, &true_v);
      return true_v;
    }
    ThrowOpenSslError(env, "ERR_TLS_INVALID_CIPHER", err, "Failed to set ciphers");
    return nullptr;
  }
  napi_value true_v = nullptr;
  napi_get_boolean(env, true, &true_v);
  return true_v;
}

napi_value CryptoSecureContextSetCipherSuites(napi_env env, napi_callback_info info) {
  size_t argc = 2;
  napi_value argv[2] = {nullptr, nullptr};
  if (napi_get_cb_info(env, info, &argc, argv, nullptr, nullptr) != napi_ok || argc < 2) return nullptr;
  SecureContextHolder* holder = nullptr;
  if (!GetSecureContextHolder(env, argv[0], &holder) || holder == nullptr || holder->ctx == nullptr) {
    ThrowError(env, "ERR_INVALID_ARG_TYPE", "context must be a secure context handle");
    return nullptr;
  }
  const std::string suites = ValueToUtf8(env, argv[1]);
#if OPENSSL_VERSION_NUMBER >= 0x10101000L
  if (SSL_CTX_set_ciphersuites(holder->ctx, suites.c_str()) != 1) {
    ThrowLastOpenSslError(env, "ERR_TLS_INVALID_CIPHER", "Failed to set TLSv1.3 ciphersuites");
    return nullptr;
  }
#else
  (void)suites;
#endif
  napi_value true_v = nullptr;
  napi_get_boolean(env, true, &true_v);
  return true_v;
}

napi_value CryptoSecureContextSetCert(napi_env env, napi_callback_info info) {
  size_t argc = 2;
  napi_value argv[2] = {nullptr, nullptr};
  if (napi_get_cb_info(env, info, &argc, argv, nullptr, nullptr) != napi_ok || argc < 2) return nullptr;
  SecureContextHolder* holder = nullptr;
  if (!GetSecureContextHolder(env, argv[0], &holder) || holder == nullptr || holder->ctx == nullptr) {
    ThrowError(env, "ERR_INVALID_ARG_TYPE", "context must be a secure context handle");
    return nullptr;
  }
  std::vector<uint8_t> cert_owned;
  uint8_t* cert_bytes = nullptr;
  size_t cert_len = 0;
  if (!GetBufferOrStringBytes(env, argv[1], &cert_owned, &cert_bytes, &cert_len)) {
    ThrowError(env, "ERR_INVALID_ARG_TYPE", "cert must be a string or Buffer");
    return nullptr;
  }
  int ok = 0;
  const bool looks_like_pem =
      cert_len >= 11 &&
      std::memcmp(cert_bytes, "-----BEGIN ", 11) == 0;
  if (looks_like_pem) {
    ok = UseCertificateChain(holder, cert_bytes, cert_len);
  }
  if (ok != 1) {
    X509* cert = ParseX509(cert_bytes, cert_len);
    if (cert == nullptr) {
      ThrowLastOpenSslError(env, "ERR_OSSL_PEM_NO_START_LINE", "Failed to parse certificate");
      return nullptr;
    }
    ok = SSL_CTX_use_certificate(holder->ctx, cert);
    if (ok == 1) {
      ResetStoredCertificate(&holder->cert, cert);
      UpdateIssuerFromStore(holder);
    }
    X509_free(cert);
  }
  if (ok != 1) {
    ThrowLastOpenSslError(env, "ERR_TLS_CERT", "Failed to use certificate");
    return nullptr;
  }
  napi_value true_v = nullptr;
  napi_get_boolean(env, true, &true_v);
  return true_v;
}

napi_value CryptoSecureContextSetKey(napi_env env, napi_callback_info info) {
  size_t argc = 3;
  napi_value argv[3] = {nullptr, nullptr, nullptr};
  if (napi_get_cb_info(env, info, &argc, argv, nullptr, nullptr) != napi_ok || argc < 2) return nullptr;
  SecureContextHolder* holder = nullptr;
  if (!GetSecureContextHolder(env, argv[0], &holder) || holder == nullptr || holder->ctx == nullptr) {
    ThrowError(env, "ERR_INVALID_ARG_TYPE", "context must be a secure context handle");
    return nullptr;
  }
  std::vector<uint8_t> key_owned;
  uint8_t* key_bytes = nullptr;
  size_t key_len = 0;
  if (!GetBufferOrStringBytes(env, argv[1], &key_owned, &key_bytes, &key_len)) {
    ThrowError(env, "ERR_INVALID_ARG_TYPE", "key must be a string or Buffer");
    return nullptr;
  }
  std::string passphrase;
  bool has_passphrase = true;
  if (argc >= 3 && !ReadPassphrase(env, argv[2], &passphrase, &has_passphrase)) {
    ThrowError(env, "ERR_INVALID_ARG_TYPE", "passphrase must be a string or Buffer");
    return nullptr;
  }
  if (argc < 3 || IsNullOrUndefined(env, argv[2])) {
    passphrase.clear();
    has_passphrase = true;
  }
  EVP_PKEY* pkey = ParsePrivateKeyWithPassphrase(key_bytes,
                                                 key_len,
                                                 reinterpret_cast<const uint8_t*>(passphrase.data()),
                                                 passphrase.size(),
                                                 has_passphrase);
  if (pkey == nullptr) {
    ThrowLastOpenSslError(env, "ERR_CRYPTO_OPERATION_FAILED", "Failed to parse private key");
    return nullptr;
  }
  const int ok = SSL_CTX_use_PrivateKey(holder->ctx, pkey);
  EVP_PKEY_free(pkey);
  if (ok != 1) {
    ThrowLastOpenSslError(env, "ERR_TLS_KEY", "Failed to use private key");
    return nullptr;
  }
  napi_value true_v = nullptr;
  napi_get_boolean(env, true, &true_v);
  return true_v;
}

napi_value CryptoSecureContextAddCACert(napi_env env, napi_callback_info info) {
  size_t argc = 2;
  napi_value argv[2] = {nullptr, nullptr};
  if (napi_get_cb_info(env, info, &argc, argv, nullptr, nullptr) != napi_ok || argc < 2) return nullptr;
  SecureContextHolder* holder = nullptr;
  if (!GetSecureContextHolder(env, argv[0], &holder) || holder == nullptr || holder->ctx == nullptr) {
    ThrowError(env, "ERR_INVALID_ARG_TYPE", "context must be a secure context handle");
    return nullptr;
  }
  std::vector<uint8_t> cert_owned;
  uint8_t* cert_bytes = nullptr;
  size_t cert_len = 0;
  if (!GetBufferOrStringBytes(env, argv[1], &cert_owned, &cert_bytes, &cert_len)) {
    ThrowError(env, "ERR_INVALID_ARG_TYPE", "ca must be a string or Buffer");
    return nullptr;
  }
  BIO* bio = BIO_new_mem_buf(cert_bytes, static_cast<int>(cert_len));
  if (bio == nullptr) {
    ThrowLastOpenSslError(env, "ERR_OSSL_PEM_NO_START_LINE", "Failed to parse CA certificate");
    return nullptr;
  }

  X509_STORE* store = SSL_CTX_get_cert_store(holder->ctx);
  if (store == nullptr) {
    BIO_free(bio);
    ThrowError(env, "ERR_TLS_CERT", "Failed to add CA certificate");
    return nullptr;
  }

  bool added_any = false;
  while (X509* cert = PEM_read_bio_X509_AUX(bio, nullptr, nullptr, nullptr)) {
    added_any = true;
    if (!AddCertificateToStore(store, cert) || SSL_CTX_add_client_CA(holder->ctx, cert) != 1) {
      X509_free(cert);
      BIO_free(bio);
      ThrowLastOpenSslError(env, "ERR_TLS_CERT", "Failed to add CA certificate");
      return nullptr;
    }
    X509_free(cert);
  }

  BIO_free(bio);
  const unsigned long err = ERR_peek_last_error();
  if (!added_any ||
      (err != 0 &&
       !(ERR_GET_LIB(err) == ERR_LIB_PEM &&
         ERR_GET_REASON(err) == PEM_R_NO_START_LINE))) {
    ThrowLastOpenSslError(env, "ERR_OSSL_PEM_NO_START_LINE", "Failed to parse CA certificate");
    return nullptr;
  }
  UpdateIssuerFromStore(holder);
  ERR_clear_error();
  napi_value true_v = nullptr;
  napi_get_boolean(env, true, &true_v);
  return true_v;
}

napi_value CryptoSecureContextAddCrl(napi_env env, napi_callback_info info) {
  size_t argc = 2;
  napi_value argv[2] = {nullptr, nullptr};
  if (napi_get_cb_info(env, info, &argc, argv, nullptr, nullptr) != napi_ok || argc < 2) return nullptr;
  SecureContextHolder* holder = nullptr;
  if (!GetSecureContextHolder(env, argv[0], &holder) || holder == nullptr || holder->ctx == nullptr) {
    ThrowError(env, "ERR_INVALID_ARG_TYPE", "context must be a secure context handle");
    return nullptr;
  }
  std::vector<uint8_t> crl_owned;
  uint8_t* crl_bytes = nullptr;
  size_t crl_len = 0;
  if (!GetBufferOrStringBytes(env, argv[1], &crl_owned, &crl_bytes, &crl_len)) {
    ThrowError(env, "ERR_INVALID_ARG_TYPE", "crl must be a string or Buffer");
    return nullptr;
  }
  X509_CRL* crl = ParseX509Crl(crl_bytes, crl_len);
  if (crl == nullptr) {
    ThrowError(env, "ERR_CRYPTO_CRL", "Failed to parse CRL");
    return nullptr;
  }
  X509_STORE* store = SSL_CTX_get_cert_store(holder->ctx);
  if (store == nullptr || X509_STORE_add_crl(store, crl) != 1) {
    X509_CRL_free(crl);
    ThrowLastOpenSslError(env, "ERR_TLS_CERT", "Failed to add CRL");
    return nullptr;
  }
  X509_CRL_free(crl);
  X509_STORE_set_flags(store, X509_V_FLAG_CRL_CHECK | X509_V_FLAG_CRL_CHECK_ALL);
  napi_value true_v = nullptr;
  napi_get_boolean(env, true, &true_v);
  return true_v;
}

napi_value CryptoSecureContextAddRootCerts(napi_env env, napi_callback_info info) {
  size_t argc = 1;
  napi_value argv[1] = {nullptr};
  if (napi_get_cb_info(env, info, &argc, argv, nullptr, nullptr) != napi_ok || argc < 1) return nullptr;
  SecureContextHolder* holder = nullptr;
  if (!GetSecureContextHolder(env, argv[0], &holder) || holder == nullptr || holder->ctx == nullptr) {
    ThrowError(env, "ERR_INVALID_ARG_TYPE", "context must be a secure context handle");
    return nullptr;
  }
  if (!AddRootCertsToContextStore(env, holder->ctx)) {
    ThrowLastOpenSslError(env, "ERR_TLS_CERT", "Failed to load default CA certificates");
    return nullptr;
  }
  UpdateIssuerFromStore(holder);
  napi_value true_v = nullptr;
  napi_get_boolean(env, true, &true_v);
  return true_v;
}

napi_value CryptoSecureContextSetAllowPartialTrustChain(napi_env env, napi_callback_info info) {
  size_t argc = 1;
  napi_value argv[1] = {nullptr};
  if (napi_get_cb_info(env, info, &argc, argv, nullptr, nullptr) != napi_ok || argc < 1) return nullptr;
  SecureContextHolder* holder = nullptr;
  if (!GetSecureContextHolder(env, argv[0], &holder) || holder == nullptr || holder->ctx == nullptr) {
    ThrowError(env, "ERR_INVALID_ARG_TYPE", "context must be a secure context handle");
    return nullptr;
  }
  X509_STORE* store = SSL_CTX_get_cert_store(holder->ctx);
  if (store == nullptr || X509_STORE_set_flags(store, X509_V_FLAG_PARTIAL_CHAIN) != 1) {
    ThrowLastOpenSslError(env, "ERR_TLS_CERT", "Failed to update certificate store flags");
    return nullptr;
  }
  napi_value true_v = nullptr;
  napi_get_boolean(env, true, &true_v);
  return true_v;
}

napi_value CryptoSecureContextSetSessionIdContext(napi_env env, napi_callback_info info) {
  size_t argc = 2;
  napi_value argv[2] = {nullptr, nullptr};
  if (napi_get_cb_info(env, info, &argc, argv, nullptr, nullptr) != napi_ok || argc < 2) return nullptr;
  SecureContextHolder* holder = nullptr;
  if (!GetSecureContextHolder(env, argv[0], &holder) || holder == nullptr || holder->ctx == nullptr) {
    ThrowError(env, "ERR_INVALID_ARG_TYPE", "context must be a secure context handle");
    return nullptr;
  }
  const std::string session_id_context = ValueToUtf8(env, argv[1]);
  const auto* data = reinterpret_cast<const unsigned char*>(session_id_context.data());
  if (SSL_CTX_set_session_id_context(holder->ctx, data, session_id_context.size()) != 1) {
    ThrowLastOpenSslError(env, "ERR_TLS_INVALID_CONTEXT", "Failed to set session id context");
    return nullptr;
  }
  napi_value true_v = nullptr;
  napi_get_boolean(env, true, &true_v);
  return true_v;
}

napi_value CryptoSecureContextSetSessionTimeout(napi_env env, napi_callback_info info) {
  size_t argc = 2;
  napi_value argv[2] = {nullptr, nullptr};
  if (napi_get_cb_info(env, info, &argc, argv, nullptr, nullptr) != napi_ok || argc < 2) return nullptr;
  SecureContextHolder* holder = nullptr;
  if (!GetSecureContextHolder(env, argv[0], &holder) || holder == nullptr || holder->ctx == nullptr) {
    ThrowError(env, "ERR_INVALID_ARG_TYPE", "context must be a secure context handle");
    return nullptr;
  }
  int32_t timeout = 0;
  if (napi_get_value_int32(env, argv[1], &timeout) != napi_ok || timeout < 0) {
    ThrowError(env, "ERR_INVALID_ARG_TYPE", "session timeout must be a non-negative integer");
    return nullptr;
  }
  SSL_CTX_set_timeout(holder->ctx, static_cast<long>(timeout));
  napi_value true_v = nullptr;
  napi_get_boolean(env, true, &true_v);
  return true_v;
}

napi_value CryptoSecureContextSetTicketKeys(napi_env env, napi_callback_info info) {
  size_t argc = 2;
  napi_value argv[2] = {nullptr, nullptr};
  if (napi_get_cb_info(env, info, &argc, argv, nullptr, nullptr) != napi_ok || argc < 2) return nullptr;
  SecureContextHolder* holder = nullptr;
  if (!GetSecureContextHolder(env, argv[0], &holder) || holder == nullptr || holder->ctx == nullptr) {
    ThrowError(env, "ERR_INVALID_ARG_TYPE", "context must be a secure context handle");
    return nullptr;
  }
  uint8_t* data = nullptr;
  size_t len = 0;
  if (!GetBufferBytes(env, argv[1], &data, &len) || len != 48) {
    ThrowError(env, "ERR_INVALID_ARG_TYPE", "ticket keys must be a 48-byte Buffer");
    return nullptr;
  }
  holder->ticket_keys.assign(data, data + len);
  EnsureTicketCallback(holder);
  napi_value true_v = nullptr;
  napi_get_boolean(env, true, &true_v);
  return true_v;
}

napi_value CryptoSecureContextGetTicketKeys(napi_env env, napi_callback_info info) {
  size_t argc = 1;
  napi_value argv[1] = {nullptr};
  if (napi_get_cb_info(env, info, &argc, argv, nullptr, nullptr) != napi_ok || argc < 1) return nullptr;
  SecureContextHolder* holder = nullptr;
  if (!GetSecureContextHolder(env, argv[0], &holder) || holder == nullptr || holder->ctx == nullptr) {
    ThrowError(env, "ERR_INVALID_ARG_TYPE", "context must be a secure context handle");
    return nullptr;
  }
  if (!EnsureTicketKeys(holder)) {
    ThrowLastOpenSslError(env, "ERR_TLS_INVALID_CONTEXT", "Error generating ticket keys");
    return nullptr;
  }
  EnsureTicketCallback(holder);
  return CreateBufferCopy(env, holder->ticket_keys.data(), holder->ticket_keys.size());
}

napi_value CryptoSecureContextEnableTicketKeyCallback(napi_env env, napi_callback_info info) {
  size_t argc = 1;
  napi_value argv[1] = {nullptr};
  if (napi_get_cb_info(env, info, &argc, argv, nullptr, nullptr) != napi_ok || argc < 1) return nullptr;
  SecureContextHolder* holder = nullptr;
  if (!GetSecureContextHolder(env, argv[0], &holder) || holder == nullptr || holder->ctx == nullptr) {
    ThrowError(env, "ERR_INVALID_ARG_TYPE", "context must be a secure context handle");
    return nullptr;
  }
  EnsureTicketCallback(holder);
  napi_value true_v = nullptr;
  napi_get_boolean(env, true, &true_v);
  return true_v;
}

napi_value CryptoSecureContextLoadPKCS12(napi_env env, napi_callback_info info) {
  size_t argc = 3;
  napi_value argv[3] = {nullptr, nullptr, nullptr};
  if (napi_get_cb_info(env, info, &argc, argv, nullptr, nullptr) != napi_ok || argc < 2) return nullptr;
  SecureContextHolder* holder = nullptr;
  if (!GetSecureContextHolder(env, argv[0], &holder) || holder == nullptr || holder->ctx == nullptr) {
    ThrowError(env, "ERR_INVALID_ARG_TYPE", "context must be a secure context handle");
    return nullptr;
  }

  uint8_t* pfx = nullptr;
  size_t pfx_len = 0;
  if (!GetBufferBytes(env, argv[1], &pfx, &pfx_len)) {
    ThrowError(env, "ERR_INVALID_ARG_TYPE", "pfx must be a string or Buffer");
    return nullptr;
  }

  std::string passphrase;
  bool has_passphrase = false;
  if (argc >= 3 && !ReadPassphrase(env, argv[2], &passphrase, &has_passphrase)) {
    ThrowError(env, "ERR_INVALID_ARG_TYPE", "passphrase must be a string or Buffer");
    return nullptr;
  }

  BIO* bio = BIO_new_mem_buf(pfx, static_cast<int>(pfx_len));
  if (bio == nullptr) {
    ThrowError(env, "ERR_CRYPTO_OPERATION_FAILED", "Unable to load PFX certificate");
    return nullptr;
  }

  ERR_clear_error();

  PKCS12* pkcs12 = nullptr;
  if (!d2i_PKCS12_bio(bio, &pkcs12)) {
    BIO_free(bio);
    const unsigned long err = ConsumeOpenSslError();
    const char* reason = ERR_reason_error_string(err);
    napi_throw_error(env, nullptr, reason != nullptr ? reason : "Unknown error");
    return nullptr;
  }
  BIO_free(bio);

  EVP_PKEY* pkey = nullptr;
  X509* cert = nullptr;
  STACK_OF(X509)* extra = nullptr;
  const int ok = PKCS12_parse(pkcs12,
                              has_passphrase ? passphrase.c_str() : nullptr,
                              &pkey,
                              &cert,
                              &extra);
  PKCS12_free(pkcs12);

  if (ok != 1) {
#if OPENSSL_VERSION_MAJOR >= 3
    const unsigned long peek_err = ERR_peek_last_error();
    if (ERR_GET_REASON(peek_err) == ERR_R_UNSUPPORTED) {
      ThrowError(env, "ERR_CRYPTO_UNSUPPORTED_OPERATION", "Unsupported PKCS12 PFX data");
      return nullptr;
    }
#endif
    const unsigned long err = ConsumeOpenSslError();
    const char* reason = ERR_reason_error_string(err);
    napi_throw_error(env, nullptr, reason != nullptr ? reason : "Unknown error");
    return nullptr;
  }

  if (pkey == nullptr) {
    if (cert != nullptr) X509_free(cert);
    if (extra != nullptr) sk_X509_pop_free(extra, X509_free);
    ThrowError(env, "ERR_CRYPTO_OPERATION_FAILED", "Unable to load private key from PFX data");
    return nullptr;
  }

  if (cert == nullptr) {
    EVP_PKEY_free(pkey);
    if (extra != nullptr) sk_X509_pop_free(extra, X509_free);
    ThrowError(env, "ERR_CRYPTO_OPERATION_FAILED", "Unable to load certificate from PFX data");
    return nullptr;
  }

  int use_ok = SSL_CTX_use_certificate(holder->ctx, cert);
  if (use_ok == 1) {
    SSL_CTX_clear_extra_chain_certs(holder->ctx);
    const int count = extra != nullptr ? sk_X509_num(extra) : 0;
    for (int i = 0; i < count; ++i) {
      X509* ca = sk_X509_value(extra, i);
      if (ca == nullptr) continue;
      if (SSL_CTX_add1_chain_cert(holder->ctx, ca) != 1) {
        use_ok = 0;
        break;
      }
    }
  }
  if (use_ok != 1 || SSL_CTX_use_PrivateKey(holder->ctx, pkey) != 1) {
    EVP_PKEY_free(pkey);
    X509_free(cert);
    if (extra != nullptr) sk_X509_pop_free(extra, X509_free);
    ThrowLastOpenSslError(env, "ERR_CRYPTO_OPERATION_FAILED", "Unable to load PFX certificate");
    return nullptr;
  }

  ResetStoredCertificate(&holder->cert, cert);
  ResetStoredCertificate(&holder->issuer, nullptr);

  X509* issuer = nullptr;
  const int count = extra != nullptr ? sk_X509_num(extra) : 0;
  X509_STORE* store = SSL_CTX_get_cert_store(holder->ctx);
  for (int i = 0; i < count; ++i) {
    X509* ca = sk_X509_value(extra, i);
    if (ca == nullptr) continue;
    if (store != nullptr) {
      if (!AddCertificateToStore(store, ca) || SSL_CTX_add_client_CA(holder->ctx, ca) != 1) {
        EVP_PKEY_free(pkey);
        X509_free(cert);
        if (extra != nullptr) sk_X509_pop_free(extra, X509_free);
        ThrowLastOpenSslError(env, "ERR_TLS_CERT", "Failed to add CA certificate");
        return nullptr;
      }
    }
    if (issuer == nullptr && X509_check_issued(ca, cert) == X509_V_OK) {
      issuer = ca;
    }
  }

  if (issuer != nullptr) {
    ResetStoredCertificate(&holder->issuer, issuer);
  } else {
    UpdateIssuerFromStore(holder);
  }

  EVP_PKEY_free(pkey);
  X509_free(cert);
  if (extra != nullptr) sk_X509_pop_free(extra, X509_free);

  napi_value true_v = nullptr;
  napi_get_boolean(env, true, &true_v);
  return true_v;
}

napi_value CryptoSecureContextSetSigalgs(napi_env env, napi_callback_info info) {
  size_t argc = 2;
  napi_value argv[2] = {nullptr, nullptr};
  if (napi_get_cb_info(env, info, &argc, argv, nullptr, nullptr) != napi_ok || argc < 2) return nullptr;
  SecureContextHolder* holder = nullptr;
  if (!GetSecureContextHolder(env, argv[0], &holder) || holder == nullptr || holder->ctx == nullptr) {
    ThrowError(env, "ERR_INVALID_ARG_TYPE", "context must be a secure context handle");
    return nullptr;
  }
  const std::string sigalgs = ValueToUtf8(env, argv[1]);
  if (sigalgs.empty() || SSL_CTX_set1_sigalgs_list(holder->ctx, sigalgs.c_str()) != 1) {
    ThrowLastOpenSslError(env, "ERR_TLS_INVALID_CONTEXT", "Failed to set signature algorithms");
    return nullptr;
  }
  napi_value true_v = nullptr;
  napi_get_boolean(env, true, &true_v);
  return true_v;
}

napi_value CryptoSecureContextSetECDHCurve(napi_env env, napi_callback_info info) {
  size_t argc = 2;
  napi_value argv[2] = {nullptr, nullptr};
  if (napi_get_cb_info(env, info, &argc, argv, nullptr, nullptr) != napi_ok || argc < 2) return nullptr;
  SecureContextHolder* holder = nullptr;
  if (!GetSecureContextHolder(env, argv[0], &holder) || holder == nullptr || holder->ctx == nullptr) {
    ThrowError(env, "ERR_INVALID_ARG_TYPE", "context must be a secure context handle");
    return nullptr;
  }
  const std::string curve = ValueToUtf8(env, argv[1]);
  if (curve.empty() || curve == "auto") {
    napi_value true_v = nullptr;
    napi_get_boolean(env, true, &true_v);
    return true_v;
  }
  if (SSL_CTX_set1_curves_list(holder->ctx, curve.c_str()) != 1) {
    ThrowLastOpenSslError(env, "ERR_TLS_INVALID_CONTEXT", "Failed to set ECDH curve");
    return nullptr;
  }
  napi_value true_v = nullptr;
  napi_get_boolean(env, true, &true_v);
  return true_v;
}

napi_value CryptoSecureContextSetDHParam(napi_env env, napi_callback_info info) {
  size_t argc = 2;
  napi_value argv[2] = {nullptr, nullptr};
  if (napi_get_cb_info(env, info, &argc, argv, nullptr, nullptr) != napi_ok || argc < 2) return nullptr;
  napi_value undefined = nullptr;
  napi_get_undefined(env, &undefined);
  SecureContextHolder* holder = nullptr;
  if (!GetSecureContextHolder(env, argv[0], &holder) || holder == nullptr || holder->ctx == nullptr) {
    ThrowError(env, "ERR_INVALID_ARG_TYPE", "context must be a secure context handle");
    return nullptr;
  }

  bool is_auto = false;
  if (napi_get_value_bool(env, argv[1], &is_auto) == napi_ok && is_auto) {
    if (SSL_CTX_set_dh_auto(holder->ctx, true) != 1) {
      ThrowLastOpenSslError(env, "ERR_TLS_INVALID_CONTEXT", "Failed to enable automatic DH parameters");
      return nullptr;
    }
    return undefined;
  }

  std::vector<uint8_t> dh_owned;
  uint8_t* dh_bytes = nullptr;
  size_t dh_len = 0;
  if (!GetBufferOrStringBytes(env, argv[1], &dh_owned, &dh_bytes, &dh_len)) {
    ThrowError(env, "ERR_INVALID_ARG_TYPE", "DH parameters must be a string or Buffer");
    return nullptr;
  }

  BIO* bio = BIO_new_mem_buf(dh_bytes, static_cast<int>(dh_len));
  if (bio == nullptr) {
    ThrowLastOpenSslError(env, "ERR_TLS_INVALID_CONTEXT", "Failed to load DH parameters");
    return nullptr;
  }

  DH* dh = PEM_read_bio_DHparams(bio, nullptr, nullptr, nullptr);
  BIO_free(bio);
  if (dh == nullptr) {
    return undefined;
  }

  const BIGNUM* p = nullptr;
  DH_get0_pqg(dh, &p, nullptr, nullptr);
  const int size = p != nullptr ? BN_num_bits(p) : 0;
  if (size < 1024) {
    DH_free(dh);
    ThrowError(env, "ERR_INVALID_ARG_VALUE", "DH parameter is less than 1024 bits");
    return nullptr;
  }

  napi_value warning = undefined;
  if (size < 2048) {
    if (napi_create_string_utf8(env,
                                "DH parameter is less than 2048 bits",
                                NAPI_AUTO_LENGTH,
                                &warning) != napi_ok ||
        warning == nullptr) {
      warning = undefined;
    }
  }

  if (SSL_CTX_set_tmp_dh(holder->ctx, dh) != 1) {
    DH_free(dh);
    ThrowError(env, "ERR_CRYPTO_OPERATION_FAILED", "Error setting temp DH parameter");
    return nullptr;
  }
  DH_free(dh);
  return warning;
}

napi_value CryptoSecureContextGetCertificate(napi_env env, napi_callback_info info) {
  size_t argc = 1;
  napi_value argv[1] = {nullptr};
  if (napi_get_cb_info(env, info, &argc, argv, nullptr, nullptr) != napi_ok || argc < 1) return nullptr;
  SecureContextHolder* holder = nullptr;
  if (!GetSecureContextHolder(env, argv[0], &holder) || holder == nullptr) {
    ThrowError(env, "ERR_INVALID_ARG_TYPE", "context must be a secure context handle");
    return nullptr;
  }
  napi_value out = CreateX509DerBuffer(env, holder->cert);
  if (out == nullptr) {
    napi_get_null(env, &out);
  }
  return out;
}

napi_value CryptoSecureContextGetIssuer(napi_env env, napi_callback_info info) {
  size_t argc = 1;
  napi_value argv[1] = {nullptr};
  if (napi_get_cb_info(env, info, &argc, argv, nullptr, nullptr) != napi_ok || argc < 1) return nullptr;
  SecureContextHolder* holder = nullptr;
  if (!GetSecureContextHolder(env, argv[0], &holder) || holder == nullptr) {
    ThrowError(env, "ERR_INVALID_ARG_TYPE", "context must be a secure context handle");
    return nullptr;
  }
  napi_value out = CreateX509DerBuffer(env, holder->issuer);
  if (out == nullptr) {
    napi_get_null(env, &out);
  }
  return out;
}

EVP_PKEY* ParsePrivateKeyWithPassphraseImpl(const uint8_t* data,
                                            size_t len,
                                            const uint8_t* passphrase,
                                            size_t passphrase_len,
                                            bool has_passphrase) {
  ERR_clear_error();
  struct PassphraseSpan {
    const char* data = reinterpret_cast<const char*>(-1);
    size_t len = 0;
  };
  auto password_callback = [](char* buf, int size, int /*rwflag*/, void* userdata) -> int {
    auto* span = static_cast<PassphraseSpan*>(userdata);
    if (span == nullptr) return -1;
    const size_t buflen = static_cast<size_t>(size);
    if (buflen < span->len) return -1;
    if (span->len > 0) {
      std::memcpy(buf, span->data, span->len);
    }
    return static_cast<int>(span->len);
  };

  BIO* bio = BIO_new_mem_buf(data, static_cast<int>(len));
  if (bio == nullptr) return nullptr;
  PassphraseSpan passphrase_span;
  if (has_passphrase) {
    if (passphrase != nullptr) {
      passphrase_span.data = reinterpret_cast<const char*>(passphrase);
    }
    passphrase_span.len = passphrase_len;
  }
  void* passphrase_arg = has_passphrase ? &passphrase_span : nullptr;
  pem_password_cb* password_cb = password_callback;
  EVP_PKEY* pkey = PEM_read_bio_PrivateKey(bio, nullptr, password_cb, passphrase_arg);
  const bool looks_like_pem = (len > 0 && data[0] == '-');
  if (pkey == nullptr) {
    (void)BIO_reset(bio);
    pkey = d2i_PKCS8PrivateKey_bio(bio, nullptr, password_cb, passphrase_arg);
  }
  if (pkey == nullptr && !looks_like_pem) {
    (void)BIO_reset(bio);
    pkey = d2i_PrivateKey_bio(bio, nullptr);
  }
  BIO_free(bio);
  if (pkey != nullptr) ERR_clear_error();
  return pkey;
}

EVP_PKEY* ParsePrivateKey(const uint8_t* data, size_t len) {
  return ParsePrivateKeyWithPassphraseImpl(data, len, nullptr, 0, false);
}

template <typename ParseFn>
EVP_PKEY* TryParsePublicPemBlock(BIO* bio, const char* name, ParseFn&& parse) {
  if (bio == nullptr || name == nullptr) return nullptr;
  if (BIO_reset(bio) != 1) return nullptr;

  unsigned char* der_data = nullptr;
  long der_len = 0;
  if (PEM_bytes_read_bio(&der_data, &der_len, nullptr, name, bio, nullptr, nullptr) != 1) {
    return nullptr;
  }

  const unsigned char* der = der_data;
  EVP_PKEY* pkey = parse(&der, der_len);
  OPENSSL_free(der_data);
  return pkey;
}

EVP_PKEY* ParsePublicKeyOrCert(const uint8_t* data, size_t len) {
  ERR_clear_error();
  BIO* bio = BIO_new_mem_buf(data, static_cast<int>(len));
  if (bio == nullptr) return nullptr;
  const bool looks_like_pem = (len > 0 && data[0] == '-');
  EVP_PKEY* pkey = nullptr;
  if (looks_like_pem) {
    pkey = TryParsePublicPemBlock(bio, "PUBLIC KEY", [](const unsigned char** der, long der_len) {
      return d2i_PUBKEY(nullptr, der, der_len);
    });
    if (pkey == nullptr) {
      pkey = TryParsePublicPemBlock(bio, "RSA PUBLIC KEY", [](const unsigned char** der, long der_len) {
        return d2i_PublicKey(EVP_PKEY_RSA, nullptr, der, der_len);
      });
    }
    if (pkey == nullptr) {
      pkey = TryParsePublicPemBlock(bio, "CERTIFICATE", [](const unsigned char** der, long der_len) {
        X509* cert = d2i_X509(nullptr, der, der_len);
        if (cert == nullptr) return static_cast<EVP_PKEY*>(nullptr);
        EVP_PKEY* public_key = X509_get_pubkey(cert);
        X509_free(cert);
        return public_key;
      });
    }
  } else {
    (void)BIO_reset(bio);
    pkey = d2i_PUBKEY_bio(bio, nullptr);
    if (pkey == nullptr) {
      (void)BIO_reset(bio);
      RSA* rsa = d2i_RSAPublicKey_bio(bio, nullptr);
      if (rsa != nullptr) {
        pkey = EVP_PKEY_new();
        if (pkey == nullptr || EVP_PKEY_assign_RSA(pkey, rsa) != 1) {
          if (pkey != nullptr) EVP_PKEY_free(pkey);
          RSA_free(rsa);
          pkey = nullptr;
        }
      }
    }
    if (pkey == nullptr) {
      (void)BIO_reset(bio);
      X509* cert = d2i_X509_bio(bio, nullptr);
      if (cert != nullptr) {
        pkey = X509_get_pubkey(cert);
        X509_free(cert);
      }
    }
  }
  BIO_free(bio);
  if (pkey != nullptr) ERR_clear_error();
  return pkey;
}

X509* ParseX509(const uint8_t* data, size_t len) {
  BIO* bio = BIO_new_mem_buf(data, static_cast<int>(len));
  if (bio == nullptr) return nullptr;
  X509* cert = PEM_read_bio_X509(bio, nullptr, nullptr, nullptr);
  if (cert == nullptr) {
    (void)BIO_reset(bio);
    cert = d2i_X509_bio(bio, nullptr);
  }
  BIO_free(bio);
  return cert;
}

X509_CRL* ParseX509Crl(const uint8_t* data, size_t len) {
  BIO* bio = BIO_new_mem_buf(data, static_cast<int>(len));
  if (bio == nullptr) return nullptr;
  X509_CRL* crl = PEM_read_bio_X509_CRL(bio, nullptr, nullptr, nullptr);
  if (crl == nullptr) {
    (void)BIO_reset(bio);
    crl = d2i_X509_CRL_bio(bio, nullptr);
  }
  BIO_free(bio);
  return crl;
}

std::string AsymmetricKeyTypeName(const EVP_PKEY* pkey) {
  if (pkey == nullptr) return "";
  switch (ncrypto::EVPKeyPointer::id(pkey)) {
    case EVP_PKEY_RSA:
      return "rsa";
    case EVP_PKEY_RSA_PSS:
      return "rsa-pss";
    case EVP_PKEY_DSA:
      return "dsa";
    case EVP_PKEY_DH:
      return "dh";
    case EVP_PKEY_EC:
      return "ec";
    case EVP_PKEY_ED25519:
      return "ed25519";
    case EVP_PKEY_ED448:
      return "ed448";
    case EVP_PKEY_X25519:
      return "x25519";
    case EVP_PKEY_X448:
      return "x448";
#if OPENSSL_WITH_PQC
    case EVP_PKEY_ML_DSA_44:
      return "ml-dsa-44";
    case EVP_PKEY_ML_DSA_65:
      return "ml-dsa-65";
    case EVP_PKEY_ML_DSA_87:
      return "ml-dsa-87";
    case EVP_PKEY_ML_KEM_512:
      return "ml-kem-512";
    case EVP_PKEY_ML_KEM_768:
      return "ml-kem-768";
    case EVP_PKEY_ML_KEM_1024:
      return "ml-kem-1024";
    case EVP_PKEY_SLH_DSA_SHA2_128S:
      return "slh-dsa-sha2-128s";
    case EVP_PKEY_SLH_DSA_SHA2_128F:
      return "slh-dsa-sha2-128f";
    case EVP_PKEY_SLH_DSA_SHA2_192S:
      return "slh-dsa-sha2-192s";
    case EVP_PKEY_SLH_DSA_SHA2_192F:
      return "slh-dsa-sha2-192f";
    case EVP_PKEY_SLH_DSA_SHA2_256S:
      return "slh-dsa-sha2-256s";
    case EVP_PKEY_SLH_DSA_SHA2_256F:
      return "slh-dsa-sha2-256f";
    case EVP_PKEY_SLH_DSA_SHAKE_128S:
      return "slh-dsa-shake-128s";
    case EVP_PKEY_SLH_DSA_SHAKE_128F:
      return "slh-dsa-shake-128f";
    case EVP_PKEY_SLH_DSA_SHAKE_192S:
      return "slh-dsa-shake-192s";
    case EVP_PKEY_SLH_DSA_SHAKE_192F:
      return "slh-dsa-shake-192f";
    case EVP_PKEY_SLH_DSA_SHAKE_256S:
      return "slh-dsa-shake-256s";
    case EVP_PKEY_SLH_DSA_SHAKE_256F:
      return "slh-dsa-shake-256f";
#endif
    default:
      return "";
  }
}

bool IsRsaVariant(int type) {
  return type == EVP_PKEY_RSA || type == EVP_PKEY_RSA_PSS
#ifdef EVP_PKEY_RSA2
         || type == EVP_PKEY_RSA2
#endif
      ;
}

bool IsOneShotVariant(int type) {
  switch (type) {
    case EVP_PKEY_ED25519:
    case EVP_PKEY_ED448:
#if OPENSSL_WITH_PQC
    case EVP_PKEY_ML_DSA_44:
    case EVP_PKEY_ML_DSA_65:
    case EVP_PKEY_ML_DSA_87:
    case EVP_PKEY_SLH_DSA_SHA2_128F:
    case EVP_PKEY_SLH_DSA_SHA2_128S:
    case EVP_PKEY_SLH_DSA_SHA2_192F:
    case EVP_PKEY_SLH_DSA_SHA2_192S:
    case EVP_PKEY_SLH_DSA_SHA2_256F:
    case EVP_PKEY_SLH_DSA_SHA2_256S:
    case EVP_PKEY_SLH_DSA_SHAKE_128F:
    case EVP_PKEY_SLH_DSA_SHAKE_128S:
    case EVP_PKEY_SLH_DSA_SHAKE_192F:
    case EVP_PKEY_SLH_DSA_SHAKE_192S:
    case EVP_PKEY_SLH_DSA_SHAKE_256F:
    case EVP_PKEY_SLH_DSA_SHAKE_256S:
#endif
      return true;
    default:
      return false;
  }
}

bool SupportsContextString(int type) {
#if OPENSSL_VERSION_NUMBER < 0x3020000fL
  return false;
#else
  switch (type) {
    case EVP_PKEY_ED448:
#if OPENSSL_WITH_PQC
    case EVP_PKEY_ML_DSA_44:
    case EVP_PKEY_ML_DSA_65:
    case EVP_PKEY_ML_DSA_87:
    case EVP_PKEY_SLH_DSA_SHA2_128F:
    case EVP_PKEY_SLH_DSA_SHA2_128S:
    case EVP_PKEY_SLH_DSA_SHA2_192F:
    case EVP_PKEY_SLH_DSA_SHA2_192S:
    case EVP_PKEY_SLH_DSA_SHA2_256F:
    case EVP_PKEY_SLH_DSA_SHA2_256S:
    case EVP_PKEY_SLH_DSA_SHAKE_128F:
    case EVP_PKEY_SLH_DSA_SHAKE_128S:
    case EVP_PKEY_SLH_DSA_SHAKE_192F:
    case EVP_PKEY_SLH_DSA_SHAKE_192S:
    case EVP_PKEY_SLH_DSA_SHAKE_256F:
    case EVP_PKEY_SLH_DSA_SHAKE_256S:
#endif
      return true;
    default:
      return false;
  }
#endif
}

size_t GetDsaSigPartLengthFromPkey(EVP_PKEY* pkey) {
  if (pkey == nullptr) return 0;
  const int type = EVP_PKEY_base_id(pkey);
  if (type == EVP_PKEY_EC) {
    EC_KEY* ec = EVP_PKEY_get1_EC_KEY(pkey);
    if (ec == nullptr) return 0;
    const EC_GROUP* group = EC_KEY_get0_group(ec);
    const int bits = group != nullptr ? EC_GROUP_order_bits(group) : 0;
    EC_KEY_free(ec);
    if (bits <= 0) return 0;
    return static_cast<size_t>((bits + 7) / 8);
  }
  if (type == EVP_PKEY_DSA) {
    DSA* dsa = EVP_PKEY_get1_DSA(pkey);
    if (dsa == nullptr) return 0;
    const BIGNUM* p = nullptr;
    const BIGNUM* q = nullptr;
    const BIGNUM* g = nullptr;
    DSA_get0_pqg(dsa, &p, &q, &g);
    const int bits = q != nullptr ? BN_num_bits(q) : 0;
    DSA_free(dsa);
    if (bits <= 0) return 0;
    return static_cast<size_t>((bits + 7) / 8);
  }
  return 0;
}

bool DerSignatureToP1363(const uint8_t* der,
                         size_t der_len,
                         size_t part_len,
                         std::vector<uint8_t>* out) {
  if (der == nullptr || out == nullptr || part_len == 0) return false;
  out->assign(part_len * 2, 0);
  ncrypto::Buffer<const unsigned char> der_buf{der, der_len};
  return ncrypto::extractP1363(der_buf, out->data(), part_len);
}

bool P1363ToDerSignature(int pkey_type,
                         const uint8_t* sig,
                         size_t sig_len,
                         size_t part_len,
                         std::vector<uint8_t>* out) {
  if (sig == nullptr || out == nullptr || part_len == 0 || sig_len != part_len * 2) return false;
  BIGNUM* r = BN_bin2bn(sig, static_cast<int>(part_len), nullptr);
  BIGNUM* s = BN_bin2bn(sig + part_len, static_cast<int>(part_len), nullptr);
  if (r == nullptr || s == nullptr) {
    if (r != nullptr) BN_free(r);
    if (s != nullptr) BN_free(s);
    return false;
  }

  unsigned char* der = nullptr;
  int der_len = 0;
  if (pkey_type == EVP_PKEY_EC) {
    ECDSA_SIG* ec_sig = ECDSA_SIG_new();
    if (ec_sig == nullptr || ECDSA_SIG_set0(ec_sig, r, s) != 1) {
      if (ec_sig != nullptr) ECDSA_SIG_free(ec_sig);
      BN_free(r);
      BN_free(s);
      return false;
    }
    der_len = i2d_ECDSA_SIG(ec_sig, &der);
    ECDSA_SIG_free(ec_sig);
  } else if (pkey_type == EVP_PKEY_DSA) {
    DSA_SIG* dsa_sig = DSA_SIG_new();
    if (dsa_sig == nullptr || DSA_SIG_set0(dsa_sig, r, s) != 1) {
      if (dsa_sig != nullptr) DSA_SIG_free(dsa_sig);
      BN_free(r);
      BN_free(s);
      return false;
    }
    der_len = i2d_DSA_SIG(dsa_sig, &der);
    DSA_SIG_free(dsa_sig);
  } else {
    BN_free(r);
    BN_free(s);
    return false;
  }

  if (der == nullptr || der_len <= 0) {
    if (der != nullptr) OPENSSL_free(der);
    return false;
  }
  out->assign(der, der + der_len);
  OPENSSL_free(der);
  return true;
}

napi_value CryptoGetAsymmetricKeyDetails(napi_env env, napi_callback_info info) {
  size_t argc = 2;
  napi_value argv[2] = {nullptr, nullptr};
  if (napi_get_cb_info(env, info, &argc, argv, nullptr, nullptr) != napi_ok || argc < 1) return nullptr;

  uint8_t* key_bytes = nullptr;
  size_t key_len = 0;
  if (!GetBufferBytes(env, argv[0], &key_bytes, &key_len)) {
    ThrowError(env, "ERR_INVALID_ARG_TYPE", "key must be a Buffer");
    return nullptr;
  }
  std::string passphrase;
  bool has_passphrase = false;
  if (argc >= 2 && argv[1] != nullptr && !IsNullOrUndefined(env, argv[1])) {
    if (!ReadPassphrase(env, argv[1], &passphrase, &has_passphrase)) {
      ThrowError(env, "ERR_INVALID_ARG_TYPE", "Invalid key passphrase");
      return nullptr;
    }
  }

  EVP_PKEY* pkey = ParsePrivateKeyWithPassphrase(key_bytes,
                                                 key_len,
                                                 reinterpret_cast<const uint8_t*>(passphrase.data()),
                                                 passphrase.size(),
                                                 has_passphrase);
  if (pkey == nullptr) pkey = ParsePublicKeyOrCert(key_bytes, key_len);
  if (pkey == nullptr) {
    napi_value null_v = nullptr;
    napi_get_null(env, &null_v);
    return null_v;
  }

  napi_value out = nullptr;
  if (napi_create_object(env, &out) != napi_ok || out == nullptr) {
    EVP_PKEY_free(pkey);
    return nullptr;
  }

  auto set_int32 = [&](const char* name, int32_t value) {
    napi_value v = nullptr;
    if (napi_create_int32(env, value, &v) == napi_ok && v != nullptr) {
      napi_set_named_property(env, out, name, v);
    }
  };
  auto set_string = [&](const char* name, std::string value) {
    if (value.empty()) return;
    napi_value v = nullptr;
    if (napi_create_string_utf8(env, value.c_str(), NAPI_AUTO_LENGTH, &v) == napi_ok && v != nullptr) {
      napi_set_named_property(env, out, name, v);
    }
  };
  auto set_buffer = [&](const char* name, const std::vector<uint8_t>& bytes) {
    napi_value v = CreateBufferCopy(env, bytes.data(), bytes.size());
    if (v != nullptr) napi_set_named_property(env, out, name, v);
  };
  auto normalize_digest_name = [](std::string in) {
    for (char& ch : in) ch = static_cast<char>(std::tolower(static_cast<unsigned char>(ch)));
    if (in.rfind("sha2-", 0) == 0) in.erase(3, 2);  // sha2-256 -> sha256
    return in;
  };

  const int pkey_type = EVP_PKEY_base_id(pkey);
  if (pkey_type == EVP_PKEY_RSA || pkey_type == EVP_PKEY_RSA_PSS) {
    const int bits = EVP_PKEY_bits(pkey);
    if (bits > 0) set_int32("modulusLength", bits);

    BIGNUM* e = nullptr;
    if (EVP_PKEY_get_bn_param(pkey, OSSL_PKEY_PARAM_RSA_E, &e) == 1 && e != nullptr) {
      const int e_len = BN_num_bytes(e);
      if (e_len > 0) {
        std::vector<uint8_t> e_bytes(static_cast<size_t>(e_len));
        BN_bn2bin(e, e_bytes.data());
        set_buffer("publicExponent", e_bytes);
      }
      BN_free(e);
    }

    if (pkey_type == EVP_PKEY_RSA_PSS) {
      RSA* rsa = EVP_PKEY_get1_RSA(pkey);
      if (rsa != nullptr) {
        const RSA_PSS_PARAMS* params = RSA_get0_pss_params(rsa);
        if (params != nullptr) {
          std::string hash_algorithm = "sha1";
          if (params->hashAlgorithm != nullptr) {
            const ASN1_OBJECT* hash_obj = nullptr;
            X509_ALGOR_get0(&hash_obj, nullptr, nullptr, params->hashAlgorithm);
            if (hash_obj != nullptr) {
              hash_algorithm = normalize_digest_name(OBJ_nid2ln(OBJ_obj2nid(hash_obj)));
            }
          }

          std::string mgf1_hash_algorithm = hash_algorithm;
          if (params->maskGenAlgorithm != nullptr) {
            const ASN1_OBJECT* mgf_obj = nullptr;
            X509_ALGOR_get0(&mgf_obj, nullptr, nullptr, params->maskGenAlgorithm);
            if (mgf_obj != nullptr && OBJ_obj2nid(mgf_obj) == NID_mgf1 && params->maskHash != nullptr) {
              const ASN1_OBJECT* mgf1_hash_obj = nullptr;
              X509_ALGOR_get0(&mgf1_hash_obj, nullptr, nullptr, params->maskHash);
              if (mgf1_hash_obj != nullptr) {
                mgf1_hash_algorithm = normalize_digest_name(OBJ_nid2ln(OBJ_obj2nid(mgf1_hash_obj)));
              }
            }
          }

          int64_t salt_len = 20;
          if (params->saltLength != nullptr) {
            if (ASN1_INTEGER_get_int64(&salt_len, params->saltLength) != 1) {
              salt_len = -1;
            }
          }

          set_string("hashAlgorithm", hash_algorithm);
          set_string("mgf1HashAlgorithm", mgf1_hash_algorithm);
          if (salt_len >= 0) {
            set_int32("saltLength", static_cast<int32_t>(salt_len));
          }
        }
        RSA_free(rsa);
      }
    }
  } else if (pkey_type == EVP_PKEY_DSA) {
    const int bits = EVP_PKEY_bits(pkey);
    if (bits > 0) set_int32("modulusLength", bits);

    int q_bits = 0;
    if (EVP_PKEY_get_int_param(pkey, OSSL_PKEY_PARAM_FFC_QBITS, &q_bits) == 1 && q_bits > 0) {
      set_int32("divisorLength", q_bits);
    } else {
      BIGNUM* q = nullptr;
      if (EVP_PKEY_get_bn_param(pkey, OSSL_PKEY_PARAM_FFC_Q, &q) == 1 && q != nullptr) {
        const int q_len = BN_num_bits(q);
        if (q_len > 0) set_int32("divisorLength", q_len);
        BN_free(q);
      }
    }
  } else if (pkey_type == EVP_PKEY_EC) {
    char group_name[80];
    size_t group_name_len = 0;
    if (EVP_PKEY_get_utf8_string_param(
            pkey, OSSL_PKEY_PARAM_GROUP_NAME, group_name, sizeof(group_name), &group_name_len) == 1 &&
        group_name_len > 0) {
      set_string("namedCurve", std::string(group_name, group_name_len));
    }
  }

  EVP_PKEY_free(pkey);
  return out;
}

napi_value CryptoGetAsymmetricKeyType(napi_env env, napi_callback_info info) {
  size_t argc = 2;
  napi_value argv[2] = {nullptr, nullptr};
  if (napi_get_cb_info(env, info, &argc, argv, nullptr, nullptr) != napi_ok || argc < 1) return nullptr;
  uint8_t* key_bytes = nullptr;
  size_t key_len = 0;
  if (!GetBufferBytes(env, argv[0], &key_bytes, &key_len)) {
    ThrowError(env, "ERR_INVALID_ARG_TYPE", "key must be a Buffer");
    return nullptr;
  }
  std::string passphrase;
  bool has_passphrase = false;
  if (argc >= 2 && argv[1] != nullptr && !IsNullOrUndefined(env, argv[1])) {
    if (!ReadPassphrase(env, argv[1], &passphrase, &has_passphrase)) {
      ThrowError(env, "ERR_INVALID_ARG_TYPE", "Invalid key passphrase");
      return nullptr;
    }
  }
  EVP_PKEY* pkey = ParsePrivateKeyWithPassphrase(key_bytes,
                                                 key_len,
                                                 reinterpret_cast<const uint8_t*>(passphrase.data()),
                                                 passphrase.size(),
                                                 has_passphrase);
  if (pkey == nullptr) pkey = ParsePublicKeyOrCert(key_bytes, key_len);
  if (pkey == nullptr) {
    napi_value null_v = nullptr;
    napi_get_null(env, &null_v);
    return null_v;
  }
  const std::string type = AsymmetricKeyTypeName(pkey);
  EVP_PKEY_free(pkey);
  if (type.empty()) {
    napi_value null_v = nullptr;
    napi_get_null(env, &null_v);
    return null_v;
  }
  napi_value out = nullptr;
  napi_create_string_utf8(env, type.c_str(), NAPI_AUTO_LENGTH, &out);
  return out;
}

napi_value CryptoPublicEncrypt(napi_env env, napi_callback_info info) {
  size_t argc = 8;
  napi_value argv[8] = {nullptr, nullptr, nullptr, nullptr, nullptr, nullptr, nullptr, nullptr};
  if (napi_get_cb_info(env, info, &argc, argv, nullptr, nullptr) != napi_ok || argc < 5) return nullptr;
  std::vector<uint8_t> owned_key;
  uint8_t* key_bytes = nullptr;
  size_t key_len = 0;
  uint8_t* input = nullptr;
  size_t in_len = 0;
  if (!GetKeyBytes(env, argv[0], &owned_key, &key_bytes, &key_len) ||
      !GetBufferBytes(env, argv[4], &input, &in_len)) {
    ThrowError(env, "ERR_INVALID_ARG_TYPE", "key and buffer must be Buffers or strings");
    return nullptr;
  }
  int32_t padding = RSA_PKCS1_OAEP_PADDING;
  if (argc >= 6 && argv[5] != nullptr) napi_get_value_int32(env, argv[5], &padding);
  std::string oaep_hash = "sha1";
  napi_valuetype hash_type = napi_undefined;
  if (argc >= 7 && argv[6] != nullptr &&
      napi_typeof(env, argv[6], &hash_type) == napi_ok &&
      hash_type == napi_string) {
    oaep_hash = ValueToUtf8(env, argv[6]);
  }
  const EVP_MD* oaep_md = (padding == RSA_PKCS1_OAEP_PADDING) ? EVP_get_digestbyname(oaep_hash.c_str()) : nullptr;
  if (padding == RSA_PKCS1_OAEP_PADDING && oaep_md == nullptr) {
    ThrowError(env, "ERR_OSSL_EVP_INVALID_DIGEST", "Invalid digest used");
    return nullptr;
  }
  uint8_t* label = nullptr;
  size_t label_len = 0;
  bool has_label = (argc >= 8 && argv[7] != nullptr && GetBufferBytes(env, argv[7], &label, &label_len));
  std::string passphrase;
  bool has_passphrase = false;
  if (argc >= 4 && argv[3] != nullptr && !ReadPassphrase(env, argv[3], &passphrase, &has_passphrase)) {
    ThrowError(env, "ERR_INVALID_ARG_TYPE", "Invalid key passphrase");
    return nullptr;
  }

  EVP_PKEY* pkey = ParsePublicKeyOrCert(key_bytes, key_len);
  if (pkey == nullptr) {
    ERR_clear_error();
    pkey = ParsePrivateKeyWithPassphrase(key_bytes,
                                         key_len,
                                         reinterpret_cast<const uint8_t*>(passphrase.data()),
                                         passphrase.size(),
                                         has_passphrase);
  }
  if (pkey == nullptr) {
    ThrowLastOpenSslError(env, "ERR_CRYPTO_OPERATION_FAILED", "Failed to parse public key");
    return nullptr;
  }
  EVP_PKEY_CTX* ctx = EVP_PKEY_CTX_new(pkey, nullptr);
  if (ctx == nullptr || EVP_PKEY_encrypt_init(ctx) != 1 ||
      EVP_PKEY_CTX_set_rsa_padding(ctx, padding) != 1) {
    if (ctx) EVP_PKEY_CTX_free(ctx);
    EVP_PKEY_free(pkey);
    ThrowLastOpenSslError(env, "ERR_CRYPTO_OPERATION_FAILED", "publicEncrypt initialization failed");
    return nullptr;
  }
  if (padding == RSA_PKCS1_OAEP_PADDING) {
    if (EVP_PKEY_CTX_set_rsa_oaep_md(ctx, oaep_md) != 1 ||
        EVP_PKEY_CTX_set_rsa_mgf1_md(ctx, oaep_md) != 1) {
      EVP_PKEY_CTX_free(ctx);
      EVP_PKEY_free(pkey);
      ThrowLastOpenSslError(env, "ERR_OSSL_EVP_INVALID_DIGEST", "Invalid digest used");
      return nullptr;
    }
    if (has_label && label_len > 0) {
      unsigned char* copied = reinterpret_cast<unsigned char*>(OPENSSL_malloc(label_len));
      if (copied == nullptr) {
        EVP_PKEY_CTX_free(ctx);
        EVP_PKEY_free(pkey);
        ThrowError(env, "ERR_CRYPTO_OPERATION_FAILED", "Failed to allocate OAEP label");
        return nullptr;
      }
      std::memcpy(copied, label, label_len);
      if (EVP_PKEY_CTX_set0_rsa_oaep_label(ctx, copied, static_cast<int>(label_len)) != 1) {
        OPENSSL_free(copied);
        EVP_PKEY_CTX_free(ctx);
        EVP_PKEY_free(pkey);
        ThrowLastOpenSslError(env, "ERR_CRYPTO_OPERATION_FAILED", "Failed to set OAEP label");
        return nullptr;
      }
    }
  }
  size_t out_len = 0;
  if (EVP_PKEY_encrypt(ctx, nullptr, &out_len, input, in_len) != 1) {
    EVP_PKEY_CTX_free(ctx);
    EVP_PKEY_free(pkey);
    ThrowLastOpenSslError(env, "ERR_CRYPTO_OPERATION_FAILED", "publicEncrypt failed");
    return nullptr;
  }
  std::vector<uint8_t> out(out_len);
  if (EVP_PKEY_encrypt(ctx, out.data(), &out_len, input, in_len) != 1) {
    EVP_PKEY_CTX_free(ctx);
    EVP_PKEY_free(pkey);
    ThrowLastOpenSslError(env, "ERR_CRYPTO_OPERATION_FAILED", "publicEncrypt failed");
    return nullptr;
  }
  EVP_PKEY_CTX_free(ctx);
  EVP_PKEY_free(pkey);
  return CreateBufferCopy(env, out.data(), out_len);
}

napi_value CryptoPrivateEncrypt(napi_env env, napi_callback_info info) {
  size_t argc = 8;
  napi_value argv[8] = {nullptr, nullptr, nullptr, nullptr, nullptr, nullptr, nullptr, nullptr};
  if (napi_get_cb_info(env, info, &argc, argv, nullptr, nullptr) != napi_ok || argc < 5) return nullptr;
  std::vector<uint8_t> owned_key;
  uint8_t* key_bytes = nullptr;
  size_t key_len = 0;
  uint8_t* input = nullptr;
  size_t in_len = 0;
  if (!GetKeyBytes(env, argv[0], &owned_key, &key_bytes, &key_len) ||
      !GetBufferBytes(env, argv[4], &input, &in_len)) {
    ThrowError(env, "ERR_INVALID_ARG_TYPE", "key and buffer must be Buffers or strings");
    return nullptr;
  }

  std::string passphrase;
  bool has_passphrase = false;
  if (argc >= 4 && argv[3] != nullptr && !ReadPassphrase(env, argv[3], &passphrase, &has_passphrase)) {
    ThrowError(env, "ERR_INVALID_ARG_TYPE", "Invalid key passphrase");
    return nullptr;
  }

  int32_t padding = RSA_PKCS1_PADDING;
  if (argc >= 6 && argv[5] != nullptr) napi_get_value_int32(env, argv[5], &padding);

  EVP_PKEY* pkey = ParsePrivateKeyWithPassphrase(key_bytes,
                                                 key_len,
                                                 reinterpret_cast<const uint8_t*>(passphrase.data()),
                                                 passphrase.size(),
                                                 has_passphrase);
  if (pkey == nullptr) {
    ThrowLastOpenSslError(env, "ERR_CRYPTO_OPERATION_FAILED", "Failed to parse private key");
    return nullptr;
  }
  RSA* rsa = EVP_PKEY_get1_RSA(pkey);
  EVP_PKEY_free(pkey);
  if (rsa == nullptr) {
    ThrowLastOpenSslError(env, "ERR_CRYPTO_INVALID_KEY_OBJECT_TYPE", "Invalid private key");
    return nullptr;
  }

  std::vector<uint8_t> out(static_cast<size_t>(RSA_size(rsa)));
  const int written = RSA_private_encrypt(static_cast<int>(in_len), input, out.data(), rsa, padding);
  RSA_free(rsa);
  if (written < 0) {
    ThrowLastOpenSslError(env, "ERR_CRYPTO_OPERATION_FAILED", "privateEncrypt failed");
    return nullptr;
  }
  return CreateBufferCopy(env, out.data(), static_cast<size_t>(written));
}

napi_value CryptoPrivateDecrypt(napi_env env, napi_callback_info info) {
  size_t argc = 8;
  napi_value argv[8] = {nullptr, nullptr, nullptr, nullptr, nullptr, nullptr, nullptr, nullptr};
  if (napi_get_cb_info(env, info, &argc, argv, nullptr, nullptr) != napi_ok || argc < 5) return nullptr;
  std::vector<uint8_t> owned_key;
  uint8_t* key_bytes = nullptr;
  size_t key_len = 0;
  uint8_t* input = nullptr;
  size_t in_len = 0;
  if (!GetKeyBytes(env, argv[0], &owned_key, &key_bytes, &key_len) ||
      !GetBufferBytes(env, argv[4], &input, &in_len)) {
    ThrowError(env, "ERR_INVALID_ARG_TYPE", "key and buffer must be Buffers or strings");
    return nullptr;
  }
  int32_t padding = RSA_PKCS1_OAEP_PADDING;
  if (argc >= 6 && argv[5] != nullptr) napi_get_value_int32(env, argv[5], &padding);
  std::string oaep_hash = "sha1";
  napi_valuetype hash_type = napi_undefined;
  if (argc >= 7 && argv[6] != nullptr &&
      napi_typeof(env, argv[6], &hash_type) == napi_ok &&
      hash_type == napi_string) {
    oaep_hash = ValueToUtf8(env, argv[6]);
  }
  const EVP_MD* oaep_md = (padding == RSA_PKCS1_OAEP_PADDING) ? EVP_get_digestbyname(oaep_hash.c_str()) : nullptr;
  if (padding == RSA_PKCS1_OAEP_PADDING && oaep_md == nullptr) {
    ThrowError(env, "ERR_OSSL_EVP_INVALID_DIGEST", "Invalid digest used");
    return nullptr;
  }
  uint8_t* label = nullptr;
  size_t label_len = 0;
  bool has_label = (argc >= 8 && argv[7] != nullptr && GetBufferBytes(env, argv[7], &label, &label_len));

  std::string passphrase;
  bool has_passphrase = false;
  if (argc >= 4 && argv[3] != nullptr && !ReadPassphrase(env, argv[3], &passphrase, &has_passphrase)) {
    ThrowError(env, "ERR_INVALID_ARG_TYPE", "Invalid key passphrase");
    return nullptr;
  }

  EVP_PKEY* pkey = ParsePrivateKeyWithPassphrase(key_bytes,
                                                 key_len,
                                                 reinterpret_cast<const uint8_t*>(passphrase.data()),
                                                 passphrase.size(),
                                                 has_passphrase);
  if (pkey == nullptr) {
    ThrowLastOpenSslError(env, "ERR_CRYPTO_OPERATION_FAILED", "Failed to parse private key");
    return nullptr;
  }
  EVP_PKEY_CTX* ctx = EVP_PKEY_CTX_new(pkey, nullptr);
  if (ctx == nullptr || EVP_PKEY_decrypt_init(ctx) != 1 ||
      EVP_PKEY_CTX_set_rsa_padding(ctx, padding) != 1) {
    if (ctx) EVP_PKEY_CTX_free(ctx);
    EVP_PKEY_free(pkey);
    ThrowLastOpenSslError(env, "ERR_CRYPTO_OPERATION_FAILED", "privateDecrypt initialization failed");
    return nullptr;
  }
  if (padding == RSA_PKCS1_OAEP_PADDING) {
    if (EVP_PKEY_CTX_set_rsa_oaep_md(ctx, oaep_md) != 1 ||
        EVP_PKEY_CTX_set_rsa_mgf1_md(ctx, oaep_md) != 1) {
      EVP_PKEY_CTX_free(ctx);
      EVP_PKEY_free(pkey);
      ThrowLastOpenSslError(env, "ERR_OSSL_EVP_INVALID_DIGEST", "Invalid digest used");
      return nullptr;
    }
    if (has_label && label_len > 0) {
      unsigned char* copied = reinterpret_cast<unsigned char*>(OPENSSL_malloc(label_len));
      if (copied == nullptr) {
        EVP_PKEY_CTX_free(ctx);
        EVP_PKEY_free(pkey);
        ThrowError(env, "ERR_CRYPTO_OPERATION_FAILED", "Failed to allocate OAEP label");
        return nullptr;
      }
      std::memcpy(copied, label, label_len);
      if (EVP_PKEY_CTX_set0_rsa_oaep_label(ctx, copied, static_cast<int>(label_len)) != 1) {
        OPENSSL_free(copied);
        EVP_PKEY_CTX_free(ctx);
        EVP_PKEY_free(pkey);
        ThrowLastOpenSslError(env, "ERR_CRYPTO_OPERATION_FAILED", "Failed to set OAEP label");
        return nullptr;
      }
    }
  }
  size_t out_len = 0;
  if (EVP_PKEY_decrypt(ctx, nullptr, &out_len, input, in_len) != 1) {
    EVP_PKEY_CTX_free(ctx);
    EVP_PKEY_free(pkey);
    ThrowLastOpenSslError(env, "ERR_CRYPTO_OPERATION_FAILED", "privateDecrypt failed");
    return nullptr;
  }
  std::vector<uint8_t> out(out_len);
  if (EVP_PKEY_decrypt(ctx, out.data(), &out_len, input, in_len) != 1) {
    EVP_PKEY_CTX_free(ctx);
    EVP_PKEY_free(pkey);
    ThrowLastOpenSslError(env, "ERR_CRYPTO_OPERATION_FAILED", "privateDecrypt failed");
    return nullptr;
  }
  EVP_PKEY_CTX_free(ctx);
  EVP_PKEY_free(pkey);
  return CreateBufferCopy(env, out.data(), out_len);
}

napi_value CryptoPublicDecrypt(napi_env env, napi_callback_info info) {
  size_t argc = 8;
  napi_value argv[8] = {nullptr, nullptr, nullptr, nullptr, nullptr, nullptr, nullptr, nullptr};
  if (napi_get_cb_info(env, info, &argc, argv, nullptr, nullptr) != napi_ok || argc < 5) return nullptr;
  std::vector<uint8_t> owned_key;
  uint8_t* key_bytes = nullptr;
  size_t key_len = 0;
  uint8_t* input = nullptr;
  size_t in_len = 0;
  if (!GetKeyBytes(env, argv[0], &owned_key, &key_bytes, &key_len) ||
      !GetBufferBytes(env, argv[4], &input, &in_len)) {
    ThrowError(env, "ERR_INVALID_ARG_TYPE", "key and buffer must be Buffers or strings");
    return nullptr;
  }

  std::string passphrase;
  bool has_passphrase = false;
  if (argc >= 4 && argv[3] != nullptr && !ReadPassphrase(env, argv[3], &passphrase, &has_passphrase)) {
    ThrowError(env, "ERR_INVALID_ARG_TYPE", "Invalid key passphrase");
    return nullptr;
  }

  int32_t padding = RSA_PKCS1_PADDING;
  if (argc >= 6 && argv[5] != nullptr) napi_get_value_int32(env, argv[5], &padding);

  EVP_PKEY* pkey = ParsePublicKeyOrCert(key_bytes, key_len);
  if (pkey == nullptr) {
    ERR_clear_error();
    pkey = ParsePrivateKeyWithPassphrase(key_bytes,
                                         key_len,
                                         reinterpret_cast<const uint8_t*>(passphrase.data()),
                                         passphrase.size(),
                                         has_passphrase);
  }
  if (pkey == nullptr) {
    ThrowLastOpenSslError(env, "ERR_CRYPTO_OPERATION_FAILED", "Failed to parse public key");
    return nullptr;
  }
  RSA* rsa = EVP_PKEY_get1_RSA(pkey);
  EVP_PKEY_free(pkey);
  if (rsa == nullptr) {
    ThrowLastOpenSslError(env, "ERR_CRYPTO_INVALID_KEY_OBJECT_TYPE", "Invalid public key");
    return nullptr;
  }

  std::vector<uint8_t> out(static_cast<size_t>(RSA_size(rsa)));
  const int written = RSA_public_decrypt(static_cast<int>(in_len), input, out.data(), rsa, padding);
  RSA_free(rsa);
  if (written < 0) {
    ThrowLastOpenSslError(env, "ERR_CRYPTO_OPERATION_FAILED", "publicDecrypt failed");
    return nullptr;
  }
  return CreateBufferCopy(env, out.data(), static_cast<size_t>(written));
}

napi_value CryptoCipherTransformAead(napi_env env, napi_callback_info info) {
  size_t argc = 8;
  napi_value argv[8] = {nullptr, nullptr, nullptr, nullptr, nullptr, nullptr, nullptr, nullptr};
  if (napi_get_cb_info(env, info, &argc, argv, nullptr, nullptr) != napi_ok || argc < 6) return nullptr;
  const std::string algo = ValueToUtf8(env, argv[0]);
  uint8_t *key = nullptr, *iv = nullptr, *input = nullptr, *aad = nullptr, *auth_tag = nullptr;
  size_t key_len = 0, iv_len = 0, in_len = 0, aad_len = 0, auth_tag_len = 0;
  if (!GetBufferBytes(env, argv[1], &key, &key_len) ||
      !GetBufferBytes(env, argv[2], &iv, &iv_len) ||
      !GetBufferBytes(env, argv[3], &input, &in_len) ||
      !GetBufferBytes(env, argv[5], &aad, &aad_len)) {
    ThrowError(env, "ERR_INVALID_ARG_TYPE", "aead arguments must be Buffers");
    return nullptr;
  }
  bool decrypt = false;
  napi_get_value_bool(env, argv[4], &decrypt);
  if (argc >= 7 && argv[6] != nullptr) {
    napi_valuetype tag_type = napi_undefined;
    napi_typeof(env, argv[6], &tag_type);
    if (tag_type != napi_undefined && tag_type != napi_null) {
      if (!GetBufferBytes(env, argv[6], &auth_tag, &auth_tag_len)) {
        ThrowError(env, "ERR_INVALID_ARG_TYPE", "auth tag must be a Buffer");
        return nullptr;
      }
    }
  }
  int32_t requested_tag_len = 16;
  if (argc >= 8 && argv[7] != nullptr) {
    napi_get_value_int32(env, argv[7], &requested_tag_len);
    if (requested_tag_len <= 0 || requested_tag_len > 16) requested_tag_len = 16;
  }

  const EVP_CIPHER* cipher = EVP_get_cipherbyname(algo.c_str());
  if (cipher == nullptr) {
    ThrowError(env, "ERR_CRYPTO_UNKNOWN_CIPHER", "Unknown cipher");
    return nullptr;
  }
  const ncrypto::Cipher resolved = ResolveCipher(algo);
  const bool is_ccm = resolved && resolved.isCcmMode();
  const bool is_ocb = resolved && resolved.isOcbMode();
  EVP_CIPHER_CTX* ctx = EVP_CIPHER_CTX_new();
  if (ctx == nullptr) {
    ThrowError(env, "ERR_CRYPTO_OPERATION_FAILED", "Failed to create cipher context");
    return nullptr;
  }
  int ok = EVP_CipherInit_ex(ctx, cipher, nullptr, nullptr, nullptr, decrypt ? 0 : 1);
  if (ok == 1) ok = EVP_CIPHER_CTX_ctrl(ctx, EVP_CTRL_AEAD_SET_IVLEN, static_cast<int>(iv_len), nullptr);
  if (ok == 1 && (is_ccm || is_ocb) && requested_tag_len > 0) {
    void* tag_ptr = (is_ccm && decrypt && auth_tag != nullptr) ? auth_tag : nullptr;
    ok = EVP_CIPHER_CTX_ctrl(ctx, EVP_CTRL_AEAD_SET_TAG, requested_tag_len, tag_ptr);
  }
  if (ok == 1) ok = EVP_CipherInit_ex(ctx, nullptr, nullptr, key, iv, decrypt ? 0 : 1);
  int tmp_len = 0;
  if (ok == 1 && is_ccm) ok = EVP_CipherUpdate(ctx, nullptr, &tmp_len, nullptr, static_cast<int>(in_len));
  if (ok == 1 && aad_len > 0) ok = EVP_CipherUpdate(ctx, nullptr, &tmp_len, aad, static_cast<int>(aad_len));
  if (ok == 1 && decrypt && auth_tag != nullptr && !is_ccm) {
    ok = EVP_CIPHER_CTX_ctrl(ctx, EVP_CTRL_AEAD_SET_TAG, static_cast<int>(auth_tag_len), auth_tag);
  }
  std::vector<uint8_t> out(in_len + 32);
  int out_len = 0;
  if (ok == 1) ok = EVP_CipherUpdate(ctx, out.data(), &out_len, input, static_cast<int>(in_len));
  int final_len = 0;
  if (ok == 1) ok = EVP_CipherFinal_ex(ctx, out.data() + out_len, &final_len);
  out.resize(static_cast<size_t>(out_len + final_len));

  std::vector<uint8_t> out_tag;
  if (ok == 1 && !decrypt) {
    out_tag.resize(static_cast<size_t>(requested_tag_len));
    if (EVP_CIPHER_CTX_ctrl(ctx, EVP_CTRL_AEAD_GET_TAG, requested_tag_len, out_tag.data()) != 1) {
      out_tag.clear();
    }
  }
  EVP_CIPHER_CTX_free(ctx);
  if (ok != 1) {
    if (decrypt) {
      ERR_clear_error();
      ThrowError(env, "ERR_CRYPTO_INVALID_STATE", "Unsupported state or unable to authenticate data");
    } else {
      ThrowLastOpenSslError(env, "ERR_CRYPTO_OPERATION_FAILED", "AEAD cipher operation failed");
    }
    return nullptr;
  }
  napi_value result = nullptr;
  napi_create_object(env, &result);
  napi_value out_v = CreateBufferCopy(env, out.data(), out.size());
  napi_set_named_property(env, result, "output", out_v);
  napi_value tag_v = out_tag.empty() ? nullptr : CreateBufferCopy(env, out_tag.data(), out_tag.size());
  if (tag_v == nullptr) {
    napi_get_null(env, &tag_v);
  }
  napi_set_named_property(env, result, "authTag", tag_v);
  return result;
}

napi_value CryptoSignOneShot(napi_env env, napi_callback_info info) {
  size_t argc = 10;
  napi_value argv[10] = {nullptr, nullptr, nullptr, nullptr, nullptr, nullptr, nullptr, nullptr, nullptr, nullptr};
  if (napi_get_cb_info(env, info, &argc, argv, nullptr, nullptr) != napi_ok || argc < 3) return nullptr;
  const bool extended_key_args = argc >= 10;

  bool null_digest = false;
  napi_valuetype digest_type = napi_undefined;
  if (napi_typeof(env, argv[0], &digest_type) == napi_ok) {
    null_digest = (digest_type == napi_null || digest_type == napi_undefined);
  }
  const std::string algo = null_digest ? std::string() : ValueToUtf8(env, argv[0]);
  uint8_t* data = nullptr;
  size_t data_len = 0;
  std::vector<uint8_t> key_owned;
  uint8_t* key_bytes = nullptr;
  size_t key_len = 0;
  napi_value key_value = argv[2];
  if (!GetBufferBytes(env, argv[1], &data, &data_len) ||
      !GetKeyBytes(env, key_value, &key_owned, &key_bytes, &key_len)) {
    ThrowError(env, "ERR_INVALID_ARG_TYPE", "data and key must be Buffers or strings");
    return nullptr;
  }

  std::string passphrase;
  bool has_passphrase = false;
  int32_t key_format = -1;
  if (extended_key_args && argc >= 4 && argv[3] != nullptr) {
    napi_valuetype key_format_type = napi_undefined;
    if (napi_typeof(env, argv[3], &key_format_type) == napi_ok && key_format_type == napi_number) {
      napi_get_value_int32(env, argv[3], &key_format);
    }
  }
  if (extended_key_args && argc >= 6 && !IsNullOrUndefined(env, argv[5])) {
    if (!ReadPassphrase(env, argv[5], &passphrase, &has_passphrase)) {
      ThrowError(env, "ERR_INVALID_ARG_TYPE", "Invalid key passphrase");
      return nullptr;
    }
  }

  int32_t padding = 0;
  int32_t salt_len = INT32_MIN;
  int32_t dsa_sig_enc = 0;
  napi_valuetype padding_type = napi_undefined;
  napi_valuetype salt_type = napi_undefined;
  napi_value padding_arg = extended_key_args ? argv[6] : (argc >= 4 ? argv[3] : nullptr);
  napi_value salt_arg = extended_key_args ? argv[7] : (argc >= 5 ? argv[4] : nullptr);
  napi_value dsa_sig_enc_arg = extended_key_args ? argv[8] : nullptr;
  if (padding_arg != nullptr &&
      napi_typeof(env, padding_arg, &padding_type) == napi_ok &&
      padding_type == napi_number) {
    napi_get_value_int32(env, padding_arg, &padding);
  }
  if (salt_arg != nullptr &&
      napi_typeof(env, salt_arg, &salt_type) == napi_ok &&
      salt_type == napi_number) {
    napi_get_value_int32(env, salt_arg, &salt_len);
  }
  if (dsa_sig_enc_arg != nullptr) {
    napi_valuetype dsa_sig_enc_type = napi_undefined;
    if (napi_typeof(env, dsa_sig_enc_arg, &dsa_sig_enc_type) == napi_ok &&
        dsa_sig_enc_type == napi_number) {
      napi_get_value_int32(env, dsa_sig_enc_arg, &dsa_sig_enc);
    }
  }
  uint8_t* context = nullptr;
  size_t context_len = 0;
  bool has_context = false;
  napi_value context_arg = extended_key_args ? argv[9] : (argc >= 6 ? argv[5] : nullptr);
  if (context_arg != nullptr) {
    napi_valuetype context_type = napi_undefined;
    if (napi_typeof(env, context_arg, &context_type) == napi_ok &&
        context_type != napi_null &&
        context_type != napi_undefined) {
      if (!GetBufferBytes(env, context_arg, &context, &context_len)) {
        ThrowError(env, "ERR_INVALID_ARG_TYPE", "context must be a Buffer");
        return nullptr;
      }
      has_context = true;
    }
  }

  EVP_PKEY* pkey = ParsePrivateKeyWithPassphrase(key_bytes,
                                                 key_len,
                                                 reinterpret_cast<const uint8_t*>(passphrase.data()),
                                                 passphrase.size(),
                                                 has_passphrase);
  if (pkey == nullptr) {
    if (!has_passphrase && key_format == 0) {
      napi_throw_type_error(env, "ERR_MISSING_PASSPHRASE", "Passphrase required for encrypted key");
      return nullptr;
    }
    ThrowLastOpenSslError(env, "ERR_CRYPTO_OPERATION_FAILED", "sign failed");
    return nullptr;
  }
  const ncrypto::Digest md = null_digest ? ncrypto::Digest(nullptr) : ResolveDigest(algo);
  if (!null_digest && !md) {
    EVP_PKEY_free(pkey);
    ThrowError(env, "ERR_CRYPTO_INVALID_DIGEST", "Invalid digest");
    return nullptr;
  }

  EVP_MD_CTX* mctx = EVP_MD_CTX_new();
  if (mctx == nullptr) {
    EVP_PKEY_free(pkey);
    ThrowError(env, "ERR_CRYPTO_OPERATION_FAILED", "Failed to create digest context");
    return nullptr;
  }
  const int pkey_type = ncrypto::EVPKeyPointer::id(pkey);
  const bool is_rsa_family = IsRsaVariant(pkey_type);
  const bool is_one_shot_variant = IsOneShotVariant(pkey_type);
  const bool is_ed_key = (pkey_type == EVP_PKEY_ED25519 || pkey_type == EVP_PKEY_ED448);
  if (has_context && context_len > 255) {
    EVP_MD_CTX_free(mctx);
    EVP_PKEY_free(pkey);
    ThrowError(env, "ERR_OUT_OF_RANGE", "context string must be at most 255 bytes");
    return nullptr;
  }
  if (has_context && context_len > 0 && !SupportsContextString(pkey_type)) {
    EVP_MD_CTX_free(mctx);
    EVP_PKEY_free(pkey);
    ThrowError(env, "ERR_CRYPTO_UNSUPPORTED_OPERATION", "Context parameter is unsupported");
    return nullptr;
  }
  int effective_padding = padding;
  if (is_rsa_family && effective_padding == 0 && pkey_type == EVP_PKEY_RSA_PSS) {
    // RSA-PSS keys require PSS padding even when callers omit padding.
    effective_padding = RSA_PKCS1_PSS_PADDING;
  }
  if (is_rsa_family && !null_digest) {
    std::vector<uint8_t> digest;
    const int digest_len = EVP_MD_get_size(md.get());
    bool ok = digest_len > 0 &&
              EVP_DigestInit_ex(mctx, md.get(), nullptr) == 1 &&
              EVP_DigestUpdate(mctx, data, data_len) == 1;
    if (ok) {
      unsigned int written = 0;
      digest.resize(static_cast<size_t>(digest_len));
      ok = EVP_DigestFinal_ex(mctx, digest.data(), &written) == 1;
      if (ok) digest.resize(written);
    }
    EVP_MD_CTX_free(mctx);
    if (!ok) {
      EVP_PKEY_free(pkey);
      ThrowLastOpenSslError(env, "ERR_CRYPTO_OPERATION_FAILED", "sign failed");
      return nullptr;
    }

    EVP_PKEY_CTX* sign_ctx = EVP_PKEY_CTX_new(pkey, nullptr);
    ok = sign_ctx != nullptr && EVP_PKEY_sign_init(sign_ctx) == 1;
    if (ok && effective_padding != 0) {
      ok = EVP_PKEY_CTX_set_rsa_padding(sign_ctx, effective_padding) == 1;
      if (ok && effective_padding == RSA_PKCS1_PSS_PADDING && salt_len != INT32_MIN) {
        ok = EVP_PKEY_CTX_set_rsa_pss_saltlen(sign_ctx, salt_len) == 1;
      }
    }
    if (ok) {
      ok = EVP_PKEY_CTX_set_signature_md(sign_ctx, md.get()) == 1;
    }

    size_t sig_len = 0;
    std::vector<uint8_t> sig;
    if (ok) ok = EVP_PKEY_sign(sign_ctx, nullptr, &sig_len, digest.data(), digest.size()) == 1;
    if (ok) {
      sig.resize(sig_len);
      ok = EVP_PKEY_sign(sign_ctx, sig.data(), &sig_len, digest.data(), digest.size()) == 1;
    }
    if (sign_ctx != nullptr) EVP_PKEY_CTX_free(sign_ctx);

    if (!ok) {
      EVP_PKEY_free(pkey);
      ThrowLastOpenSslError(env, "ERR_CRYPTO_OPERATION_FAILED", "sign failed");
      return nullptr;
    }

    if (dsa_sig_enc == 1 && (pkey_type == EVP_PKEY_EC || pkey_type == EVP_PKEY_DSA)) {
      const size_t part_len = GetDsaSigPartLengthFromPkey(pkey);
      std::vector<uint8_t> p1363;
      if (part_len == 0 || !DerSignatureToP1363(sig.data(), sig_len, part_len, &p1363)) {
        EVP_PKEY_free(pkey);
        ThrowError(env, "ERR_CRYPTO_OPERATION_FAILED", "Malformed signature");
        return nullptr;
      }
      sig = std::move(p1363);
      sig_len = sig.size();
    }
    EVP_PKEY_free(pkey);
    return CreateBufferCopy(env, sig.data(), sig_len);
  }
  EVP_PKEY_CTX* pctx = nullptr;
  bool ok = false;
#ifdef OSSL_SIGNATURE_PARAM_CONTEXT_STRING
  if (has_context && context_len > 0) {
    OSSL_PARAM params[] = {
        OSSL_PARAM_construct_octet_string(
            OSSL_SIGNATURE_PARAM_CONTEXT_STRING,
            const_cast<unsigned char*>(context),
            context_len),
        OSSL_PARAM_END};
    ok = EVP_DigestSignInit_ex(
             mctx,
             &pctx,
             nullptr,
             nullptr,
             nullptr,
             pkey,
             params) == 1;
    if (!ok) {
      EVP_MD_CTX_free(mctx);
      EVP_PKEY_free(pkey);
      ThrowError(env, "ERR_CRYPTO_UNSUPPORTED_OPERATION", "Context parameter is unsupported");
      return nullptr;
    }
  } else {
    ok = EVP_DigestSignInit(mctx, &pctx, null_digest ? nullptr : md.get(), nullptr, pkey) == 1;
  }
#else
  if (has_context && context_len > 0) {
    EVP_MD_CTX_free(mctx);
    EVP_PKEY_free(pkey);
    ThrowError(env, "ERR_CRYPTO_UNSUPPORTED_OPERATION", "Context parameter is unsupported");
    return nullptr;
  }
  ok = EVP_DigestSignInit(mctx, &pctx, null_digest ? nullptr : md.get(), nullptr, pkey) == 1;
#endif
  if (ok && pctx != nullptr && is_rsa_family && effective_padding != 0) {
    ok = EVP_PKEY_CTX_set_rsa_padding(pctx, effective_padding) == 1;
    if (ok && effective_padding == RSA_PKCS1_PSS_PADDING && salt_len != INT32_MIN) {
      ok = EVP_PKEY_CTX_set_rsa_pss_saltlen(pctx, salt_len) == 1;
    }
  }
  if (ok && is_ed_key && !null_digest) {
    EVP_MD_CTX_free(mctx);
    EVP_PKEY_free(pkey);
    ThrowError(env, "ERR_CRYPTO_UNSUPPORTED_OPERATION", "Unsupported crypto operation");
    return nullptr;
  }
  size_t sig_len = 0;
  std::vector<uint8_t> sig;
  if (ok && is_one_shot_variant) {
    ok = EVP_DigestSign(mctx, nullptr, &sig_len, data, data_len) == 1;
    if (ok) {
      sig.resize(sig_len);
      ok = EVP_DigestSign(mctx, sig.data(), &sig_len, data, data_len) == 1;
    }
  } else {
    if (ok) ok = EVP_DigestSignUpdate(mctx, data, data_len) == 1;
    if (ok) ok = EVP_DigestSignFinal(mctx, nullptr, &sig_len) == 1;
    if (ok) {
      sig.resize(sig_len);
      ok = EVP_DigestSignFinal(mctx, sig.data(), &sig_len) == 1;
    }
  }
  EVP_MD_CTX_free(mctx);

  if (!ok) {
    EVP_PKEY_free(pkey);
    ThrowLastOpenSslError(env, "ERR_CRYPTO_OPERATION_FAILED", "sign failed");
    return nullptr;
  }

  if (dsa_sig_enc == 1 && (pkey_type == EVP_PKEY_EC || pkey_type == EVP_PKEY_DSA)) {
    const size_t part_len = GetDsaSigPartLengthFromPkey(pkey);
    std::vector<uint8_t> p1363;
    if (part_len == 0 || !DerSignatureToP1363(sig.data(), sig_len, part_len, &p1363)) {
      EVP_PKEY_free(pkey);
      ThrowError(env, "ERR_CRYPTO_OPERATION_FAILED", "Malformed signature");
      return nullptr;
    }
    sig = std::move(p1363);
    sig_len = sig.size();
  }
  EVP_PKEY_free(pkey);
  return CreateBufferCopy(env, sig.data(), sig_len);
}

napi_value CryptoVerifyOneShot(napi_env env, napi_callback_info info) {
  size_t argc = 11;
  napi_value argv[11] = {
      nullptr, nullptr, nullptr, nullptr, nullptr, nullptr, nullptr, nullptr, nullptr, nullptr, nullptr};
  if (napi_get_cb_info(env, info, &argc, argv, nullptr, nullptr) != napi_ok || argc < 4) return nullptr;
  const bool extended_key_args = argc >= 11;

  bool null_digest = false;
  napi_valuetype digest_type = napi_undefined;
  if (napi_typeof(env, argv[0], &digest_type) == napi_ok) {
    null_digest = (digest_type == napi_null || digest_type == napi_undefined);
  }
  const std::string algo = null_digest ? std::string() : ValueToUtf8(env, argv[0]);
  uint8_t* data = nullptr;
  size_t data_len = 0;
  std::vector<uint8_t> key_owned;
  uint8_t* key_bytes = nullptr;
  size_t key_len = 0;
  uint8_t* sig = nullptr;
  size_t sig_len = 0;
  std::vector<uint8_t> signature_storage;
  napi_value key_value = argv[2];
  napi_value signature_value = extended_key_args ? argv[6] : argv[3];
  if (!GetBufferBytes(env, argv[1], &data, &data_len) ||
      !GetKeyBytes(env, key_value, &key_owned, &key_bytes, &key_len) ||
      !GetBufferBytes(env, signature_value, &sig, &sig_len)) {
    ThrowError(env, "ERR_INVALID_ARG_TYPE", "data, key and signature must be Buffers or strings");
    return nullptr;
  }
  std::string passphrase;
  bool has_passphrase = false;
  int32_t key_format = -1;
  if (extended_key_args && argc >= 4 && argv[3] != nullptr) {
    napi_valuetype key_format_type = napi_undefined;
    if (napi_typeof(env, argv[3], &key_format_type) == napi_ok && key_format_type == napi_number) {
      napi_get_value_int32(env, argv[3], &key_format);
    }
  }
  if (extended_key_args && !IsNullOrUndefined(env, argv[5])) {
    if (!ReadPassphrase(env, argv[5], &passphrase, &has_passphrase)) {
      ThrowError(env, "ERR_INVALID_ARG_TYPE", "Invalid key passphrase");
      return nullptr;
    }
  }
  int32_t padding = 0;
  int32_t salt_len = INT32_MIN;
  int32_t dsa_sig_enc = 0;
  napi_valuetype padding_type = napi_undefined;
  napi_valuetype salt_type = napi_undefined;
  napi_value padding_arg = extended_key_args ? argv[7] : (argc >= 5 ? argv[4] : nullptr);
  napi_value salt_arg = extended_key_args ? argv[8] : (argc >= 6 ? argv[5] : nullptr);
  napi_value dsa_sig_enc_arg = extended_key_args ? argv[9] : nullptr;
  if (padding_arg != nullptr &&
      napi_typeof(env, padding_arg, &padding_type) == napi_ok &&
      padding_type == napi_number) {
    napi_get_value_int32(env, padding_arg, &padding);
  }
  if (salt_arg != nullptr &&
      napi_typeof(env, salt_arg, &salt_type) == napi_ok &&
      salt_type == napi_number) {
    napi_get_value_int32(env, salt_arg, &salt_len);
  }
  if (dsa_sig_enc_arg != nullptr) {
    napi_valuetype dsa_sig_enc_type = napi_undefined;
    if (napi_typeof(env, dsa_sig_enc_arg, &dsa_sig_enc_type) == napi_ok &&
        dsa_sig_enc_type == napi_number) {
      napi_get_value_int32(env, dsa_sig_enc_arg, &dsa_sig_enc);
    }
  }
  uint8_t* context = nullptr;
  size_t context_len = 0;
  bool has_context = false;
  napi_value context_arg = extended_key_args ? argv[10] : (argc >= 7 ? argv[6] : nullptr);
  if (context_arg != nullptr) {
    napi_valuetype context_type = napi_undefined;
    if (napi_typeof(env, context_arg, &context_type) == napi_ok &&
        context_type != napi_null &&
        context_type != napi_undefined) {
      if (!GetBufferBytes(env, context_arg, &context, &context_len)) {
        ThrowError(env, "ERR_INVALID_ARG_TYPE", "context must be a Buffer");
        return nullptr;
      }
      has_context = true;
    }
  }

  EVP_PKEY* pkey = ParsePublicKeyOrCert(key_bytes, key_len);
  if (pkey == nullptr) {
    pkey = ParsePrivateKeyWithPassphrase(key_bytes,
                                         key_len,
                                         reinterpret_cast<const uint8_t*>(passphrase.data()),
                                         passphrase.size(),
                                         has_passphrase);
  }
  if (pkey == nullptr) {
    if (!has_passphrase && key_format == 0) {
      napi_throw_type_error(env, "ERR_MISSING_PASSPHRASE", "Passphrase required for encrypted key");
      return nullptr;
    }
    ThrowLastOpenSslError(env, "ERR_CRYPTO_OPERATION_FAILED", "verify failed");
    return nullptr;
  }
  const ncrypto::Digest md = null_digest ? ncrypto::Digest(nullptr) : ResolveDigest(algo);
  if (!null_digest && !md) {
    EVP_PKEY_free(pkey);
    ThrowError(env, "ERR_CRYPTO_INVALID_DIGEST", "Invalid digest");
    return nullptr;
  }

  EVP_MD_CTX* mctx = EVP_MD_CTX_new();
  if (mctx == nullptr) {
    EVP_PKEY_free(pkey);
    ThrowError(env, "ERR_CRYPTO_OPERATION_FAILED", "Failed to create digest context");
    return nullptr;
  }
  const int pkey_type = ncrypto::EVPKeyPointer::id(pkey);
  const bool is_rsa_family = IsRsaVariant(pkey_type);
  const bool is_one_shot_variant = IsOneShotVariant(pkey_type);
  const bool is_ed_key = (pkey_type == EVP_PKEY_ED25519 || pkey_type == EVP_PKEY_ED448);
  if (dsa_sig_enc == 1 && (pkey_type == EVP_PKEY_EC || pkey_type == EVP_PKEY_DSA)) {
    const size_t part_len = GetDsaSigPartLengthFromPkey(pkey);
    if (part_len == 0 || sig_len != part_len * 2 ||
        !P1363ToDerSignature(pkey_type, sig, sig_len, part_len, &signature_storage)) {
      EVP_MD_CTX_free(mctx);
      EVP_PKEY_free(pkey);
      napi_value out = nullptr;
      napi_get_boolean(env, false, &out);
      return out != nullptr ? out : nullptr;
    }
    sig = signature_storage.data();
    sig_len = signature_storage.size();
  }
  if (has_context && context_len > 255) {
    EVP_MD_CTX_free(mctx);
    EVP_PKEY_free(pkey);
    ThrowError(env, "ERR_OUT_OF_RANGE", "context string must be at most 255 bytes");
    return nullptr;
  }
  if (has_context && context_len > 0 && !SupportsContextString(pkey_type)) {
    EVP_MD_CTX_free(mctx);
    EVP_PKEY_free(pkey);
    ThrowError(env, "ERR_CRYPTO_UNSUPPORTED_OPERATION", "Context parameter is unsupported");
    return nullptr;
  }
  int effective_padding = padding;
  if (is_rsa_family && effective_padding == 0 && pkey_type == EVP_PKEY_RSA_PSS) {
    effective_padding = RSA_PKCS1_PSS_PADDING;
  }
  if (is_rsa_family && !null_digest) {
    std::vector<uint8_t> digest;
    const int digest_len = EVP_MD_get_size(md.get());
    bool ok = digest_len > 0 &&
              EVP_DigestInit_ex(mctx, md.get(), nullptr) == 1 &&
              EVP_DigestUpdate(mctx, data, data_len) == 1;
    if (ok) {
      unsigned int written = 0;
      digest.resize(static_cast<size_t>(digest_len));
      ok = EVP_DigestFinal_ex(mctx, digest.data(), &written) == 1;
      if (ok) digest.resize(written);
    }
    EVP_MD_CTX_free(mctx);
    if (!ok) {
      EVP_PKEY_free(pkey);
      ThrowLastOpenSslError(env, "ERR_CRYPTO_OPERATION_FAILED", "verify failed");
      return nullptr;
    }

    EVP_PKEY_CTX* verify_ctx = EVP_PKEY_CTX_new(pkey, nullptr);
    ok = verify_ctx != nullptr && EVP_PKEY_verify_init(verify_ctx) == 1;
    if (ok && effective_padding != 0) {
      ok = EVP_PKEY_CTX_set_rsa_padding(verify_ctx, effective_padding) == 1;
      if (ok && effective_padding == RSA_PKCS1_PSS_PADDING && salt_len != INT32_MIN) {
        ok = EVP_PKEY_CTX_set_rsa_pss_saltlen(verify_ctx, salt_len) == 1;
      }
    }
    if (ok) {
      ok = EVP_PKEY_CTX_set_signature_md(verify_ctx, md.get()) == 1;
    }

    int vr = 0;
    if (ok) {
      vr = EVP_PKEY_verify(verify_ctx, sig, sig_len, digest.data(), digest.size());
    }
    if (verify_ctx != nullptr) EVP_PKEY_CTX_free(verify_ctx);
    EVP_PKEY_free(pkey);

    if (!ok) {
      ThrowLastOpenSslError(env, "ERR_CRYPTO_OPERATION_FAILED", "verify failed");
      return nullptr;
    }
    if (vr != 1) ERR_clear_error();

    napi_value out = nullptr;
    napi_get_boolean(env, vr == 1, &out);
    return out;
  }
  EVP_PKEY_CTX* pctx = nullptr;
  bool ok = false;
#ifdef OSSL_SIGNATURE_PARAM_CONTEXT_STRING
  if (has_context && context_len > 0) {
    OSSL_PARAM params[] = {
        OSSL_PARAM_construct_octet_string(
            OSSL_SIGNATURE_PARAM_CONTEXT_STRING,
            const_cast<unsigned char*>(context),
            context_len),
        OSSL_PARAM_END};
    ok = EVP_DigestVerifyInit_ex(
             mctx,
             &pctx,
             nullptr,
             nullptr,
             nullptr,
             pkey,
             params) == 1;
    if (!ok) {
      EVP_MD_CTX_free(mctx);
      EVP_PKEY_free(pkey);
      ThrowError(env, "ERR_CRYPTO_UNSUPPORTED_OPERATION", "Context parameter is unsupported");
      return nullptr;
    }
  } else {
    ok = EVP_DigestVerifyInit(mctx, &pctx, null_digest ? nullptr : md.get(), nullptr, pkey) == 1;
  }
#else
  if (has_context && context_len > 0) {
    EVP_MD_CTX_free(mctx);
    EVP_PKEY_free(pkey);
    ThrowError(env, "ERR_CRYPTO_UNSUPPORTED_OPERATION", "Context parameter is unsupported");
    return nullptr;
  }
  ok = EVP_DigestVerifyInit(mctx, &pctx, null_digest ? nullptr : md.get(), nullptr, pkey) == 1;
#endif
  if (ok && pctx != nullptr && is_rsa_family && effective_padding != 0) {
    ok = EVP_PKEY_CTX_set_rsa_padding(pctx, effective_padding) == 1;
    if (ok && effective_padding == RSA_PKCS1_PSS_PADDING && salt_len != INT32_MIN) {
      ok = EVP_PKEY_CTX_set_rsa_pss_saltlen(pctx, salt_len) == 1;
    }
  }
  int vr = 0;
  if (ok && is_ed_key && !null_digest) {
    ok = false;
    ThrowError(env, "ERR_CRYPTO_UNSUPPORTED_OPERATION", "Unsupported crypto operation");
  }
  if (ok && is_one_shot_variant) {
    vr = EVP_DigestVerify(mctx, sig, sig_len, data, data_len);
  } else {
    if (ok) ok = EVP_DigestVerifyUpdate(mctx, data, data_len) == 1;
    vr = ok ? EVP_DigestVerifyFinal(mctx, sig, sig_len) : 0;
  }
  EVP_MD_CTX_free(mctx);
  EVP_PKEY_free(pkey);

  if (!ok) {
    bool pending = false;
    if (napi_is_exception_pending(env, &pending) != napi_ok || !pending) {
      ThrowLastOpenSslError(env, "ERR_CRYPTO_OPERATION_FAILED", "verify failed");
    }
    return nullptr;
  }
  if (vr != 1) ERR_clear_error();

  napi_value out = nullptr;
  napi_get_boolean(env, vr == 1, &out);
  return out;
}

void SetMethod(napi_env env, napi_value obj, const char* name, napi_callback fn) {
  napi_value method = nullptr;
  if (napi_create_function(env, name, NAPI_AUTO_LENGTH, fn, nullptr, &method) == napi_ok && method != nullptr) {
    napi_set_named_property(env, obj, name, method);
  }
}

}  // namespace

bool GetAnyBufferSourceBytes(napi_env env, napi_value value, uint8_t** data, size_t* len) {
  return GetAnyBufferSourceBytesImpl(env, value, data, len);
}

EVP_PKEY* ParsePrivateKeyWithPassphrase(const uint8_t* data,
                                        size_t len,
                                        const uint8_t* passphrase,
                                        size_t passphrase_len,
                                        bool has_passphrase) {
  return ParsePrivateKeyWithPassphraseImpl(data, len, passphrase, passphrase_len, has_passphrase);
}

napi_value CryptoGetBundledRootCertificates(napi_env env, napi_callback_info /*info*/) {
  std::vector<std::string> certs;
  certs.reserve(sizeof(kBundledRootCerts) / sizeof(kBundledRootCerts[0]));
  for (const char* pem : kBundledRootCerts) {
    if (pem != nullptr) certs.emplace_back(pem);
  }
  return CreateStringArray(env, certs);
}

napi_value CryptoGetExtraCACertificates(napi_env env, napi_callback_info /*info*/) {
  return CreatePemArrayFromX509Vector(env, GetExtraRootCertificatesParsed());
}

napi_value CryptoGetSystemCACertificates(napi_env env, napi_callback_info /*info*/) {
  return CreatePemArrayFromX509Vector(env, GetSystemRootCertificatesParsed());
}

napi_value CryptoGetUserRootCertificates(napi_env env, napi_callback_info /*info*/) {
  std::lock_guard<std::mutex> lock(g_user_root_certs_mutex);
  if (g_user_root_certs == nullptr) {
    napi_value out = nullptr;
    napi_create_array(env, &out);
    return out;
  }

  std::vector<X509*> certs;
  certs.reserve(g_user_root_certs->size());
  for (X509* cert : *g_user_root_certs) {
    certs.push_back(cert);
  }
  return CreatePemArrayFromX509Vector(env, certs);
}

napi_value CryptoResetRootCertStore(napi_env env, napi_callback_info info) {
  size_t argc = 1;
  napi_value argv[1] = {nullptr};
  if (napi_get_cb_info(env, info, &argc, argv, nullptr, nullptr) != napi_ok || argc < 1 || argv[0] == nullptr) {
    return nullptr;
  }

  std::vector<std::string> cert_texts;
  if (!GetByteStringArray(env, argv[0], &cert_texts)) {
    ThrowError(env, "ERR_INVALID_ARG_TYPE", "certs must be an Array");
    return nullptr;
  }

  auto next_user_root_certs = std::make_unique<X509Set>();
  std::vector<X509*> parsed_certs;
  parsed_certs.reserve(cert_texts.size());
  bool saw_no_valid_cert = false;

  for (const auto& cert_text : cert_texts) {
    if (IsPlainTextBlob(cert_text) && !LooksLikePemCertificateText(cert_text)) {
      saw_no_valid_cert = true;
      continue;
    }

    BIO* bio = BIO_new_mem_buf(cert_text.data(), static_cast<int>(cert_text.size()));
    if (bio == nullptr) {
      FreeCertificates(&parsed_certs);
      ThrowLastOpenSslError(env, "ERR_TLS_CERT", "Failed to parse certificate");
      return nullptr;
    }

    std::vector<X509*> certs_from_bio;
    const unsigned long err = LoadCertsFromBio(&certs_from_bio, bio);
    BIO_free(bio);
    if (err != 0) {
      const char* reason = ERR_reason_error_string(err);
      if ((ERR_GET_LIB(err) == ERR_LIB_PEM &&
           ERR_GET_REASON(err) == PEM_R_NO_START_LINE) ||
          (reason != nullptr && std::strstr(reason, "no start line") != nullptr)) {
        ERR_clear_error();
        FreeCertificates(&certs_from_bio);
        saw_no_valid_cert = true;
        continue;
      }
      FreeCertificates(&certs_from_bio);
      FreeCertificates(&parsed_certs);
      ThrowOpenSslError(env, nullptr, err, "Failed to parse certificate");
      return nullptr;
    }
    if (certs_from_bio.empty()) {
      ERR_clear_error();
      X509* cert = ParseX509(reinterpret_cast<const uint8_t*>(cert_text.data()), cert_text.size());
      if (cert == nullptr) {
        const unsigned long parse_err = ERR_peek_last_error();
        const char* parse_reason = parse_err != 0 ? ERR_reason_error_string(parse_err) : nullptr;
        if (parse_err != 0 &&
            !((ERR_GET_LIB(parse_err) == ERR_LIB_PEM &&
               ERR_GET_REASON(parse_err) == PEM_R_NO_START_LINE) ||
              (parse_reason != nullptr && std::strstr(parse_reason, "no start line") != nullptr))) {
          FreeCertificates(&parsed_certs);
          ERR_clear_error();
          ThrowOpenSslError(env, nullptr, parse_err, "Failed to parse certificate");
          return nullptr;
        }
        ERR_clear_error();
      } else {
        X509_free(cert);
      }
      FreeCertificates(&certs_from_bio);
      saw_no_valid_cert = true;
      continue;
    }

    for (X509* cert : certs_from_bio) {
      auto [it, inserted] = next_user_root_certs->insert(cert);
      if (!inserted) {
        X509_free(cert);
      } else {
        parsed_certs.push_back(cert);
      }
    }
  }

  if (parsed_certs.empty() && saw_no_valid_cert) {
    ERR_clear_error();
    ThrowError(env, "ERR_CRYPTO_OPERATION_FAILED", "No valid certificates found in the provided array");
    return nullptr;
  }

  std::lock_guard<std::mutex> lock(g_user_root_certs_mutex);
  if (g_user_root_certs != nullptr) {
    for (X509* cert : *g_user_root_certs) {
      X509_free(cert);
    }
  }
  g_user_root_certs = std::move(next_user_root_certs);

  napi_value undefined = nullptr;
  napi_get_undefined(env, &undefined);
  return undefined;
}

napi_value CryptoStartLoadingCertificatesOffThread(napi_env env, napi_callback_info /*info*/) {
  bool use_openssl_ca = false;
  bool use_system_ca = false;
  (void)GetEffectiveCaOptions(env, &use_openssl_ca, &use_system_ca);
  if (use_openssl_ca) {
    napi_value undefined = nullptr;
    napi_get_undefined(env, &undefined);
    return undefined;
  }

  bool expected = false;
  if (!g_tried_cert_loading_off_thread.compare_exchange_strong(expected, true)) {
    napi_value undefined = nullptr;
    napi_get_undefined(env, &undefined);
    return undefined;
  }

  RootCertLoadPlan plan;
  plan.load_bundled = true;
  plan.load_extra = std::getenv("NODE_EXTRA_CA_CERTS") != nullptr;
  plan.load_system = use_system_ca;

  {
    std::lock_guard<std::mutex> lock(g_cert_loading_thread_mutex);
    g_root_cert_load_plan = plan;
    const int rc = uv_thread_create(&g_cert_loading_thread, LoadCACertificatesThread, &g_root_cert_load_plan);
    g_cert_loading_thread_started.store(rc == 0);
    if (rc == 0) EnsureRootCertThreadCleanupHook(env);
  }

  napi_value undefined = nullptr;
  napi_get_undefined(env, &undefined);
  return undefined;
}

napi_value InstallCryptoBinding(napi_env env) {
  napi_value binding = nullptr;
  if (napi_create_object(env, &binding) != napi_ok || binding == nullptr) return nullptr;

  SetMethod(env, binding, "hashOneShot", CryptoHashOneShot);
  SetMethod(env, binding, "hashOneShotXof", CryptoHashOneShotXof);
  SetMethod(env, binding, "hmacOneShot", CryptoHmacOneShot);
  SetMethod(env, binding, "randomFillSync", CryptoRandomFillSync);
  SetMethod(env, binding, "randomBytes", CryptoRandomBytes);
  SetMethod(env, binding, "pbkdf2Sync", CryptoPbkdf2Sync);
  SetMethod(env, binding, "scryptSync", CryptoScryptSync);
  SetMethod(env, binding, "hkdfSync", CryptoHkdfSync);
  SetMethod(env, binding, "cipherTransform", CryptoCipherTransform);
  SetMethod(env, binding, "getHashes", CryptoGetHashes);
  SetMethod(env, binding, "getCiphers", CryptoGetCiphers);
  SetMethod(env, binding, "getSSLCiphers", CryptoGetSSLCiphers);
  SetMethod(env, binding, "getCurves", CryptoGetCurves);
  SetMethod(env, binding, "getCipherInfo", CryptoGetCipherInfo);
  SetMethod(env, binding, "parsePfx", CryptoParsePfx);
  SetMethod(env, binding, "parseCrl", CryptoParseCrl);
  SetMethod(env, binding, "secureContextCreate", CryptoSecureContextCreate);
  SetMethod(env, binding, "secureContextInit", CryptoSecureContextInit);
  SetMethod(env, binding, "secureContextSetMinProto", CryptoSecureContextSetMinProto);
  SetMethod(env, binding, "secureContextSetMaxProto", CryptoSecureContextSetMaxProto);
  SetMethod(env, binding, "secureContextGetMinProto", CryptoSecureContextGetMinProto);
  SetMethod(env, binding, "secureContextGetMaxProto", CryptoSecureContextGetMaxProto);
  SetMethod(env, binding, "secureContextSetOptions", CryptoSecureContextSetOptions);
  SetMethod(env, binding, "secureContextSetCiphers", CryptoSecureContextSetCiphers);
  SetMethod(env, binding, "secureContextSetCipherSuites", CryptoSecureContextSetCipherSuites);
  SetMethod(env, binding, "secureContextSetCert", CryptoSecureContextSetCert);
  SetMethod(env, binding, "secureContextSetKey", CryptoSecureContextSetKey);
  SetMethod(env, binding, "secureContextAddCACert", CryptoSecureContextAddCACert);
  SetMethod(env, binding, "secureContextAddCrl", CryptoSecureContextAddCrl);
  SetMethod(env, binding, "secureContextAddRootCerts", CryptoSecureContextAddRootCerts);
  SetMethod(env, binding, "secureContextSetAllowPartialTrustChain", CryptoSecureContextSetAllowPartialTrustChain);
  SetMethod(env, binding, "secureContextSetSessionIdContext", CryptoSecureContextSetSessionIdContext);
  SetMethod(env, binding, "secureContextSetSessionTimeout", CryptoSecureContextSetSessionTimeout);
  SetMethod(env, binding, "secureContextSetTicketKeys", CryptoSecureContextSetTicketKeys);
  SetMethod(env, binding, "secureContextGetTicketKeys", CryptoSecureContextGetTicketKeys);
  SetMethod(env, binding, "secureContextEnableTicketKeyCallback", CryptoSecureContextEnableTicketKeyCallback);
  SetMethod(env, binding, "secureContextLoadPKCS12", CryptoSecureContextLoadPKCS12);
  SetMethod(env, binding, "secureContextSetSigalgs", CryptoSecureContextSetSigalgs);
  SetMethod(env, binding, "secureContextSetECDHCurve", CryptoSecureContextSetECDHCurve);
  SetMethod(env, binding, "secureContextSetDHParam", CryptoSecureContextSetDHParam);
  SetMethod(env, binding, "secureContextGetCertificate", CryptoSecureContextGetCertificate);
  SetMethod(env, binding, "secureContextGetIssuer", CryptoSecureContextGetIssuer);
  SetMethod(env, binding, "signOneShot", CryptoSignOneShot);
  SetMethod(env, binding, "verifyOneShot", CryptoVerifyOneShot);
  SetMethod(env, binding, "getAsymmetricKeyDetails", CryptoGetAsymmetricKeyDetails);
  SetMethod(env, binding, "getAsymmetricKeyType", CryptoGetAsymmetricKeyType);
  SetMethod(env, binding, "publicEncrypt", CryptoPublicEncrypt);
  SetMethod(env, binding, "privateDecrypt", CryptoPrivateDecrypt);
  SetMethod(env, binding, "privateEncrypt", CryptoPrivateEncrypt);
  SetMethod(env, binding, "publicDecrypt", CryptoPublicDecrypt);
  SetMethod(env, binding, "cipherTransformAead", CryptoCipherTransformAead);

  return binding;
}

}  // namespace edge::crypto
