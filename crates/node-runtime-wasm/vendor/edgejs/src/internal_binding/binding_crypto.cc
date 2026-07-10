#include "internal_binding/binding_initializers.h"

#include <array>
#include <cctype>
#include <climits>
#include <cmath>
#include <cstdio>
#include <cstdint>
#include <cstring>
#include <inttypes.h>
#include <limits>
#include <memory>
#include <string>
#include <unordered_map>
#include <unordered_set>
#include <utility>
#include <vector>

#include <openssl/asn1err.h>
#include <openssl/bio.h>
#include <openssl/bn.h>
#include <openssl/bnerr.h>
#include <openssl/crypto.h>
#include <openssl/core_names.h>
#include <openssl/dh.h>
#include <openssl/dherr.h>
#include <openssl/ec.h>
#include <openssl/ecdh.h>
#include <openssl/ecerr.h>
#include <openssl/err.h>
#include <openssl/evp.h>
#include <openssl/evperr.h>
#include <openssl/obj_mac.h>
#include <openssl/pem.h>
#include <openssl/pemerr.h>
#include <openssl/proverr.h>
#include <openssl/rsa.h>
#include <openssl/rsaerr.h>
#include <openssl/x509.h>

#include "ncrypto.h"
#include "edge_crypto.h"
#include "crypto/edge_crypto_binding.h"
#include "internal_binding/helpers.h"
#include "edge_environment.h"
#include "crypto/edge_secure_context_bridge.h"
#include "edge_async_wrap.h"
#include "edge_handle_scope.h"
#include "edge_module_loader.h"
#include "edge_runtime.h"
#include "edge_runtime_platform.h"

namespace internal_binding {

namespace {

constexpr int32_t kCryptoJobAsync = 0;
constexpr int32_t kCryptoJobSync = 1;
constexpr int32_t kSignJobModeSign = 0;
constexpr int32_t kSignJobModeVerify = 1;
constexpr int32_t kKeyTypeSecret = 0;
constexpr int32_t kKeyTypePublic = 1;
constexpr int32_t kKeyTypePrivate = 2;
constexpr int32_t kKeyFormatDER = 0;
constexpr int32_t kKeyFormatPEM = 1;
constexpr int32_t kKeyFormatJWK = 2;
constexpr int32_t kKeyEncodingPKCS1 = 0;
constexpr int32_t kKeyEncodingPKCS8 = 1;
constexpr int32_t kKeyEncodingSPKI = 2;
constexpr int32_t kKeyEncodingSEC1 = 3;
constexpr int32_t kKeyVariantRSA_SSA_PKCS1_v1_5 = 0;
constexpr int32_t kKeyVariantRSA_PSS = 1;
constexpr int32_t kKeyVariantRSA_OAEP = 2;

void ResetRef(napi_env env, napi_ref* ref_ptr);
EVP_PKEY* GetAsymmetricKeyFromValue(napi_env env,
                                    napi_value key_value,
                                    napi_value key_passphrase_value,
                                    bool require_private,
                                    std::string* error_code,
                                    std::string* error_message);

struct CryptoBindingState {
  explicit CryptoBindingState(napi_env env_in) : env(env_in) {}
  ~CryptoBindingState() {
    ResetRef(env, &binding_ref);
    ResetRef(env, &native_key_object_ctor_ref);
    ResetRef(env, &secret_key_object_ctor_ref);
    ResetRef(env, &public_key_object_ctor_ref);
    ResetRef(env, &private_key_object_ctor_ref);
  }

  napi_env env = nullptr;
  napi_ref binding_ref = nullptr;
  napi_ref native_key_object_ctor_ref = nullptr;
  napi_ref secret_key_object_ctor_ref = nullptr;
  napi_ref public_key_object_ctor_ref = nullptr;
  napi_ref private_key_object_ctor_ref = nullptr;
  int32_t fips_mode = 0;
};

CryptoBindingState* GetState(napi_env env) {
  return EdgeEnvironmentGetSlotData<CryptoBindingState>(env, kEdgeEnvironmentSlotCryptoBindingState);
}

CryptoBindingState& EnsureState(napi_env env) {
  return EdgeEnvironmentGetOrCreateSlotData<CryptoBindingState>(
      env, kEdgeEnvironmentSlotCryptoBindingState);
}

napi_value GetRefValue(napi_env env, napi_ref ref) {
  if (ref == nullptr) return nullptr;
  napi_value out = nullptr;
  if (napi_get_reference_value(env, ref, &out) != napi_ok || out == nullptr) return nullptr;
  return out;
}

napi_value GetBinding(napi_env env) {
  CryptoBindingState* state = GetState(env);
  return state == nullptr ? nullptr : GetRefValue(env, state->binding_ref);
}

void ResetRef(napi_env env, napi_ref* ref_ptr) {
  if (ref_ptr == nullptr || *ref_ptr == nullptr) return;
  napi_delete_reference(env, *ref_ptr);
  *ref_ptr = nullptr;
}

void SetNamedMethod(napi_env env, napi_value obj, const char* name, napi_callback cb) {
  napi_value fn = nullptr;
  if (napi_create_function(env, name, NAPI_AUTO_LENGTH, cb, nullptr, &fn) == napi_ok && fn != nullptr) {
    napi_set_named_property(env, obj, name, fn);
  }
}

void SetNamedInt(napi_env env, napi_value obj, const char* name, int32_t value) {
  napi_value v = nullptr;
  if (napi_create_int32(env, value, &v) == napi_ok && v != nullptr) napi_set_named_property(env, obj, name, v);
}

void SetNamedBool(napi_env env, napi_value obj, const char* name, bool value) {
  napi_value v = nullptr;
  if (napi_get_boolean(env, value, &v) == napi_ok && v != nullptr) napi_set_named_property(env, obj, name, v);
}

void ClearPendingException(napi_env env) {
  if (env == nullptr) return;
  // TODO: Make napi_get_and_clear_last_exception() match upstream Node so
  // this helper does not need a separate pending-exception check first.
  bool pending = false;
  if (napi_is_exception_pending(env, &pending) != napi_ok || !pending) return;
  napi_value ignored = nullptr;
  napi_get_and_clear_last_exception(env, &ignored);
}

void EmitProcessDeprecationWarning(napi_env env, const char* message, const char* code) {
  if (env == nullptr || message == nullptr) return;
  napi_value global = nullptr;
  napi_value process = nullptr;
  napi_value emit_warning = nullptr;
  napi_valuetype type = napi_undefined;
  if (napi_get_global(env, &global) != napi_ok || global == nullptr ||
      napi_get_named_property(env, global, "process", &process) != napi_ok || process == nullptr ||
      napi_get_named_property(env, process, "emitWarning", &emit_warning) != napi_ok || emit_warning == nullptr ||
      napi_typeof(env, emit_warning, &type) != napi_ok || type != napi_function) {
    return;
  }

  napi_value argv[3] = {nullptr, nullptr, nullptr};
  size_t argc = 1;
  if (napi_create_string_utf8(env, message, NAPI_AUTO_LENGTH, &argv[0]) != napi_ok || argv[0] == nullptr) return;
  if (napi_create_string_utf8(env, "DeprecationWarning", NAPI_AUTO_LENGTH, &argv[1]) == napi_ok &&
      argv[1] != nullptr) {
    argc = 2;
    if (code != nullptr &&
        napi_create_string_utf8(env, code, NAPI_AUTO_LENGTH, &argv[2]) == napi_ok &&
        argv[2] != nullptr) {
      argc = 3;
    }
  }

  napi_value ignored = nullptr;
  if (napi_call_function(env, process, emit_warning, argc, argv, &ignored) != napi_ok) {
    ClearPendingException(env);
  }
}

bool GetByteSpan(napi_env env, napi_value value, const uint8_t** data, size_t* length) {
  if (value == nullptr || data == nullptr || length == nullptr) return false;
  *data = nullptr;
  *length = 0;

  uint8_t* mutable_data = nullptr;
  size_t mutable_length = 0;
  if (edge::crypto::GetAnyBufferSourceBytes(env, value, &mutable_data, &mutable_length)) {
    *data = mutable_data;
    *length = mutable_length;
    return true;
  }

  bool is_buffer = false;
  if (napi_is_buffer(env, value, &is_buffer) == napi_ok && is_buffer) {
    void* raw = nullptr;
    if (napi_get_buffer_info(env, value, &raw, length) != napi_ok || raw == nullptr) return false;
    *data = static_cast<const uint8_t*>(raw);
    return true;
  }

  bool is_typed_array = false;
  if (napi_is_typedarray(env, value, &is_typed_array) == napi_ok && is_typed_array) {
    napi_typedarray_type type = napi_uint8_array;
    size_t len = 0;
    void* raw = nullptr;
    napi_value arraybuffer = nullptr;
    size_t offset = 0;
    if (napi_get_typedarray_info(env, value, &type, &len, &raw, &arraybuffer, &offset) != napi_ok || raw == nullptr) {
      return false;
    }
    size_t bytes_per_element = 1;
    switch (type) {
      case napi_int8_array:
      case napi_uint8_array:
      case napi_uint8_clamped_array:
        bytes_per_element = 1;
        break;
      case napi_int16_array:
      case napi_uint16_array:
      case napi_float16_array:
        bytes_per_element = 2;
        break;
      case napi_int32_array:
      case napi_uint32_array:
      case napi_float32_array:
        bytes_per_element = 4;
        break;
      case napi_float64_array:
      case napi_bigint64_array:
      case napi_biguint64_array:
        bytes_per_element = 8;
        break;
      default:
        bytes_per_element = 1;
        break;
    }
    *data = static_cast<const uint8_t*>(raw);
    *length = len * bytes_per_element;
    return true;
  }

  return false;
}

std::vector<uint8_t> ValueToBytes(napi_env env, napi_value value) {
  std::vector<uint8_t> out;
  const uint8_t* span = nullptr;
  size_t span_len = 0;
  if (GetByteSpan(env, value, &span, &span_len)) {
    out.assign(span, span + span_len);
    return out;
  }

  if (value != nullptr) {
    napi_value export_fn = nullptr;
    napi_valuetype t = napi_undefined;
    if (napi_get_named_property(env, value, "export", &export_fn) == napi_ok &&
        export_fn != nullptr &&
        napi_typeof(env, export_fn, &t) == napi_ok &&
        t == napi_function) {
      napi_value exported = nullptr;
      if (napi_call_function(env, value, export_fn, 0, nullptr, &exported) == napi_ok && exported != nullptr) {
        return ValueToBytes(env, exported);
      }
    }
  }

  size_t len = 0;
  if (napi_get_value_string_utf8(env, value, nullptr, 0, &len) == napi_ok) {
    std::string text(len + 1, '\0');
    size_t copied = 0;
    if (napi_get_value_string_utf8(env, value, text.data(), text.size(), &copied) == napi_ok) {
      text.resize(copied);
      out.assign(text.begin(), text.end());
      return out;
    }
  }

  return out;
}

std::vector<uint8_t> ValueToBytesWithEncoding(napi_env env, napi_value value, napi_value encoding) {
  if (value == nullptr) return {};

  napi_valuetype value_type = napi_undefined;
  if (napi_typeof(env, value, &value_type) == napi_ok &&
      value_type == napi_string &&
      encoding != nullptr &&
      !IsUndefined(env, encoding)) {
    napi_valuetype encoding_type = napi_undefined;
    if (napi_typeof(env, encoding, &encoding_type) == napi_ok && encoding_type == napi_string) {
      napi_value global = GetGlobal(env);
      napi_value buffer_ctor = nullptr;
      napi_value from_fn = nullptr;
      napi_valuetype from_type = napi_undefined;
      if (global != nullptr &&
          napi_get_named_property(env, global, "Buffer", &buffer_ctor) == napi_ok &&
          buffer_ctor != nullptr &&
          napi_get_named_property(env, buffer_ctor, "from", &from_fn) == napi_ok &&
          from_fn != nullptr &&
          napi_typeof(env, from_fn, &from_type) == napi_ok &&
          from_type == napi_function) {
        napi_value argv[2] = {value, encoding};
        napi_value out = nullptr;
        if (napi_call_function(env, buffer_ctor, from_fn, 2, argv, &out) == napi_ok && out != nullptr) {
          return ValueToBytes(env, out);
        }
        ClearPendingException(env);
      }
    }
  }

  return ValueToBytes(env, value);
}

napi_value BytesToBuffer(napi_env env, const std::vector<uint8_t>& data) {
  napi_value out = nullptr;
  void* copied = nullptr;
  if (napi_create_buffer_copy(env, data.size(), data.empty() ? nullptr : data.data(), &copied, &out) != napi_ok ||
      out == nullptr) {
    return Undefined(env);
  }
  return out;
}

napi_value EnsureBufferValue(napi_env env, napi_value value) {
  bool is_buffer = false;
  if (value != nullptr && napi_is_buffer(env, value, &is_buffer) == napi_ok && is_buffer) return value;
  return BytesToBuffer(env, ValueToBytes(env, value));
}

napi_value NormalizeToBufferObject(napi_env env, napi_value value) {
  napi_value global = GetGlobal(env);
  napi_value buffer_ctor = nullptr;
  napi_value from_fn = nullptr;
  napi_valuetype type = napi_undefined;
  if (global == nullptr ||
      napi_get_named_property(env, global, "Buffer", &buffer_ctor) != napi_ok ||
      buffer_ctor == nullptr ||
      napi_get_named_property(env, buffer_ctor, "from", &from_fn) != napi_ok ||
      from_fn == nullptr ||
      napi_typeof(env, from_fn, &type) != napi_ok ||
      type != napi_function) {
    return EnsureBufferValue(env, value);
  }
  napi_value argv[1] = {value != nullptr ? value : Undefined(env)};
  napi_value out = nullptr;
  if (napi_call_function(env, buffer_ctor, from_fn, 1, argv, &out) != napi_ok || out == nullptr) {
    ClearPendingException(env);
    return EnsureBufferValue(env, value);
  }
  return out;
}

bool CallBindingMethod(napi_env env,
                       napi_value binding,
                       const char* method,
                       size_t argc,
                       napi_value* argv,
                       napi_value* out) {
  if (binding == nullptr || method == nullptr || out == nullptr) return false;
  *out = nullptr;
  napi_value fn = nullptr;
  if (napi_get_named_property(env, binding, method, &fn) != napi_ok || fn == nullptr) return false;
  napi_valuetype t = napi_undefined;
  if (napi_typeof(env, fn, &t) != napi_ok || t != napi_function) return false;
  return napi_call_function(env, binding, fn, argc, argv, out) == napi_ok && *out != nullptr;
}

napi_value MaybeToEncodedOutput(napi_env env, napi_value buffer_value, napi_value encoding_value) {
  if (buffer_value == nullptr) return Undefined(env);
  napi_value as_buffer = NormalizeToBufferObject(env, buffer_value);
  if (encoding_value == nullptr || IsUndefined(env, encoding_value)) return as_buffer;

  napi_value encoding_string = nullptr;
  if (napi_coerce_to_string(env, encoding_value, &encoding_string) != napi_ok || encoding_string == nullptr) {
    return nullptr;
  }
  size_t len = 0;
  if (napi_get_value_string_utf8(env, encoding_string, nullptr, 0, &len) != napi_ok) return buffer_value;
  std::string encoding(len + 1, '\0');
  size_t copied = 0;
  if (napi_get_value_string_utf8(env, encoding_string, encoding.data(), encoding.size(), &copied) != napi_ok) {
    return buffer_value;
  }
  encoding.resize(copied);
  if (encoding == "buffer" || encoding.empty()) return as_buffer;

  napi_value to_string_fn = nullptr;
  napi_valuetype type = napi_undefined;
  if (napi_get_named_property(env, as_buffer, "toString", &to_string_fn) != napi_ok ||
      to_string_fn == nullptr ||
      napi_typeof(env, to_string_fn, &type) != napi_ok ||
      type != napi_function) {
    return as_buffer;
  }
  napi_value enc = nullptr;
  if (napi_create_string_utf8(env, encoding.c_str(), NAPI_AUTO_LENGTH, &enc) != napi_ok || enc == nullptr) {
    return as_buffer;
  }
  napi_value out = nullptr;
  if (napi_call_function(env, as_buffer, to_string_fn, 1, &enc, &out) != napi_ok || out == nullptr) {
    ClearPendingException(env);
    return as_buffer;
  }
  return out;
}

bool IsNullOrUndefinedValue(napi_env env, napi_value value) {
  if (value == nullptr) return true;
  napi_valuetype type = napi_undefined;
  if (napi_typeof(env, value, &type) != napi_ok) return true;
  return type == napi_undefined || type == napi_null;
}

std::string GetStringValue(napi_env env, napi_value value) {
  if (value == nullptr) return "";
  size_t len = 0;
  if (napi_get_value_string_utf8(env, value, nullptr, 0, &len) != napi_ok) return "";
  std::string out(len + 1, '\0');
  size_t copied = 0;
  if (napi_get_value_string_utf8(env, value, out.data(), out.size(), &copied) != napi_ok) return "";
  out.resize(copied);
  return out;
}

int32_t GetInt32Value(napi_env env, napi_value value, int32_t fallback = 0) {
  if (value == nullptr) return fallback;
  int32_t out = fallback;
  (void)napi_get_value_int32(env, value, &out);
  return out;
}

bool GetNamedInt32Value(napi_env env, napi_value value, const char* key, int32_t* out) {
  if (value == nullptr || key == nullptr || out == nullptr) return false;
  napi_value prop = nullptr;
  if (napi_get_named_property(env, value, key, &prop) != napi_ok || prop == nullptr) return false;
  napi_valuetype type = napi_undefined;
  if (napi_typeof(env, prop, &type) != napi_ok || type != napi_number) return false;
  return napi_get_value_int32(env, prop, out) == napi_ok;
}

bool GetNamedStringValue(napi_env env, napi_value value, const char* key, std::string* out) {
  if (value == nullptr || key == nullptr || out == nullptr) return false;
  napi_value prop = nullptr;
  if (napi_get_named_property(env, value, key, &prop) != napi_ok || prop == nullptr) return false;
  size_t len = 0;
  if (napi_get_value_string_utf8(env, prop, nullptr, 0, &len) != napi_ok) return false;
  std::string text(len + 1, '\0');
  size_t copied = 0;
  if (napi_get_value_string_utf8(env, prop, text.data(), text.size(), &copied) != napi_ok) return false;
  text.resize(copied);
  *out = std::move(text);
  return true;
}

napi_value CopyAsArrayBuffer(napi_env env, napi_value value) {
  if (value == nullptr || IsUndefined(env, value)) return value;
  const uint8_t* data = nullptr;
  size_t len = 0;
  if (!GetByteSpan(env, value, &data, &len)) return value;
  napi_value out = nullptr;
  void* raw = nullptr;
  if (napi_create_arraybuffer(env, len, &raw, &out) != napi_ok || out == nullptr || raw == nullptr) {
    return value;
  }
  if (len > 0) std::memcpy(raw, data, len);
  return out;
}

napi_value CreateArrayBufferCopy(napi_env env, const uint8_t* data, size_t len) {
  napi_value out = nullptr;
  void* raw = nullptr;
  if (napi_create_arraybuffer(env, len, &raw, &out) != napi_ok || out == nullptr) {
    return Undefined(env);
  }
  if (len > 0) {
    if (raw == nullptr) return Undefined(env);
    std::memcpy(raw, data, len);
  }
  return out;
}

napi_value CreateArrayBufferCopy(napi_env env, const std::vector<uint8_t>& bytes) {
  return CreateArrayBufferCopy(env, bytes.empty() ? nullptr : bytes.data(), bytes.size());
}

void SetClassPrototypeMethod(napi_env env,
                             napi_value binding,
                             const char* class_name,
                             const char* method_name,
                             napi_callback cb) {
  if (binding == nullptr || class_name == nullptr || method_name == nullptr || cb == nullptr) return;
  napi_value ctor = nullptr;
  if (napi_get_named_property(env, binding, class_name, &ctor) != napi_ok || ctor == nullptr) return;
  napi_valuetype ctor_type = napi_undefined;
  if (napi_typeof(env, ctor, &ctor_type) != napi_ok || ctor_type != napi_function) return;
  napi_value proto = nullptr;
  if (napi_get_named_property(env, ctor, "prototype", &proto) != napi_ok || proto == nullptr) return;
  napi_value fn = nullptr;
  if (napi_create_function(env, method_name, NAPI_AUTO_LENGTH, cb, nullptr, &fn) != napi_ok || fn == nullptr) return;
  napi_set_named_property(env, proto, method_name, fn);
}

bool IsSupportedDigestName(napi_env env, napi_value binding, napi_value digest) {
  if (binding == nullptr || digest == nullptr || IsUndefined(env, digest) || IsNullOrUndefinedValue(env, digest)) {
    return false;
  }
  const napi_value empty = BytesToBuffer(env, {});
  napi_value argv[2] = {digest, empty != nullptr ? empty : Undefined(env)};
  napi_value out = nullptr;
  if (CallBindingMethod(env, binding, "hashOneShot", 2, argv, &out)) return true;
  bool pending = false;
  if (napi_is_exception_pending(env, &pending) == napi_ok && pending) {
    napi_value ignored = nullptr;
    napi_get_and_clear_last_exception(env, &ignored);
  }
  return false;
}

napi_value CreateErrorWithCode(napi_env env, const char* code, const std::string& message) {
  napi_value code_v = nullptr;
  napi_value msg_v = nullptr;
  napi_value err_v = nullptr;
  const bool has_code = code != nullptr;
  if (code != nullptr) {
    napi_create_string_utf8(env, code, NAPI_AUTO_LENGTH, &code_v);
  } else {
    napi_get_undefined(env, &code_v);
  }
  napi_create_string_utf8(env, message.c_str(), NAPI_AUTO_LENGTH, &msg_v);
  bool as_type_error = false;
  bool as_range_error = false;
  if (code != nullptr) {
    if (std::strcmp(code, "ERR_INVALID_ARG_TYPE") == 0 ||
        std::strcmp(code, "ERR_INVALID_ARG_VALUE") == 0 ||
        std::strcmp(code, "ERR_MISSING_OPTION") == 0 ||
        std::strcmp(code, "ERR_CRYPTO_INVALID_CURVE") == 0 ||
        std::strcmp(code, "ERR_CRYPTO_INVALID_AUTH_TAG") == 0) {
      as_type_error = true;
    } else if (std::strcmp(code, "ERR_OUT_OF_RANGE") == 0 ||
               std::strcmp(code, "ERR_CRYPTO_INVALID_KEYLEN") == 0 ||
               std::strcmp(code, "ERR_CRYPTO_INVALID_KEYPAIR") == 0) {
      as_range_error = true;
    }
  }
  if (as_type_error) {
    napi_create_type_error(env, code_v, msg_v, &err_v);
  } else if (as_range_error) {
    napi_create_range_error(env, code_v, msg_v, &err_v);
  } else {
    napi_create_error(env, code_v, msg_v, &err_v);
  }
  if (has_code && err_v != nullptr && code_v != nullptr) napi_set_named_property(env, err_v, "code", code_v);
  return err_v != nullptr ? err_v : Undefined(env);
}

void SetErrorStringProperty(napi_env env, napi_value err, const char* name, const char* value) {
  if (err == nullptr || name == nullptr || value == nullptr || value[0] == '\0') return;
  napi_value v = nullptr;
  if (napi_create_string_utf8(env, value, NAPI_AUTO_LENGTH, &v) == napi_ok && v != nullptr) {
    napi_set_named_property(env, err, name, v);
  }
}

const char* MapOpenSslErrorCode(unsigned long err) {
  if (err == 0) return nullptr;
  switch (ERR_PACK(ERR_GET_LIB(err), 0, ERR_GET_REASON(err))) {
    case ERR_PACK(ERR_LIB_EVP, 0, EVP_R_INVALID_DIGEST):
    case ERR_PACK(ERR_LIB_RSA, 0, RSA_R_INVALID_DIGEST):
    case ERR_PACK(ERR_LIB_PROV, 0, PROV_R_INVALID_DIGEST):
      return "ERR_OSSL_INVALID_DIGEST";
    case ERR_PACK(ERR_LIB_EVP, 0, EVP_R_BAD_DECRYPT):
    case ERR_PACK(ERR_LIB_PROV, 0, PROV_R_BAD_DECRYPT):
    case ERR_PACK(ERR_LIB_PEM, 0, PEM_R_BAD_DECRYPT):
      return "ERR_OSSL_BAD_DECRYPT";
    case ERR_PACK(ERR_LIB_EVP, 0, EVP_R_OPERATION_NOT_SUPPORTED_FOR_THIS_KEYTYPE):
    case ERR_PACK(ERR_LIB_RSA, 0, RSA_R_OPERATION_NOT_SUPPORTED_FOR_THIS_KEYTYPE):
    case ERR_PACK(ERR_LIB_PROV, 0, PROV_R_OPERATION_NOT_SUPPORTED_FOR_THIS_KEYTYPE):
      return "ERR_OSSL_EVP_OPERATION_NOT_SUPPORTED_FOR_THIS_KEYTYPE";
    case ERR_PACK(ERR_LIB_EVP, 0, EVP_R_COMMAND_NOT_SUPPORTED):
      return "ERR_OSSL_EVP_COMMAND_NOT_SUPPORTED";
    case ERR_PACK(ERR_LIB_EVP, 0, EVP_R_DECODE_ERROR):
      return "ERR_OSSL_EVP_DECODE_ERROR";
    case ERR_PACK(ERR_LIB_ASN1, 0, ASN1_R_DECODE_ERROR):
      return "ERR_OSSL_ASN1_DECODE_ERROR";
    case ERR_PACK(ERR_LIB_ASN1, 0, ASN1_R_WRONG_TAG):
      return "ERR_OSSL_ASN1_WRONG_TAG";
    case ERR_PACK(ERR_LIB_EVP, 0, EVP_R_WRONG_FINAL_BLOCK_LENGTH):
    case ERR_PACK(ERR_LIB_PROV, 0, PROV_R_WRONG_FINAL_BLOCK_LENGTH):
      return "ERR_OSSL_WRONG_FINAL_BLOCK_LENGTH";
    case ERR_PACK(ERR_LIB_EVP, 0, EVP_R_DATA_NOT_MULTIPLE_OF_BLOCK_LENGTH):
      return "ERR_OSSL_DATA_NOT_MULTIPLE_OF_BLOCK_LENGTH";
    case ERR_PACK(ERR_LIB_BN, 0, BN_R_BIGNUM_TOO_LONG):
      return "ERR_OSSL_BN_BIGNUM_TOO_LONG";
    case ERR_PACK(ERR_LIB_EC, 0, EC_R_MISSING_OID):
    case ERR_PACK(ERR_LIB_PROV, 0, PROV_R_MISSING_OID):
      return "ERR_OSSL_MISSING_OID";
    case ERR_PACK(ERR_LIB_DH, 0, DH_R_MODULUS_TOO_SMALL):
      return "ERR_OSSL_DH_MODULUS_TOO_SMALL";
    case ERR_PACK(ERR_LIB_BN, 0, BN_R_BITS_TOO_SMALL):
      return "ERR_OSSL_BN_BITS_TOO_SMALL";
    case ERR_PACK(ERR_LIB_DH, 0, DH_R_BAD_GENERATOR):
      return "ERR_OSSL_DH_BAD_GENERATOR";
    case ERR_PACK(ERR_LIB_PROV, 0, PROV_R_MISMATCHING_DOMAIN_PARAMETERS):
      return "ERR_OSSL_MISMATCHING_DOMAIN_PARAMETERS";
    case ERR_PACK(ERR_LIB_EVP, 0, EVP_R_DIFFERENT_PARAMETERS):
      return "ERR_OSSL_EVP_DIFFERENT_PARAMETERS";
    case ERR_PACK(ERR_LIB_EVP, 0, ERR_R_INTERRUPTED_OR_CANCELLED):
      return "ERR_OSSL_CRYPTO_INTERRUPTED_OR_CANCELLED";
    case ERR_PACK(ERR_LIB_RSA, 0, RSA_R_ILLEGAL_OR_UNSUPPORTED_PADDING_MODE):
      return "ERR_OSSL_RSA_ILLEGAL_OR_UNSUPPORTED_PADDING_MODE";
    case ERR_PACK(ERR_LIB_PROV, 0, PROV_R_ILLEGAL_OR_UNSUPPORTED_PADDING_MODE):
      return "ERR_OSSL_ILLEGAL_OR_UNSUPPORTED_PADDING_MODE";
    case ERR_PACK(ERR_LIB_EVP, 0, EVP_R_UNSUPPORTED_ALGORITHM):
      return "ERR_OSSL_EVP_UNSUPPORTED_ALGORITHM";
    case ERR_PACK(ERR_LIB_EVP, 0, EVP_R_UNSUPPORTED_KEY_TYPE):
      return "ERR_OSSL_EVP_UNSUPPORTED_KEY_TYPE";
    case ERR_PACK(ERR_LIB_OSSL_DECODER, 0, ERR_R_UNSUPPORTED):
      return "ERR_OSSL_UNSUPPORTED";
#if OPENSSL_VERSION_NUMBER < 0x30000000L
    case ERR_PACK(ERR_LIB_PEM, 0, PEM_R_NO_START_LINE):
      return "ERR_OSSL_PEM_NO_START_LINE";
#endif
  }
  return nullptr;
}

bool IsOpenSslDecoderUnsupportedError(unsigned long err) {
  if (err == 0) return false;
  return ERR_GET_LIB(err) == ERR_LIB_OSSL_DECODER &&
         ERR_GET_REASON(err) == ERR_R_UNSUPPORTED;
}

const char* MapOpenSslKeyParseErrorCode(unsigned long err, bool require_private) {
  if (!require_private && IsOpenSslDecoderUnsupportedError(err)) {
    return "ERR_OSSL_EVP_DECODE_ERROR";
  }
  return MapOpenSslErrorCode(err);
}

int OpenSslMappedErrorPriority(unsigned long err, bool require_private) {
  if (err == 0) return 0;
  if (!require_private && IsOpenSslDecoderUnsupportedError(err)) return 3;

  switch (ERR_PACK(ERR_GET_LIB(err), 0, ERR_GET_REASON(err))) {
    case ERR_PACK(ERR_LIB_EVP, 0, EVP_R_DECODE_ERROR):
      return 3;
    case ERR_PACK(ERR_LIB_ASN1, 0, ASN1_R_DECODE_ERROR):
    case ERR_PACK(ERR_LIB_ASN1, 0, ASN1_R_WRONG_TAG):
      return 1;
  }

  return MapOpenSslErrorCode(err) != nullptr ? 2 : 0;
}

napi_value CreateOpenSslError(napi_env env,
                              const char* code,
                              unsigned long err,
                              const char* fallback_message) {
  std::string message = fallback_message != nullptr ? fallback_message : "OpenSSL error";
  if (err != 0) {
    char buf[256];
    ERR_error_string_n(err, buf, sizeof(buf));
    message = buf;
  }
  napi_value error = CreateErrorWithCode(env, code, message);
  if (error == nullptr || IsUndefined(env, error) || err == 0) return error;
  SetErrorStringProperty(env, error, "library", ERR_lib_error_string(err));
#if OPENSSL_VERSION_NUMBER < 0x30000000L
  SetErrorStringProperty(env, error, "function", ERR_func_error_string(err));
#endif
  SetErrorStringProperty(env, error, "reason", ERR_reason_error_string(err));
  return error;
}

napi_value CreateOpenSslErrorStackException(napi_env env, const ncrypto::CryptoErrorList& errors) {
  if (errors.empty()) {
    return CreateErrorWithCode(env, nullptr, "Ok");
  }

  const std::string& message = errors.peek_back();
  napi_value message_value = nullptr;
  napi_value error = nullptr;
  if (napi_create_string_utf8(env, message.c_str(), message.size(), &message_value) != napi_ok ||
      message_value == nullptr ||
      napi_create_error(env, nullptr, message_value, &error) != napi_ok ||
      error == nullptr) {
    return CreateErrorWithCode(env, nullptr, message.c_str());
  }

  if (errors.size() > 1) {
    napi_value stack = nullptr;
    if (napi_create_array_with_length(env, errors.size() - 1, &stack) == napi_ok && stack != nullptr) {
      uint32_t index = 0;
      auto current = errors.begin();
      auto last = errors.end();
      --last;
      for (; current != last; ++current, ++index) {
        napi_value entry = nullptr;
        if (napi_create_string_utf8(env, current->c_str(), current->size(), &entry) == napi_ok && entry != nullptr) {
          napi_set_element(env, stack, index, entry);
        }
      }
      napi_set_named_property(env, error, "opensslErrorStack", stack);
    }
  }

  return error;
}

void ThrowLastOpenSslMessage(napi_env env, const char* fallback_message) {
  const unsigned long selected = ERR_get_error();
  while (ERR_get_error() != 0) {
  }
  if (selected == 0) {
    napi_throw_error(env, nullptr, fallback_message);
    return;
  }
  napi_throw(env, CreateOpenSslError(env, MapOpenSslErrorCode(selected), selected, fallback_message));
}

unsigned long ConsumePreferredOpenSslKeyParseError(bool require_private) {
  unsigned long first = 0;
  unsigned long selected = 0;
  int selected_priority = 0;
  unsigned long err = 0;
  while ((err = ERR_get_error()) != 0) {
    if (first == 0) first = err;
    const int priority = OpenSslMappedErrorPriority(err, require_private);
    if (priority > selected_priority) {
      selected = err;
      selected_priority = priority;
    }
  }
  return selected != 0 ? selected : first;
}

unsigned long ConsumePreferredOpenSslError() {
  return ConsumePreferredOpenSslKeyParseError(true);
}

constexpr unsigned long kOpenSslDecoderUnsupportedError = 0x1E08010CUL;

napi_value BuildJobResult(napi_env env, napi_value err, napi_value value);

napi_value BuildJobErrorResult(napi_env env, const char* code, const std::string& message) {
  return BuildJobResult(env, CreateErrorWithCode(env, code, message), Undefined(env));
}

napi_value BuildJobOpenSslErrorResult(napi_env env,
                                      const char* fallback_code,
                                      const char* fallback_message) {
  const unsigned long err = ConsumePreferredOpenSslError();
  napi_value error = err != 0 ? CreateOpenSslError(env, MapOpenSslErrorCode(err), err, fallback_message)
                              : CreateErrorWithCode(env, fallback_code, fallback_message);
  return BuildJobResult(env, error, Undefined(env));
}

void SetPreferredOpenSslError(std::string* code,
                              std::string* message,
                              const char* fallback_code,
                              const char* fallback_message) {
  if (code == nullptr || message == nullptr) return;

  const unsigned long selected = ConsumePreferredOpenSslError();

  if (selected != 0) {
    if (const char* mapped = MapOpenSslErrorCode(selected)) {
      *code = mapped;
    } else if (fallback_code != nullptr) {
      *code = fallback_code;
    } else {
      code->clear();
    }

    char buf[256];
    ERR_error_string_n(selected, buf, sizeof(buf));
    *message = buf;
    return;
  }

  *code = fallback_code != nullptr ? fallback_code : "";
  *message = fallback_message != nullptr ? fallback_message : "OpenSSL error";
}

void ThrowLastOpenSslKeyParseError(napi_env env, bool require_private, const char* fallback_message) {
  const unsigned long selected = ConsumePreferredOpenSslKeyParseError(require_private);
  if (selected == 0) {
    napi_throw_error(env, nullptr, fallback_message);
    return;
  }
  napi_throw(
      env,
      CreateOpenSslError(env, MapOpenSslKeyParseErrorCode(selected, require_private), selected, fallback_message));
}

void SetPreferredOpenSslKeyParseError(std::string* code,
                                      std::string* message,
                                      bool require_private,
                                      const char* fallback_code,
                                      const char* fallback_message) {
  if (code == nullptr || message == nullptr) return;

  const unsigned long selected = ConsumePreferredOpenSslKeyParseError(require_private);

  if (selected != 0) {
    if (const char* mapped = MapOpenSslKeyParseErrorCode(selected, require_private)) {
      *code = mapped;
    } else if (fallback_code != nullptr) {
      *code = fallback_code;
    } else {
      code->clear();
    }

    char buf[256];
    ERR_error_string_n(selected, buf, sizeof(buf));
    *message = buf;
    return;
  }

  *code = fallback_code != nullptr ? fallback_code : "";
  *message = fallback_message != nullptr ? fallback_message : "OpenSSL error";
}

std::vector<uint8_t> BytesFromBio(BIO* bio) {
  std::vector<uint8_t> out;
  if (bio == nullptr) return out;
  const char* data = nullptr;
  const long len = BIO_get_mem_data(bio, &data);
  if (len <= 0 || data == nullptr) return out;
  out.assign(reinterpret_cast<const uint8_t*>(data), reinterpret_cast<const uint8_t*>(data) + len);
  return out;
}

std::vector<uint8_t> BigNumToBytes(const BIGNUM* bn) {
  std::vector<uint8_t> out;
  if (bn == nullptr) return out;
  const int len = BN_num_bytes(bn);
  if (len <= 0) return out;
  out.resize(static_cast<size_t>(len));
  BN_bn2bin(bn, out.data());
  return out;
}

std::string Base64UrlEncode(const std::vector<uint8_t>& in) {
  if (in.empty()) return "";
  const int encoded_len = 4 * static_cast<int>((in.size() + 2) / 3);
  std::string out(static_cast<size_t>(encoded_len), '\0');
  const int written = EVP_EncodeBlock(reinterpret_cast<unsigned char*>(out.data()),
                                      reinterpret_cast<const unsigned char*>(in.data()),
                                      static_cast<int>(in.size()));
  if (written <= 0) return "";
  out.resize(static_cast<size_t>(written));
  for (char& ch : out) {
    if (ch == '+') {
      ch = '-';
    } else if (ch == '/') {
      ch = '_';
    }
  }
  while (!out.empty() && out.back() == '=') out.pop_back();
  return out;
}

void SetObjectString(napi_env env, napi_value obj, const char* name, const std::string& value) {
  napi_value v = nullptr;
  if (napi_create_string_utf8(env, value.c_str(), NAPI_AUTO_LENGTH, &v) == napi_ok && v != nullptr) {
    napi_set_named_property(env, obj, name, v);
  }
}

void SetObjectBuffer(napi_env env, napi_value obj, const char* name, const std::vector<uint8_t>& value) {
  napi_value v = BytesToBuffer(env, value);
  if (v != nullptr && !IsUndefined(env, v)) napi_set_named_property(env, obj, name, v);
}

void SetJwkFieldFromBigNum(napi_env env, napi_value obj, const char* name, const BIGNUM* bn) {
  if (obj == nullptr || name == nullptr || bn == nullptr) return;
  const std::string encoded = Base64UrlEncode(BigNumToBytes(bn));
  SetObjectString(env, obj, name, encoded);
}

std::vector<uint8_t> BigNumToPaddedBytes(const BIGNUM* bn, size_t size) {
  if (bn == nullptr) return {};
  if (size == 0) return BigNumToBytes(bn);
  std::vector<uint8_t> out(size, 0);
  if (BN_bn2binpad(bn, out.data(), static_cast<int>(size)) < 0) return {};
  return out;
}

int CurveNidFromName(const std::string& name) {
  if (name.empty()) return NID_undef;
  if (name == "P-256") return NID_X9_62_prime256v1;
  if (name == "P-384") return NID_secp384r1;
  if (name == "P-521") return NID_secp521r1;
  int nid = OBJ_txt2nid(name.c_str());
  if (nid == NID_undef) nid = OBJ_sn2nid(name.c_str());
  if (nid == NID_undef) nid = OBJ_ln2nid(name.c_str());
  return nid;
}

std::string JwkCurveFromNid(int nid) {
  switch (nid) {
    case NID_X9_62_prime256v1:
      return "P-256";
    case NID_secp384r1:
      return "P-384";
    case NID_secp521r1:
      return "P-521";
    case NID_secp256k1:
      return "secp256k1";
    default:
      return "";
  }
}

bool ResolveEcJwkCurve(EVP_PKEY* pkey,
                       const std::string& curve_name_hint,
                       int* out_nid,
                       std::string* out_curve_name,
                       size_t* out_field_bytes) {
  if (out_nid != nullptr) *out_nid = NID_undef;
  if (out_curve_name != nullptr) *out_curve_name = curve_name_hint;
  if (out_field_bytes != nullptr) *out_field_bytes = 0;
  if (pkey == nullptr) return false;

  int nid = NID_undef;
  std::string curve_name = curve_name_hint;
  char group_name[80];
  size_t group_name_len = 0;
  if (EVP_PKEY_get_utf8_string_param(
          pkey, OSSL_PKEY_PARAM_GROUP_NAME, group_name, sizeof(group_name), &group_name_len) == 1 &&
      group_name_len > 0) {
    curve_name.assign(group_name, group_name_len);
    nid = CurveNidFromName(curve_name);
  }

  EC_KEY* ec = EVP_PKEY_get1_EC_KEY(pkey);
  if (ec != nullptr) {
    const EC_GROUP* group = EC_KEY_get0_group(ec);
    if (group != nullptr) {
      if (nid == NID_undef) nid = EC_GROUP_get_curve_name(group);
      if (out_field_bytes != nullptr) {
        const int degree = EC_GROUP_get_degree(group);
        if (degree > 0) *out_field_bytes = static_cast<size_t>((degree + 7) / 8);
      }
    }
    EC_KEY_free(ec);
  }

  if (out_nid != nullptr) *out_nid = nid;
  if (out_curve_name != nullptr) *out_curve_name = curve_name;
  return nid != NID_undef;
}

int RawKeyNidFromName(const std::string& name) {
  if (name == "Ed25519") return EVP_PKEY_ED25519;
  if (name == "Ed448") return EVP_PKEY_ED448;
  if (name == "X25519") return EVP_PKEY_X25519;
  if (name == "X448") return EVP_PKEY_X448;
#if OPENSSL_WITH_PQC
  if (name == "ML-DSA-44") return EVP_PKEY_ML_DSA_44;
  if (name == "ML-DSA-65") return EVP_PKEY_ML_DSA_65;
  if (name == "ML-DSA-87") return EVP_PKEY_ML_DSA_87;
  if (name == "ML-KEM-512") return EVP_PKEY_ML_KEM_512;
  if (name == "ML-KEM-768") return EVP_PKEY_ML_KEM_768;
  if (name == "ML-KEM-1024") return EVP_PKEY_ML_KEM_1024;
#endif
  return NID_undef;
}

int AsymmetricKeyId(const EVP_PKEY* pkey) {
  return ncrypto::EVPKeyPointer::id(pkey);
}

#if OPENSSL_WITH_PQC
const char* PqcAsymmetricKeyTypeName(int id) {
  switch (id) {
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
    default:
      return nullptr;
  }
}

const char* MlDsaAlgorithmName(int id) {
  switch (id) {
    case EVP_PKEY_ML_DSA_44:
      return "ML-DSA-44";
    case EVP_PKEY_ML_DSA_65:
      return "ML-DSA-65";
    case EVP_PKEY_ML_DSA_87:
      return "ML-DSA-87";
    default:
      return nullptr;
  }
}
#endif

std::string AsymmetricKeyTypeName(const EVP_PKEY* pkey) {
  if (pkey == nullptr) return "";
  const int id = AsymmetricKeyId(pkey);
  switch (id) {
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
    case EVP_PKEY_ML_DSA_65:
    case EVP_PKEY_ML_DSA_87:
    case EVP_PKEY_ML_KEM_512:
    case EVP_PKEY_ML_KEM_768:
    case EVP_PKEY_ML_KEM_1024:
    case EVP_PKEY_SLH_DSA_SHA2_128S:
    case EVP_PKEY_SLH_DSA_SHA2_128F:
    case EVP_PKEY_SLH_DSA_SHA2_192S:
    case EVP_PKEY_SLH_DSA_SHA2_192F:
    case EVP_PKEY_SLH_DSA_SHA2_256S:
    case EVP_PKEY_SLH_DSA_SHA2_256F:
    case EVP_PKEY_SLH_DSA_SHAKE_128S:
    case EVP_PKEY_SLH_DSA_SHAKE_128F:
    case EVP_PKEY_SLH_DSA_SHAKE_192S:
    case EVP_PKEY_SLH_DSA_SHAKE_192F:
    case EVP_PKEY_SLH_DSA_SHAKE_256S:
    case EVP_PKEY_SLH_DSA_SHAKE_256F: {
      const char* name = PqcAsymmetricKeyTypeName(id);
      return name == nullptr ? "" : name;
    }
#endif
    default:
      return "";
  }
}

std::vector<uint8_t> Base64UrlDecode(std::string in) {
  std::vector<uint8_t> out;
  if (in.empty()) return out;
  for (char& ch : in) {
    if (ch == '-') {
      ch = '+';
    } else if (ch == '_') {
      ch = '/';
    }
  }
  const size_t remainder = in.size() % 4;
  if (remainder == 1) return {};
  const size_t padding = remainder == 0 ? 0 : 4 - remainder;
  in.append(padding, '=');
  out.resize((in.size() / 4) * 3);
  const int written =
      EVP_DecodeBlock(out.data(),
                      reinterpret_cast<const unsigned char*>(in.data()),
                      static_cast<int>(in.size()));
  if (written < 0) return {};
  size_t out_len = static_cast<size_t>(written);
  if (padding > 0 && out_len >= padding) out_len -= padding;
  out.resize(out_len);
  return out;
}

EVP_PKEY* ParsePrivateKeyBytesWithPassphrase(const uint8_t* data,
                                             size_t len,
                                             const uint8_t* passphrase,
                                             size_t passphrase_len,
                                             bool has_passphrase) {
  return edge::crypto::ParsePrivateKeyWithPassphrase(data, len, passphrase, passphrase_len, has_passphrase);
}

EVP_PKEY* ParsePrivateKeyBytes(const uint8_t* data, size_t len) {
  return ParsePrivateKeyBytesWithPassphrase(data, len, nullptr, 0, false);
}

EVP_PKEY* ParsePublicKeyBytes(const uint8_t* data, size_t len) {
  if (data == nullptr || len == 0) return nullptr;
  auto looks_like_pem = [](const uint8_t* bytes, size_t size) {
    size_t i = 0;
    while (i < size && (bytes[i] == ' ' || bytes[i] == '\t' || bytes[i] == '\r' || bytes[i] == '\n')) ++i;
    static constexpr char kPemPrefix[] = "-----BEGIN ";
    const size_t prefix_len = sizeof(kPemPrefix) - 1;
    return size >= i + prefix_len && std::memcmp(bytes + i, kPemPrefix, prefix_len) == 0;
  };
  BIO* bio = BIO_new_mem_buf(data, static_cast<int>(len));
  if (bio == nullptr) return nullptr;
  EVP_PKEY* pkey = nullptr;
  X509* cert = nullptr;
  if (looks_like_pem(data, len)) {
    pkey = PEM_read_bio_PUBKEY(bio, nullptr, nullptr, nullptr);
    if (pkey == nullptr) {
      (void)BIO_reset(bio);
      cert = PEM_read_bio_X509(bio, nullptr, nullptr, nullptr);
    }
  } else {
    pkey = d2i_PUBKEY_bio(bio, nullptr);
    if (pkey == nullptr) {
      (void)BIO_reset(bio);
      cert = d2i_X509_bio(bio, nullptr);
    }
  }
  if (cert != nullptr) {
    pkey = X509_get_pubkey(cert);
    X509_free(cert);
  }
  BIO_free(bio);
  return pkey;
}

EVP_PKEY* ParseAnyKeyBytes(const std::vector<uint8_t>& data) {
  EVP_PKEY* pkey = ParsePrivateKeyBytes(data.data(), data.size());
  if (pkey == nullptr) pkey = ParsePublicKeyBytes(data.data(), data.size());
  return pkey;
}

EVP_PKEY* ParseAnyKeyBytes(const std::vector<uint8_t>& data,
                          const std::vector<uint8_t>& passphrase,
                          bool has_passphrase) {
  EVP_PKEY* pkey = ParsePrivateKeyBytesWithPassphrase(
      data.data(), data.size(), passphrase.data(), passphrase.size(), has_passphrase);
  if (pkey == nullptr) pkey = ParsePublicKeyBytes(data.data(), data.size());
  return pkey;
}

struct HashWrap {
  napi_ref wrapper_ref = nullptr;
  std::string algorithm;
  std::vector<uint8_t> data;
  int32_t output_len = -1;
  bool use_xof = false;
  bool finalized = false;
  std::vector<uint8_t> digest_cache;
};

void HashFinalize(napi_env env, void* data, void* /*hint*/) {
  auto* wrap = static_cast<HashWrap*>(data);
  if (wrap == nullptr) return;
  ResetRef(env, &wrap->wrapper_ref);
  delete wrap;
}

HashWrap* UnwrapHash(napi_env env, napi_value this_arg) {
  if (this_arg == nullptr) return nullptr;
  void* data = nullptr;
  if (napi_unwrap(env, this_arg, &data) != napi_ok || data == nullptr) return nullptr;
  return static_cast<HashWrap*>(data);
}

napi_value HashCtor(napi_env env, napi_callback_info info) {
  size_t argc = 4;
  napi_value argv[4] = {nullptr, nullptr, nullptr, nullptr};
  napi_value this_arg = nullptr;
  if (napi_get_cb_info(env, info, &argc, argv, &this_arg, nullptr) != napi_ok || this_arg == nullptr) {
    return nullptr;
  }
  auto* wrap = new HashWrap();
  if (argc >= 1 && argv[0] != nullptr) {
    HashWrap* source = UnwrapHash(env, argv[0]);
    if (source != nullptr) {
      wrap->algorithm = source->algorithm;
      wrap->data = source->data;
      wrap->finalized = source->finalized;
      wrap->digest_cache = source->digest_cache;
    } else {
      size_t len = 0;
      if (napi_get_value_string_utf8(env, argv[0], nullptr, 0, &len) == napi_ok) {
        wrap->algorithm.resize(len + 1);
        size_t copied = 0;
        if (napi_get_value_string_utf8(
                env, argv[0], wrap->algorithm.data(), wrap->algorithm.size(), &copied) == napi_ok) {
          wrap->algorithm.resize(copied);
        }
      }
    }
  }

  if (wrap->algorithm.empty()) {
    delete wrap;
    napi_throw_error(env, nullptr, "Digest method not supported");
    return nullptr;
  }

  const ncrypto::Digest md = ncrypto::Digest::FromName(wrap->algorithm.c_str());
  if (!md) {
    delete wrap;
    napi_throw_error(env, nullptr, "Digest method not supported");
    return nullptr;
  }

  const EVP_MD* evp_md = md.get();
  const bool is_xof = evp_md != nullptr && (EVP_MD_flags(evp_md) & EVP_MD_FLAG_XOF) != 0;
  const int32_t digest_size = evp_md != nullptr ? EVP_MD_size(evp_md) : -1;
  if (argc >= 2 && argv[1] != nullptr && !IsNullOrUndefinedValue(env, argv[1])) {
    if (napi_get_value_int32(env, argv[1], &wrap->output_len) != napi_ok || wrap->output_len < 0) {
      delete wrap;
      napi_throw_error(env, "ERR_INVALID_ARG_VALUE", "Invalid output length");
      return nullptr;
    }
    if (is_xof) {
      wrap->use_xof = true;
    } else if (wrap->output_len != digest_size) {
      delete wrap;
      napi_throw_error(env,
                       "ERR_OSSL_EVP_NOT_XOF_OR_INVALID_LENGTH",
                       "error:030000B2:digital envelope routines::not XOF or invalid length");
      return nullptr;
    }
  }

  napi_wrap(env, this_arg, wrap, HashFinalize, nullptr, &wrap->wrapper_ref);
  return this_arg;
}

napi_value HashUpdate(napi_env env, napi_callback_info info) {
  size_t argc = 2;
  napi_value argv[2] = {nullptr, nullptr};
  napi_value this_arg = nullptr;
  napi_get_cb_info(env, info, &argc, argv, &this_arg, nullptr);
  HashWrap* wrap = UnwrapHash(env, this_arg);
  if (wrap == nullptr) return this_arg != nullptr ? this_arg : Undefined(env);
  if (wrap->finalized) return this_arg != nullptr ? this_arg : Undefined(env);
  if (argc >= 1 && argv[0] != nullptr) {
    std::vector<uint8_t> input = ValueToBytesWithEncoding(env, argv[0], argc >= 2 ? argv[1] : nullptr);
    wrap->data.insert(wrap->data.end(), input.begin(), input.end());
  }
  return this_arg != nullptr ? this_arg : Undefined(env);
}

napi_value HashDigest(napi_env env, napi_callback_info info) {
  size_t argc = 1;
  napi_value argv[1] = {nullptr};
  napi_value this_arg = nullptr;
  napi_get_cb_info(env, info, &argc, argv, &this_arg, nullptr);
  HashWrap* wrap = UnwrapHash(env, this_arg);
  if (wrap == nullptr) return Undefined(env);

  if (wrap->finalized) {
    napi_value cached = BytesToBuffer(env, wrap->digest_cache);
    return MaybeToEncodedOutput(env, cached, argc >= 1 ? argv[0] : nullptr);
  }
  napi_value binding = GetBinding(env);
  if (binding == nullptr) return Undefined(env);

  napi_value algorithm = nullptr;
  napi_create_string_utf8(env, wrap->algorithm.c_str(), NAPI_AUTO_LENGTH, &algorithm);
  napi_value data = BytesToBuffer(env, wrap->data);
  napi_value out = nullptr;
  if (wrap->use_xof) {
    napi_value xof_len_value = nullptr;
    napi_create_int32(env, wrap->output_len, &xof_len_value);
    napi_value call_argv[3] = {algorithm, data, xof_len_value};
    if (!CallBindingMethod(env, binding, "hashOneShotXof", 3, call_argv, &out)) return Undefined(env);
  } else {
    napi_value call_argv[2] = {algorithm, data};
    if (!CallBindingMethod(env, binding, "hashOneShot", 2, call_argv, &out)) return Undefined(env);
  }
  napi_value as_buffer = EnsureBufferValue(env, out);
  wrap->digest_cache = ValueToBytes(env, as_buffer);
  wrap->finalized = true;
  wrap->data.clear();
  return MaybeToEncodedOutput(env, as_buffer, argc >= 1 ? argv[0] : nullptr);
}

struct HmacWrap {
  napi_ref wrapper_ref = nullptr;
  std::string algorithm;
  std::vector<uint8_t> key;
  std::vector<uint8_t> data;
};

void HmacFinalize(napi_env env, void* data, void* /*hint*/) {
  auto* wrap = static_cast<HmacWrap*>(data);
  if (wrap == nullptr) return;
  ResetRef(env, &wrap->wrapper_ref);
  delete wrap;
}

HmacWrap* UnwrapHmac(napi_env env, napi_value this_arg) {
  if (this_arg == nullptr) return nullptr;
  void* data = nullptr;
  if (napi_unwrap(env, this_arg, &data) != napi_ok || data == nullptr) return nullptr;
  return static_cast<HmacWrap*>(data);
}

napi_value HmacCtor(napi_env env, napi_callback_info info) {
  napi_value this_arg = nullptr;
  size_t argc = 0;
  napi_get_cb_info(env, info, &argc, nullptr, &this_arg, nullptr);
  if (this_arg == nullptr) return nullptr;
  auto* wrap = new HmacWrap();
  wrap->algorithm = "sha256";
  napi_wrap(env, this_arg, wrap, HmacFinalize, nullptr, &wrap->wrapper_ref);
  return this_arg;
}

napi_value HmacInit(napi_env env, napi_callback_info info) {
  size_t argc = 2;
  napi_value argv[2] = {nullptr, nullptr};
  napi_value this_arg = nullptr;
  napi_get_cb_info(env, info, &argc, argv, &this_arg, nullptr);
  HmacWrap* wrap = UnwrapHmac(env, this_arg);
  if (wrap == nullptr) return Undefined(env);
  if (argc >= 1 && argv[0] != nullptr) {
    size_t len = 0;
    if (napi_get_value_string_utf8(env, argv[0], nullptr, 0, &len) == napi_ok) {
      wrap->algorithm.resize(len + 1);
      size_t copied = 0;
      if (napi_get_value_string_utf8(env, argv[0], wrap->algorithm.data(), wrap->algorithm.size(), &copied) == napi_ok) {
        wrap->algorithm.resize(copied);
      }
    }
  }
  wrap->key = (argc >= 2 && argv[1] != nullptr) ? ValueToBytes(env, argv[1]) : std::vector<uint8_t>{};
  wrap->data.clear();

  // Match Node behavior: invalid digest names throw during createHmac/init.
  napi_value binding = GetBinding(env);
  if (binding != nullptr) {
    napi_value algorithm = nullptr;
    napi_create_string_utf8(env, wrap->algorithm.c_str(), NAPI_AUTO_LENGTH, &algorithm);
    napi_value key = BytesToBuffer(env, wrap->key);
    napi_value empty = BytesToBuffer(env, {});
    napi_value validate_argv[3] = {algorithm != nullptr ? algorithm : Undefined(env),
                                   key != nullptr ? key : Undefined(env),
                                   empty != nullptr ? empty : Undefined(env)};
    napi_value ignored = nullptr;
    if (!CallBindingMethod(env, binding, "hmacOneShot", 3, validate_argv, &ignored)) {
      return nullptr;
    }
  }
  return Undefined(env);
}

napi_value HmacUpdate(napi_env env, napi_callback_info info) {
  size_t argc = 2;
  napi_value argv[2] = {nullptr, nullptr};
  napi_value this_arg = nullptr;
  napi_get_cb_info(env, info, &argc, argv, &this_arg, nullptr);
  HmacWrap* wrap = UnwrapHmac(env, this_arg);
  if (wrap == nullptr) return this_arg != nullptr ? this_arg : Undefined(env);
  if (argc >= 1 && argv[0] != nullptr) {
    std::vector<uint8_t> bytes = ValueToBytesWithEncoding(env, argv[0], argc >= 2 ? argv[1] : nullptr);
    wrap->data.insert(wrap->data.end(), bytes.begin(), bytes.end());
  }
  return this_arg != nullptr ? this_arg : Undefined(env);
}

napi_value HmacDigest(napi_env env, napi_callback_info info) {
  size_t argc = 1;
  napi_value argv[1] = {nullptr};
  napi_value this_arg = nullptr;
  napi_get_cb_info(env, info, &argc, argv, &this_arg, nullptr);
  HmacWrap* wrap = UnwrapHmac(env, this_arg);
  if (wrap == nullptr) return Undefined(env);
  napi_value binding = GetBinding(env);
  if (binding == nullptr) return Undefined(env);

  napi_value algorithm = nullptr;
  napi_create_string_utf8(env, wrap->algorithm.c_str(), NAPI_AUTO_LENGTH, &algorithm);
  napi_value key = BytesToBuffer(env, wrap->key);
  napi_value data = BytesToBuffer(env, wrap->data);
  napi_value call_argv[3] = {algorithm, key, data};
  napi_value out = nullptr;
  if (!CallBindingMethod(env, binding, "hmacOneShot", 3, call_argv, &out)) return Undefined(env);
  napi_value as_buffer = EnsureBufferValue(env, out);
  wrap->data.clear();
  return MaybeToEncodedOutput(env, as_buffer, argc >= 1 ? argv[0] : nullptr);
}

struct SignVerifyWrap {
  napi_ref wrapper_ref = nullptr;
  std::string algorithm;
  std::vector<uint8_t> data;
  bool verify = false;
};

void SignVerifyFinalize(napi_env env, void* data, void* /*hint*/) {
  auto* wrap = static_cast<SignVerifyWrap*>(data);
  if (wrap == nullptr) return;
  ResetRef(env, &wrap->wrapper_ref);
  delete wrap;
}

SignVerifyWrap* UnwrapSignVerify(napi_env env, napi_value this_arg) {
  if (this_arg == nullptr) return nullptr;
  void* data = nullptr;
  if (napi_unwrap(env, this_arg, &data) != napi_ok || data == nullptr) return nullptr;
  return static_cast<SignVerifyWrap*>(data);
}

napi_value SignCtor(napi_env env, napi_callback_info info) {
  napi_value this_arg = nullptr;
  size_t argc = 0;
  napi_get_cb_info(env, info, &argc, nullptr, &this_arg, nullptr);
  if (this_arg == nullptr) return nullptr;
  auto* wrap = new SignVerifyWrap();
  wrap->algorithm = "sha256";
  wrap->verify = false;
  napi_wrap(env, this_arg, wrap, SignVerifyFinalize, nullptr, &wrap->wrapper_ref);
  return this_arg;
}

napi_value VerifyCtor(napi_env env, napi_callback_info info) {
  napi_value this_arg = nullptr;
  size_t argc = 0;
  napi_get_cb_info(env, info, &argc, nullptr, &this_arg, nullptr);
  if (this_arg == nullptr) return nullptr;
  auto* wrap = new SignVerifyWrap();
  wrap->algorithm = "sha256";
  wrap->verify = true;
  napi_wrap(env, this_arg, wrap, SignVerifyFinalize, nullptr, &wrap->wrapper_ref);
  return this_arg;
}

napi_value SignVerifyInit(napi_env env, napi_callback_info info) {
  size_t argc = 1;
  napi_value argv[1] = {nullptr};
  napi_value this_arg = nullptr;
  napi_get_cb_info(env, info, &argc, argv, &this_arg, nullptr);
  SignVerifyWrap* wrap = UnwrapSignVerify(env, this_arg);
  if (wrap == nullptr) return Undefined(env);
  if (argc >= 1 && argv[0] != nullptr) {
    size_t len = 0;
    if (napi_get_value_string_utf8(env, argv[0], nullptr, 0, &len) == napi_ok) {
      wrap->algorithm.resize(len + 1);
      size_t copied = 0;
      if (napi_get_value_string_utf8(env, argv[0], wrap->algorithm.data(), wrap->algorithm.size(), &copied) ==
          napi_ok) {
        wrap->algorithm.resize(copied);
      }
    }
  }
  wrap->data.clear();

  if (!wrap->algorithm.empty()) {
    napi_value binding = GetBinding(env);
    napi_value algorithm = nullptr;
    napi_create_string_utf8(env, wrap->algorithm.c_str(), NAPI_AUTO_LENGTH, &algorithm);
    if (binding != nullptr && algorithm != nullptr && !IsSupportedDigestName(env, binding, algorithm)) {
      napi_throw(env, CreateErrorWithCode(env, "ERR_CRYPTO_INVALID_DIGEST", "Invalid digest"));
      return nullptr;
    }
  }
  return Undefined(env);
}

napi_value SignVerifyUpdate(napi_env env, napi_callback_info info) {
  size_t argc = 2;
  napi_value argv[2] = {nullptr, nullptr};
  napi_value this_arg = nullptr;
  napi_get_cb_info(env, info, &argc, argv, &this_arg, nullptr);
  SignVerifyWrap* wrap = UnwrapSignVerify(env, this_arg);
  if (wrap == nullptr) return Undefined(env);
  if (argc >= 1 && argv[0] != nullptr) {
    std::vector<uint8_t> bytes = ValueToBytesWithEncoding(env, argv[0], argc >= 2 ? argv[1] : nullptr);
    wrap->data.insert(wrap->data.end(), bytes.begin(), bytes.end());
  }
  return this_arg != nullptr ? this_arg : Undefined(env);
}

bool IsOneShotKeyType(const EVP_PKEY* pkey) {
  if (pkey == nullptr) return false;
  switch (EVP_PKEY_base_id(pkey)) {
    case EVP_PKEY_ED25519:
    case EVP_PKEY_ED448:
#if OPENSSL_WITH_PQC
    case EVP_PKEY_ML_DSA_44:
    case EVP_PKEY_ML_DSA_65:
    case EVP_PKEY_ML_DSA_87:
#endif
      return true;
    default:
      return false;
  }
}

bool ShouldRejectStreamingForOneShotKey(napi_env env,
                                        napi_value key_value,
                                        napi_value key_passphrase_value,
                                        bool require_private) {
  std::string error_code;
  std::string error_message;
  EVP_PKEY* pkey =
      GetAsymmetricKeyFromValue(env, key_value, key_passphrase_value, require_private, &error_code, &error_message);

  if (pkey == nullptr) {
    while (ERR_get_error() != 0) {
    }
    return false;
  }

  const bool is_one_shot_key = IsOneShotKeyType(pkey);
  EVP_PKEY_free(pkey);
  return is_one_shot_key;
}

napi_value SignSign(napi_env env, napi_callback_info info) {
  size_t argc = 7;
  napi_value argv[7] = {nullptr, nullptr, nullptr, nullptr, nullptr, nullptr, nullptr};
  napi_value this_arg = nullptr;
  napi_get_cb_info(env, info, &argc, argv, &this_arg, nullptr);
  SignVerifyWrap* wrap = UnwrapSignVerify(env, this_arg);
  if (wrap == nullptr) return Undefined(env);
  napi_value binding = GetBinding(env);
  if (binding == nullptr) return Undefined(env);

  napi_value algorithm = nullptr;
  napi_create_string_utf8(env, wrap->algorithm.c_str(), NAPI_AUTO_LENGTH, &algorithm);
  if (algorithm == nullptr) napi_get_null(env, &algorithm);

  napi_value data = BytesToBuffer(env, wrap->data);
  napi_value key_data = argc >= 1 ? argv[0] : Undefined(env);
  napi_value key_format = argc >= 2 ? argv[1] : Undefined(env);
  napi_value key_type = argc >= 3 ? argv[2] : Undefined(env);
  napi_value key_passphrase = argc >= 4 ? argv[3] : Undefined(env);
  napi_value padding = argc >= 5 ? argv[4] : Undefined(env);
  napi_value salt = argc >= 6 ? argv[5] : Undefined(env);
  napi_value dsa_sig_enc = argc >= 7 ? argv[6] : Undefined(env);
  if (ShouldRejectStreamingForOneShotKey(env, key_data, key_passphrase, true)) {
    napi_throw_error(env, "ERR_CRYPTO_UNSUPPORTED_OPERATION", "Unsupported crypto operation");
    return nullptr;
  }
  napi_value context = Undefined(env);
  napi_value call_argv[10] = {
      algorithm,
      data,
      key_data,
      key_format,
      key_type,
      key_passphrase,
      padding,
      salt,
      dsa_sig_enc,
      context,
  };
  napi_value out = nullptr;
  if (!CallBindingMethod(env, binding, "signOneShot", 10, call_argv, &out)) return nullptr;
  wrap->data.clear();
  return EnsureBufferValue(env, out);
}

napi_value VerifyVerify(napi_env env, napi_callback_info info) {
  size_t argc = 8;
  napi_value argv[8] = {nullptr, nullptr, nullptr, nullptr, nullptr, nullptr, nullptr, nullptr};
  napi_value this_arg = nullptr;
  napi_get_cb_info(env, info, &argc, argv, &this_arg, nullptr);
  SignVerifyWrap* wrap = UnwrapSignVerify(env, this_arg);
  if (wrap == nullptr) return Undefined(env);
  napi_value binding = GetBinding(env);
  if (binding == nullptr) return Undefined(env);

  napi_value algorithm = nullptr;
  napi_create_string_utf8(env, wrap->algorithm.c_str(), NAPI_AUTO_LENGTH, &algorithm);
  if (algorithm == nullptr) napi_get_null(env, &algorithm);

  napi_value data = BytesToBuffer(env, wrap->data);
  napi_value key_data = argc >= 1 ? argv[0] : Undefined(env);
  napi_value key_format = argc >= 2 ? argv[1] : Undefined(env);
  napi_value key_type = argc >= 3 ? argv[2] : Undefined(env);
  napi_value key_passphrase = argc >= 4 ? argv[3] : Undefined(env);
  napi_value signature = EnsureBufferValue(env, argc >= 5 ? argv[4] : Undefined(env));
  napi_value padding = argc >= 6 ? argv[5] : Undefined(env);
  napi_value salt = argc >= 7 ? argv[6] : Undefined(env);
  napi_value dsa_sig_enc = argc >= 8 ? argv[7] : Undefined(env);
  if (ShouldRejectStreamingForOneShotKey(env, key_data, key_passphrase, false)) {
    napi_throw_error(env, "ERR_CRYPTO_UNSUPPORTED_OPERATION", "Unsupported crypto operation");
    return nullptr;
  }
  napi_value context = Undefined(env);
  napi_value call_argv[11] = {
      algorithm,
      data,
      key_data,
      key_format,
      key_type,
      key_passphrase,
      signature,
      padding,
      salt,
      dsa_sig_enc,
      context,
  };
  napi_value out = nullptr;
  if (!CallBindingMethod(env, binding, "verifyOneShot", 11, call_argv, &out)) return nullptr;
  wrap->data.clear();
  return out != nullptr ? out : Undefined(env);
}

struct DiffieHellmanWrap {
  napi_ref wrapper_ref = nullptr;
  DH* dh = nullptr;
  int32_t verify_error = 0;
};

enum class DhCheckPublicKeyResult {
  kNone,
  kInvalid,
  kTooSmall,
  kTooLarge,
  kCheckFailed,
};

void DiffieHellmanFinalize(napi_env env, void* data, void* /*hint*/) {
  auto* wrap = static_cast<DiffieHellmanWrap*>(data);
  if (wrap == nullptr) return;
  if (wrap->dh != nullptr) {
    DH_free(wrap->dh);
    wrap->dh = nullptr;
  }
  ResetRef(env, &wrap->wrapper_ref);
  delete wrap;
}

DiffieHellmanWrap* UnwrapDiffieHellman(napi_env env, napi_value this_arg) {
  if (this_arg == nullptr) return nullptr;
  void* data = nullptr;
  if (napi_unwrap(env, this_arg, &data) != napi_ok || data == nullptr) return nullptr;
  return static_cast<DiffieHellmanWrap*>(data);
}

DhCheckPublicKeyResult CheckDhPublicKey(DH* dh, const BIGNUM* peer_key) {
  if (dh == nullptr || peer_key == nullptr) return DhCheckPublicKeyResult::kCheckFailed;
  int codes = 0;
  if (DH_check_pub_key(dh, peer_key, &codes) != 1) return DhCheckPublicKeyResult::kCheckFailed;
  if ((codes & DH_CHECK_PUBKEY_TOO_SMALL) != 0) return DhCheckPublicKeyResult::kTooSmall;
  if ((codes & DH_CHECK_PUBKEY_TOO_LARGE) != 0) return DhCheckPublicKeyResult::kTooLarge;
  if ((codes & DH_CHECK_PUBKEY_INVALID) != 0) return DhCheckPublicKeyResult::kInvalid;
  return DhCheckPublicKeyResult::kNone;
}

DH* CreateDhFromPQG(BIGNUM* prime, BIGNUM* generator, int32_t* verify_error) {
  if (prime == nullptr || generator == nullptr) {
    if (prime != nullptr) BN_free(prime);
    if (generator != nullptr) BN_free(generator);
    return nullptr;
  }
  DH* dh = DH_new();
  if (dh == nullptr) {
    BN_free(prime);
    BN_free(generator);
    return nullptr;
  }
  if (DH_set0_pqg(dh, prime, nullptr, generator) != 1) {
    BN_free(prime);
    BN_free(generator);
    DH_free(dh);
    return nullptr;
  }
  int codes = 0;
  if (DH_check(dh, &codes) == 1 && verify_error != nullptr) *verify_error = codes;
  return dh;
}

BIGNUM* DhPrimeFromBitLength(int32_t bits) {
  switch (bits) {
    case 768:
      return BN_get_rfc2409_prime_768(nullptr);
    case 1024:
      return BN_get_rfc2409_prime_1024(nullptr);
    case 1536:
      return BN_get_rfc3526_prime_1536(nullptr);
    case 2048:
      return BN_get_rfc3526_prime_2048(nullptr);
    case 3072:
      return BN_get_rfc3526_prime_3072(nullptr);
    case 4096:
      return BN_get_rfc3526_prime_4096(nullptr);
    case 6144:
      return BN_get_rfc3526_prime_6144(nullptr);
    case 8192:
      return BN_get_rfc3526_prime_8192(nullptr);
    default:
      return nullptr;
  }
}

BIGNUM* DhPrimeFromGroupName(const std::string& name) {
  if (name == "modp1") return BN_get_rfc2409_prime_768(nullptr);
  if (name == "modp2") return BN_get_rfc2409_prime_1024(nullptr);
  if (name == "modp5") return BN_get_rfc3526_prime_1536(nullptr);
  if (name == "modp14") return BN_get_rfc3526_prime_2048(nullptr);
  if (name == "modp15") return BN_get_rfc3526_prime_3072(nullptr);
  if (name == "modp16") return BN_get_rfc3526_prime_4096(nullptr);
  if (name == "modp17") return BN_get_rfc3526_prime_6144(nullptr);
  if (name == "modp18") return BN_get_rfc3526_prime_8192(nullptr);
  return nullptr;
}

BIGNUM* DhGeneratorFromNumber(int32_t generator_number) {
  if (generator_number == 0) generator_number = 2;
  BIGNUM* g = BN_new();
  if (g == nullptr) return nullptr;
  if (BN_set_word(g, static_cast<BN_ULONG>(generator_number)) != 1) {
    BN_free(g);
    return nullptr;
  }
  return g;
}

void PushDhModulusTooSmallError() {
#if OPENSSL_VERSION_NUMBER >= 0x30000000L
  ERR_raise(ERR_LIB_DH, DH_R_MODULUS_TOO_SMALL);
#else
  ERR_raise(ERR_LIB_BN, BN_R_BITS_TOO_SMALL);
#endif
}

void PushDhBadGeneratorError() {
  ERR_raise(ERR_LIB_DH, DH_R_BAD_GENERATOR);
}

bool IsValidDhGenerator(const BIGNUM* generator) {
  return generator != nullptr && !BN_is_zero(generator) && !BN_is_one(generator);
}

DH* CreateDhFromSize(int32_t bits,
                     int32_t generator_number,
                     const std::vector<uint8_t>& generator_bytes,
                     bool generator_from_bytes,
                     int32_t* verify_error) {
  if (bits < 2) {
    PushDhModulusTooSmallError();
    return nullptr;
  }

  BIGNUM* prime = DhPrimeFromBitLength(bits);
  if (prime != nullptr) {
    BIGNUM* generator = nullptr;
    if (generator_from_bytes) {
      if (generator_bytes.empty()) {
        PushDhBadGeneratorError();
        BN_free(prime);
        return nullptr;
      }
      generator = BN_bin2bn(generator_bytes.data(), static_cast<int>(generator_bytes.size()), nullptr);
    } else {
      if (generator_number < 2) {
        PushDhBadGeneratorError();
        BN_free(prime);
        return nullptr;
      }
      generator = DhGeneratorFromNumber(generator_number);
    }
    if (!IsValidDhGenerator(generator)) {
      if (generator != nullptr) BN_free(generator);
      BN_free(prime);
      PushDhBadGeneratorError();
      return nullptr;
    }
    return CreateDhFromPQG(prime, generator, verify_error);
  }

  if (generator_from_bytes) {
    return nullptr;
  }
  if (generator_number < 2) {
    PushDhBadGeneratorError();
    return nullptr;
  }
  DH* dh = DH_new();
  if (dh == nullptr) return nullptr;
  if (DH_generate_parameters_ex(dh, bits, generator_number, nullptr) != 1) {
    DH_free(dh);
    return nullptr;
  }
  int codes = 0;
  if (DH_check(dh, &codes) == 1 && verify_error != nullptr) *verify_error = codes;
  return dh;
}

DH* CreateDhFromPrimeAndGenerator(const std::vector<uint8_t>& prime_bytes,
                                  int32_t generator_number,
                                  const std::vector<uint8_t>& generator_bytes,
                                  bool generator_from_bytes,
                                  int32_t* verify_error) {
  if (prime_bytes.empty()) return nullptr;
  BIGNUM* prime = BN_bin2bn(prime_bytes.data(), static_cast<int>(prime_bytes.size()), nullptr);
  BIGNUM* generator = nullptr;
  if (generator_from_bytes) {
    if (generator_bytes.empty()) {
      if (prime != nullptr) BN_free(prime);
      PushDhBadGeneratorError();
      return nullptr;
    }
    generator = BN_bin2bn(generator_bytes.data(), static_cast<int>(generator_bytes.size()), nullptr);
  } else {
    if (generator_number < 2) {
      if (prime != nullptr) BN_free(prime);
      PushDhBadGeneratorError();
      return nullptr;
    }
    generator = DhGeneratorFromNumber(generator_number);
  }
  if (!IsValidDhGenerator(generator)) {
    if (prime != nullptr) BN_free(prime);
    if (generator != nullptr) BN_free(generator);
    PushDhBadGeneratorError();
    return nullptr;
  }
  return CreateDhFromPQG(prime, generator, verify_error);
}

DH* CreateDhFromGroupName(const std::string& group_name, int32_t* verify_error) {
  BIGNUM* prime = DhPrimeFromGroupName(group_name);
  if (prime == nullptr) return nullptr;
  BIGNUM* generator = DhGeneratorFromNumber(2);
  return CreateDhFromPQG(prime, generator, verify_error);
}

napi_value DiffieHellmanCtor(napi_env env, napi_callback_info info) {
  size_t argc = 2;
  napi_value argv[2] = {nullptr, nullptr};
  napi_value this_arg = nullptr;
  if (napi_get_cb_info(env, info, &argc, argv, &this_arg, nullptr) != napi_ok || this_arg == nullptr) {
    return nullptr;
  }

  int32_t verify_error = 0;
  DH* dh = nullptr;
  napi_valuetype size_or_key_type = napi_undefined;
  if (argc >= 1 && argv[0] != nullptr) napi_typeof(env, argv[0], &size_or_key_type);

  int32_t generator_number = 2;
  std::vector<uint8_t> generator_bytes;
  bool generator_from_bytes = false;
  if (argc >= 2 && argv[1] != nullptr && !IsNullOrUndefinedValue(env, argv[1])) {
    napi_valuetype generator_type = napi_undefined;
    if (napi_typeof(env, argv[1], &generator_type) == napi_ok && generator_type == napi_number) {
      generator_number = GetInt32Value(env, argv[1], 2);
    } else {
      generator_bytes = ValueToBytes(env, argv[1]);
      generator_from_bytes = true;
    }
  }

  if (size_or_key_type == napi_number) {
    const int32_t bits = GetInt32Value(env, argv[0], 0);
    dh = CreateDhFromSize(bits, generator_number, generator_bytes, generator_from_bytes, &verify_error);
  } else {
    const std::vector<uint8_t> prime_bytes = ValueToBytes(env, argv[0]);
    dh = CreateDhFromPrimeAndGenerator(prime_bytes,
                                       generator_number,
                                       generator_bytes,
                                       generator_from_bytes,
                                       &verify_error);
  }

  if (dh == nullptr) {
    ThrowLastOpenSslMessage(env, "Diffie-Hellman initialization failed");
    return nullptr;
  }

  auto* wrap = new DiffieHellmanWrap();
  wrap->dh = dh;
  wrap->verify_error = verify_error;
  napi_wrap(env, this_arg, wrap, DiffieHellmanFinalize, nullptr, &wrap->wrapper_ref);
  SetNamedInt(env, this_arg, "verifyError", wrap->verify_error);
  return this_arg;
}

napi_value DiffieHellmanGroupCtor(napi_env env, napi_callback_info info) {
  size_t argc = 1;
  napi_value argv[1] = {nullptr};
  napi_value this_arg = nullptr;
  if (napi_get_cb_info(env, info, &argc, argv, &this_arg, nullptr) != napi_ok || this_arg == nullptr) {
    return nullptr;
  }
  if (argc < 1 || argv[0] == nullptr) {
    napi_throw_error(env, "ERR_CRYPTO_UNKNOWN_DH_GROUP", "Unknown DH group");
    return nullptr;
  }
  const std::string group_name = GetStringValue(env, argv[0]);
  int32_t verify_error = 0;
  DH* dh = CreateDhFromGroupName(group_name, &verify_error);
  if (dh == nullptr) {
    napi_throw_error(env, "ERR_CRYPTO_UNKNOWN_DH_GROUP", "Unknown DH group");
    return nullptr;
  }

  auto* wrap = new DiffieHellmanWrap();
  wrap->dh = dh;
  wrap->verify_error = verify_error;
  napi_wrap(env, this_arg, wrap, DiffieHellmanFinalize, nullptr, &wrap->wrapper_ref);
  SetNamedInt(env, this_arg, "verifyError", wrap->verify_error);
  return this_arg;
}

napi_value DiffieHellmanGenerateKeys(napi_env env, napi_callback_info info) {
  napi_value this_arg = nullptr;
  size_t argc = 0;
  napi_get_cb_info(env, info, &argc, nullptr, &this_arg, nullptr);
  DiffieHellmanWrap* wrap = UnwrapDiffieHellman(env, this_arg);
  if (wrap == nullptr || wrap->dh == nullptr) return Undefined(env);
  if (DH_generate_key(wrap->dh) != 1) {
    napi_throw_error(env, "ERR_CRYPTO_OPERATION_FAILED", "Diffie-Hellman key generation failed");
    return nullptr;
  }
  const BIGNUM* public_key = nullptr;
  const BIGNUM* private_key = nullptr;
  DH_get0_key(wrap->dh, &public_key, &private_key);
  return BytesToBuffer(env, BigNumToBytes(public_key));
}

napi_value DiffieHellmanComputeSecret(napi_env env, napi_callback_info info) {
  size_t argc = 1;
  napi_value argv[1] = {nullptr};
  napi_value this_arg = nullptr;
  napi_get_cb_info(env, info, &argc, argv, &this_arg, nullptr);
  DiffieHellmanWrap* wrap = UnwrapDiffieHellman(env, this_arg);
  if (wrap == nullptr || wrap->dh == nullptr || argc < 1 || argv[0] == nullptr) return Undefined(env);

  const std::vector<uint8_t> peer_bytes = ValueToBytes(env, argv[0]);
  BIGNUM* peer_key = BN_bin2bn(peer_bytes.data(), static_cast<int>(peer_bytes.size()), nullptr);
  if (peer_key == nullptr) {
    napi_throw_error(env, "ERR_INVALID_ARG_TYPE", "Invalid peer public key");
    return nullptr;
  }

  switch (CheckDhPublicKey(wrap->dh, peer_key)) {
    case DhCheckPublicKeyResult::kTooSmall:
      BN_free(peer_key);
      napi_throw(env, CreateErrorWithCode(env, "ERR_CRYPTO_INVALID_KEYLEN", "Supplied key is too small"));
      return nullptr;
    case DhCheckPublicKeyResult::kTooLarge:
      BN_free(peer_key);
      napi_throw(env, CreateErrorWithCode(env, "ERR_CRYPTO_INVALID_KEYLEN", "Supplied key is too large"));
      return nullptr;
    case DhCheckPublicKeyResult::kInvalid:
    case DhCheckPublicKeyResult::kCheckFailed:
      BN_free(peer_key);
      napi_throw(env, CreateErrorWithCode(env, "ERR_CRYPTO_INVALID_KEYTYPE", "Unspecified validation error"));
      return nullptr;
    case DhCheckPublicKeyResult::kNone:
      break;
  }

  std::vector<uint8_t> secret(static_cast<size_t>(DH_size(wrap->dh)));
  const int secret_len = DH_compute_key(secret.data(), peer_key, wrap->dh);
  BN_free(peer_key);

  if (secret_len <= 0) {
    napi_value err_text = nullptr;
    napi_create_string_utf8(env, "invalid public key", NAPI_AUTO_LENGTH, &err_text);
    return err_text;
  }
  if (static_cast<size_t>(secret_len) < secret.size()) {
    const size_t padding = secret.size() - static_cast<size_t>(secret_len);
    std::memmove(secret.data() + padding, secret.data(), static_cast<size_t>(secret_len));
    std::memset(secret.data(), 0, padding);
  }
  return BytesToBuffer(env, secret);
}

napi_value DiffieHellmanGetPrime(napi_env env, napi_callback_info info) {
  napi_value this_arg = nullptr;
  size_t argc = 0;
  napi_get_cb_info(env, info, &argc, nullptr, &this_arg, nullptr);
  DiffieHellmanWrap* wrap = UnwrapDiffieHellman(env, this_arg);
  if (wrap == nullptr || wrap->dh == nullptr) return Undefined(env);
  const BIGNUM* prime = nullptr;
  const BIGNUM* q = nullptr;
  const BIGNUM* generator = nullptr;
  DH_get0_pqg(wrap->dh, &prime, &q, &generator);
  return BytesToBuffer(env, BigNumToBytes(prime));
}

napi_value DiffieHellmanGetGenerator(napi_env env, napi_callback_info info) {
  napi_value this_arg = nullptr;
  size_t argc = 0;
  napi_get_cb_info(env, info, &argc, nullptr, &this_arg, nullptr);
  DiffieHellmanWrap* wrap = UnwrapDiffieHellman(env, this_arg);
  if (wrap == nullptr || wrap->dh == nullptr) return Undefined(env);
  const BIGNUM* prime = nullptr;
  const BIGNUM* q = nullptr;
  const BIGNUM* generator = nullptr;
  DH_get0_pqg(wrap->dh, &prime, &q, &generator);
  return BytesToBuffer(env, BigNumToBytes(generator));
}

napi_value DiffieHellmanGetPublicKey(napi_env env, napi_callback_info info) {
  napi_value this_arg = nullptr;
  size_t argc = 0;
  napi_get_cb_info(env, info, &argc, nullptr, &this_arg, nullptr);
  DiffieHellmanWrap* wrap = UnwrapDiffieHellman(env, this_arg);
  if (wrap == nullptr || wrap->dh == nullptr) return Undefined(env);
  const BIGNUM* public_key = nullptr;
  const BIGNUM* private_key = nullptr;
  DH_get0_key(wrap->dh, &public_key, &private_key);
  return BytesToBuffer(env, BigNumToBytes(public_key));
}

napi_value DiffieHellmanGetPrivateKey(napi_env env, napi_callback_info info) {
  napi_value this_arg = nullptr;
  size_t argc = 0;
  napi_get_cb_info(env, info, &argc, nullptr, &this_arg, nullptr);
  DiffieHellmanWrap* wrap = UnwrapDiffieHellman(env, this_arg);
  if (wrap == nullptr || wrap->dh == nullptr) return Undefined(env);
  const BIGNUM* public_key = nullptr;
  const BIGNUM* private_key = nullptr;
  DH_get0_key(wrap->dh, &public_key, &private_key);
  return BytesToBuffer(env, BigNumToBytes(private_key));
}

napi_value DiffieHellmanSetPublicKey(napi_env env, napi_callback_info info) {
  size_t argc = 1;
  napi_value argv[1] = {nullptr};
  napi_value this_arg = nullptr;
  napi_get_cb_info(env, info, &argc, argv, &this_arg, nullptr);
  DiffieHellmanWrap* wrap = UnwrapDiffieHellman(env, this_arg);
  if (wrap == nullptr || wrap->dh == nullptr || argc < 1 || argv[0] == nullptr) return Undefined(env);
  const std::vector<uint8_t> public_bytes = ValueToBytes(env, argv[0]);
  BIGNUM* new_public = BN_bin2bn(public_bytes.data(), static_cast<int>(public_bytes.size()), nullptr);
  if (new_public == nullptr) {
    napi_throw_error(env, "ERR_INVALID_ARG_TYPE", "Invalid public key");
    return nullptr;
  }
  const BIGNUM* current_public = nullptr;
  const BIGNUM* current_private = nullptr;
  DH_get0_key(wrap->dh, &current_public, &current_private);
  BIGNUM* private_copy = current_private != nullptr ? BN_dup(current_private) : nullptr;
  if (DH_set0_key(wrap->dh, new_public, private_copy) != 1) {
    BN_free(new_public);
    if (private_copy != nullptr) BN_free(private_copy);
    napi_throw_error(env, "ERR_CRYPTO_OPERATION_FAILED", "Failed to set public key");
    return nullptr;
  }
  return Undefined(env);
}

napi_value DiffieHellmanSetPrivateKey(napi_env env, napi_callback_info info) {
  size_t argc = 1;
  napi_value argv[1] = {nullptr};
  napi_value this_arg = nullptr;
  napi_get_cb_info(env, info, &argc, argv, &this_arg, nullptr);
  DiffieHellmanWrap* wrap = UnwrapDiffieHellman(env, this_arg);
  if (wrap == nullptr || wrap->dh == nullptr || argc < 1 || argv[0] == nullptr) return Undefined(env);
  const std::vector<uint8_t> private_bytes = ValueToBytes(env, argv[0]);
  BIGNUM* new_private = BN_bin2bn(private_bytes.data(), static_cast<int>(private_bytes.size()), nullptr);
  if (new_private == nullptr) {
    napi_throw_error(env, "ERR_INVALID_ARG_TYPE", "Invalid private key");
    return nullptr;
  }
  const BIGNUM* current_public = nullptr;
  const BIGNUM* current_private = nullptr;
  DH_get0_key(wrap->dh, &current_public, &current_private);
  BIGNUM* public_copy = current_public != nullptr ? BN_dup(current_public) : nullptr;
  if (DH_set0_key(wrap->dh, public_copy, new_private) != 1) {
    if (public_copy != nullptr) BN_free(public_copy);
    BN_free(new_private);
    napi_throw_error(env, "ERR_CRYPTO_OPERATION_FAILED", "Failed to set private key");
    return nullptr;
  }
  return Undefined(env);
}

struct EcdhWrap {
  napi_ref wrapper_ref = nullptr;
  EC_KEY* ec = nullptr;
};

void EcdhFinalize(napi_env env, void* data, void* /*hint*/) {
  auto* wrap = static_cast<EcdhWrap*>(data);
  if (wrap == nullptr) return;
  if (wrap->ec != nullptr) {
    EC_KEY_free(wrap->ec);
    wrap->ec = nullptr;
  }
  ResetRef(env, &wrap->wrapper_ref);
  delete wrap;
}

EcdhWrap* UnwrapEcdh(napi_env env, napi_value this_arg) {
  if (this_arg == nullptr) return nullptr;
  void* data = nullptr;
  if (napi_unwrap(env, this_arg, &data) != napi_ok || data == nullptr) return nullptr;
  return static_cast<EcdhWrap*>(data);
}

napi_value EcdhCtor(napi_env env, napi_callback_info info) {
  size_t argc = 1;
  napi_value argv[1] = {nullptr};
  napi_value this_arg = nullptr;
  if (napi_get_cb_info(env, info, &argc, argv, &this_arg, nullptr) != napi_ok || this_arg == nullptr) {
    return nullptr;
  }
  if (argc < 1 || argv[0] == nullptr) {
    napi_throw_error(env, "ERR_INVALID_ARG_TYPE", "Invalid ECDH curve");
    return nullptr;
  }
  const std::string curve = GetStringValue(env, argv[0]);
  int nid = OBJ_txt2nid(curve.c_str());
  if (nid == NID_undef) nid = OBJ_sn2nid(curve.c_str());
  if (nid == NID_undef) nid = OBJ_ln2nid(curve.c_str());
  if (nid == NID_undef) {
    napi_throw_type_error(env, "ERR_CRYPTO_INVALID_CURVE", "Invalid ECDH curve name");
    return nullptr;
  }
  EC_KEY* ec = EC_KEY_new_by_curve_name(nid);
  if (ec == nullptr) {
    napi_throw_error(env, "ERR_CRYPTO_OPERATION_FAILED", "ECDH initialization failed");
    return nullptr;
  }
  auto* wrap = new EcdhWrap();
  wrap->ec = ec;
  napi_wrap(env, this_arg, wrap, EcdhFinalize, nullptr, &wrap->wrapper_ref);
  return this_arg;
}

napi_value EcdhGenerateKeys(napi_env env, napi_callback_info info) {
  napi_value this_arg = nullptr;
  size_t argc = 0;
  napi_get_cb_info(env, info, &argc, nullptr, &this_arg, nullptr);
  EcdhWrap* wrap = UnwrapEcdh(env, this_arg);
  if (wrap == nullptr || wrap->ec == nullptr) return Undefined(env);
  if (EC_KEY_generate_key(wrap->ec) != 1) {
    napi_throw_error(env, "ERR_CRYPTO_OPERATION_FAILED", "ECDH key generation failed");
    return nullptr;
  }
  return Undefined(env);
}

napi_value EcdhGetPublicKey(napi_env env, napi_callback_info info) {
  size_t argc = 1;
  napi_value argv[1] = {nullptr};
  napi_value this_arg = nullptr;
  napi_get_cb_info(env, info, &argc, argv, &this_arg, nullptr);
  EcdhWrap* wrap = UnwrapEcdh(env, this_arg);
  if (wrap == nullptr || wrap->ec == nullptr) return Undefined(env);
  int32_t form = POINT_CONVERSION_UNCOMPRESSED;
  if (argc >= 1 && argv[0] != nullptr && !IsUndefined(env, argv[0])) {
    form = GetInt32Value(env, argv[0], POINT_CONVERSION_UNCOMPRESSED);
  }
  point_conversion_form_t conversion = POINT_CONVERSION_UNCOMPRESSED;
  if (form == POINT_CONVERSION_COMPRESSED) {
    conversion = POINT_CONVERSION_COMPRESSED;
  } else if (form == POINT_CONVERSION_HYBRID) {
    conversion = POINT_CONVERSION_HYBRID;
  }
  const EC_GROUP* group = EC_KEY_get0_group(wrap->ec);
  const EC_POINT* point = EC_KEY_get0_public_key(wrap->ec);
  if (group == nullptr || point == nullptr) {
    napi_throw_error(env, "ERR_CRYPTO_OPERATION_FAILED", "Failed to get ECDH public key");
    return nullptr;
  }
  const size_t len = EC_POINT_point2oct(group, point, conversion, nullptr, 0, nullptr);
  std::vector<uint8_t> out(len);
  if (len == 0 ||
      EC_POINT_point2oct(group, point, conversion, out.data(), out.size(), nullptr) != len) {
    napi_throw_error(env, "ERR_CRYPTO_OPERATION_FAILED", "Failed to export ECDH public key");
    return nullptr;
  }
  return BytesToBuffer(env, out);
}

napi_value EcdhConvertKey(napi_env env, napi_callback_info info) {
  size_t argc = 3;
  napi_value argv[3] = {nullptr, nullptr, nullptr};
  if (napi_get_cb_info(env, info, &argc, argv, nullptr, nullptr) != napi_ok || argc < 3) return nullptr;

  const uint8_t* key_bytes = nullptr;
  size_t key_len = 0;
  if (!GetByteSpan(env, argv[0], &key_bytes, &key_len)) {
    napi_throw_type_error(env, "ERR_INVALID_ARG_TYPE", "key must be an ArrayBuffer or BufferView");
    return nullptr;
  }
  if (key_len == 0) {
    napi_value empty = nullptr;
    napi_create_string_utf8(env, "", 0, &empty);
    return empty;
  }

  const std::string curve = GetStringValue(env, argv[1]);
  int nid = OBJ_sn2nid(curve.c_str());
  if (nid == NID_undef) {
    ERR_clear_error();
    napi_throw_type_error(env, "ERR_CRYPTO_INVALID_CURVE", "Invalid EC curve name");
    return nullptr;
  }

  const int32_t form = GetInt32Value(env, argv[2], POINT_CONVERSION_UNCOMPRESSED);
  point_conversion_form_t conversion = POINT_CONVERSION_UNCOMPRESSED;
  if (form == POINT_CONVERSION_COMPRESSED) {
    conversion = POINT_CONVERSION_COMPRESSED;
  } else if (form == POINT_CONVERSION_HYBRID) {
    conversion = POINT_CONVERSION_HYBRID;
  }

  EC_GROUP* group = EC_GROUP_new_by_curve_name(nid);
  EC_POINT* point = group != nullptr ? EC_POINT_new(group) : nullptr;
  if (group == nullptr ||
      point == nullptr ||
      EC_POINT_oct2point(group, point, key_bytes, key_len, nullptr) != 1) {
    if (point != nullptr) EC_POINT_free(point);
    if (group != nullptr) EC_GROUP_free(group);
    ERR_clear_error();
    napi_throw_error(env, "ERR_CRYPTO_OPERATION_FAILED", "Failed to convert Buffer to EC_POINT");
    return nullptr;
  }

  const size_t out_len = EC_POINT_point2oct(group, point, conversion, nullptr, 0, nullptr);
  std::vector<uint8_t> out(out_len);
  if (out_len == 0 || EC_POINT_point2oct(group, point, conversion, out.data(), out.size(), nullptr) != out_len) {
    EC_POINT_free(point);
    EC_GROUP_free(group);
    ERR_clear_error();
    napi_throw_error(env, "ERR_CRYPTO_OPERATION_FAILED", "Failed to convert EC_POINT to Buffer");
    return nullptr;
  }

  EC_POINT_free(point);
  EC_GROUP_free(group);
  ERR_clear_error();
  return BytesToBuffer(env, out);
}

napi_value EcdhGetPrivateKey(napi_env env, napi_callback_info info) {
  napi_value this_arg = nullptr;
  size_t argc = 0;
  napi_get_cb_info(env, info, &argc, nullptr, &this_arg, nullptr);
  EcdhWrap* wrap = UnwrapEcdh(env, this_arg);
  if (wrap == nullptr || wrap->ec == nullptr) return Undefined(env);
  const BIGNUM* private_key = EC_KEY_get0_private_key(wrap->ec);
  if (private_key == nullptr) {
    napi_throw_error(env, "ERR_CRYPTO_OPERATION_FAILED", "Failed to get ECDH private key");
    return nullptr;
  }
  return BytesToBuffer(env, BigNumToBytes(private_key));
}

napi_value EcdhSetPrivateKey(napi_env env, napi_callback_info info) {
  size_t argc = 1;
  napi_value argv[1] = {nullptr};
  napi_value this_arg = nullptr;
  napi_get_cb_info(env, info, &argc, argv, &this_arg, nullptr);
  EcdhWrap* wrap = UnwrapEcdh(env, this_arg);
  if (wrap == nullptr || wrap->ec == nullptr || argc < 1 || argv[0] == nullptr) return Undefined(env);
  const std::vector<uint8_t> key_bytes = ValueToBytes(env, argv[0]);
  BIGNUM* private_key = BN_bin2bn(key_bytes.data(), static_cast<int>(key_bytes.size()), nullptr);
  if (private_key == nullptr) {
    napi_throw_error(env, "ERR_INVALID_ARG_TYPE", "Invalid private key");
    return nullptr;
  }
  const EC_GROUP* group = EC_KEY_get0_group(wrap->ec);
  BIGNUM* order = group != nullptr ? BN_new() : nullptr;
  const bool valid_private_key =
      group != nullptr &&
      order != nullptr &&
      !BN_is_zero(private_key) &&
      !BN_is_negative(private_key) &&
      EC_GROUP_get_order(group, order, nullptr) == 1 &&
      BN_cmp(private_key, order) < 0;
  if (order != nullptr) BN_free(order);
  if (!valid_private_key) {
    BN_free(private_key);
    napi_throw(env,
               CreateErrorWithCode(env,
                                   "ERR_CRYPTO_INVALID_KEYTYPE",
                                   "Private key is not valid for specified curve."));
    return nullptr;
  }
  if (EC_KEY_set_private_key(wrap->ec, private_key) != 1) {
    BN_free(private_key);
    napi_throw_error(env, "ERR_CRYPTO_OPERATION_FAILED", "Failed to set ECDH private key");
    return nullptr;
  }
  EC_POINT* public_key = group != nullptr ? EC_POINT_new(group) : nullptr;
  BN_CTX* bn_ctx = BN_CTX_new();
  if (group == nullptr ||
      public_key == nullptr ||
      bn_ctx == nullptr ||
      EC_POINT_mul(group, public_key, private_key, nullptr, nullptr, bn_ctx) != 1 ||
      EC_KEY_set_public_key(wrap->ec, public_key) != 1) {
    if (public_key != nullptr) EC_POINT_free(public_key);
    if (bn_ctx != nullptr) BN_CTX_free(bn_ctx);
    BN_free(private_key);
    napi_throw_error(env, "ERR_CRYPTO_OPERATION_FAILED", "Failed to derive ECDH public key");
    return nullptr;
  }
  EC_POINT_free(public_key);
  BN_CTX_free(bn_ctx);
  BN_free(private_key);
  return Undefined(env);
}

napi_value EcdhSetPublicKey(napi_env env, napi_callback_info info) {
  size_t argc = 1;
  napi_value argv[1] = {nullptr};
  napi_value this_arg = nullptr;
  napi_get_cb_info(env, info, &argc, argv, &this_arg, nullptr);
  EcdhWrap* wrap = UnwrapEcdh(env, this_arg);
  if (wrap == nullptr || wrap->ec == nullptr || argc < 1 || argv[0] == nullptr) return Undefined(env);
  const std::vector<uint8_t> key_bytes = ValueToBytes(env, argv[0]);
  const EC_GROUP* group = EC_KEY_get0_group(wrap->ec);
  EC_POINT* public_key = group != nullptr ? EC_POINT_new(group) : nullptr;
  if (group == nullptr || public_key == nullptr ||
      EC_POINT_oct2point(group, public_key, key_bytes.data(), key_bytes.size(), nullptr) != 1 ||
      EC_KEY_set_public_key(wrap->ec, public_key) != 1) {
    if (public_key != nullptr) EC_POINT_free(public_key);
    napi_throw_error(env, "ERR_CRYPTO_OPERATION_FAILED", "Failed to convert Buffer to EC_POINT");
    return nullptr;
  }
  EC_POINT_free(public_key);
  return Undefined(env);
}

napi_value EcdhComputeSecret(napi_env env, napi_callback_info info) {
  size_t argc = 1;
  napi_value argv[1] = {nullptr};
  napi_value this_arg = nullptr;
  napi_get_cb_info(env, info, &argc, argv, &this_arg, nullptr);
  EcdhWrap* wrap = UnwrapEcdh(env, this_arg);
  if (wrap == nullptr || wrap->ec == nullptr || argc < 1 || argv[0] == nullptr) return Undefined(env);
  if (EC_KEY_check_key(wrap->ec) != 1) {
    ERR_clear_error();
    napi_throw(env, CreateErrorWithCode(env, "ERR_CRYPTO_INVALID_KEYPAIR", "Invalid key pair"));
    return nullptr;
  }
  const std::vector<uint8_t> peer_bytes = ValueToBytes(env, argv[0]);
  const EC_GROUP* group = EC_KEY_get0_group(wrap->ec);
  EC_POINT* peer_point = group != nullptr ? EC_POINT_new(group) : nullptr;
  if (group == nullptr ||
      peer_point == nullptr ||
      EC_POINT_oct2point(group, peer_point, peer_bytes.data(), peer_bytes.size(), nullptr) != 1) {
    if (peer_point != nullptr) EC_POINT_free(peer_point);
    napi_value err_text = nullptr;
    napi_create_string_utf8(env, "invalid public key", NAPI_AUTO_LENGTH, &err_text);
    return err_text;
  }
  const size_t secret_len = static_cast<size_t>((EC_GROUP_get_degree(group) + 7) / 8);
  std::vector<uint8_t> secret(secret_len);
  const int written = ECDH_compute_key(secret.data(), secret.size(), peer_point, wrap->ec, nullptr);
  EC_POINT_free(peer_point);
  if (written <= 0) {
    napi_value err_text = nullptr;
    napi_create_string_utf8(env, "invalid public key", NAPI_AUTO_LENGTH, &err_text);
    return err_text;
  }
  secret.resize(static_cast<size_t>(written));
  return BytesToBuffer(env, secret);
}

struct CipherBaseWrap {
  napi_ref wrapper_ref = nullptr;
  EVP_CIPHER_CTX* aead_ctx = nullptr;
  bool encrypt = true;
  bool auto_padding = true;
  bool aead_mode = false;
  bool gcm_mode = false;
  bool ccm_mode = false;
  bool ocb_mode = false;
  bool chacha20_poly1305_mode = false;
  bool auth_tag_length_specified = false;
  bool auth_tag_set = false;
  bool aad_called = false;
  std::string algorithm;
  std::vector<uint8_t> key;
  bool iv_is_null = false;
  std::vector<uint8_t> iv;
  std::vector<uint8_t> input;
  std::vector<uint8_t> aad;
  std::vector<uint8_t> auth_tag;
  int32_t auth_tag_length = -1;
  int32_t plaintext_length = -1;
  int32_t max_message_size = std::numeric_limits<int32_t>::max();
  bool pending_auth_failed = false;
  bool finalized = false;
};

struct KeyEncodingSelection {
  bool has_public_encoding = false;
  int32_t public_format = kKeyFormatPEM;
  int32_t public_type = kKeyEncodingSPKI;
  bool has_private_encoding = false;
  int32_t private_format = kKeyFormatPEM;
  int32_t private_type = kKeyEncodingPKCS8;
  std::string private_cipher;
  std::vector<uint8_t> private_passphrase;
  bool private_passphrase_provided = false;
};

bool ExportPublicKeyValue(napi_env env,
                          EVP_PKEY* pkey,
                          const KeyEncodingSelection& encoding,
                          const std::string& curve_name_hint,
                          napi_value* out_value,
                          std::string* error_code,
                          std::string* error_message);
bool ExportPrivateKeyValue(napi_env env,
                           EVP_PKEY* pkey,
                           const KeyEncodingSelection& encoding,
                           const std::string& curve_name_hint,
                           napi_value* out_value,
                           std::string* error_code,
                           std::string* error_message);

void CipherBaseFinalize(napi_env env, void* data, void* /*hint*/) {
  auto* wrap = static_cast<CipherBaseWrap*>(data);
  if (wrap == nullptr) return;
  if (wrap->aead_ctx != nullptr) {
    EVP_CIPHER_CTX_free(wrap->aead_ctx);
    wrap->aead_ctx = nullptr;
  }
  ResetRef(env, &wrap->wrapper_ref);
  delete wrap;
}

CipherBaseWrap* UnwrapCipherBase(napi_env env, napi_value this_arg) {
  if (this_arg == nullptr) return nullptr;
  void* data = nullptr;
  if (napi_unwrap(env, this_arg, &data) != napi_ok || data == nullptr) return nullptr;
  return static_cast<CipherBaseWrap*>(data);
}

bool IsAeadCipherName(std::string name) {
  for (char& ch : name) ch = static_cast<char>(std::tolower(static_cast<unsigned char>(ch)));
  return name.find("gcm") != std::string::npos ||
         name.find("ccm") != std::string::npos ||
         name.find("ocb") != std::string::npos ||
         name.find("chacha20-poly1305") != std::string::npos;
}

bool IsGcmCipherName(std::string name) {
  for (char& ch : name) ch = static_cast<char>(std::tolower(static_cast<unsigned char>(ch)));
  return name.find("gcm") != std::string::npos;
}

bool IsCcmCipherName(std::string name) {
  for (char& ch : name) ch = static_cast<char>(std::tolower(static_cast<unsigned char>(ch)));
  return name.find("ccm") != std::string::npos;
}

bool IsOcbCipherName(std::string name) {
  for (char& ch : name) ch = static_cast<char>(std::tolower(static_cast<unsigned char>(ch)));
  return name.find("ocb") != std::string::npos;
}

bool IsChaCha20Poly1305CipherName(std::string name) {
  for (char& ch : name) ch = static_cast<char>(std::tolower(static_cast<unsigned char>(ch)));
  return name.find("chacha20-poly1305") != std::string::npos;
}

bool IsWrapCipherName(std::string name) {
  for (char& ch : name) ch = static_cast<char>(std::tolower(static_cast<unsigned char>(ch)));
  return name.find("wrap") != std::string::npos;
}

bool IsValidGcmAuthTagLength(size_t tag_len) {
  return tag_len == 4 || tag_len == 8 || (tag_len >= 12 && tag_len <= 16);
}

bool IsValidCcmAuthTagLength(int32_t tag_len) {
  return tag_len >= 4 && tag_len <= 16 && (tag_len % 2) == 0;
}

bool IsValidGenericAeadTagLength(int32_t tag_len) {
  return tag_len >= 1 && tag_len <= 16;
}

int32_t ComputeCcmMaxMessageSize(size_t iv_len) {
  if (iv_len == 12) return 16777215;
  if (iv_len == 13) return 65535;
  return std::numeric_limits<int32_t>::max();
}

bool CipherBaseCheckAeadTagLength(CipherBaseWrap* wrap, size_t provided_tag_len) {
  if (wrap == nullptr || !wrap->aead_mode) return false;
  if (wrap->gcm_mode) {
    return (!wrap->auth_tag_length_specified ||
            wrap->auth_tag_length == static_cast<int32_t>(provided_tag_len)) &&
           IsValidGcmAuthTagLength(provided_tag_len);
  }
  if (wrap->auth_tag_length < 0) return false;
  return wrap->auth_tag_length == static_cast<int32_t>(provided_tag_len) &&
         IsValidGenericAeadTagLength(static_cast<int32_t>(provided_tag_len));
}

void ThrowInvalidAuthTagLength(napi_env env, size_t provided_tag_len) {
  napi_throw(
      env,
      CreateErrorWithCode(env,
                          "ERR_CRYPTO_INVALID_AUTH_TAG",
                          "Invalid authentication tag length: " + std::to_string(provided_tag_len)));
}

bool InitCipherBaseAeadContext(napi_env env, CipherBaseWrap* wrap) {
  if (wrap == nullptr || !wrap->aead_mode) return true;
  const EVP_CIPHER* cipher = EVP_get_cipherbyname(wrap->algorithm.c_str());
  if (cipher == nullptr) {
    napi_throw(env, CreateErrorWithCode(env, "ERR_CRYPTO_UNKNOWN_CIPHER", "Unknown cipher"));
    return false;
  }

  wrap->aead_ctx = EVP_CIPHER_CTX_new();
  if (wrap->aead_ctx == nullptr) {
    ThrowLastOpenSslMessage(env, "cipher initialization failed");
    return false;
  }

  const int enc = wrap->encrypt ? 1 : 0;
  int ok = EVP_CipherInit_ex(wrap->aead_ctx, cipher, nullptr, nullptr, nullptr, enc);
  if (ok == 1) {
    ok = EVP_CIPHER_CTX_ctrl(wrap->aead_ctx,
                             EVP_CTRL_AEAD_SET_IVLEN,
                             static_cast<int>(wrap->iv.size()),
                             nullptr);
  }
  if (ok == 1 && (wrap->ccm_mode || wrap->ocb_mode) && wrap->auth_tag_length > 0) {
    ok = EVP_CIPHER_CTX_ctrl(wrap->aead_ctx, EVP_CTRL_AEAD_SET_TAG, wrap->auth_tag_length, nullptr);
  }
  if (ok == 1) {
    ok = EVP_CipherInit_ex(wrap->aead_ctx,
                           nullptr,
                           nullptr,
                           wrap->key.empty() ? nullptr : wrap->key.data(),
                           wrap->iv.empty() ? nullptr : wrap->iv.data(),
                           enc);
  }
  if (ok != 1) {
    EVP_CIPHER_CTX_free(wrap->aead_ctx);
    wrap->aead_ctx = nullptr;
    ThrowLastOpenSslMessage(env, "cipher initialization failed");
    return false;
  }
  return true;
}

napi_value CipherBaseCtor(napi_env env, napi_callback_info info) {
  size_t argc = 5;
  napi_value argv[5] = {nullptr, nullptr, nullptr, nullptr, nullptr};
  napi_value this_arg = nullptr;
  if (napi_get_cb_info(env, info, &argc, argv, &this_arg, nullptr) != napi_ok || this_arg == nullptr) {
    return nullptr;
  }
  auto* wrap = new CipherBaseWrap();
  if (argc >= 1 && argv[0] != nullptr) {
    bool encrypt = true;
    if (napi_get_value_bool(env, argv[0], &encrypt) == napi_ok) wrap->encrypt = encrypt;
  }
  if (argc >= 2 && argv[1] != nullptr) {
    size_t len = 0;
    if (napi_get_value_string_utf8(env, argv[1], nullptr, 0, &len) == napi_ok) {
      wrap->algorithm.resize(len + 1);
      size_t copied = 0;
      if (napi_get_value_string_utf8(env, argv[1], wrap->algorithm.data(), wrap->algorithm.size(), &copied) ==
          napi_ok) {
        wrap->algorithm.resize(copied);
      }
    }
  }
  if (argc >= 3 && argv[2] != nullptr) {
    wrap->key = ValueToBytes(env, argv[2]);
  }
  if (argc < 4 || argv[3] == nullptr || IsUndefined(env, argv[3])) {
    napi_throw(
        env,
        CreateErrorWithCode(env,
                            "ERR_INVALID_ARG_TYPE",
                            "The \"iv\" argument must be of type string, Buffer, ArrayBuffer, TypedArray, or DataView."));
    delete wrap;
    return nullptr;
  }
  if (argc >= 4 && argv[3] != nullptr) {
    napi_valuetype iv_type = napi_undefined;
    if (napi_typeof(env, argv[3], &iv_type) == napi_ok && iv_type == napi_null) {
      wrap->iv_is_null = true;
    } else {
      wrap->iv = ValueToBytes(env, argv[3]);
    }
  }
  if (argc >= 5 && argv[4] != nullptr && !IsUndefined(env, argv[4])) {
    napi_get_value_int32(env, argv[4], &wrap->auth_tag_length);
    wrap->auth_tag_length_specified = wrap->auth_tag_length >= 0;
    if (!wrap->auth_tag_length_specified) wrap->auth_tag_length = -1;
  }
  wrap->gcm_mode = IsGcmCipherName(wrap->algorithm);
  wrap->ccm_mode = IsCcmCipherName(wrap->algorithm);
  wrap->ocb_mode = IsOcbCipherName(wrap->algorithm);
  wrap->chacha20_poly1305_mode = IsChaCha20Poly1305CipherName(wrap->algorithm);
  wrap->aead_mode = wrap->gcm_mode || wrap->ccm_mode || wrap->ocb_mode || wrap->chacha20_poly1305_mode;

  // Validate cipher/key/iv shape at construction time without performing
  // a data transform, so valid ciphers do not fail early.
  napi_value binding = GetBinding(env);
  if (binding != nullptr) {
    napi_value opts = nullptr;
    napi_value algo = nullptr;
    if (napi_create_object(env, &opts) != napi_ok ||
        opts == nullptr ||
        napi_create_string_utf8(env, wrap->algorithm.c_str(), NAPI_AUTO_LENGTH, &algo) != napi_ok ||
        algo == nullptr) {
      delete wrap;
      return nullptr;
    }
    napi_value argv_info[2] = {opts, algo};
    napi_value info = nullptr;
    if (!CallBindingMethod(env, binding, "getCipherInfo", 2, argv_info, &info) ||
        info == nullptr ||
        IsUndefined(env, info)) {
      napi_throw(env, CreateErrorWithCode(env, "ERR_CRYPTO_UNKNOWN_CIPHER", "Unknown cipher"));
      delete wrap;
      return nullptr;
    }

    int32_t expected_key_len = -1;
    int32_t expected_iv_len = -1;
    GetNamedInt32Value(env, info, "keyLength", &expected_key_len);
    GetNamedInt32Value(env, info, "ivLength", &expected_iv_len);
    if (expected_iv_len < 0) {
      std::string mode;
      if (GetNamedStringValue(env, info, "mode", &mode) && mode == "ecb") {
        expected_iv_len = 0;
      }
    }

    if (expected_key_len > 0 && static_cast<int32_t>(wrap->key.size()) != expected_key_len) {
      napi_throw(env, CreateErrorWithCode(env, "ERR_CRYPTO_INVALID_KEYLEN", "Invalid key length"));
      delete wrap;
      return nullptr;
    }

    const int32_t actual_iv_len = wrap->iv_is_null ? 0 : static_cast<int32_t>(wrap->iv.size());
    if (wrap->aead_mode) {
      if (actual_iv_len <= 0) {
        napi_throw(env, CreateErrorWithCode(env, "ERR_CRYPTO_INVALID_IV", "Invalid initialization vector"));
        delete wrap;
        return nullptr;
      }
      if (wrap->ccm_mode && (actual_iv_len < 7 || actual_iv_len > 13)) {
        napi_throw(env, CreateErrorWithCode(env, "ERR_CRYPTO_INVALID_IV", "Invalid initialization vector"));
        delete wrap;
        return nullptr;
      }
      if (wrap->chacha20_poly1305_mode && actual_iv_len > 12) {
        napi_throw(env, CreateErrorWithCode(env, "ERR_CRYPTO_INVALID_IV", "Invalid initialization vector"));
        delete wrap;
        return nullptr;
      }
    } else if (expected_iv_len >= 0 && actual_iv_len != expected_iv_len) {
      napi_throw(env, CreateErrorWithCode(env, "ERR_CRYPTO_INVALID_IV", "Invalid initialization vector"));
      delete wrap;
      return nullptr;
    }
  }

  if (wrap->aead_mode) {
    if (wrap->gcm_mode) {
      if (wrap->auth_tag_length_specified &&
          !IsValidGcmAuthTagLength(static_cast<size_t>(wrap->auth_tag_length))) {
        ThrowInvalidAuthTagLength(env, static_cast<size_t>(wrap->auth_tag_length));
        delete wrap;
        return nullptr;
      }
    } else if (wrap->chacha20_poly1305_mode) {
      if (!wrap->auth_tag_length_specified) {
        wrap->auth_tag_length = 16;
      } else if (!IsValidGenericAeadTagLength(wrap->auth_tag_length)) {
        ThrowInvalidAuthTagLength(env, static_cast<size_t>(wrap->auth_tag_length));
        delete wrap;
        return nullptr;
      }
    } else {
      if (!wrap->auth_tag_length_specified) {
        napi_throw(
            env,
            CreateErrorWithCode(env,
                                "ERR_CRYPTO_INVALID_AUTH_TAG",
                                "authTagLength required for " + wrap->algorithm));
        delete wrap;
        return nullptr;
      }
      const bool valid =
          wrap->ccm_mode ? IsValidCcmAuthTagLength(wrap->auth_tag_length)
                         : IsValidGenericAeadTagLength(wrap->auth_tag_length);
      if (!valid) {
        ThrowInvalidAuthTagLength(env, static_cast<size_t>(wrap->auth_tag_length));
        delete wrap;
        return nullptr;
      }
    }

    if (wrap->ccm_mode) {
      wrap->max_message_size = ComputeCcmMaxMessageSize(wrap->iv.size());
    }
  }
  if (wrap->aead_mode && !InitCipherBaseAeadContext(env, wrap)) {
    delete wrap;
    return nullptr;
  }
  napi_wrap(env, this_arg, wrap, CipherBaseFinalize, nullptr, &wrap->wrapper_ref);
  return this_arg;
}

napi_value CipherBaseUpdate(napi_env env, napi_callback_info info) {
  size_t argc = 2;
  napi_value argv[2] = {nullptr, nullptr};
  napi_value this_arg = nullptr;
  napi_get_cb_info(env, info, &argc, argv, &this_arg, nullptr);
  CipherBaseWrap* wrap = UnwrapCipherBase(env, this_arg);
  if (wrap == nullptr) return Undefined(env);
  if (wrap->finalized) return BytesToBuffer(env, {});
  if (wrap->aead_mode && wrap->aead_ctx != nullptr) {
    if (argc < 1 || argv[0] == nullptr) return BytesToBuffer(env, {});
    std::vector<uint8_t> bytes = ValueToBytesWithEncoding(env, argv[0], argc >= 2 ? argv[1] : nullptr);
    if (wrap->ccm_mode) {
      const size_t next_size = wrap->input.size() + bytes.size();
      if (next_size > static_cast<size_t>(wrap->max_message_size) ||
          (wrap->plaintext_length >= 0 && next_size > static_cast<size_t>(wrap->plaintext_length))) {
        napi_throw(env, CreateErrorWithCode(env, "ERR_CRYPTO_INVALID_MESSAGELEN", "Invalid message length"));
        return nullptr;
      }
      wrap->input.insert(wrap->input.end(), bytes.begin(), bytes.end());
    }
    std::vector<uint8_t> out(bytes.size() + 32);
    int out_len = 0;
    static const uint8_t kEmptyAeadInput = 0;
    const uint8_t* input_ptr = bytes.empty() ? &kEmptyAeadInput : bytes.data();
    const int ok =
        EVP_CipherUpdate(wrap->aead_ctx, out.data(), &out_len, input_ptr, static_cast<int>(bytes.size()));
    if (ok != 1) {
      if (wrap->ccm_mode && !wrap->encrypt) {
        wrap->pending_auth_failed = true;
        return BytesToBuffer(env, {});
      }
      ERR_clear_error();
      napi_throw(env,
                 CreateErrorWithCode(env,
                                     "ERR_CRYPTO_INVALID_STATE",
                                     "Unsupported state or unable to authenticate data"));
      return nullptr;
    }
    out.resize(static_cast<size_t>(out_len));
    return BytesToBuffer(env, out);
  }
  if (argc >= 1 && argv[0] != nullptr) {
    std::vector<uint8_t> bytes = ValueToBytesWithEncoding(env, argv[0], argc >= 2 ? argv[1] : nullptr);
    if (wrap->ccm_mode) {
      const size_t next_size = wrap->input.size() + bytes.size();
      if (next_size > static_cast<size_t>(wrap->max_message_size) ||
          (wrap->plaintext_length >= 0 && next_size > static_cast<size_t>(wrap->plaintext_length))) {
        napi_throw(env, CreateErrorWithCode(env, "ERR_CRYPTO_INVALID_MESSAGELEN", "Invalid message length"));
        return nullptr;
      }
    }
    wrap->input.insert(wrap->input.end(), bytes.begin(), bytes.end());
  }
  // RFC3394/5649 wrap algorithms emit their full output on update() in Node.
  if (IsWrapCipherName(wrap->algorithm)) {
    napi_value binding = GetBinding(env);
    if (binding == nullptr) return Undefined(env);
    napi_value algo = nullptr;
    napi_value key = BytesToBuffer(env, wrap->key);
    napi_value iv = wrap->iv_is_null ? nullptr : BytesToBuffer(env, wrap->iv);
    napi_value input = BytesToBuffer(env, wrap->input);
    napi_value decrypt = nullptr;
    napi_get_boolean(env, !wrap->encrypt, &decrypt);
    napi_value iv_arg = nullptr;
    if (wrap->iv_is_null) {
      napi_get_null(env, &iv_arg);
    } else {
      iv_arg = (iv != nullptr ? iv : Undefined(env));
    }
    napi_create_string_utf8(env, wrap->algorithm.c_str(), NAPI_AUTO_LENGTH, &algo);
    napi_value auto_padding = nullptr;
    napi_get_boolean(env, wrap->auto_padding, &auto_padding);
    napi_value argv_transform[7] = {algo != nullptr ? algo : Undefined(env),
                                    key != nullptr ? key : Undefined(env),
                                    iv_arg,
                                    input != nullptr ? input : Undefined(env),
                                    decrypt != nullptr ? decrypt : Undefined(env),
                                    Undefined(env),
                                    auto_padding != nullptr ? auto_padding : Undefined(env)};
    napi_value out = nullptr;
    if (!CallBindingMethod(env, binding, "cipherTransform", 7, argv_transform, &out)) return nullptr;
    wrap->input.clear();
    wrap->finalized = true;
    return EnsureBufferValue(env, out);
  }
  return BytesToBuffer(env, {});
}

napi_value CipherBaseFinal(napi_env env, napi_callback_info info) {
  napi_value this_arg = nullptr;
  size_t argc = 0;
  napi_get_cb_info(env, info, &argc, nullptr, &this_arg, nullptr);
  CipherBaseWrap* wrap = UnwrapCipherBase(env, this_arg);
  if (wrap == nullptr) return Undefined(env);
  if (wrap->finalized) return BytesToBuffer(env, {});
  if (wrap->aead_mode && wrap->aead_ctx != nullptr) {
    wrap->finalized = true;
    auto close_ctx = [&]() {
      if (wrap->aead_ctx != nullptr) {
        EVP_CIPHER_CTX_free(wrap->aead_ctx);
        wrap->aead_ctx = nullptr;
      }
    };

    if (!wrap->encrypt && !wrap->auth_tag_set) {
      close_ctx();
      napi_throw(
          env,
          CreateErrorWithCode(env,
                              "ERR_CRYPTO_INVALID_STATE",
                              "Unsupported state or unable to authenticate data"));
      return nullptr;
    }

    if (!wrap->encrypt && !wrap->ccm_mode) {
      if (EVP_CIPHER_CTX_ctrl(wrap->aead_ctx,
                              EVP_CTRL_AEAD_SET_TAG,
                              wrap->auth_tag_length,
                              wrap->auth_tag.empty() ? nullptr : wrap->auth_tag.data()) != 1) {
        close_ctx();
        ERR_clear_error();
        napi_throw(
            env,
            CreateErrorWithCode(env,
                                "ERR_CRYPTO_INVALID_STATE",
                                "Unsupported state or unable to authenticate data"));
        return nullptr;
      }
    }

    if (!wrap->encrypt && wrap->ccm_mode) {
      close_ctx();
      if (wrap->pending_auth_failed) {
        napi_throw(
            env,
            CreateErrorWithCode(env,
                                "ERR_CRYPTO_INVALID_STATE",
                                "Unsupported state or unable to authenticate data"));
        return nullptr;
      }
      return BytesToBuffer(env, {});
    }

    if (wrap->encrypt && wrap->ccm_mode && wrap->plaintext_length < 0 && wrap->input.empty()) {
      close_ctx();
#if OPENSSL_VERSION_NUMBER >= 0x30000000L
      napi_throw(env, CreateErrorWithCode(env, "ERR_OSSL_TAG_NOT_SET", "tag not set"));
#else
      napi_throw(env, CreateErrorWithCode(env, "ERR_CRYPTO_INVALID_STATE", "Unsupported state"));
#endif
      return nullptr;
    }

    std::vector<uint8_t> out(32);
    int out_len = 0;
    const int ok = EVP_CipherFinal_ex(wrap->aead_ctx, out.data(), &out_len);
    if (ok != 1) {
      close_ctx();
      if (!wrap->encrypt) {
        ERR_clear_error();
        napi_throw(
            env,
            CreateErrorWithCode(env,
                                "ERR_CRYPTO_INVALID_STATE",
                                "Unsupported state or unable to authenticate data"));
      } else {
        ThrowLastOpenSslMessage(env, "AEAD cipher operation failed");
      }
      return nullptr;
    }

    if (wrap->encrypt) {
      const int tag_len = wrap->auth_tag_length >= 0 ? wrap->auth_tag_length : 16;
      wrap->auth_tag.assign(static_cast<size_t>(tag_len), 0);
      if (EVP_CIPHER_CTX_ctrl(wrap->aead_ctx,
                              EVP_CTRL_AEAD_GET_TAG,
                              tag_len,
                              wrap->auth_tag.data()) != 1) {
        wrap->auth_tag.clear();
      }
    }

    close_ctx();
    out.resize(static_cast<size_t>(out_len));
    return BytesToBuffer(env, out);
  }

  napi_value binding = GetBinding(env);
  if (binding == nullptr) return Undefined(env);

  if (!wrap->encrypt && wrap->aead_mode && !wrap->auth_tag_set) {
    wrap->finalized = true;
    napi_throw(
        env,
        CreateErrorWithCode(env,
                            "ERR_CRYPTO_INVALID_STATE",
                            "Unsupported state or unable to authenticate data"));
    return nullptr;
  }
  if (wrap->ccm_mode &&
      (wrap->input.size() > static_cast<size_t>(wrap->max_message_size) ||
       (wrap->plaintext_length >= 0 && wrap->input.size() > static_cast<size_t>(wrap->plaintext_length)))) {
    wrap->finalized = true;
    napi_throw(env, CreateErrorWithCode(env, "ERR_CRYPTO_INVALID_MESSAGELEN", "Invalid message length"));
    return nullptr;
  }

  wrap->finalized = true;

  napi_value algo = nullptr;
  napi_create_string_utf8(env, wrap->algorithm.c_str(), NAPI_AUTO_LENGTH, &algo);
  napi_value key = BytesToBuffer(env, wrap->key);
  napi_value iv = wrap->iv_is_null ? nullptr : BytesToBuffer(env, wrap->iv);
  napi_value input = BytesToBuffer(env, wrap->input);
  napi_value decrypt = nullptr;
  napi_get_boolean(env, !wrap->encrypt, &decrypt);

  napi_value output = nullptr;
  if (wrap->aead_mode) {
    napi_value aad = BytesToBuffer(env, wrap->aad);
    napi_value auth_tag = wrap->auth_tag.empty() ? Undefined(env) : BytesToBuffer(env, wrap->auth_tag);
    napi_value auth_tag_len = nullptr;
    napi_create_int32(env, wrap->auth_tag_length, &auth_tag_len);
    napi_value argv_aead[8] = {
        algo != nullptr ? algo : Undefined(env),  key != nullptr ? key : Undefined(env),
        iv != nullptr ? iv : Undefined(env),      input != nullptr ? input : Undefined(env),
        decrypt != nullptr ? decrypt : Undefined(env), aad != nullptr ? aad : Undefined(env),
        auth_tag != nullptr ? auth_tag : Undefined(env), auth_tag_len != nullptr ? auth_tag_len : Undefined(env)};
    napi_value out = nullptr;
    if (!CallBindingMethod(env, binding, "cipherTransformAead", 8, argv_aead, &out) || out == nullptr) {
      return nullptr;
    }
    napi_value out_buf = nullptr;
    if (napi_get_named_property(env, out, "output", &out_buf) != napi_ok || out_buf == nullptr) {
      return nullptr;
    }
    if (wrap->encrypt) {
      napi_value out_tag = nullptr;
      if (napi_get_named_property(env, out, "authTag", &out_tag) == napi_ok && out_tag != nullptr &&
          !IsUndefined(env, out_tag)) {
        wrap->auth_tag = ValueToBytes(env, out_tag);
      }
    }
    output = EnsureBufferValue(env, out_buf);
  } else {
    napi_value iv_arg = nullptr;
    if (wrap->iv_is_null) {
      napi_get_null(env, &iv_arg);
    } else {
      iv_arg = (iv != nullptr ? iv : Undefined(env));
    }
    napi_value auto_padding = nullptr;
    napi_get_boolean(env, wrap->auto_padding, &auto_padding);
    napi_value argv_transform[7] = {algo != nullptr ? algo : Undefined(env),
                                    key != nullptr ? key : Undefined(env),
                                    iv_arg,
                                    input != nullptr ? input : Undefined(env),
                                    decrypt != nullptr ? decrypt : Undefined(env),
                                    Undefined(env),
                                    auto_padding != nullptr ? auto_padding : Undefined(env)};
    napi_value out = nullptr;
    if (!CallBindingMethod(env, binding, "cipherTransform", 7, argv_transform, &out)) return nullptr;
    output = EnsureBufferValue(env, out);
  }

  return output != nullptr ? output : Undefined(env);
}

napi_value CipherBaseSetAutoPadding(napi_env env, napi_callback_info info) {
  napi_value this_arg = nullptr;
  size_t argc = 1;
  napi_value argv[1] = {nullptr};
  napi_get_cb_info(env, info, &argc, argv, &this_arg, nullptr);
  CipherBaseWrap* wrap = UnwrapCipherBase(env, this_arg);
  if (wrap == nullptr) return Undefined(env);
  bool enabled = true;
  if (argc >= 1 && argv[0] != nullptr) napi_get_value_bool(env, argv[0], &enabled);
  if (wrap->finalized) {
    napi_value out = nullptr;
    napi_get_boolean(env, false, &out);
    return out != nullptr ? out : Undefined(env);
  }
  wrap->auto_padding = enabled;
  napi_value out = nullptr;
  napi_get_boolean(env, true, &out);
  return out != nullptr ? out : Undefined(env);
}

napi_value CipherBaseSetAAD(napi_env env, napi_callback_info info) {
  size_t argc = 2;
  napi_value argv[2] = {nullptr, nullptr};
  napi_value this_arg = nullptr;
  napi_get_cb_info(env, info, &argc, argv, &this_arg, nullptr);
  CipherBaseWrap* wrap = UnwrapCipherBase(env, this_arg);
  if (wrap == nullptr) return Undefined(env);
  if (wrap->finalized || !wrap->aead_mode) {
    napi_value out = nullptr;
    napi_get_boolean(env, false, &out);
    return out != nullptr ? out : Undefined(env);
  }
  const int32_t plaintext_length = argc >= 2 && argv[1] != nullptr ? GetInt32Value(env, argv[1], -1) : -1;
  if (wrap->ccm_mode) {
    if (plaintext_length < 0) {
      napi_throw(env,
                 CreateErrorWithCode(env, nullptr, "options.plaintextLength required for CCM mode with AAD"));
      return nullptr;
    }
    if (plaintext_length > wrap->max_message_size) {
      napi_throw(env, CreateErrorWithCode(env, "ERR_CRYPTO_INVALID_MESSAGELEN", "Invalid message length"));
      return nullptr;
    }
    wrap->plaintext_length = plaintext_length;
  }
  wrap->aad.clear();
  if (argc >= 1 && argv[0] != nullptr) {
    wrap->aad = ValueToBytes(env, argv[0]);
  }
  if (wrap->aead_ctx != nullptr) {
    int tmp_len = 0;
    if (wrap->ccm_mode) {
      if (EVP_CipherUpdate(wrap->aead_ctx, nullptr, &tmp_len, nullptr, plaintext_length) != 1) {
        ThrowLastOpenSslMessage(env, "Invalid message length");
        return nullptr;
      }
    }
    if (!wrap->aad.empty() &&
        EVP_CipherUpdate(
            wrap->aead_ctx, nullptr, &tmp_len, wrap->aad.data(), static_cast<int>(wrap->aad.size())) != 1) {
      ThrowLastOpenSslMessage(env, "Unsupported state");
      return nullptr;
    }
  }
  wrap->aad_called = true;
  napi_value out = nullptr;
  napi_get_boolean(env, true, &out);
  return out != nullptr ? out : Undefined(env);
}

napi_value CipherBaseGetAuthTag(napi_env env, napi_callback_info info) {
  napi_value this_arg = nullptr;
  size_t argc = 0;
  napi_get_cb_info(env, info, &argc, nullptr, &this_arg, nullptr);
  CipherBaseWrap* wrap = UnwrapCipherBase(env, this_arg);
  if (wrap == nullptr || !wrap->encrypt || !wrap->finalized || wrap->auth_tag.empty()) return Undefined(env);
  return BytesToBuffer(env, wrap->auth_tag);
}

napi_value CipherBaseSetAuthTag(napi_env env, napi_callback_info info) {
  size_t argc = 1;
  napi_value argv[1] = {nullptr};
  napi_value this_arg = nullptr;
  napi_get_cb_info(env, info, &argc, argv, &this_arg, nullptr);
  CipherBaseWrap* wrap = UnwrapCipherBase(env, this_arg);
  if (wrap == nullptr) return Undefined(env);
  if (wrap->finalized || !wrap->aead_mode || wrap->encrypt || wrap->auth_tag_set) {
    napi_value out = nullptr;
    napi_get_boolean(env, false, &out);
    return out != nullptr ? out : Undefined(env);
  }
  wrap->auth_tag.clear();
  if (argc >= 1 && argv[0] != nullptr) {
    wrap->auth_tag = ValueToBytes(env, argv[0]);
  }
  if (!CipherBaseCheckAeadTagLength(wrap, wrap->auth_tag.size())) {
    ThrowInvalidAuthTagLength(env, wrap->auth_tag.size());
    return nullptr;
  }
  if (wrap->gcm_mode && !wrap->auth_tag_length_specified) {
    wrap->auth_tag_length = static_cast<int32_t>(wrap->auth_tag.size());
    if (wrap->auth_tag.size() != static_cast<size_t>(EVP_GCM_TLS_TAG_LEN)) {
      EmitProcessDeprecationWarning(
          env,
          "Using AES-GCM authentication tags of less than 128 bits without "
          "specifying the authTagLength option when initializing decryption is deprecated.",
          "DEP0182");
    }
  }
  if (wrap->ccm_mode && wrap->aead_ctx != nullptr) {
    if (EVP_CIPHER_CTX_ctrl(wrap->aead_ctx,
                            EVP_CTRL_AEAD_SET_TAG,
                            wrap->auth_tag_length,
                            wrap->auth_tag.empty() ? nullptr : wrap->auth_tag.data()) != 1) {
      ThrowLastOpenSslMessage(env, "Unsupported state or unable to authenticate data");
      return nullptr;
    }
  }
  wrap->auth_tag_set = true;
  napi_value out = nullptr;
  napi_get_boolean(env, true, &out);
  return out != nullptr ? out : Undefined(env);
}

napi_value EmptyString(napi_env env) {
  napi_value out = nullptr;
  napi_create_string_utf8(env, "", 0, &out);
  return out != nullptr ? out : Undefined(env);
}

NETSCAPE_SPKI* DecodeSpkac(const uint8_t* data, size_t len) {
  if (data == nullptr || len == 0 || len > static_cast<size_t>(std::numeric_limits<int>::max())) return nullptr;
  return NETSCAPE_SPKI_b64_decode(reinterpret_cast<const char*>(data), static_cast<int>(len));
}

napi_value CertVerifySpkac(napi_env env, napi_callback_info info) {
  size_t argc = 1;
  napi_value argv[1] = {nullptr};
  if (napi_get_cb_info(env, info, &argc, argv, nullptr, nullptr) != napi_ok || argc < 1 || argv[0] == nullptr) {
    return nullptr;
  }
  const uint8_t* data = nullptr;
  size_t len = 0;
  if (!GetByteSpan(env, argv[0], &data, &len)) return nullptr;
  if (len == 0) return EmptyString(env);
  if (len > static_cast<size_t>(std::numeric_limits<int>::max())) {
    napi_throw(env, CreateErrorWithCode(env, "ERR_OUT_OF_RANGE", "spkac is too large"));
    return nullptr;
  }

  NETSCAPE_SPKI* spki = DecodeSpkac(data, len);
  bool verified = false;
  if (spki != nullptr) {
    EVP_PKEY* pkey = NETSCAPE_SPKI_get_pubkey(spki);
    if (pkey != nullptr) {
      verified = NETSCAPE_SPKI_verify(spki, pkey) == 1;
      EVP_PKEY_free(pkey);
    }
    NETSCAPE_SPKI_free(spki);
  }

  napi_value out = nullptr;
  napi_get_boolean(env, verified, &out);
  return out != nullptr ? out : Undefined(env);
}

napi_value CertExportPublicKey(napi_env env, napi_callback_info info) {
  size_t argc = 1;
  napi_value argv[1] = {nullptr};
  if (napi_get_cb_info(env, info, &argc, argv, nullptr, nullptr) != napi_ok || argc < 1 || argv[0] == nullptr) {
    return nullptr;
  }
  const uint8_t* data = nullptr;
  size_t len = 0;
  if (!GetByteSpan(env, argv[0], &data, &len)) return nullptr;
  if (len == 0) return EmptyString(env);
  if (len > static_cast<size_t>(std::numeric_limits<int>::max())) {
    napi_throw(env, CreateErrorWithCode(env, "ERR_OUT_OF_RANGE", "spkac is too large"));
    return nullptr;
  }

  NETSCAPE_SPKI* spki = DecodeSpkac(data, len);
  if (spki == nullptr) return EmptyString(env);

  napi_value out = EmptyString(env);
  EVP_PKEY* pkey = NETSCAPE_SPKI_get_pubkey(spki);
  BIO* bio = pkey != nullptr ? BIO_new(BIO_s_mem()) : nullptr;
  if (pkey != nullptr && bio != nullptr && PEM_write_bio_PUBKEY(bio, pkey) == 1) {
    BUF_MEM* bio_mem = nullptr;
    BIO_get_mem_ptr(bio, &bio_mem);
    if (bio_mem != nullptr && bio_mem->data != nullptr && bio_mem->length > 0) {
      out = BytesToBuffer(env,
                          std::vector<uint8_t>(reinterpret_cast<const uint8_t*>(bio_mem->data),
                                               reinterpret_cast<const uint8_t*>(bio_mem->data) +
                                                   static_cast<size_t>(bio_mem->length)));
    }
  }
  if (bio != nullptr) BIO_free(bio);
  if (pkey != nullptr) EVP_PKEY_free(pkey);
  NETSCAPE_SPKI_free(spki);
  return out != nullptr ? out : EmptyString(env);
}

napi_value CertExportChallenge(napi_env env, napi_callback_info info) {
  size_t argc = 1;
  napi_value argv[1] = {nullptr};
  if (napi_get_cb_info(env, info, &argc, argv, nullptr, nullptr) != napi_ok || argc < 1 || argv[0] == nullptr) {
    return nullptr;
  }
  const uint8_t* data = nullptr;
  size_t len = 0;
  if (!GetByteSpan(env, argv[0], &data, &len)) return nullptr;
  if (len == 0) return EmptyString(env);
  if (len > static_cast<size_t>(std::numeric_limits<int>::max())) {
    napi_throw(env, CreateErrorWithCode(env, "ERR_OUT_OF_RANGE", "spkac is too large"));
    return nullptr;
  }

  NETSCAPE_SPKI* spki = DecodeSpkac(data, len);
  if (spki == nullptr || spki->spkac == nullptr || spki->spkac->challenge == nullptr) {
    if (spki != nullptr) NETSCAPE_SPKI_free(spki);
    return EmptyString(env);
  }

  ASN1_IA5STRING* challenge = spki->spkac->challenge;
  napi_value out = BytesToBuffer(
      env,
      std::vector<uint8_t>(challenge->data, challenge->data + challenge->length));
  NETSCAPE_SPKI_free(spki);
  return out != nullptr ? out : EmptyString(env);
}

struct KeyObjectWrap {
  napi_ref wrapper_ref = nullptr;
  int32_t key_type = kKeyTypeSecret;
  EVP_PKEY* native_pkey = nullptr;
  std::vector<uint8_t> key_data;
  std::vector<uint8_t> key_passphrase;
  bool has_key_passphrase = false;
};

struct NativeKeyObjectWrap {
  napi_ref wrapper_ref = nullptr;
  int32_t key_type = kKeyTypeSecret;
  EVP_PKEY* native_pkey = nullptr;
  std::vector<uint8_t> key_data;
  std::vector<uint8_t> key_passphrase;
  bool has_key_passphrase = false;
};

std::vector<uint8_t> ExportPublicDerSpki(EVP_PKEY* pkey);
std::vector<uint8_t> ExportPrivateDerPkcs8(EVP_PKEY* pkey);
napi_value CreateKeyObjectHandleValue(napi_env env, int32_t key_type, const std::vector<uint8_t>& key_data);

std::vector<uint8_t> ExportPublicDerSpki(EVP_PKEY* pkey);
std::vector<uint8_t> ExportPrivateDerPkcs8(EVP_PKEY* pkey);
napi_value ExportJwkPublic(napi_env env,
                           EVP_PKEY* pkey,
                           std::string* error_code,
                           std::string* error_message,
                           const std::string& curve_name_hint);
napi_value ExportJwkPrivate(napi_env env,
                            EVP_PKEY* pkey,
                            std::string* error_code,
                            std::string* error_message,
                            const std::string& curve_name_hint);

void KeyObjectFinalize(napi_env env, void* data, void* /*hint*/) {
  auto* wrap = static_cast<KeyObjectWrap*>(data);
  if (wrap == nullptr) return;
  if (wrap->native_pkey != nullptr) {
    EVP_PKEY_free(wrap->native_pkey);
    wrap->native_pkey = nullptr;
  }
  ResetRef(env, &wrap->wrapper_ref);
  delete wrap;
}

void NativeKeyObjectFinalize(napi_env env, void* data, void* /*hint*/) {
  auto* wrap = static_cast<NativeKeyObjectWrap*>(data);
  if (wrap == nullptr) return;
  if (wrap->native_pkey != nullptr) {
    EVP_PKEY_free(wrap->native_pkey);
    wrap->native_pkey = nullptr;
  }
  ResetRef(env, &wrap->wrapper_ref);
  delete wrap;
}

KeyObjectWrap* UnwrapKeyObject(napi_env env, napi_value this_arg) {
  if (this_arg == nullptr) return nullptr;
  void* data = nullptr;
  if (napi_unwrap(env, this_arg, &data) != napi_ok || data == nullptr) return nullptr;
  return static_cast<KeyObjectWrap*>(data);
}

NativeKeyObjectWrap* UnwrapNativeKeyObject(napi_env env, napi_value this_arg) {
  if (this_arg == nullptr) return nullptr;
  void* data = nullptr;
  if (napi_unwrap(env, this_arg, &data) != napi_ok || data == nullptr) return nullptr;
  return static_cast<NativeKeyObjectWrap*>(data);
}

bool GetNamedProperty(napi_env env, napi_value obj, const char* name, napi_value* out) {
  if (obj == nullptr || name == nullptr || out == nullptr) return false;
  *out = nullptr;
  return napi_get_named_property(env, obj, name, out) == napi_ok && *out != nullptr;
}

BIGNUM* GetJwkBigNum(napi_env env, napi_value jwk, const char* name) {
  std::string encoded;
  if (!GetNamedStringValue(env, jwk, name, &encoded)) return nullptr;
  const std::vector<uint8_t> bytes = Base64UrlDecode(encoded);
  if (bytes.empty()) return nullptr;
  return BN_bin2bn(bytes.data(), static_cast<int>(bytes.size()), nullptr);
}

EVP_PKEY* ParseKeyObjectAsymmetricKey(KeyObjectWrap* wrap) {
  if (wrap == nullptr || wrap->key_type == kKeyTypeSecret) return nullptr;
  if (wrap->native_pkey != nullptr) {
    if (EVP_PKEY_up_ref(wrap->native_pkey) != 1) return nullptr;
    return wrap->native_pkey;
  }
  if (wrap->key_type == kKeyTypePrivate) {
    return ParsePrivateKeyBytesWithPassphrase(
        wrap->key_data.data(),
        wrap->key_data.size(),
        wrap->key_passphrase.data(),
        wrap->key_passphrase.size(),
        wrap->has_key_passphrase);
  }
  return ParsePublicKeyBytes(wrap->key_data.data(), wrap->key_data.size());
}

void ResetKeyObjectNativeKey(KeyObjectWrap* wrap) {
  if (wrap == nullptr || wrap->native_pkey == nullptr) return;
  EVP_PKEY_free(wrap->native_pkey);
  wrap->native_pkey = nullptr;
}

void SetObjectInt32(napi_env env, napi_value obj, const char* name, int32_t value) {
  napi_value out = nullptr;
  if (napi_create_int32(env, value, &out) == napi_ok && out != nullptr) {
    napi_set_named_property(env, obj, name, out);
  }
}

bool ExtractNormalizedKeyCloneData(int32_t source_key_type,
                                   EVP_PKEY* source_native_pkey,
                                   const std::vector<uint8_t>& source_key_data,
                                   const std::vector<uint8_t>& source_key_passphrase,
                                   bool source_has_key_passphrase,
                                   int32_t* key_type_out,
                                   std::vector<uint8_t>* key_data_out) {
  if (key_type_out != nullptr) *key_type_out = kKeyTypeSecret;
  if (key_data_out != nullptr) key_data_out->clear();
  if (key_type_out == nullptr || key_data_out == nullptr) return false;

  *key_type_out = source_key_type;
  if (source_key_type == kKeyTypeSecret) {
    *key_data_out = source_key_data;
    return true;
  }

  EVP_PKEY* pkey = nullptr;
  if (source_native_pkey != nullptr) {
    if (EVP_PKEY_up_ref(source_native_pkey) != 1) return false;
    pkey = source_native_pkey;
  } else if (source_key_type == kKeyTypePrivate) {
    pkey = ParsePrivateKeyBytesWithPassphrase(
        source_key_data.data(),
        source_key_data.size(),
        source_key_passphrase.data(),
        source_key_passphrase.size(),
        source_has_key_passphrase);
  } else {
    pkey = ParsePublicKeyBytes(source_key_data.data(), source_key_data.size());
  }
  if (pkey == nullptr) return false;

  std::vector<uint8_t> normalized =
      source_key_type == kKeyTypePublic ? ExportPublicDerSpki(pkey) : ExportPrivateDerPkcs8(pkey);
  EVP_PKEY_free(pkey);
  if (normalized.empty()) return false;
  *key_data_out = std::move(normalized);
  return true;
}

bool ExtractNormalizedKeyCloneData(KeyObjectWrap* source,
                                   int32_t* key_type_out,
                                   std::vector<uint8_t>* key_data_out) {
  if (source == nullptr) return false;
  return ExtractNormalizedKeyCloneData(source->key_type,
                                       source->native_pkey,
                                       source->key_data,
                                       source->key_passphrase,
                                       source->has_key_passphrase,
                                       key_type_out,
                                       key_data_out);
}

bool StoreNativeKeyObjectConstructorsFromExports(napi_env env, napi_value exports) {
  if (env == nullptr || exports == nullptr) return false;
  auto& state = EnsureState(env);
  bool is_array = false;
  if (napi_is_array(env, exports, &is_array) != napi_ok || !is_array) return false;

  auto store_ctor = [&](uint32_t index, napi_ref* slot) -> bool {
    napi_value ctor = nullptr;
    napi_valuetype type = napi_undefined;
    if (napi_get_element(env, exports, index, &ctor) != napi_ok ||
        ctor == nullptr ||
        napi_typeof(env, ctor, &type) != napi_ok ||
        type != napi_function) {
      return false;
    }
    ResetRef(env, slot);
    return napi_create_reference(env, ctor, 1, slot) == napi_ok && *slot != nullptr;
  };

  return store_ctor(0, &state.native_key_object_ctor_ref) &&
         store_ctor(1, &state.secret_key_object_ctor_ref) &&
         store_ctor(2, &state.public_key_object_ctor_ref) &&
         store_ctor(3, &state.private_key_object_ctor_ref);
}

bool EnsureNativeKeyObjectConstructorsLoaded(napi_env env) {
  CryptoBindingState& state = EnsureState(env);
  if (state.secret_key_object_ctor_ref != nullptr &&
      state.public_key_object_ctor_ref != nullptr &&
      state.private_key_object_ctor_ref != nullptr) {
    return true;
  }

  napi_value ignored = nullptr;
  if (!EdgeRequireBuiltin(env, "internal/crypto/keys", &ignored)) {
    return false;
  }

  return state.secret_key_object_ctor_ref != nullptr &&
         state.public_key_object_ctor_ref != nullptr &&
         state.private_key_object_ctor_ref != nullptr;
}

napi_value CreateKeyObjectHandleValueFromCloneData(napi_env env, int32_t key_type, const std::vector<uint8_t>& key_data) {
  return CreateKeyObjectHandleValue(env, key_type, key_data);
}

napi_value InstantiateKeyObjectFromCloneData(napi_env env, int32_t key_type, const std::vector<uint8_t>& key_data) {
  if (!EnsureNativeKeyObjectConstructorsLoaded(env)) return nullptr;

  CryptoBindingState& state = EnsureState(env);
  napi_ref ctor_ref = nullptr;
  switch (key_type) {
    case kKeyTypeSecret:
      ctor_ref = state.secret_key_object_ctor_ref;
      break;
    case kKeyTypePublic:
      ctor_ref = state.public_key_object_ctor_ref;
      break;
    case kKeyTypePrivate:
      ctor_ref = state.private_key_object_ctor_ref;
      break;
    default:
      return nullptr;
  }

  napi_value ctor = GetRefValue(env, ctor_ref);
  napi_valuetype ctor_type = napi_undefined;
  if (ctor == nullptr || napi_typeof(env, ctor, &ctor_type) != napi_ok || ctor_type != napi_function) {
    return nullptr;
  }

  napi_value handle = CreateKeyObjectHandleValueFromCloneData(env, key_type, key_data);
  if (handle == nullptr || IsUndefined(env, handle)) return nullptr;

  napi_value argv[1] = {handle};
  napi_value out = nullptr;
  if (napi_new_instance(env, ctor, 1, argv, &out) != napi_ok || out == nullptr) {
    return nullptr;
  }
  return out;
}

std::string NormalizeDigestName(std::string in) {
  for (char& ch : in) ch = static_cast<char>(std::tolower(static_cast<unsigned char>(ch)));
  if (in.rfind("sha2-", 0) == 0) in.erase(3, 2);
  return in;
}

bool PopulateAsymmetricKeyDetails(napi_env env, EVP_PKEY* pkey, napi_value target) {
  if (env == nullptr || pkey == nullptr || target == nullptr) return false;

  const int pkey_type = EVP_PKEY_base_id(pkey);
  if (pkey_type == EVP_PKEY_RSA || pkey_type == EVP_PKEY_RSA_PSS) {
    const int bits = EVP_PKEY_bits(pkey);
    if (bits > 0) SetObjectInt32(env, target, "modulusLength", bits);

    BIGNUM* e = nullptr;
    if (EVP_PKEY_get_bn_param(pkey, OSSL_PKEY_PARAM_RSA_E, &e) == 1 && e != nullptr) {
      SetObjectBuffer(env, target, "publicExponent", BigNumToBytes(e));
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
              hash_algorithm = NormalizeDigestName(OBJ_nid2ln(OBJ_obj2nid(hash_obj)));
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
                mgf1_hash_algorithm = NormalizeDigestName(OBJ_nid2ln(OBJ_obj2nid(mgf1_hash_obj)));
              }
            }
          }

          int64_t salt_len = 20;
          if (params->saltLength != nullptr && ASN1_INTEGER_get_int64(&salt_len, params->saltLength) != 1) {
            salt_len = -1;
          }

          SetObjectString(env, target, "hashAlgorithm", hash_algorithm);
          SetObjectString(env, target, "mgf1HashAlgorithm", mgf1_hash_algorithm);
          if (salt_len >= 0) SetObjectInt32(env, target, "saltLength", static_cast<int32_t>(salt_len));
        }
        RSA_free(rsa);
      }
    }
    return true;
  }

  if (pkey_type == EVP_PKEY_DSA) {
    const int bits = EVP_PKEY_bits(pkey);
    if (bits > 0) SetObjectInt32(env, target, "modulusLength", bits);

    int q_bits = 0;
    if (EVP_PKEY_get_int_param(pkey, OSSL_PKEY_PARAM_FFC_QBITS, &q_bits) == 1 && q_bits > 0) {
      SetObjectInt32(env, target, "divisorLength", q_bits);
    } else {
      BIGNUM* q = nullptr;
      if (EVP_PKEY_get_bn_param(pkey, OSSL_PKEY_PARAM_FFC_Q, &q) == 1 && q != nullptr) {
        const int q_len = BN_num_bits(q);
        if (q_len > 0) SetObjectInt32(env, target, "divisorLength", q_len);
        BN_free(q);
      }
    }
    return true;
  }

  if (pkey_type == EVP_PKEY_EC) {
    std::string curve_name;
    ResolveEcJwkCurve(pkey, "", nullptr, &curve_name, nullptr);
    if (!curve_name.empty()) SetObjectString(env, target, "namedCurve", curve_name);
    return true;
  }

  return true;
}

EVP_PKEY* ImportJwkRsaKey(napi_env env, napi_value jwk, bool is_private) {
  BIGNUM* n = GetJwkBigNum(env, jwk, "n");
  BIGNUM* e = GetJwkBigNum(env, jwk, "e");
  if (n == nullptr || e == nullptr) {
    if (n != nullptr) BN_free(n);
    if (e != nullptr) BN_free(e);
    return nullptr;
  }

  BIGNUM* d = nullptr;
  BIGNUM* p = nullptr;
  BIGNUM* q = nullptr;
  BIGNUM* dp = nullptr;
  BIGNUM* dq = nullptr;
  BIGNUM* qi = nullptr;
  if (is_private) {
    d = GetJwkBigNum(env, jwk, "d");
    p = GetJwkBigNum(env, jwk, "p");
    q = GetJwkBigNum(env, jwk, "q");
    dp = GetJwkBigNum(env, jwk, "dp");
    dq = GetJwkBigNum(env, jwk, "dq");
    qi = GetJwkBigNum(env, jwk, "qi");
    if (d == nullptr || p == nullptr || q == nullptr || dp == nullptr || dq == nullptr || qi == nullptr) {
      if (d != nullptr) BN_free(d);
      if (p != nullptr) BN_free(p);
      if (q != nullptr) BN_free(q);
      if (dp != nullptr) BN_free(dp);
      if (dq != nullptr) BN_free(dq);
      if (qi != nullptr) BN_free(qi);
      BN_free(n);
      BN_free(e);
      return nullptr;
    }
  }

  RSA* rsa = RSA_new();
  if (rsa == nullptr || RSA_set0_key(rsa, n, e, d) != 1) {
    if (rsa != nullptr) RSA_free(rsa);
    else {
      BN_free(n);
      BN_free(e);
      if (d != nullptr) BN_free(d);
    }
    if (p != nullptr) BN_free(p);
    if (q != nullptr) BN_free(q);
    if (dp != nullptr) BN_free(dp);
    if (dq != nullptr) BN_free(dq);
    if (qi != nullptr) BN_free(qi);
    return nullptr;
  }

  if (is_private) {
    if (RSA_set0_factors(rsa, p, q) != 1 || RSA_set0_crt_params(rsa, dp, dq, qi) != 1) {
      RSA_free(rsa);
      return nullptr;
    }
  }

  EVP_PKEY* pkey = EVP_PKEY_new();
  if (pkey == nullptr || EVP_PKEY_assign_RSA(pkey, rsa) != 1) {
    if (pkey != nullptr) EVP_PKEY_free(pkey);
    else RSA_free(rsa);
    return nullptr;
  }
  return pkey;
}

EVP_PKEY* ImportJwkEcKey(napi_env env, napi_value jwk, const std::string& curve_hint) {
  std::string curve = curve_hint;
  if (curve.empty() && !GetNamedStringValue(env, jwk, "crv", &curve)) return nullptr;
  const int nid = CurveNidFromName(curve);
  if (nid == NID_undef) return nullptr;

  BIGNUM* x = GetJwkBigNum(env, jwk, "x");
  BIGNUM* y = GetJwkBigNum(env, jwk, "y");
  if (x == nullptr || y == nullptr) {
    if (x != nullptr) BN_free(x);
    if (y != nullptr) BN_free(y);
    return nullptr;
  }

  BIGNUM* d = GetJwkBigNum(env, jwk, "d");
  EC_KEY* ec = EC_KEY_new_by_curve_name(nid);
  if (ec == nullptr) {
    BN_free(x);
    BN_free(y);
    if (d != nullptr) BN_free(d);
    return nullptr;
  }

  const EC_GROUP* group = EC_KEY_get0_group(ec);
  EC_POINT* point = group == nullptr ? nullptr : EC_POINT_new(group);
  if (point == nullptr ||
      EC_POINT_set_affine_coordinates(group, point, x, y, nullptr) != 1 ||
      EC_KEY_set_public_key(ec, point) != 1 ||
      (d != nullptr && EC_KEY_set_private_key(ec, d) != 1) ||
      EC_KEY_check_key(ec) != 1) {
    if (point != nullptr) EC_POINT_free(point);
    EC_KEY_free(ec);
    BN_free(x);
    BN_free(y);
    if (d != nullptr) BN_free(d);
    return nullptr;
  }

  EC_POINT_free(point);
  BN_free(x);
  BN_free(y);
  if (d != nullptr) BN_free(d);

  EVP_PKEY* pkey = EVP_PKEY_new();
  if (pkey == nullptr || EVP_PKEY_assign_EC_KEY(pkey, ec) != 1) {
    if (pkey != nullptr) EVP_PKEY_free(pkey);
    else EC_KEY_free(ec);
    return nullptr;
  }
  return pkey;
}

EVP_PKEY* ImportECRawPublicKey(const std::string& named_curve,
                               const std::vector<uint8_t>& key_data) {
  const int nid = CurveNidFromName(named_curve);
  if (nid == NID_undef) return nullptr;
  EC_KEY* ec = EC_KEY_new_by_curve_name(nid);
  if (ec == nullptr) return nullptr;
  const EC_GROUP* group = EC_KEY_get0_group(ec);
  EC_POINT* point = group == nullptr ? nullptr : EC_POINT_new(group);
  if (point == nullptr ||
      EC_POINT_oct2point(group, point, key_data.data(), key_data.size(), nullptr) != 1 ||
      EC_KEY_set_public_key(ec, point) != 1 ||
      EC_KEY_check_key(ec) != 1) {
    if (point != nullptr) EC_POINT_free(point);
    EC_KEY_free(ec);
    return nullptr;
  }
  EC_POINT_free(point);

  EVP_PKEY* pkey = EVP_PKEY_new();
  if (pkey == nullptr || EVP_PKEY_assign_EC_KEY(pkey, ec) != 1) {
    if (pkey != nullptr) EVP_PKEY_free(pkey);
    else EC_KEY_free(ec);
    return nullptr;
  }
  return pkey;
}

EVP_PKEY* ImportEdRawKey(const std::string& name,
                         const std::vector<uint8_t>& key_data,
                         int32_t key_type) {
  const int nid = RawKeyNidFromName(name);
  if (nid == NID_undef) return nullptr;
  return key_type == kKeyTypePrivate
             ? EVP_PKEY_new_raw_private_key(nid, nullptr, key_data.data(), key_data.size())
             : EVP_PKEY_new_raw_public_key(nid, nullptr, key_data.data(), key_data.size());
}

napi_value KeyObjectCtor(napi_env env, napi_callback_info info) {
  napi_value this_arg = nullptr;
  size_t argc = 0;
  napi_get_cb_info(env, info, &argc, nullptr, &this_arg, nullptr);
  if (this_arg == nullptr) return nullptr;
  auto* wrap = new KeyObjectWrap();
  napi_wrap(env, this_arg, wrap, KeyObjectFinalize, nullptr, &wrap->wrapper_ref);
  return this_arg;
}

napi_value KeyObjectInit(napi_env env, napi_callback_info info) {
  size_t argc = 5;
  napi_value argv[5] = {nullptr, nullptr, nullptr, nullptr, nullptr};
  napi_value this_arg = nullptr;
  napi_get_cb_info(env, info, &argc, argv, &this_arg, nullptr);
  KeyObjectWrap* wrap = UnwrapKeyObject(env, this_arg);
  if (wrap == nullptr) return Undefined(env);
  ResetKeyObjectNativeKey(wrap);
  int32_t key_type = kKeyTypeSecret;
  if (argc >= 1 && argv[0] != nullptr) napi_get_value_int32(env, argv[0], &key_type);
  wrap->key_type = key_type;
  KeyObjectWrap* source_wrap = (argc >= 2 && argv[1] != nullptr) ? UnwrapKeyObject(env, argv[1]) : nullptr;
  wrap->key_data.clear();
  wrap->key_passphrase.clear();
  wrap->has_key_passphrase = false;
  if (argc >= 5 && !IsNullOrUndefinedValue(env, argv[4])) {
    wrap->key_passphrase = ValueToBytes(env, argv[4]);
    wrap->has_key_passphrase = true;
  }

  if (source_wrap != nullptr) {
    if (wrap->key_type == kKeyTypeSecret || source_wrap->key_type == kKeyTypeSecret) {
      napi_throw(env,
                 CreateErrorWithCode(env,
                                     "ERR_CRYPTO_INVALID_KEY_OBJECT_TYPE",
                                     "Invalid key object type"));
      return nullptr;
    }

    EVP_PKEY* pkey = ParseKeyObjectAsymmetricKey(source_wrap);
    if (pkey == nullptr) {
      std::string error_code;
      std::string error_message;
      SetPreferredOpenSslKeyParseError(
          &error_code, &error_message, source_wrap->key_type == kKeyTypePrivate, "ERR_CRYPTO_OPERATION_FAILED", "Failed to read key");
      napi_throw(env, CreateErrorWithCode(env, error_code.c_str(), error_message.c_str()));
      return nullptr;
    }

    if (wrap->key_type == kKeyTypePrivate) {
      if (source_wrap->key_type != kKeyTypePrivate) {
        EVP_PKEY_free(pkey);
        napi_throw(env,
                   CreateErrorWithCode(env,
                                       "ERR_CRYPTO_INVALID_KEY_OBJECT_TYPE",
                                       "Invalid key object type"));
        return nullptr;
      }
      wrap->key_data = ExportPrivateDerPkcs8(pkey);
    } else if (wrap->key_type == kKeyTypePublic) {
      wrap->key_data = ExportPublicDerSpki(pkey);
    }
    EVP_PKEY_free(pkey);

    wrap->key_passphrase.clear();
    wrap->has_key_passphrase = false;
    if (wrap->key_data.empty()) {
      napi_throw(env, CreateErrorWithCode(env, "ERR_CRYPTO_OPERATION_FAILED", "Failed to read key"));
      return nullptr;
    }

    napi_value out = nullptr;
    napi_get_boolean(env, true, &out);
    return out != nullptr ? out : Undefined(env);
  }

  wrap->key_data = (argc >= 2 && argv[1] != nullptr) ? ValueToBytes(env, argv[1]) : std::vector<uint8_t>{};

  if (wrap->key_type == kKeyTypePrivate) {
    EVP_PKEY* pkey = ParsePrivateKeyBytesWithPassphrase(
        wrap->key_data.data(),
        wrap->key_data.size(),
        wrap->key_passphrase.data(),
        wrap->key_passphrase.size(),
        wrap->has_key_passphrase);
    if (pkey == nullptr) {
      if (wrap->key_data.empty()) {
        napi_throw(env,
                   CreateOpenSslError(env,
                                      "ERR_OSSL_UNSUPPORTED",
                                      kOpenSslDecoderUnsupportedError,
                                      "error:1E08010C:DECODER routines::unsupported"));
        return nullptr;
      }
      ThrowLastOpenSslMessage(env, "Failed to read private key");
      return nullptr;
    }
    EVP_PKEY_free(pkey);
  } else if (wrap->key_type == kKeyTypePublic) {
    EVP_PKEY* pkey = ParsePublicKeyBytes(wrap->key_data.data(), wrap->key_data.size());
    if (pkey != nullptr) {
      wrap->key_data = ExportPublicDerSpki(pkey);
    } else {
      pkey = ParsePrivateKeyBytesWithPassphrase(
          wrap->key_data.data(),
          wrap->key_data.size(),
          wrap->key_passphrase.data(),
          wrap->key_passphrase.size(),
          wrap->has_key_passphrase);
      if (pkey != nullptr) {
        wrap->key_data = ExportPublicDerSpki(pkey);
        wrap->key_passphrase.clear();
        wrap->has_key_passphrase = false;
      }
    }
    if (pkey == nullptr) {
      ThrowLastOpenSslKeyParseError(env, false, "Failed to read public key");
      return nullptr;
    }
    EVP_PKEY_free(pkey);
    if (wrap->key_data.empty()) {
      napi_throw(env, CreateErrorWithCode(env, "ERR_CRYPTO_OPERATION_FAILED", "Failed to read public key"));
      return nullptr;
    }
  }

  napi_value out = nullptr;
  napi_get_boolean(env, true, &out);
  return out != nullptr ? out : Undefined(env);
}

napi_value KeyObjectInitJwk(napi_env env, napi_callback_info info) {
  size_t argc = 2;
  napi_value argv[2] = {nullptr, nullptr};
  napi_value this_arg = nullptr;
  napi_get_cb_info(env, info, &argc, argv, &this_arg, nullptr);
  KeyObjectWrap* wrap = UnwrapKeyObject(env, this_arg);
  if (wrap == nullptr || argc < 1 || argv[0] == nullptr) return Undefined(env);
  ResetKeyObjectNativeKey(wrap);

  std::string kty;
  if (!GetNamedStringValue(env, argv[0], "kty", &kty)) {
    napi_throw(env, CreateErrorWithCode(env, "ERR_CRYPTO_INVALID_JWK", "Invalid JWK"));
    return nullptr;
  }

  std::string curve_hint;
  if (argc >= 2 && argv[1] != nullptr && !IsNullOrUndefinedValue(env, argv[1])) {
    curve_hint = GetStringValue(env, argv[1]);
  }

  EVP_PKEY* pkey = nullptr;
  if (kty == "RSA") {
    std::string private_value;
    const bool is_private = GetNamedStringValue(env, argv[0], "d", &private_value);
    pkey = ImportJwkRsaKey(env, argv[0], is_private);
    wrap->key_type = is_private ? kKeyTypePrivate : kKeyTypePublic;
  } else if (kty == "EC") {
    std::string private_value;
    const bool is_private = GetNamedStringValue(env, argv[0], "d", &private_value);
    pkey = ImportJwkEcKey(env, argv[0], curve_hint);
    wrap->key_type = is_private ? kKeyTypePrivate : kKeyTypePublic;
  } else if (kty == "oct") {
    std::string encoded;
    if (!GetNamedStringValue(env, argv[0], "k", &encoded)) {
      napi_throw(env, CreateErrorWithCode(env, "ERR_CRYPTO_INVALID_JWK", "Invalid JWK secret key format"));
      return nullptr;
    }
    wrap->key_type = kKeyTypeSecret;
    wrap->key_data = Base64UrlDecode(encoded);
    if (wrap->key_data.empty()) {
      napi_throw(env, CreateErrorWithCode(env, "ERR_CRYPTO_INVALID_JWK", "Invalid JWK secret key format"));
      return nullptr;
    }
    wrap->key_passphrase.clear();
    wrap->has_key_passphrase = false;
    napi_value out = nullptr;
    napi_create_int32(env, wrap->key_type, &out);
    return out != nullptr ? out : Undefined(env);
  } else {
    napi_throw(env, CreateErrorWithCode(env, "ERR_CRYPTO_INVALID_JWK", "Invalid JWK"));
    return nullptr;
  }

  if (pkey == nullptr) {
    napi_throw(env, CreateErrorWithCode(env, "ERR_CRYPTO_INVALID_JWK", "Invalid JWK"));
    return nullptr;
  }

  wrap->key_passphrase.clear();
  wrap->has_key_passphrase = false;
  wrap->key_data = wrap->key_type == kKeyTypePrivate ? ExportPrivateDerPkcs8(pkey) : ExportPublicDerSpki(pkey);
  EVP_PKEY_free(pkey);
  if (wrap->key_data.empty()) {
    napi_throw(env, CreateErrorWithCode(env, "ERR_CRYPTO_INVALID_JWK", "Invalid JWK"));
    return nullptr;
  }

  napi_value out = nullptr;
  napi_create_int32(env, wrap->key_type, &out);
  return out != nullptr ? out : Undefined(env);
}

napi_value KeyObjectExportJwk(napi_env env, napi_callback_info info) {
  size_t argc = 2;
  napi_value argv[2] = {nullptr, nullptr};
  napi_value this_arg = nullptr;
  napi_get_cb_info(env, info, &argc, argv, &this_arg, nullptr);
  KeyObjectWrap* wrap = UnwrapKeyObject(env, this_arg);
  if (wrap == nullptr || argc < 1 || argv[0] == nullptr) return Undefined(env);

  if (wrap->key_type == kKeyTypeSecret) {
    SetObjectString(env, argv[0], "kty", "oct");
    SetObjectString(env, argv[0], "k", Base64UrlEncode(wrap->key_data));
    return argv[0];
  }

  bool handle_rsa_pss = false;
  if (argc >= 2 && argv[1] != nullptr && !IsNullOrUndefinedValue(env, argv[1])) {
    napi_get_value_bool(env, argv[1], &handle_rsa_pss);
  }
  EVP_PKEY* pkey = ParseKeyObjectAsymmetricKey(wrap);
  if (pkey == nullptr) {
    napi_throw(env, CreateErrorWithCode(env, "ERR_CRYPTO_OPERATION_FAILED", "Failed to parse key"));
    return nullptr;
  }

  const int base = EVP_PKEY_base_id(pkey);
  std::string error_code;
  std::string error_message;
  napi_value exported = nullptr;
  if (base == EVP_PKEY_RSA_PSS && !handle_rsa_pss) {
    error_code = "ERR_CRYPTO_JWK_UNSUPPORTED_KEY_TYPE";
    error_message = "Unsupported JWK Key Type.";
  } else if (wrap->key_type == kKeyTypePrivate) {
    exported = ExportJwkPrivate(env, pkey, &error_code, &error_message, "");
  } else {
    exported = ExportJwkPublic(env, pkey, &error_code, &error_message, "");
  }
  EVP_PKEY_free(pkey);

  if (exported == nullptr) {
    if (error_code.empty()) error_code = "ERR_CRYPTO_OPERATION_FAILED";
    if (error_message.empty()) error_message = "Failed to export JWK";
    napi_throw(env, CreateErrorWithCode(env, error_code.c_str(), error_message));
    return nullptr;
  }

  napi_value names = nullptr;
  if (napi_get_property_names(env, exported, &names) != napi_ok || names == nullptr) return argv[0];
  uint32_t length = 0;
  if (napi_get_array_length(env, names, &length) != napi_ok) return argv[0];
  for (uint32_t i = 0; i < length; ++i) {
    napi_value key = nullptr;
    napi_value value = nullptr;
    if (napi_get_element(env, names, i, &key) != napi_ok || key == nullptr) continue;
    if (napi_get_property(env, exported, key, &value) != napi_ok || value == nullptr) continue;
    napi_set_property(env, argv[0], key, value);
  }
  return argv[0];
}

napi_value KeyObjectInitECRaw(napi_env env, napi_callback_info info) {
  size_t argc = 2;
  napi_value argv[2] = {nullptr, nullptr};
  napi_value this_arg = nullptr;
  napi_get_cb_info(env, info, &argc, argv, &this_arg, nullptr);
  KeyObjectWrap* wrap = UnwrapKeyObject(env, this_arg);
  if (wrap == nullptr || argc < 2) return Undefined(env);
  ResetKeyObjectNativeKey(wrap);

  EVP_PKEY* pkey = ImportECRawPublicKey(GetStringValue(env, argv[0]), ValueToBytes(env, argv[1]));
  bool ok = pkey != nullptr;
  if (ok) {
    wrap->key_type = kKeyTypePublic;
    wrap->key_passphrase.clear();
    wrap->has_key_passphrase = false;
    wrap->key_data = ExportPublicDerSpki(pkey);
    ok = !wrap->key_data.empty();
    EVP_PKEY_free(pkey);
  }
  napi_value out = nullptr;
  napi_get_boolean(env, ok, &out);
  return out != nullptr ? out : Undefined(env);
}

napi_value KeyObjectInitEDRaw(napi_env env, napi_callback_info info) {
  size_t argc = 3;
  napi_value argv[3] = {nullptr, nullptr, nullptr};
  napi_value this_arg = nullptr;
  napi_get_cb_info(env, info, &argc, argv, &this_arg, nullptr);
  KeyObjectWrap* wrap = UnwrapKeyObject(env, this_arg);
  if (wrap == nullptr || argc < 3) return Undefined(env);
  ResetKeyObjectNativeKey(wrap);

  int32_t key_type = kKeyTypePublic;
  napi_get_value_int32(env, argv[2], &key_type);
  EVP_PKEY* pkey = ImportEdRawKey(GetStringValue(env, argv[0]), ValueToBytes(env, argv[1]), key_type);
  bool ok = pkey != nullptr;
  if (ok) {
    wrap->key_type = key_type;
    wrap->key_passphrase.clear();
    wrap->has_key_passphrase = false;
    wrap->key_data = key_type == kKeyTypePrivate ? ExportPrivateDerPkcs8(pkey) : ExportPublicDerSpki(pkey);
    ok = !wrap->key_data.empty();
    EVP_PKEY_free(pkey);
  }
  napi_value out = nullptr;
  napi_get_boolean(env, ok, &out);
  return out != nullptr ? out : Undefined(env);
}

napi_value KeyObjectInitPqcRaw(napi_env env, napi_callback_info info) {
  size_t argc = 3;
  napi_value argv[3] = {nullptr, nullptr, nullptr};
  napi_value this_arg = nullptr;
  napi_get_cb_info(env, info, &argc, argv, &this_arg, nullptr);
  KeyObjectWrap* wrap = UnwrapKeyObject(env, this_arg);
  if (wrap == nullptr || argc < 3) return Undefined(env);
  ResetKeyObjectNativeKey(wrap);

#if OPENSSL_WITH_PQC
  int32_t key_type = kKeyTypePublic;
  napi_get_value_int32(env, argv[2], &key_type);
  const std::string name = GetStringValue(env, argv[0]);
  const std::vector<uint8_t> key_data = ValueToBytes(env, argv[1]);
  const int nid = RawKeyNidFromName(name);
  ncrypto::EVPKeyPointer pkey = key_type == kKeyTypePrivate
                                    ? ncrypto::EVPKeyPointer::NewRawSeed(
                                          nid,
                                          ncrypto::Buffer<const unsigned char>{
                                              .data = key_data.data(),
                                              .len = key_data.size(),
                                          })
                                    : ncrypto::EVPKeyPointer::NewRawPublic(
                                          nid,
                                          ncrypto::Buffer<const unsigned char>{
                                              .data = key_data.data(),
                                              .len = key_data.size(),
                                          });

  bool ok = static_cast<bool>(pkey);
  if (ok) {
    wrap->key_type = key_type;
    wrap->key_passphrase.clear();
    wrap->has_key_passphrase = false;
    wrap->key_data =
        key_type == kKeyTypePrivate ? ExportPrivateDerPkcs8(pkey.get()) : ExportPublicDerSpki(pkey.get());
    ok = !wrap->key_data.empty();
  }

  napi_value out = nullptr;
  napi_get_boolean(env, ok, &out);
  return out != nullptr ? out : Undefined(env);
#else
  napi_value out = nullptr;
  napi_get_boolean(env, false, &out);
  return out != nullptr ? out : Undefined(env);
#endif
}

napi_value KeyObjectRawPublicKey(napi_env env, napi_callback_info info) {
  napi_value this_arg = nullptr;
  size_t argc = 0;
  napi_get_cb_info(env, info, &argc, nullptr, &this_arg, nullptr);
  KeyObjectWrap* wrap = UnwrapKeyObject(env, this_arg);
  if (wrap == nullptr || wrap->key_type == kKeyTypeSecret) return Undefined(env);

  EVP_PKEY* pkey = ParseKeyObjectAsymmetricKey(wrap);
  if (pkey == nullptr) {
    napi_throw(env, CreateErrorWithCode(env, "ERR_CRYPTO_OPERATION_FAILED", "Failed to parse key"));
    return nullptr;
  }

  size_t raw_len = 0;
  if (EVP_PKEY_get_raw_public_key(pkey, nullptr, &raw_len) != 1 || raw_len == 0) {
    EVP_PKEY_free(pkey);
    napi_throw(env, CreateErrorWithCode(env, "ERR_CRYPTO_OPERATION_FAILED", "Failed to get raw public key"));
    return nullptr;
  }

  std::vector<uint8_t> raw(raw_len);
  if (EVP_PKEY_get_raw_public_key(pkey, raw.data(), &raw_len) != 1) {
    EVP_PKEY_free(pkey);
    napi_throw(env, CreateErrorWithCode(env, "ERR_CRYPTO_OPERATION_FAILED", "Failed to get raw public key"));
    return nullptr;
  }
  EVP_PKEY_free(pkey);
  raw.resize(raw_len);
  return BytesToBuffer(env, raw);
}

napi_value KeyObjectRawSeed(napi_env env, napi_callback_info info) {
  napi_value this_arg = nullptr;
  size_t argc = 0;
  napi_get_cb_info(env, info, &argc, nullptr, &this_arg, nullptr);
  KeyObjectWrap* wrap = UnwrapKeyObject(env, this_arg);
  if (wrap == nullptr || wrap->key_type != kKeyTypePrivate) return Undefined(env);

  EVP_PKEY* pkey = ParseKeyObjectAsymmetricKey(wrap);
  if (pkey == nullptr) {
    napi_throw(env, CreateErrorWithCode(env, "ERR_CRYPTO_OPERATION_FAILED", "Failed to parse key"));
    return nullptr;
  }

#if OPENSSL_WITH_PQC
  const int key_id = AsymmetricKeyId(pkey);
  const char* param_name = nullptr;
  size_t seed_len = 0;
  switch (key_id) {
    case EVP_PKEY_ML_DSA_44:
    case EVP_PKEY_ML_DSA_65:
    case EVP_PKEY_ML_DSA_87:
      param_name = OSSL_PKEY_PARAM_ML_DSA_SEED;
      seed_len = 32;
      break;
    case EVP_PKEY_ML_KEM_512:
    case EVP_PKEY_ML_KEM_768:
    case EVP_PKEY_ML_KEM_1024:
      param_name = OSSL_PKEY_PARAM_ML_KEM_SEED;
      seed_len = 64;
      break;
    default:
      break;
  }

  if (param_name == nullptr || seed_len == 0) {
    EVP_PKEY_free(pkey);
    napi_throw(env, CreateErrorWithCode(env, "ERR_CRYPTO_OPERATION_FAILED", "Failed to get raw seed"));
    return nullptr;
  }

  std::vector<uint8_t> seed(seed_len);
  if (EVP_PKEY_get_octet_string_param(pkey, param_name, seed.data(), seed.size(), &seed_len) != 1 || seed_len == 0) {
    EVP_PKEY_free(pkey);
    napi_throw(env, CreateErrorWithCode(env, "ERR_CRYPTO_OPERATION_FAILED", "Failed to get raw seed"));
    return nullptr;
  }
  EVP_PKEY_free(pkey);
  seed.resize(seed_len);
  return BytesToBuffer(env, seed);
#else
  EVP_PKEY_free(pkey);
  napi_throw(env, CreateErrorWithCode(env, "ERR_CRYPTO_OPERATION_FAILED", "Failed to get raw seed"));
  return nullptr;
#endif
}

napi_value KeyObjectCheckEcKeyData(napi_env env, napi_callback_info info) {
  napi_value this_arg = nullptr;
  size_t argc = 0;
  napi_get_cb_info(env, info, &argc, nullptr, &this_arg, nullptr);
  KeyObjectWrap* wrap = UnwrapKeyObject(env, this_arg);
  bool ok = false;
  if (wrap != nullptr) {
    EVP_PKEY* pkey = ParseKeyObjectAsymmetricKey(wrap);
    if (pkey != nullptr && EVP_PKEY_base_id(pkey) == EVP_PKEY_EC) {
      EVP_PKEY_CTX* ctx = EVP_PKEY_CTX_new(pkey, nullptr);
      if (ctx != nullptr) {
        ok = wrap->key_type == kKeyTypePrivate ? EVP_PKEY_private_check(ctx) == 1
                                               : EVP_PKEY_public_check(ctx) == 1;
        EVP_PKEY_CTX_free(ctx);
      }
      EVP_PKEY_free(pkey);
    }
  }
  napi_value out = nullptr;
  napi_get_boolean(env, ok, &out);
  return out != nullptr ? out : Undefined(env);
}

napi_value KeyObjectExport(napi_env env, napi_callback_info info) {
  size_t argc = 4;
  napi_value argv[4] = {nullptr, nullptr, nullptr, nullptr};
  napi_value this_arg = nullptr;
  napi_get_cb_info(env, info, &argc, argv, &this_arg, nullptr);
  KeyObjectWrap* wrap = UnwrapKeyObject(env, this_arg);
  if (wrap == nullptr) return Undefined(env);
  if (wrap->key_type == kKeyTypeSecret) return BytesToBuffer(env, wrap->key_data);

  EVP_PKEY* pkey = ParseKeyObjectAsymmetricKey(wrap);
  if (pkey == nullptr) return BytesToBuffer(env, wrap->key_data);

  KeyEncodingSelection encoding;
  std::string error_code;
  std::string error_message;
  napi_value out = nullptr;
  if (wrap->key_type == kKeyTypePublic) {
    encoding.has_public_encoding = true;
    encoding.public_format = argc >= 1 ? GetInt32Value(env, argv[0], kKeyFormatPEM) : kKeyFormatPEM;
    encoding.public_type = argc >= 2 ? GetInt32Value(env, argv[1], kKeyEncodingSPKI) : kKeyEncodingSPKI;
    if (!ExportPublicKeyValue(env, pkey, encoding, "", &out, &error_code, &error_message)) {
      EVP_PKEY_free(pkey);
      if (error_code.empty()) error_code = "ERR_CRYPTO_OPERATION_FAILED";
      if (error_message.empty()) error_message = "Public key export failed";
      napi_throw(env, CreateErrorWithCode(env, error_code.c_str(), error_message));
      return nullptr;
    }
  } else {
    encoding.has_private_encoding = true;
    encoding.private_format = argc >= 1 ? GetInt32Value(env, argv[0], kKeyFormatPEM) : kKeyFormatPEM;
    encoding.private_type = argc >= 2 ? GetInt32Value(env, argv[1], kKeyEncodingPKCS8) : kKeyEncodingPKCS8;
    if (argc >= 3 && !IsNullOrUndefinedValue(env, argv[2])) encoding.private_cipher = GetStringValue(env, argv[2]);
    if (argc >= 4 && !IsNullOrUndefinedValue(env, argv[3])) {
      encoding.private_passphrase = ValueToBytes(env, argv[3]);
      encoding.private_passphrase_provided = true;
    }
    if (!ExportPrivateKeyValue(env, pkey, encoding, "", &out, &error_code, &error_message)) {
      EVP_PKEY_free(pkey);
      if (error_code.empty()) error_code = "ERR_CRYPTO_OPERATION_FAILED";
      if (error_message.empty()) error_message = "Private key export failed";
      napi_throw(env, CreateErrorWithCode(env, error_code.c_str(), error_message));
      return nullptr;
    }
  }
  EVP_PKEY_free(pkey);
  return out != nullptr ? out : Undefined(env);
}

napi_value KeyObjectGetAsymmetricKeyType(napi_env env, napi_callback_info info) {
  napi_value this_arg = nullptr;
  size_t argc = 0;
  napi_get_cb_info(env, info, &argc, nullptr, &this_arg, nullptr);
  KeyObjectWrap* wrap = UnwrapKeyObject(env, this_arg);
  if (wrap == nullptr) return Undefined(env);
  EVP_PKEY* pkey = ParseKeyObjectAsymmetricKey(wrap);
  if (pkey == nullptr) return Undefined(env);
  const std::string type = AsymmetricKeyTypeName(pkey);
  EVP_PKEY_free(pkey);
  if (type.empty()) return Undefined(env);
  napi_value out = nullptr;
  napi_create_string_utf8(env, type.c_str(), NAPI_AUTO_LENGTH, &out);
  return out != nullptr ? out : Undefined(env);
}

napi_value KeyObjectGetAsymmetricKeyDetails(napi_env env, napi_callback_info info) {
  size_t argc = 1;
  napi_value argv[1] = {nullptr};
  napi_value this_arg = nullptr;
  napi_get_cb_info(env, info, &argc, argv, &this_arg, nullptr);
  KeyObjectWrap* wrap = UnwrapKeyObject(env, this_arg);
  if (wrap == nullptr) return Undefined(env);
  napi_value target = argc >= 1 && argv[0] != nullptr ? argv[0] : nullptr;
  if (target == nullptr) napi_create_object(env, &target);
  if (wrap->key_type == kKeyTypeSecret) {
    SetObjectInt32(env, target, "length", static_cast<int32_t>(wrap->key_data.size() * CHAR_BIT));
    return target != nullptr ? target : Undefined(env);
  }
  EVP_PKEY* pkey = ParseKeyObjectAsymmetricKey(wrap);
  if (pkey == nullptr) return target != nullptr ? target : Undefined(env);
  PopulateAsymmetricKeyDetails(env, pkey, target);
  EVP_PKEY_free(pkey);
  return target != nullptr ? target : Undefined(env);
}

napi_value KeyObjectGetSymmetricKeySize(napi_env env, napi_callback_info info) {
  napi_value this_arg = nullptr;
  size_t argc = 0;
  napi_get_cb_info(env, info, &argc, nullptr, &this_arg, nullptr);
  KeyObjectWrap* wrap = UnwrapKeyObject(env, this_arg);
  uint32_t size = wrap == nullptr ? 0 : static_cast<uint32_t>(wrap->key_data.size());
  napi_value out = nullptr;
  napi_create_uint32(env, size, &out);
  return out != nullptr ? out : Undefined(env);
}

napi_value KeyObjectEquals(napi_env env, napi_callback_info info) {
  size_t argc = 1;
  napi_value argv[1] = {nullptr};
  napi_value this_arg = nullptr;
  napi_get_cb_info(env, info, &argc, argv, &this_arg, nullptr);
  KeyObjectWrap* left = UnwrapKeyObject(env, this_arg);
  bool equal = false;
  if (left != nullptr && argc >= 1 && argv[0] != nullptr) {
    KeyObjectWrap* right = UnwrapKeyObject(env, argv[0]);
    if (right != nullptr && left->key_type == right->key_type) {
      if (left->key_type == kKeyTypeSecret) {
        equal = left->key_data.size() == right->key_data.size() &&
                std::memcmp(left->key_data.data(), right->key_data.data(), left->key_data.size()) == 0;
      } else {
        EVP_PKEY* left_pkey = ParseKeyObjectAsymmetricKey(left);
        EVP_PKEY* right_pkey = ParseKeyObjectAsymmetricKey(right);
        if (left_pkey != nullptr && right_pkey != nullptr) {
#if OPENSSL_VERSION_NUMBER >= 0x30000000L
          equal = EVP_PKEY_eq(left_pkey, right_pkey) == 1;
#else
          equal = EVP_PKEY_cmp(left_pkey, right_pkey) == 1;
#endif
        }
        if (left_pkey != nullptr) EVP_PKEY_free(left_pkey);
        if (right_pkey != nullptr) EVP_PKEY_free(right_pkey);
      }
    }
  }
  napi_value out = nullptr;
  napi_get_boolean(env, equal, &out);
  return out != nullptr ? out : Undefined(env);
}

struct SecureContextWrap {
  napi_ref wrapper_ref = nullptr;
  napi_ref handle_ref = nullptr;
  int32_t min_proto = 0;
  int32_t max_proto = 0;
};

edge::crypto::SecureContextHolder* GetSecureContextHolderFromWrap(napi_env env, SecureContextWrap* wrap);

void SecureContextFinalize(napi_env env, void* data, void* /*hint*/) {
  auto* wrap = static_cast<SecureContextWrap*>(data);
  if (wrap == nullptr) return;
  ResetRef(env, &wrap->wrapper_ref);
  ResetRef(env, &wrap->handle_ref);
  delete wrap;
}

SecureContextWrap* UnwrapSecureContext(napi_env env, napi_value this_arg) {
  if (this_arg == nullptr) return nullptr;
  void* data = nullptr;
  if (napi_unwrap(env, this_arg, &data) != napi_ok || data == nullptr) return nullptr;
  return static_cast<SecureContextWrap*>(data);
}

SecureContextWrap* RequireSecureContext(napi_env env, napi_value this_arg) {
  SecureContextWrap* wrap = UnwrapSecureContext(env, this_arg);
  if (wrap != nullptr) return wrap;
  napi_throw_type_error(env, nullptr, "Illegal invocation");
  return nullptr;
}

napi_value SecureContextExternalGetter(napi_env env, napi_callback_info info) {
  napi_value this_arg = nullptr;
  size_t argc = 0;
  if (napi_get_cb_info(env, info, &argc, nullptr, &this_arg, nullptr) != napi_ok) return nullptr;
  SecureContextWrap* wrap = RequireSecureContext(env, this_arg);
  if (wrap == nullptr) return nullptr;

  edge::crypto::SecureContextHolder* holder = GetSecureContextHolderFromWrap(env, wrap);
  if (holder == nullptr || holder->ctx == nullptr) return Undefined(env);

  napi_value external = nullptr;
  if (napi_create_external(env, holder->ctx, nullptr, nullptr, &external) != napi_ok) return nullptr;
  return external;
}

napi_value SecureContextSetClientCertEngine(napi_env env, napi_callback_info info) {
  size_t argc = 1;
  napi_value argv[1] = {nullptr};
  napi_value this_arg = nullptr;
  if (napi_get_cb_info(env, info, &argc, argv, &this_arg, nullptr) != napi_ok) return nullptr;
  SecureContextWrap* wrap = RequireSecureContext(env, this_arg);
  if (wrap == nullptr) return nullptr;

#ifndef OPENSSL_NO_ENGINE
  if (argc < 1 || argv[0] == nullptr) return Undefined(env);
  std::string engine_id = GetStringValue(env, argv[0]);

  ncrypto::EnginePointer::initEnginesOnce();
  ncrypto::CryptoErrorList errors(ncrypto::CryptoErrorList::Option::NONE);
  auto engine = ncrypto::EnginePointer::getEngineByName(engine_id.c_str(), &errors);
  if (!engine) {
    napi_throw(env, CreateOpenSslErrorStackException(env, errors));
    return nullptr;
  }

  edge::crypto::SecureContextHolder* holder = GetSecureContextHolderFromWrap(env, wrap);
  if (holder == nullptr || holder->ctx == nullptr) {
    napi_throw_error(env, "ERR_INVALID_ARG_TYPE", "context must be a secure context handle");
    return nullptr;
  }

  if (!SSL_CTX_set_client_cert_engine(holder->ctx, engine.get())) {
    errors.capture();
    napi_throw(env, CreateOpenSslErrorStackException(env, errors));
    return nullptr;
  }
  return Undefined(env);
#else
  napi_throw(env, CreateErrorWithCode(env,
                                      "ERR_CRYPTO_CUSTOM_ENGINE_NOT_SUPPORTED",
                                      "Custom engines not supported by this OpenSSL"));
  return nullptr;
#endif
}

void SecureContextCallBindingMethod(napi_env env,
                                    SecureContextWrap* wrap,
                                    const char* method,
                                    size_t extra_argc,
                                    napi_value* extra_argv) {
  if (wrap == nullptr || method == nullptr) return;
  napi_value binding = GetBinding(env);
  napi_value handle = GetRefValue(env, wrap->handle_ref);
  if (binding == nullptr || handle == nullptr) return;
  std::vector<napi_value> call_argv;
  call_argv.reserve(1 + extra_argc);
  call_argv.push_back(handle);
  for (size_t i = 0; i < extra_argc; ++i) {
    call_argv.push_back(extra_argv != nullptr ? extra_argv[i] : Undefined(env));
  }
  napi_value ignored = nullptr;
  CallBindingMethod(env, binding, method, call_argv.size(), call_argv.data(), &ignored);
}

edge::crypto::SecureContextHolder* GetSecureContextHolderFromWrap(napi_env env, SecureContextWrap* wrap) {
  if (wrap == nullptr || wrap->handle_ref == nullptr) return nullptr;
  napi_value handle = GetRefValue(env, wrap->handle_ref);
  if (handle == nullptr) return nullptr;
  edge::crypto::SecureContextHolder* holder = nullptr;
  if (!edge::crypto::GetSecureContextHolder(env, handle, &holder)) return nullptr;
  return holder;
}

napi_value CallSecureContextBindingMethodReturningValue(napi_env env,
                                                        SecureContextWrap* wrap,
                                                        const char* method,
                                                        size_t extra_argc,
                                                        napi_value* extra_argv) {
  if (wrap == nullptr || method == nullptr) return nullptr;
  napi_value binding = GetBinding(env);
  napi_value handle = GetRefValue(env, wrap->handle_ref);
  if (binding == nullptr || handle == nullptr) return nullptr;
  std::vector<napi_value> call_argv;
  call_argv.reserve(1 + extra_argc);
  call_argv.push_back(handle);
  for (size_t i = 0; i < extra_argc; ++i) {
    call_argv.push_back(extra_argv != nullptr ? extra_argv[i] : Undefined(env));
  }
  napi_value out = nullptr;
  if (!CallBindingMethod(env, binding, method, call_argv.size(), call_argv.data(), &out)) return nullptr;
  return out;
}

napi_value SecureContextCtor(napi_env env, napi_callback_info info) {
  napi_value this_arg = nullptr;
  size_t argc = 0;
  napi_get_cb_info(env, info, &argc, nullptr, &this_arg, nullptr);
  if (this_arg == nullptr) return nullptr;
  auto* wrap = new SecureContextWrap();
  napi_wrap(env, this_arg, wrap, SecureContextFinalize, nullptr, &wrap->wrapper_ref);
  return this_arg;
}

napi_value SecureContextInit(napi_env env, napi_callback_info info) {
  size_t argc = 3;
  napi_value argv[3] = {nullptr, nullptr, nullptr};
  napi_value this_arg = nullptr;
  napi_get_cb_info(env, info, &argc, argv, &this_arg, nullptr);
  SecureContextWrap* wrap = RequireSecureContext(env, this_arg);
  if (wrap == nullptr) return Undefined(env);
  if (argc >= 2 && argv[1] != nullptr) napi_get_value_int32(env, argv[1], &wrap->min_proto);
  if (argc >= 3 && argv[2] != nullptr) napi_get_value_int32(env, argv[2], &wrap->max_proto);

  napi_value binding = GetBinding(env);
  if (binding != nullptr) {
    napi_value handle = nullptr;
    if (CallBindingMethod(env, binding, "secureContextCreate", 0, nullptr, &handle)) {
      ResetRef(env, &wrap->handle_ref);
      napi_create_reference(env, handle, 1, &wrap->handle_ref);
      napi_value init_argv[4] = {
          handle,
          argc >= 1 ? argv[0] : Undefined(env),
          argc >= 2 ? argv[1] : Undefined(env),
          argc >= 3 ? argv[2] : Undefined(env),
      };
      napi_value ignored = nullptr;
      CallBindingMethod(env, binding, "secureContextInit", 4, init_argv, &ignored);
    }
  }
  return Undefined(env);
}

napi_value SecureContextSetOptions(napi_env env, napi_callback_info info) {
  size_t argc = 1;
  napi_value argv[1] = {nullptr};
  napi_value this_arg = nullptr;
  napi_get_cb_info(env, info, &argc, argv, &this_arg, nullptr);
  SecureContextWrap* wrap = RequireSecureContext(env, this_arg);
  if (wrap == nullptr) return Undefined(env);
  napi_value call_argv[1] = {argc >= 1 ? argv[0] : Undefined(env)};
  SecureContextCallBindingMethod(env, wrap, "secureContextSetOptions", 1, call_argv);
  return Undefined(env);
}

napi_value SecureContextSetCiphers(napi_env env, napi_callback_info info) {
  size_t argc = 1;
  napi_value argv[1] = {nullptr};
  napi_value this_arg = nullptr;
  napi_get_cb_info(env, info, &argc, argv, &this_arg, nullptr);
  SecureContextWrap* wrap = RequireSecureContext(env, this_arg);
  if (wrap == nullptr) return Undefined(env);
  napi_value call_argv[1] = {argc >= 1 ? argv[0] : Undefined(env)};
  SecureContextCallBindingMethod(env, wrap, "secureContextSetCiphers", 1, call_argv);
  return Undefined(env);
}

napi_value SecureContextSetCipherSuites(napi_env env, napi_callback_info info) {
  size_t argc = 1;
  napi_value argv[1] = {nullptr};
  napi_value this_arg = nullptr;
  napi_get_cb_info(env, info, &argc, argv, &this_arg, nullptr);
  SecureContextWrap* wrap = RequireSecureContext(env, this_arg);
  if (wrap == nullptr) return Undefined(env);
  napi_value call_argv[1] = {argc >= 1 ? argv[0] : Undefined(env)};
  SecureContextCallBindingMethod(env, wrap, "secureContextSetCipherSuites", 1, call_argv);
  return Undefined(env);
}

napi_value SecureContextSetCert(napi_env env, napi_callback_info info) {
  size_t argc = 1;
  napi_value argv[1] = {nullptr};
  napi_value this_arg = nullptr;
  napi_get_cb_info(env, info, &argc, argv, &this_arg, nullptr);
  SecureContextWrap* wrap = RequireSecureContext(env, this_arg);
  if (wrap == nullptr) return Undefined(env);
  napi_value call_argv[1] = {argc >= 1 ? argv[0] : Undefined(env)};
  SecureContextCallBindingMethod(env, wrap, "secureContextSetCert", 1, call_argv);
  return Undefined(env);
}

napi_value SecureContextSetKey(napi_env env, napi_callback_info info) {
  size_t argc = 2;
  napi_value argv[2] = {nullptr, nullptr};
  napi_value this_arg = nullptr;
  napi_get_cb_info(env, info, &argc, argv, &this_arg, nullptr);
  SecureContextWrap* wrap = RequireSecureContext(env, this_arg);
  if (wrap == nullptr) return Undefined(env);
  napi_value call_argv[2] = {argc >= 1 ? argv[0] : Undefined(env), argc >= 2 ? argv[1] : Undefined(env)};
  SecureContextCallBindingMethod(env, wrap, "secureContextSetKey", 2, call_argv);
  return Undefined(env);
}

napi_value SecureContextAddCACert(napi_env env, napi_callback_info info) {
  size_t argc = 1;
  napi_value argv[1] = {nullptr};
  napi_value this_arg = nullptr;
  napi_get_cb_info(env, info, &argc, argv, &this_arg, nullptr);
  SecureContextWrap* wrap = RequireSecureContext(env, this_arg);
  if (wrap == nullptr) return Undefined(env);
  napi_value call_argv[1] = {argc >= 1 ? argv[0] : Undefined(env)};
  SecureContextCallBindingMethod(env, wrap, "secureContextAddCACert", 1, call_argv);
  return Undefined(env);
}

napi_value SecureContextAddCrl(napi_env env, napi_callback_info info) {
  size_t argc = 1;
  napi_value argv[1] = {nullptr};
  napi_value this_arg = nullptr;
  napi_get_cb_info(env, info, &argc, argv, &this_arg, nullptr);
  SecureContextWrap* wrap = RequireSecureContext(env, this_arg);
  if (wrap == nullptr) return Undefined(env);
  napi_value call_argv[1] = {argc >= 1 ? argv[0] : Undefined(env)};
  SecureContextCallBindingMethod(env, wrap, "secureContextAddCrl", 1, call_argv);
  return Undefined(env);
}

napi_value SecureContextAddRootCerts(napi_env env, napi_callback_info info) {
  napi_value this_arg = nullptr;
  size_t argc = 0;
  napi_get_cb_info(env, info, &argc, nullptr, &this_arg, nullptr);
  SecureContextWrap* wrap = RequireSecureContext(env, this_arg);
  if (wrap == nullptr) return Undefined(env);
  SecureContextCallBindingMethod(env, wrap, "secureContextAddRootCerts", 0, nullptr);
  return Undefined(env);
}

napi_value SecureContextSetECDHCurve(napi_env env, napi_callback_info info) {
  size_t argc = 1;
  napi_value argv[1] = {nullptr};
  napi_value this_arg = nullptr;
  napi_get_cb_info(env, info, &argc, argv, &this_arg, nullptr);
  SecureContextWrap* wrap = RequireSecureContext(env, this_arg);
  if (wrap == nullptr) return Undefined(env);
  napi_value call_argv[1] = {argc >= 1 ? argv[0] : Undefined(env)};
  SecureContextCallBindingMethod(env, wrap, "secureContextSetECDHCurve", 1, call_argv);
  return Undefined(env);
}

napi_value SecureContextSetDHParam(napi_env env, napi_callback_info info) {
  size_t argc = 1;
  napi_value argv[1] = {nullptr};
  napi_value this_arg = nullptr;
  napi_get_cb_info(env, info, &argc, argv, &this_arg, nullptr);
  SecureContextWrap* wrap = RequireSecureContext(env, this_arg);
  if (wrap == nullptr) return Undefined(env);
  napi_value call_argv[1] = {argc >= 1 ? argv[0] : Undefined(env)};
  napi_value out = CallSecureContextBindingMethodReturningValue(env, wrap, "secureContextSetDHParam", 1, call_argv);
  return out != nullptr ? out : Undefined(env);
}

napi_value SecureContextSetSigalgs(napi_env env, napi_callback_info info) {
  size_t argc = 1;
  napi_value argv[1] = {nullptr};
  napi_value this_arg = nullptr;
  napi_get_cb_info(env, info, &argc, argv, &this_arg, nullptr);
  SecureContextWrap* wrap = RequireSecureContext(env, this_arg);
  if (wrap == nullptr) return Undefined(env);
  napi_value call_argv[1] = {argc >= 1 ? argv[0] : Undefined(env)};
  SecureContextCallBindingMethod(env, wrap, "secureContextSetSigalgs", 1, call_argv);
  return Undefined(env);
}

napi_value SecureContextSetSessionIdContext(napi_env env, napi_callback_info info) {
  size_t argc = 1;
  napi_value argv[1] = {nullptr};
  napi_value this_arg = nullptr;
  napi_get_cb_info(env, info, &argc, argv, &this_arg, nullptr);
  SecureContextWrap* wrap = RequireSecureContext(env, this_arg);
  if (wrap == nullptr) return Undefined(env);
  napi_value call_argv[1] = {argc >= 1 ? argv[0] : Undefined(env)};
  SecureContextCallBindingMethod(env, wrap, "secureContextSetSessionIdContext", 1, call_argv);
  return Undefined(env);
}

napi_value SecureContextSetSessionTimeout(napi_env env, napi_callback_info info) {
  size_t argc = 1;
  napi_value argv[1] = {nullptr};
  napi_value this_arg = nullptr;
  napi_get_cb_info(env, info, &argc, argv, &this_arg, nullptr);
  SecureContextWrap* wrap = RequireSecureContext(env, this_arg);
  if (wrap == nullptr) return Undefined(env);
  napi_value call_argv[1] = {argc >= 1 ? argv[0] : Undefined(env)};
  SecureContextCallBindingMethod(env, wrap, "secureContextSetSessionTimeout", 1, call_argv);
  return Undefined(env);
}

napi_value SecureContextSetTicketKeys(napi_env env, napi_callback_info info) {
  size_t argc = 1;
  napi_value argv[1] = {nullptr};
  napi_value this_arg = nullptr;
  napi_get_cb_info(env, info, &argc, argv, &this_arg, nullptr);
  SecureContextWrap* wrap = RequireSecureContext(env, this_arg);
  if (wrap == nullptr) return Undefined(env);
  napi_value call_argv[1] = {argc >= 1 ? argv[0] : Undefined(env)};
  SecureContextCallBindingMethod(env, wrap, "secureContextSetTicketKeys", 1, call_argv);
  return Undefined(env);
}

napi_value SecureContextGetTicketKeys(napi_env env, napi_callback_info info) {
  napi_value this_arg = nullptr;
  size_t argc = 0;
  napi_get_cb_info(env, info, &argc, nullptr, &this_arg, nullptr);
  SecureContextWrap* wrap = RequireSecureContext(env, this_arg);
  if (wrap == nullptr) return Undefined(env);
  napi_value out = CallSecureContextBindingMethodReturningValue(env, wrap, "secureContextGetTicketKeys", 0, nullptr);
  return out != nullptr ? out : Undefined(env);
}

napi_value SecureContextEnableTicketKeyCallback(napi_env env, napi_callback_info info) {
  napi_value this_arg = nullptr;
  size_t argc = 0;
  napi_get_cb_info(env, info, &argc, nullptr, &this_arg, nullptr);
  SecureContextWrap* wrap = RequireSecureContext(env, this_arg);
  if (wrap == nullptr) return Undefined(env);
  SecureContextCallBindingMethod(env, wrap, "secureContextEnableTicketKeyCallback", 0, nullptr);
  return Undefined(env);
}

napi_value SecureContextSetAllowPartialTrustChain(napi_env env, napi_callback_info info) {
  napi_value this_arg = nullptr;
  size_t argc = 0;
  napi_get_cb_info(env, info, &argc, nullptr, &this_arg, nullptr);
  SecureContextWrap* wrap = RequireSecureContext(env, this_arg);
  if (wrap == nullptr) return Undefined(env);
  SecureContextCallBindingMethod(env, wrap, "secureContextSetAllowPartialTrustChain", 0, nullptr);
  return Undefined(env);
}

napi_value SecureContextGetCertificate(napi_env env, napi_callback_info info) {
  napi_value this_arg = nullptr;
  size_t argc = 0;
  napi_get_cb_info(env, info, &argc, nullptr, &this_arg, nullptr);
  SecureContextWrap* wrap = RequireSecureContext(env, this_arg);
  if (wrap == nullptr) return Undefined(env);
  napi_value out = CallSecureContextBindingMethodReturningValue(env, wrap, "secureContextGetCertificate", 0, nullptr);
  if (out != nullptr) return out;
  napi_value null_value = nullptr;
  napi_get_null(env, &null_value);
  return null_value;
}

napi_value SecureContextGetIssuer(napi_env env, napi_callback_info info) {
  napi_value this_arg = nullptr;
  size_t argc = 0;
  napi_get_cb_info(env, info, &argc, nullptr, &this_arg, nullptr);
  SecureContextWrap* wrap = RequireSecureContext(env, this_arg);
  if (wrap == nullptr) return Undefined(env);
  napi_value out = CallSecureContextBindingMethodReturningValue(env, wrap, "secureContextGetIssuer", 0, nullptr);
  if (out != nullptr) return out;
  napi_value null_value = nullptr;
  napi_get_null(env, &null_value);
  return null_value;
}

napi_value SecureContextNoop(napi_env env, napi_callback_info info) {
  napi_value this_arg = nullptr;
  size_t argc = 0;
  napi_get_cb_info(env, info, &argc, nullptr, &this_arg, nullptr);
  SecureContextWrap* wrap = RequireSecureContext(env, this_arg);
  if (wrap == nullptr) return Undefined(env);
  return Undefined(env);
}

napi_value SecureContextSetMinProto(napi_env env, napi_callback_info info) {
  size_t argc = 1;
  napi_value argv[1] = {nullptr};
  napi_value this_arg = nullptr;
  napi_get_cb_info(env, info, &argc, argv, &this_arg, nullptr);
  SecureContextWrap* wrap = RequireSecureContext(env, this_arg);
  if (wrap == nullptr) return Undefined(env);
  int32_t value = 0;
  if (argc >= 1 && argv[0] != nullptr) napi_get_value_int32(env, argv[0], &value);
  wrap->min_proto = value;
  napi_value call_argv[1] = {argc >= 1 ? argv[0] : Undefined(env)};
  SecureContextCallBindingMethod(env, wrap, "secureContextSetMinProto", 1, call_argv);
  return Undefined(env);
}

napi_value SecureContextSetMaxProto(napi_env env, napi_callback_info info) {
  size_t argc = 1;
  napi_value argv[1] = {nullptr};
  napi_value this_arg = nullptr;
  napi_get_cb_info(env, info, &argc, argv, &this_arg, nullptr);
  SecureContextWrap* wrap = RequireSecureContext(env, this_arg);
  if (wrap == nullptr) return Undefined(env);
  int32_t value = 0;
  if (argc >= 1 && argv[0] != nullptr) napi_get_value_int32(env, argv[0], &value);
  wrap->max_proto = value;
  napi_value call_argv[1] = {argc >= 1 ? argv[0] : Undefined(env)};
  SecureContextCallBindingMethod(env, wrap, "secureContextSetMaxProto", 1, call_argv);
  return Undefined(env);
}

napi_value SecureContextLoadPKCS12(napi_env env, napi_callback_info info) {
  size_t argc = 2;
  napi_value argv[2] = {nullptr, nullptr};
  napi_value this_arg = nullptr;
  napi_get_cb_info(env, info, &argc, argv, &this_arg, nullptr);
  SecureContextWrap* wrap = RequireSecureContext(env, this_arg);
  if (wrap == nullptr) return Undefined(env);

  napi_value call_argv[2] = {argc >= 1 ? argv[0] : Undefined(env), argc >= 2 ? argv[1] : Undefined(env)};
  SecureContextCallBindingMethod(env, wrap, "secureContextLoadPKCS12", argc, call_argv);
  return Undefined(env);
}

napi_value SecureContextGetMinProto(napi_env env, napi_callback_info info) {
  napi_value this_arg = nullptr;
  size_t argc = 0;
  napi_get_cb_info(env, info, &argc, nullptr, &this_arg, nullptr);
  SecureContextWrap* wrap = RequireSecureContext(env, this_arg);
  if (wrap == nullptr) return Undefined(env);
  napi_value out = CallSecureContextBindingMethodReturningValue(env, wrap, "secureContextGetMinProto", 0, nullptr);
  if (out != nullptr) return out;
  napi_create_int32(env, wrap->min_proto, &out);
  return out != nullptr ? out : Undefined(env);
}

napi_value SecureContextGetMaxProto(napi_env env, napi_callback_info info) {
  napi_value this_arg = nullptr;
  size_t argc = 0;
  napi_get_cb_info(env, info, &argc, nullptr, &this_arg, nullptr);
  SecureContextWrap* wrap = RequireSecureContext(env, this_arg);
  if (wrap == nullptr) return Undefined(env);
  napi_value out = CallSecureContextBindingMethodReturningValue(env, wrap, "secureContextGetMaxProto", 0, nullptr);
  if (out != nullptr) return out;
  napi_create_int32(env, wrap->max_proto, &out);
  return out != nullptr ? out : Undefined(env);
}

napi_value SecureContextClose(napi_env env, napi_callback_info info) {
  napi_value this_arg = nullptr;
  size_t argc = 0;
  napi_get_cb_info(env, info, &argc, nullptr, &this_arg, nullptr);
  SecureContextWrap* wrap = RequireSecureContext(env, this_arg);
  if (wrap != nullptr) ResetRef(env, &wrap->handle_ref);
  return Undefined(env);
}

struct JobWrap {
  napi_ref wrapper_ref = nullptr;
  int32_t mode = kCryptoJobSync;
  std::vector<napi_ref> args;
};

void JobFinalize(napi_env env, void* data, void* /*hint*/) {
  auto* wrap = static_cast<JobWrap*>(data);
  if (wrap == nullptr) return;
  for (auto& arg : wrap->args) ResetRef(env, &arg);
  ResetRef(env, &wrap->wrapper_ref);
  delete wrap;
}

JobWrap* UnwrapJob(napi_env env, napi_value this_arg) {
  if (this_arg == nullptr) return nullptr;
  void* data = nullptr;
  if (napi_unwrap(env, this_arg, &data) != napi_ok || data == nullptr) return nullptr;
  return static_cast<JobWrap*>(data);
}

void MaybeAttachCurrentDomain(napi_env env, napi_value target);

bool FinalizeJobCtor(napi_env env, napi_value this_arg, size_t argc, napi_value* argv) {
  if (this_arg == nullptr) return false;
  auto* wrap = new JobWrap();
  if (argc >= 1 && argv[0] != nullptr) napi_get_value_int32(env, argv[0], &wrap->mode);
  for (size_t i = 1; i < argc; ++i) {
    napi_ref ref = nullptr;
    if (argv[i] != nullptr) napi_create_reference(env, argv[i], 1, &ref);
    wrap->args.push_back(ref);
  }
  if (napi_wrap(env, this_arg, wrap, JobFinalize, nullptr, &wrap->wrapper_ref) != napi_ok) {
    JobFinalize(env, wrap, nullptr);
    return false;
  }
  if (wrap->mode == kCryptoJobAsync) {
    MaybeAttachCurrentDomain(env, this_arg);
  }
  return true;
}

napi_value JobCtor(napi_env env, napi_callback_info info) {
  size_t argc = 16;
  napi_value argv[16] = {nullptr};
  napi_value this_arg = nullptr;
  napi_get_cb_info(env, info, &argc, argv, &this_arg, nullptr);
  if (this_arg == nullptr) return nullptr;
  if (!FinalizeJobCtor(env, this_arg, argc, argv)) return nullptr;
  return this_arg;
}

napi_value PBKDF2JobCtor(napi_env env, napi_callback_info info) {
  size_t argc = 16;
  napi_value argv[16] = {nullptr};
  napi_value this_arg = nullptr;
  napi_get_cb_info(env, info, &argc, argv, &this_arg, nullptr);
  if (this_arg == nullptr) return nullptr;

  if (argc >= 6 && argv[5] != nullptr && !IsNullOrUndefinedValue(env, argv[5])) {
    napi_value binding = GetBinding(env);
    if (!IsSupportedDigestName(env, binding, argv[5])) {
      const std::string digest = GetStringValue(env, argv[5]);
      const std::string message = "Invalid digest: " + digest;
      napi_throw_type_error(env, "ERR_CRYPTO_INVALID_DIGEST", message.c_str());
      return nullptr;
    }
  }

  if (!FinalizeJobCtor(env, this_arg, argc, argv)) return nullptr;
  return this_arg;
}

napi_value CheckPrimeJobCtor(napi_env env, napi_callback_info info) {
  size_t argc = 16;
  napi_value argv[16] = {nullptr};
  napi_value this_arg = nullptr;
  napi_get_cb_info(env, info, &argc, argv, &this_arg, nullptr);
  if (this_arg == nullptr) return nullptr;

  if (argc >= 2 && argv[1] != nullptr && !IsNullOrUndefinedValue(env, argv[1])) {
    const uint8_t* candidate = nullptr;
    size_t candidate_len = 0;
    if (!GetByteSpan(env, argv[1], &candidate, &candidate_len)) {
      napi_throw_type_error(env, "ERR_INVALID_ARG_TYPE", "candidate must be an ArrayBuffer or BufferView");
      return nullptr;
    }
    ncrypto::BignumPointer bn(candidate, candidate_len);
    if (!bn) {
      const unsigned long err = ConsumePreferredOpenSslError();
      napi_throw(env, CreateOpenSslError(env, MapOpenSslErrorCode(err), err, "BignumPointer"));
      return nullptr;
    }
  }

  if (!FinalizeJobCtor(env, this_arg, argc, argv)) return nullptr;
  return this_arg;
}

napi_value ScryptJobCtor(napi_env env, napi_callback_info info) {
  size_t argc = 16;
  napi_value argv[16] = {nullptr};
  napi_value this_arg = nullptr;
  napi_get_cb_info(env, info, &argc, argv, &this_arg, nullptr);
  if (this_arg == nullptr) return nullptr;

  if (argc >= 8) {
    auto to_u64 = [&](napi_value value) -> uint64_t {
      if (value == nullptr || IsUndefined(env, value)) return 0;
      double v = 0;
      if (napi_get_value_double(env, value, &v) != napi_ok || !std::isfinite(v) || v < 0) return 0;
      return static_cast<uint64_t>(v);
    };

    const uint64_t N = to_u64(argv[3]);
    const uint64_t r = to_u64(argv[4]);
    const uint64_t p = to_u64(argv[5]);
    const uint64_t maxmem = to_u64(argv[6]);
    if (!ncrypto::checkScryptParams(N, r, p, maxmem)) {
      const uint32_t err = ERR_peek_last_error();
      if (err != 0) {
        char buf[256];
        ERR_error_string_n(err, buf, sizeof(buf));
        const std::string message = std::string("Invalid scrypt params: ") + buf;
        napi_throw_error(env, "ERR_CRYPTO_INVALID_SCRYPT_PARAMS", message.c_str());
      } else {
        napi_throw_error(env, "ERR_CRYPTO_INVALID_SCRYPT_PARAMS", "Invalid scrypt params");
      }
      return nullptr;
    }
  }

  if (!FinalizeJobCtor(env, this_arg, argc, argv)) return nullptr;
  return this_arg;
}

napi_value HKDFJobCtor(napi_env env, napi_callback_info info) {
  size_t argc = 16;
  napi_value argv[16] = {nullptr};
  napi_value this_arg = nullptr;
  napi_get_cb_info(env, info, &argc, argv, &this_arg, nullptr);
  if (this_arg == nullptr) return nullptr;

  if (argc >= 2 && argv[1] != nullptr && !IsNullOrUndefinedValue(env, argv[1])) {
    napi_value binding = GetBinding(env);
    if (!IsSupportedDigestName(env, binding, argv[1])) {
      const std::string digest = GetStringValue(env, argv[1]);
      const std::string message = "Invalid digest: " + digest;
      napi_throw_type_error(env, "ERR_CRYPTO_INVALID_DIGEST", message.c_str());
      return nullptr;
    }

    if (argc >= 6 && argv[5] != nullptr && !IsNullOrUndefinedValue(env, argv[5])) {
      uint32_t length = 0;
      if (napi_get_value_uint32(env, argv[5], &length) == napi_ok) {
        const std::string digest = GetStringValue(env, argv[1]);
        const ncrypto::Digest md = ncrypto::Digest::FromName(digest.c_str());
        if (md && !ncrypto::checkHkdfLength(md, static_cast<size_t>(length))) {
          napi_throw_range_error(env, "ERR_CRYPTO_INVALID_KEYLEN", "Invalid key length");
          return nullptr;
        }
      }
    }
  }

  if (!FinalizeJobCtor(env, this_arg, argc, argv)) return nullptr;
  return this_arg;
}

napi_value RsaKeyPairGenJobCtor(napi_env env, napi_callback_info info) {
  size_t argc = 16;
  napi_value argv[16] = {nullptr};
  napi_value this_arg = nullptr;
  napi_get_cb_info(env, info, &argc, argv, &this_arg, nullptr);
  if (this_arg == nullptr) return nullptr;

  const int32_t variant = (argc >= 2 && argv[1] != nullptr)
                              ? GetInt32Value(env, argv[1], kKeyVariantRSA_SSA_PKCS1_v1_5)
                              : kKeyVariantRSA_SSA_PKCS1_v1_5;
  if (variant == kKeyVariantRSA_PSS) {
    napi_value binding = GetBinding(env);
    if (binding != nullptr && argc >= 5 && argv[4] != nullptr && !IsNullOrUndefinedValue(env, argv[4])) {
      if (!IsSupportedDigestName(env, binding, argv[4])) {
        const std::string digest = GetStringValue(env, argv[4]);
        const std::string message = "Invalid digest: " + digest;
        napi_throw_type_error(env, "ERR_CRYPTO_INVALID_DIGEST", message.c_str());
        return nullptr;
      }
    }
    if (binding != nullptr && argc >= 6 && argv[5] != nullptr && !IsNullOrUndefinedValue(env, argv[5])) {
      if (!IsSupportedDigestName(env, binding, argv[5])) {
        const std::string digest = GetStringValue(env, argv[5]);
        const std::string message = "Invalid MGF1 digest: " + digest;
        napi_throw_type_error(env, "ERR_CRYPTO_INVALID_DIGEST", message.c_str());
        return nullptr;
      }
    }
  }

  auto* wrap = new JobWrap();
  if (argc >= 1 && argv[0] != nullptr) napi_get_value_int32(env, argv[0], &wrap->mode);
  for (size_t i = 1; i < argc; ++i) {
    napi_ref ref = nullptr;
    if (argv[i] != nullptr) napi_create_reference(env, argv[i], 1, &ref);
    wrap->args.push_back(ref);
  }
  napi_wrap(env, this_arg, wrap, JobFinalize, nullptr, &wrap->wrapper_ref);
  if (wrap->mode == kCryptoJobAsync) {
    MaybeAttachCurrentDomain(env, this_arg);
  }
  return this_arg;
}

napi_value DhKeyPairGenJobCtor(napi_env env, napi_callback_info info) {
  size_t argc = 16;
  napi_value argv[16] = {nullptr};
  napi_value this_arg = nullptr;
  napi_get_cb_info(env, info, &argc, argv, &this_arg, nullptr);
  if (this_arg == nullptr) return nullptr;

  if (argc >= 2 && argv[1] != nullptr) {
    napi_valuetype source_type = napi_undefined;
    if (napi_typeof(env, argv[1], &source_type) == napi_ok && source_type == napi_string) {
      const std::string group_name = GetStringValue(env, argv[1]);
      BIGNUM* prime = DhPrimeFromGroupName(group_name);
      if (prime == nullptr) {
        napi_throw_error(env, "ERR_CRYPTO_UNKNOWN_DH_GROUP", "Unknown DH group");
        return nullptr;
      }
      BN_free(prime);
    }
  }

  auto* wrap = new JobWrap();
  if (argc >= 1 && argv[0] != nullptr) napi_get_value_int32(env, argv[0], &wrap->mode);
  for (size_t i = 1; i < argc; ++i) {
    napi_ref ref = nullptr;
    if (argv[i] != nullptr) napi_create_reference(env, argv[i], 1, &ref);
    wrap->args.push_back(ref);
  }
  napi_wrap(env, this_arg, wrap, JobFinalize, nullptr, &wrap->wrapper_ref);
  if (wrap->mode == kCryptoJobAsync) {
    MaybeAttachCurrentDomain(env, this_arg);
  }
  return this_arg;
}

napi_value BuildJobResult(napi_env env, napi_value err, napi_value value) {
  napi_value out = nullptr;
  napi_create_array_with_length(env, 2, &out);
  if (out == nullptr) return Undefined(env);
  napi_set_element(env, out, 0, err != nullptr ? err : Undefined(env));
  napi_set_element(env, out, 1, value != nullptr ? value : Undefined(env));
  return out;
}

napi_value RunSyncCall(napi_env env, napi_value this_arg, const char* method, std::vector<napi_value> call_args) {
  napi_value binding = GetBinding(env);
  if (binding == nullptr) return BuildJobResult(env, nullptr, Undefined(env));
  for (napi_value& arg : call_args) {
    if (arg == nullptr) arg = Undefined(env);
  }
  napi_value result = nullptr;
  if (!CallBindingMethod(env, binding, method, call_args.size(), call_args.data(), &result)) {
    bool pending = false;
    if (napi_is_exception_pending(env, &pending) == napi_ok && pending) {
      napi_value err = nullptr;
      napi_get_and_clear_last_exception(env, &err);
      return BuildJobResult(env, err, Undefined(env));
    }
    return BuildJobResult(env, Undefined(env), Undefined(env));
  }
  return BuildJobResult(env, nullptr, result);
}

bool ThrowSyncJobErrorIfPresent(napi_env env, napi_value result) {
  if (result == nullptr) return false;
  napi_value err = nullptr;
  if (napi_get_element(env, result, 0, &err) != napi_ok || err == nullptr) return false;
  napi_valuetype type = napi_undefined;
  if (napi_typeof(env, err, &type) != napi_ok || type == napi_undefined || type == napi_null) {
    return false;
  }
  napi_throw(env, err);
  return true;
}

void MaybeAttachCurrentDomain(napi_env env, napi_value target) {
  if (env == nullptr || target == nullptr) return;

  napi_value global = GetGlobal(env);
  if (global == nullptr) return;

  napi_value process = nullptr;
  if (napi_get_named_property(env, global, "process", &process) != napi_ok || process == nullptr) return;

  napi_value domain = nullptr;
  if (napi_get_named_property(env, process, "domain", &domain) != napi_ok ||
      domain == nullptr ||
      IsNullOrUndefinedValue(env, domain)) {
    return;
  }

  napi_property_descriptor desc{
      "domain",
      nullptr,
      nullptr,
      nullptr,
      nullptr,
      domain,
      static_cast<napi_property_attributes>(napi_writable | napi_configurable),
      nullptr};
  (void)napi_define_properties(env, target, 1, &desc);
}

struct CryptoOnDoneTask {
  napi_env env = nullptr;
  napi_ref this_arg_ref = nullptr;
  napi_ref ondone_ref = nullptr;
  napi_ref err_ref = nullptr;
  napi_ref value_ref = nullptr;
};

void CleanupCryptoOnDoneTask(napi_env env, void* data) {
  auto* task = static_cast<CryptoOnDoneTask*>(data);
  if (task == nullptr) return;
  napi_env cleanup_env = env != nullptr ? env : task->env;
  if (cleanup_env != nullptr) {
    ResetRef(cleanup_env, &task->this_arg_ref);
    ResetRef(cleanup_env, &task->ondone_ref);
    ResetRef(cleanup_env, &task->err_ref);
    ResetRef(cleanup_env, &task->value_ref);
  }
  delete task;
}

void RunCryptoOnDoneTask(napi_env env, void* data) {
  auto* task = static_cast<CryptoOnDoneTask*>(data);
  if (task == nullptr || env == nullptr) return;
  edge::HandleScope scope(env);
  if (!scope.is_open()) return;

  napi_value this_arg = GetRefValue(env, task->this_arg_ref);
  napi_value ondone = GetRefValue(env, task->ondone_ref);
  napi_value err = GetRefValue(env, task->err_ref);
  napi_value value = GetRefValue(env, task->value_ref);
  if (this_arg == nullptr || ondone == nullptr) return;

  napi_value argv[2] = {err != nullptr ? err : Undefined(env), value != nullptr ? value : Undefined(env)};
  napi_value ignored = nullptr;
  (void)EdgeAsyncWrapMakeCallback(
      env, 0, this_arg, this_arg, ondone, 2, argv, &ignored, kEdgeMakeCallbackNone);
}

void InvokeJobOnDone(napi_env env, napi_value this_arg, napi_value result) {
  if (this_arg == nullptr || result == nullptr) return;

  napi_value ondone = nullptr;
  napi_valuetype type = napi_undefined;
  if (napi_get_named_property(env, this_arg, "ondone", &ondone) != napi_ok ||
      ondone == nullptr ||
      napi_typeof(env, ondone, &type) != napi_ok ||
      type != napi_function) {
    return;
  }

  napi_value err = nullptr;
  napi_value value = nullptr;
  if (napi_get_element(env, result, 0, &err) != napi_ok || err == nullptr) err = Undefined(env);
  if (napi_get_element(env, result, 1, &value) != napi_ok || value == nullptr) value = Undefined(env);

  auto* task = new CryptoOnDoneTask();
  task->env = env;
  if (napi_create_reference(env, this_arg, 1, &task->this_arg_ref) != napi_ok ||
      napi_create_reference(env, ondone, 1, &task->ondone_ref) != napi_ok ||
      napi_create_reference(env, err, 1, &task->err_ref) != napi_ok ||
      napi_create_reference(env, value, 1, &task->value_ref) != napi_ok ||
      EdgeRuntimePlatformEnqueueTask(
          env, RunCryptoOnDoneTask, task, CleanupCryptoOnDoneTask, kEdgeRuntimePlatformTaskRefed) != napi_ok) {
    CleanupCryptoOnDoneTask(env, task);
    napi_value argv[2] = {err, value};
    napi_value ignored = nullptr;
    (void)EdgeAsyncWrapMakeCallback(
        env, 0, this_arg, this_arg, ondone, 2, argv, &ignored, kEdgeMakeCallbackNone);
  }
}

bool ReadKeyEncodingSelection(napi_env env,
                              JobWrap* wrap,
                              size_t index,
                              KeyEncodingSelection* out,
                              std::string* error_code,
                              std::string* error_message) {
  if (wrap == nullptr || out == nullptr || error_code == nullptr || error_message == nullptr) return false;
  if (wrap->args.size() < index + 6) {
    *error_code = "ERR_CRYPTO_OPERATION_FAILED";
    *error_message = "Invalid key encoding arguments";
    return false;
  }

  napi_value pub_format_v = GetRefValue(env, wrap->args[index + 0]);
  napi_value pub_type_v = GetRefValue(env, wrap->args[index + 1]);
  napi_value priv_format_v = GetRefValue(env, wrap->args[index + 2]);
  napi_value priv_type_v = GetRefValue(env, wrap->args[index + 3]);
  napi_value cipher_v = GetRefValue(env, wrap->args[index + 4]);
  napi_value passphrase_v = GetRefValue(env, wrap->args[index + 5]);

  out->has_public_encoding = !IsNullOrUndefinedValue(env, pub_format_v);
  if (out->has_public_encoding) {
    out->public_format = GetInt32Value(env, pub_format_v, kKeyFormatPEM);
    out->public_type = GetInt32Value(env, pub_type_v, kKeyEncodingSPKI);
  }

  out->has_private_encoding = !IsNullOrUndefinedValue(env, priv_format_v);
  if (out->has_private_encoding) {
    out->private_format = GetInt32Value(env, priv_format_v, kKeyFormatPEM);
    out->private_type = GetInt32Value(env, priv_type_v, kKeyEncodingPKCS8);
    if (!IsNullOrUndefinedValue(env, cipher_v)) out->private_cipher = GetStringValue(env, cipher_v);
    if (!IsNullOrUndefinedValue(env, passphrase_v)) {
      out->private_passphrase = ValueToBytes(env, passphrase_v);
      out->private_passphrase_provided = true;
    }
  }
  return true;
}

std::vector<uint8_t> ExportPublicDerSpki(EVP_PKEY* pkey) {
  std::vector<uint8_t> out;
  BIO* bio = BIO_new(BIO_s_mem());
  if (bio == nullptr) return out;
  if (i2d_PUBKEY_bio(bio, pkey) == 1) out = BytesFromBio(bio);
  BIO_free(bio);
  return out;
}

std::vector<uint8_t> ExportEcPublicDerSpkiUncompressed(EVP_PKEY* pkey) {
  std::vector<uint8_t> out;
  if (pkey == nullptr || EVP_PKEY_base_id(pkey) != EVP_PKEY_EC) return out;

  EC_KEY* source = EVP_PKEY_get1_EC_KEY(pkey);
  if (source == nullptr) return out;

  const EC_GROUP* group = EC_KEY_get0_group(source);
  const EC_POINT* point = EC_KEY_get0_public_key(source);
  if (group == nullptr || point == nullptr) {
    EC_KEY_free(source);
    return out;
  }

  const point_conversion_form_t form = POINT_CONVERSION_UNCOMPRESSED;
  const size_t octets_len = EC_POINT_point2oct(group, point, form, nullptr, 0, nullptr);
  if (octets_len == 0) {
    EC_KEY_free(source);
    return out;
  }

  std::vector<uint8_t> octets(octets_len);
  if (EC_POINT_point2oct(group, point, form, octets.data(), octets.size(), nullptr) != octets_len) {
    EC_KEY_free(source);
    return out;
  }

  EC_KEY* exported = EC_KEY_new();
  EC_POINT* uncompressed = EC_POINT_new(group);
  if (exported == nullptr || uncompressed == nullptr || EC_KEY_set_group(exported, group) != 1 ||
      EC_POINT_oct2point(group, uncompressed, octets.data(), octets.size(), nullptr) != 1 ||
      EC_KEY_set_public_key(exported, uncompressed) != 1) {
    if (uncompressed != nullptr) EC_POINT_free(uncompressed);
    if (exported != nullptr) EC_KEY_free(exported);
    EC_KEY_free(source);
    return out;
  }

  EC_KEY_set_conv_form(exported, form);
  EVP_PKEY* normalized = EVP_PKEY_new();
  if (normalized != nullptr && EVP_PKEY_assign_EC_KEY(normalized, exported) == 1) {
    exported = nullptr;
    out = ExportPublicDerSpki(normalized);
  }

  if (normalized != nullptr) EVP_PKEY_free(normalized);
  if (exported != nullptr) EC_KEY_free(exported);
  if (uncompressed != nullptr) EC_POINT_free(uncompressed);
  EC_KEY_free(source);
  return out;
}

std::vector<uint8_t> ExportPrivateDerPkcs8(EVP_PKEY* pkey) {
  std::vector<uint8_t> out;
  BIO* bio = BIO_new(BIO_s_mem());
  if (bio == nullptr) return out;
  if (i2d_PKCS8PrivateKey_bio(bio, pkey, nullptr, nullptr, 0, nullptr, nullptr) == 1) {
    out = BytesFromBio(bio);
  }
  BIO_free(bio);
  return out;
}

napi_value CreateKeyObjectHandleValue(napi_env env, int32_t key_type, const std::vector<uint8_t>& key_data) {
  napi_value binding = GetBinding(env);
  if (binding == nullptr) return Undefined(env);
  napi_value ctor = nullptr;
  if (napi_get_named_property(env, binding, "KeyObjectHandle", &ctor) != napi_ok || ctor == nullptr) {
    return Undefined(env);
  }
  napi_valuetype ctor_type = napi_undefined;
  if (napi_typeof(env, ctor, &ctor_type) != napi_ok || ctor_type != napi_function) return Undefined(env);

  napi_value handle = nullptr;
  if (napi_new_instance(env, ctor, 0, nullptr, &handle) != napi_ok || handle == nullptr) return Undefined(env);

  napi_value init_fn = nullptr;
  if (napi_get_named_property(env, handle, "init", &init_fn) != napi_ok || init_fn == nullptr) return Undefined(env);
  napi_valuetype init_type = napi_undefined;
  if (napi_typeof(env, init_fn, &init_type) != napi_ok || init_type != napi_function) return Undefined(env);

  napi_value key_type_v = nullptr;
  napi_create_int32(env, key_type, &key_type_v);
  napi_value key_data_v = BytesToBuffer(env, key_data);
  napi_value argv[2] = {key_type_v != nullptr ? key_type_v : Undefined(env),
                        key_data_v != nullptr ? key_data_v : Undefined(env)};
  napi_value ignored = nullptr;
  napi_call_function(env, handle, init_fn, 2, argv, &ignored);
  return handle;
}

napi_value CreateNativeKeyObjectHandleValue(napi_env env, int32_t key_type, EVP_PKEY* pkey) {
  if (pkey == nullptr) return Undefined(env);
  napi_value binding = GetBinding(env);
  if (binding == nullptr) return Undefined(env);
  napi_value ctor = nullptr;
  if (napi_get_named_property(env, binding, "KeyObjectHandle", &ctor) != napi_ok || ctor == nullptr) {
    return Undefined(env);
  }
  napi_valuetype ctor_type = napi_undefined;
  if (napi_typeof(env, ctor, &ctor_type) != napi_ok || ctor_type != napi_function) return Undefined(env);

  napi_value handle = nullptr;
  if (napi_new_instance(env, ctor, 0, nullptr, &handle) != napi_ok || handle == nullptr) return Undefined(env);

  KeyObjectWrap* wrap = UnwrapKeyObject(env, handle);
  if (wrap == nullptr || EVP_PKEY_up_ref(pkey) != 1) return Undefined(env);
  wrap->key_type = key_type;
  wrap->key_data.clear();
  wrap->key_passphrase.clear();
  wrap->has_key_passphrase = false;
  wrap->native_pkey = pkey;
  return handle;
}

bool ExportRsaPublic(EVP_PKEY* pkey,
                     BIO* bio,
                     int32_t format,
                     int32_t type,
                     std::string* error_code,
                     std::string* error_message) {
  if (pkey == nullptr || bio == nullptr) return false;
  if (type == kKeyEncodingPKCS1) {
    RSA* rsa = EVP_PKEY_get1_RSA(pkey);
    if (rsa == nullptr) {
      *error_code = "ERR_CRYPTO_OPERATION_FAILED";
      *error_message = "RSA key export failed";
      return false;
    }
    const int ok = (format == kKeyFormatPEM) ? PEM_write_bio_RSAPublicKey(bio, rsa) : i2d_RSAPublicKey_bio(bio, rsa);
    RSA_free(rsa);
    if (ok != 1) {
      SetPreferredOpenSslError(error_code, error_message, "ERR_CRYPTO_OPERATION_FAILED", "RSA key export failed");
      return false;
    }
    return true;
  }
  const int ok = (format == kKeyFormatPEM) ? PEM_write_bio_PUBKEY(bio, pkey) : i2d_PUBKEY_bio(bio, pkey);
  if (ok != 1) {
    SetPreferredOpenSslError(error_code, error_message, "ERR_CRYPTO_OPERATION_FAILED", "Public key export failed");
    return false;
  }
  return true;
}

bool ExportRsaPrivate(EVP_PKEY* pkey,
                      BIO* bio,
                      int32_t format,
                      int32_t type,
                      const EVP_CIPHER* cipher,
                      const std::vector<uint8_t>& passphrase,
                      bool passphrase_provided,
                      std::string* error_code,
                      std::string* error_message) {
  if (pkey == nullptr || bio == nullptr) return false;
  unsigned char* pass_ptr = const_cast<unsigned char*>(
      passphrase_provided ? (passphrase.empty() ? reinterpret_cast<const unsigned char*>("") : passphrase.data())
                          : nullptr);
  int pass_len = passphrase_provided ? static_cast<int>(passphrase.size()) : 0;

  if (format == kKeyFormatPEM) {
    if (type == kKeyEncodingPKCS1) {
      RSA* rsa = EVP_PKEY_get1_RSA(pkey);
      if (rsa == nullptr) {
        *error_code = "ERR_CRYPTO_OPERATION_FAILED";
        *error_message = "RSA private key export failed";
        return false;
      }
      const int ok = PEM_write_bio_RSAPrivateKey(
          bio, rsa, cipher, pass_ptr, pass_len, nullptr, nullptr);
      RSA_free(rsa);
      if (ok != 1) {
        SetPreferredOpenSslError(
            error_code, error_message, "ERR_CRYPTO_OPERATION_FAILED", "RSA private key export failed");
        return false;
      }
      return true;
    }
    const int ok = PEM_write_bio_PKCS8PrivateKey(
        bio, pkey, cipher, reinterpret_cast<char*>(pass_ptr), pass_len, nullptr, nullptr);
    if (ok != 1) {
      SetPreferredOpenSslError(
          error_code, error_message, "ERR_CRYPTO_OPERATION_FAILED", "PKCS8 private key export failed");
      return false;
    }
    return true;
  }

  if (type == kKeyEncodingPKCS1) {
    if (cipher != nullptr || passphrase_provided) {
      *error_code = "ERR_CRYPTO_INCOMPATIBLE_KEY_OPTIONS";
      *error_message = "PKCS1 DER does not support encryption";
      return false;
    }
    RSA* rsa = EVP_PKEY_get1_RSA(pkey);
    if (rsa == nullptr || i2d_RSAPrivateKey_bio(bio, rsa) != 1) {
      if (rsa != nullptr) RSA_free(rsa);
      SetPreferredOpenSslError(
          error_code, error_message, "ERR_CRYPTO_OPERATION_FAILED", "RSA private key export failed");
      return false;
    }
    RSA_free(rsa);
    return true;
  }

  const int ok = i2d_PKCS8PrivateKey_bio(
      bio, pkey, cipher, reinterpret_cast<char*>(pass_ptr), pass_len, nullptr, nullptr);
  if (ok != 1) {
    SetPreferredOpenSslError(
        error_code, error_message, "ERR_CRYPTO_OPERATION_FAILED", "PKCS8 private key export failed");
    return false;
  }
  return true;
}

bool ExportEcPrivate(EVP_PKEY* pkey,
                     BIO* bio,
                     int32_t format,
                     int32_t type,
                     const EVP_CIPHER* cipher,
                     const std::vector<uint8_t>& passphrase,
                     bool passphrase_provided,
                     std::string* error_code,
                     std::string* error_message) {
  if (pkey == nullptr || bio == nullptr) return false;
  unsigned char* pass_ptr = const_cast<unsigned char*>(
      passphrase_provided ? (passphrase.empty() ? reinterpret_cast<const unsigned char*>("") : passphrase.data())
                          : nullptr);
  int pass_len = passphrase_provided ? static_cast<int>(passphrase.size()) : 0;

  if (type == kKeyEncodingSEC1) {
    EC_KEY* ec = EVP_PKEY_get1_EC_KEY(pkey);
    if (ec == nullptr) {
      *error_code = "ERR_CRYPTO_OPERATION_FAILED";
      *error_message = "EC private key export failed";
      return false;
    }
    int ok = 0;
    if (format == kKeyFormatPEM) {
      ok = PEM_write_bio_ECPrivateKey(ec == nullptr ? nullptr : bio, ec, cipher, pass_ptr, pass_len, nullptr, nullptr);
    } else {
      if (cipher != nullptr || passphrase_provided) {
        EC_KEY_free(ec);
        *error_code = "ERR_CRYPTO_INCOMPATIBLE_KEY_OPTIONS";
        *error_message = "SEC1 DER does not support encryption";
        return false;
      }
      ok = i2d_ECPrivateKey_bio(bio, ec);
    }
    EC_KEY_free(ec);
    if (ok != 1) {
      SetPreferredOpenSslError(error_code, error_message, "ERR_CRYPTO_OPERATION_FAILED", "EC private key export failed");
      return false;
    }
    return true;
  }

  if (format == kKeyFormatPEM) {
    const int ok = PEM_write_bio_PKCS8PrivateKey(
        bio, pkey, cipher, reinterpret_cast<char*>(pass_ptr), pass_len, nullptr, nullptr);
    if (ok != 1) {
      SetPreferredOpenSslError(
          error_code, error_message, "ERR_CRYPTO_OPERATION_FAILED", "PKCS8 private key export failed");
      return false;
    }
    return true;
  }

  const int ok = i2d_PKCS8PrivateKey_bio(
      bio, pkey, cipher, reinterpret_cast<char*>(pass_ptr), pass_len, nullptr, nullptr);
  if (ok != 1) {
    SetPreferredOpenSslError(
        error_code, error_message, "ERR_CRYPTO_OPERATION_FAILED", "PKCS8 private key export failed");
    return false;
  }
  return true;
}

napi_value ExportJwkPublic(napi_env env,
                           EVP_PKEY* pkey,
                           std::string* error_code,
                           std::string* error_message,
                           const std::string& curve_name_hint) {
  napi_value jwk = nullptr;
  if (napi_create_object(env, &jwk) != napi_ok || jwk == nullptr) return Undefined(env);

  const int base = AsymmetricKeyId(pkey);
  if (base == EVP_PKEY_RSA || base == EVP_PKEY_RSA_PSS) {
    BIGNUM* n = nullptr;
    BIGNUM* e = nullptr;
    if (EVP_PKEY_get_bn_param(pkey, OSSL_PKEY_PARAM_RSA_N, &n) != 1 ||
        EVP_PKEY_get_bn_param(pkey, OSSL_PKEY_PARAM_RSA_E, &e) != 1 ||
        n == nullptr || e == nullptr) {
      if (n != nullptr) BN_free(n);
      if (e != nullptr) BN_free(e);
      *error_code = "ERR_CRYPTO_OPERATION_FAILED";
      *error_message = "Failed to export RSA key as JWK";
      return nullptr;
    }
    SetObjectString(env, jwk, "kty", "RSA");
    SetObjectString(env, jwk, "n", Base64UrlEncode(BigNumToBytes(n)));
    SetObjectString(env, jwk, "e", Base64UrlEncode(BigNumToBytes(e)));
    BN_free(n);
    BN_free(e);
    return jwk;
  }

  if (base == EVP_PKEY_EC) {
    int nid = NID_undef;
    std::string curve_name = curve_name_hint;
    size_t field_bytes = 0;
    ResolveEcJwkCurve(pkey, curve_name_hint, &nid, &curve_name, &field_bytes);
    const std::string jwk_curve = JwkCurveFromNid(nid);
    if (jwk_curve.empty()) {
      *error_code = "ERR_CRYPTO_JWK_UNSUPPORTED_CURVE";
      *error_message = "Unsupported JWK EC curve: " + (curve_name.empty() ? "unknown" : curve_name) + ".";
      return nullptr;
    }

    BIGNUM* x = nullptr;
    BIGNUM* y = nullptr;
    if (EVP_PKEY_get_bn_param(pkey, OSSL_PKEY_PARAM_EC_PUB_X, &x) != 1 ||
        EVP_PKEY_get_bn_param(pkey, OSSL_PKEY_PARAM_EC_PUB_Y, &y) != 1 ||
        x == nullptr || y == nullptr) {
      if (x != nullptr) BN_free(x);
      if (y != nullptr) BN_free(y);
      *error_code = "ERR_CRYPTO_OPERATION_FAILED";
      *error_message = "Failed to export EC key as JWK";
      return nullptr;
    }
    std::vector<uint8_t> x_bytes = field_bytes == 0 ? BigNumToBytes(x) : BigNumToPaddedBytes(x, field_bytes);
    std::vector<uint8_t> y_bytes = field_bytes == 0 ? BigNumToBytes(y) : BigNumToPaddedBytes(y, field_bytes);
    SetObjectString(env, jwk, "kty", "EC");
    SetObjectString(env, jwk, "crv", jwk_curve);
    SetObjectString(env, jwk, "x", Base64UrlEncode(x_bytes));
    SetObjectString(env, jwk, "y", Base64UrlEncode(y_bytes));
    BN_free(x);
    BN_free(y);
    return jwk;
  }

  if (base == EVP_PKEY_ED25519 || base == EVP_PKEY_ED448 ||
      base == EVP_PKEY_X25519 || base == EVP_PKEY_X448) {
    size_t raw_len = 0;
    if (EVP_PKEY_get_raw_public_key(pkey, nullptr, &raw_len) != 1 || raw_len == 0) {
      *error_code = "ERR_CRYPTO_OPERATION_FAILED";
      *error_message = "Failed to export OKP key as JWK";
      return nullptr;
    }
    std::vector<uint8_t> raw(raw_len);
    if (EVP_PKEY_get_raw_public_key(pkey, raw.data(), &raw_len) != 1) {
      *error_code = "ERR_CRYPTO_OPERATION_FAILED";
      *error_message = "Failed to export OKP key as JWK";
      return nullptr;
    }
    raw.resize(raw_len);

    const char* curve_name = nullptr;
    switch (base) {
      case EVP_PKEY_ED25519:
        curve_name = "Ed25519";
        break;
      case EVP_PKEY_ED448:
        curve_name = "Ed448";
        break;
      case EVP_PKEY_X25519:
        curve_name = "X25519";
        break;
      case EVP_PKEY_X448:
        curve_name = "X448";
        break;
      default:
        break;
    }

    SetObjectString(env, jwk, "kty", "OKP");
    if (curve_name != nullptr) SetObjectString(env, jwk, "crv", curve_name);
    SetObjectString(env, jwk, "x", Base64UrlEncode(raw));
    return jwk;
  }

#if OPENSSL_WITH_PQC
  if (const char* alg = MlDsaAlgorithmName(base)) {
    size_t raw_len = 0;
    if (EVP_PKEY_get_raw_public_key(pkey, nullptr, &raw_len) != 1 || raw_len == 0) {
      *error_code = "ERR_CRYPTO_OPERATION_FAILED";
      *error_message = "Failed to export ML-DSA key as JWK";
      return nullptr;
    }
    std::vector<uint8_t> raw(raw_len);
    if (EVP_PKEY_get_raw_public_key(pkey, raw.data(), &raw_len) != 1) {
      *error_code = "ERR_CRYPTO_OPERATION_FAILED";
      *error_message = "Failed to export ML-DSA key as JWK";
      return nullptr;
    }
    raw.resize(raw_len);
    SetObjectString(env, jwk, "kty", "AKP");
    SetObjectString(env, jwk, "alg", alg);
    SetObjectString(env, jwk, "pub", Base64UrlEncode(raw));
    return jwk;
  }
#endif

  *error_code = "ERR_CRYPTO_JWK_UNSUPPORTED_KEY_TYPE";
  *error_message = "Unsupported JWK Key Type.";
  return nullptr;
}

napi_value ExportJwkPrivate(napi_env env,
                            EVP_PKEY* pkey,
                            std::string* error_code,
                            std::string* error_message,
                            const std::string& curve_name_hint) {
  napi_value jwk = ExportJwkPublic(env, pkey, error_code, error_message, curve_name_hint);
  if (jwk == nullptr) return nullptr;

  const int base = AsymmetricKeyId(pkey);
  if (base == EVP_PKEY_RSA || base == EVP_PKEY_RSA_PSS) {
    BIGNUM* d = nullptr;
    BIGNUM* p = nullptr;
    BIGNUM* q = nullptr;
    BIGNUM* dp = nullptr;
    BIGNUM* dq = nullptr;
    BIGNUM* qi = nullptr;
    bool ok = EVP_PKEY_get_bn_param(pkey, OSSL_PKEY_PARAM_RSA_D, &d) == 1 &&
              EVP_PKEY_get_bn_param(pkey, OSSL_PKEY_PARAM_RSA_FACTOR1, &p) == 1 &&
              EVP_PKEY_get_bn_param(pkey, OSSL_PKEY_PARAM_RSA_FACTOR2, &q) == 1 &&
              EVP_PKEY_get_bn_param(pkey, OSSL_PKEY_PARAM_RSA_EXPONENT1, &dp) == 1 &&
              EVP_PKEY_get_bn_param(pkey, OSSL_PKEY_PARAM_RSA_EXPONENT2, &dq) == 1 &&
              EVP_PKEY_get_bn_param(pkey, OSSL_PKEY_PARAM_RSA_COEFFICIENT1, &qi) == 1 &&
              d != nullptr && p != nullptr && q != nullptr && dp != nullptr && dq != nullptr && qi != nullptr;
    if (!ok) {
      if (d != nullptr) BN_free(d);
      if (p != nullptr) BN_free(p);
      if (q != nullptr) BN_free(q);
      if (dp != nullptr) BN_free(dp);
      if (dq != nullptr) BN_free(dq);
      if (qi != nullptr) BN_free(qi);
      *error_code = "ERR_CRYPTO_OPERATION_FAILED";
      *error_message = "Failed to export RSA private key as JWK";
      return nullptr;
    }
    SetObjectString(env, jwk, "d", Base64UrlEncode(BigNumToBytes(d)));
    SetObjectString(env, jwk, "p", Base64UrlEncode(BigNumToBytes(p)));
    SetObjectString(env, jwk, "q", Base64UrlEncode(BigNumToBytes(q)));
    SetObjectString(env, jwk, "dp", Base64UrlEncode(BigNumToBytes(dp)));
    SetObjectString(env, jwk, "dq", Base64UrlEncode(BigNumToBytes(dq)));
    SetObjectString(env, jwk, "qi", Base64UrlEncode(BigNumToBytes(qi)));
    BN_free(d);
    BN_free(p);
    BN_free(q);
    BN_free(dp);
    BN_free(dq);
    BN_free(qi);
    return jwk;
  }

  if (base == EVP_PKEY_EC) {
    BIGNUM* d = nullptr;
    if (EVP_PKEY_get_bn_param(pkey, OSSL_PKEY_PARAM_PRIV_KEY, &d) != 1 || d == nullptr) {
      if (d != nullptr) BN_free(d);
      *error_code = "ERR_CRYPTO_OPERATION_FAILED";
      *error_message = "Failed to export EC private key as JWK";
      return nullptr;
    }
    size_t field_bytes = 0;
    ResolveEcJwkCurve(pkey, curve_name_hint, nullptr, nullptr, &field_bytes);
    const std::vector<uint8_t> d_bytes = field_bytes == 0 ? BigNumToBytes(d) : BigNumToPaddedBytes(d, field_bytes);
    SetObjectString(env, jwk, "d", Base64UrlEncode(d_bytes));
    BN_free(d);
    return jwk;
  }

  if (base == EVP_PKEY_ED25519 || base == EVP_PKEY_ED448 ||
      base == EVP_PKEY_X25519 || base == EVP_PKEY_X448) {
    size_t raw_len = 0;
    if (EVP_PKEY_get_raw_private_key(pkey, nullptr, &raw_len) != 1 || raw_len == 0) {
      *error_code = "ERR_CRYPTO_OPERATION_FAILED";
      *error_message = "Failed to export OKP private key as JWK";
      return nullptr;
    }
    std::vector<uint8_t> raw(raw_len);
    if (EVP_PKEY_get_raw_private_key(pkey, raw.data(), &raw_len) != 1) {
      *error_code = "ERR_CRYPTO_OPERATION_FAILED";
      *error_message = "Failed to export OKP private key as JWK";
      return nullptr;
    }
    raw.resize(raw_len);
    SetObjectString(env, jwk, "d", Base64UrlEncode(raw));
    return jwk;
  }

#if OPENSSL_WITH_PQC
  if (MlDsaAlgorithmName(base) != nullptr) {
    std::vector<uint8_t> seed(32);
    size_t seed_len = seed.size();
    if (EVP_PKEY_get_octet_string_param(pkey, OSSL_PKEY_PARAM_ML_DSA_SEED, seed.data(), seed_len, &seed_len) != 1 ||
        seed_len == 0) {
      *error_code = "ERR_CRYPTO_OPERATION_FAILED";
      *error_message = "key does not have an available seed";
      return nullptr;
    }
    seed.resize(seed_len);
    SetObjectString(env, jwk, "priv", Base64UrlEncode(seed));
    return jwk;
  }
#endif

  *error_code = "ERR_CRYPTO_JWK_UNSUPPORTED_KEY_TYPE";
  *error_message = "Unsupported JWK Key Type.";
  return nullptr;
}

bool ExportPublicKeyValue(napi_env env,
                          EVP_PKEY* pkey,
                          const KeyEncodingSelection& encoding,
                          const std::string& curve_name_hint,
                          napi_value* out_value,
                          std::string* error_code,
                          std::string* error_message) {
  if (!encoding.has_public_encoding) {
    *out_value = CreateNativeKeyObjectHandleValue(env, kKeyTypePublic, pkey);
    if (*out_value == nullptr || IsUndefined(env, *out_value)) {
      *error_code = "ERR_CRYPTO_OPERATION_FAILED";
      *error_message = "Failed to create public key object";
      return false;
    }
    return true;
  }

  if (encoding.public_format == kKeyFormatJWK) {
    *out_value = ExportJwkPublic(env, pkey, error_code, error_message, curve_name_hint);
    return *out_value != nullptr;
  }

  BIO* bio = BIO_new(BIO_s_mem());
  if (bio == nullptr) {
    *error_code = "ERR_CRYPTO_OPERATION_FAILED";
    *error_message = "Failed to allocate BIO";
    return false;
  }
  const bool ok = ExportRsaPublic(pkey, bio, encoding.public_format, encoding.public_type, error_code, error_message);
  if (!ok) {
    BIO_free(bio);
    return false;
  }
  const std::vector<uint8_t> bytes = BytesFromBio(bio);
  BIO_free(bio);
  if (encoding.public_format == kKeyFormatPEM) {
    std::string text(bytes.begin(), bytes.end());
    napi_create_string_utf8(env, text.c_str(), text.size(), out_value);
  } else {
    *out_value = BytesToBuffer(env, bytes);
  }
  return *out_value != nullptr && !IsUndefined(env, *out_value);
}

bool ExportPrivateKeyValue(napi_env env,
                           EVP_PKEY* pkey,
                           const KeyEncodingSelection& encoding,
                           const std::string& curve_name_hint,
                           napi_value* out_value,
                           std::string* error_code,
                           std::string* error_message) {
  if (!encoding.has_private_encoding) {
    *out_value = CreateNativeKeyObjectHandleValue(env, kKeyTypePrivate, pkey);
    if (*out_value == nullptr || IsUndefined(env, *out_value)) {
      *error_code = "ERR_CRYPTO_OPERATION_FAILED";
      *error_message = "Failed to create private key object";
      return false;
    }
    return true;
  }

  if (encoding.private_format == kKeyFormatJWK) {
    *out_value = ExportJwkPrivate(env, pkey, error_code, error_message, curve_name_hint);
    return *out_value != nullptr;
  }

  const EVP_CIPHER* cipher = nullptr;
  if (!encoding.private_cipher.empty()) {
    cipher = EVP_get_cipherbyname(encoding.private_cipher.c_str());
    if (cipher == nullptr) {
      *error_code = "ERR_CRYPTO_UNKNOWN_CIPHER";
      *error_message = "Unknown cipher";
      return false;
    }
  }

  BIO* bio = BIO_new(BIO_s_mem());
  if (bio == nullptr) {
    *error_code = "ERR_CRYPTO_OPERATION_FAILED";
    *error_message = "Failed to allocate BIO";
    return false;
  }

  const int base = EVP_PKEY_base_id(pkey);
  bool ok = false;
  if (base == EVP_PKEY_RSA || base == EVP_PKEY_RSA_PSS) {
    ok = ExportRsaPrivate(pkey,
                          bio,
                          encoding.private_format,
                          encoding.private_type,
                          cipher,
                          encoding.private_passphrase,
                          encoding.private_passphrase_provided,
                          error_code,
                          error_message);
  } else if (base == EVP_PKEY_EC) {
    ok = ExportEcPrivate(pkey,
                         bio,
                         encoding.private_format,
                         encoding.private_type,
                         cipher,
                         encoding.private_passphrase,
                         encoding.private_passphrase_provided,
                         error_code,
                         error_message);
  } else {
    if (encoding.private_format == kKeyFormatPEM) {
      unsigned char* pass_ptr = const_cast<unsigned char*>(
          encoding.private_passphrase_provided
              ? (encoding.private_passphrase.empty() ? reinterpret_cast<const unsigned char*>("")
                                                     : encoding.private_passphrase.data())
              : nullptr);
      int pass_len = encoding.private_passphrase_provided ? static_cast<int>(encoding.private_passphrase.size()) : 0;
      ok = PEM_write_bio_PrivateKey(
               bio, pkey, cipher, pass_ptr, pass_len, nullptr, nullptr) == 1;
    } else {
      unsigned char* pass_ptr = const_cast<unsigned char*>(
          encoding.private_passphrase_provided
              ? (encoding.private_passphrase.empty() ? reinterpret_cast<const unsigned char*>("")
                                                     : encoding.private_passphrase.data())
              : nullptr);
      int pass_len = encoding.private_passphrase_provided ? static_cast<int>(encoding.private_passphrase.size()) : 0;
      ok = i2d_PKCS8PrivateKey_bio(
               bio, pkey, cipher, reinterpret_cast<char*>(pass_ptr), pass_len, nullptr, nullptr) == 1;
    }
    if (!ok) {
      SetPreferredOpenSslError(
          error_code, error_message, "ERR_CRYPTO_OPERATION_FAILED", "Private key export failed");
    }
  }

  if (!ok) {
    BIO_free(bio);
    return false;
  }

  const std::vector<uint8_t> bytes = BytesFromBio(bio);
  BIO_free(bio);
  if (encoding.private_format == kKeyFormatPEM) {
    std::string text(bytes.begin(), bytes.end());
    napi_create_string_utf8(env, text.c_str(), text.size(), out_value);
  } else {
    *out_value = BytesToBuffer(env, bytes);
  }
  return *out_value != nullptr && !IsUndefined(env, *out_value);
}

EVP_PKEY* GenerateRsaKeyPairNative(int32_t variant,
                                   int32_t modulus_length,
                                   uint32_t public_exponent,
                                   const std::string& hash_algorithm,
                                   const std::string& mgf1_hash_algorithm,
                                   int32_t salt_length,
                                   std::string* error_code,
                                   std::string* error_message) {
  if (modulus_length <= 0) {
    *error_code = "ERR_INVALID_ARG_VALUE";
    *error_message = "Invalid modulusLength";
    return nullptr;
  }
  const bool rsa_pss = variant == kKeyVariantRSA_PSS;
  EVP_PKEY_CTX* ctx = EVP_PKEY_CTX_new_id(rsa_pss ? EVP_PKEY_RSA_PSS : EVP_PKEY_RSA, nullptr);
  if (ctx == nullptr) {
    *error_code = "ERR_CRYPTO_OPERATION_FAILED";
    *error_message = "Failed to allocate RSA keygen context";
    return nullptr;
  }
  if (EVP_PKEY_keygen_init(ctx) != 1 ||
      EVP_PKEY_CTX_set_rsa_keygen_bits(ctx, modulus_length) != 1) {
    EVP_PKEY_CTX_free(ctx);
    *error_code = "ERR_CRYPTO_OPERATION_FAILED";
    *error_message = "RSA key generation initialization failed";
    return nullptr;
  }

  BIGNUM* exp = BN_new();
  if (exp == nullptr || BN_set_word(exp, public_exponent) != 1 ||
      EVP_PKEY_CTX_set_rsa_keygen_pubexp(ctx, exp) != 1) {
    const unsigned long err = ERR_get_error();
    if (exp != nullptr) BN_free(exp);
    EVP_PKEY_CTX_free(ctx);
    *error_code = "ERR_CRYPTO_OPERATION_FAILED";
    const char* reason = err == 0 ? nullptr : ERR_reason_error_string(err);
    *error_message = (reason != nullptr && reason[0] != '\0') ? reason : "RSA public exponent configuration failed";
    return nullptr;
  }
  // Ownership transferred on success.
  exp = nullptr;

  if (rsa_pss) {
    const EVP_MD* hash_md = nullptr;
    const EVP_MD* mgf1_md = nullptr;
    if (!hash_algorithm.empty()) {
      hash_md = EVP_get_digestbyname(hash_algorithm.c_str());
      if (hash_md == nullptr || EVP_PKEY_CTX_set_rsa_pss_keygen_md(ctx, hash_md) != 1) {
        EVP_PKEY_CTX_free(ctx);
        *error_code = "ERR_CRYPTO_INVALID_DIGEST";
        *error_message = "Invalid digest";
        return nullptr;
      }
    }
    std::string mgf1_name = mgf1_hash_algorithm;
    if (mgf1_name.empty() && hash_md != nullptr) mgf1_name = hash_algorithm;
    if (!mgf1_name.empty()) {
      mgf1_md = EVP_get_digestbyname(mgf1_name.c_str());
      if (mgf1_md == nullptr || EVP_PKEY_CTX_set_rsa_pss_keygen_mgf1_md(ctx, mgf1_md) != 1) {
        EVP_PKEY_CTX_free(ctx);
        *error_code = "ERR_CRYPTO_INVALID_DIGEST";
        *error_message = "Invalid digest";
        return nullptr;
      }
    }
    int32_t effective_salt_length = salt_length;
    if (effective_salt_length < 0 && hash_md != nullptr) {
      const int digest_size = EVP_MD_get_size(hash_md);
      if (digest_size > 0) effective_salt_length = digest_size;
    }
    if (effective_salt_length >= 0 &&
        EVP_PKEY_CTX_set_rsa_pss_keygen_saltlen(ctx, effective_salt_length) != 1) {
      EVP_PKEY_CTX_free(ctx);
      *error_code = "ERR_CRYPTO_OPERATION_FAILED";
      *error_message = "RSA-PSS salt length configuration failed";
      return nullptr;
    }
  }

  EVP_PKEY* pkey = nullptr;
  if (EVP_PKEY_keygen(ctx, &pkey) != 1 || pkey == nullptr) {
    const unsigned long err = ERR_get_error();
    EVP_PKEY_CTX_free(ctx);
    *error_code = "ERR_CRYPTO_OPERATION_FAILED";
    const char* reason = err == 0 ? nullptr : ERR_reason_error_string(err);
    *error_message = (reason != nullptr && reason[0] != '\0') ? reason : "RSA key generation failed";
    return nullptr;
  }
  EVP_PKEY_CTX_free(ctx);
  return pkey;
}

EVP_PKEY* GenerateDsaKeyPairNative(int32_t modulus_length,
                                   int32_t divisor_length,
                                   std::string* error_code,
                                   std::string* error_message) {
  EVP_PKEY_CTX* param_ctx = EVP_PKEY_CTX_new_id(EVP_PKEY_DSA, nullptr);
  if (param_ctx == nullptr) {
    *error_code = "ERR_CRYPTO_OPERATION_FAILED";
    *error_message = "Failed to allocate DSA paramgen context";
    return nullptr;
  }
  if (EVP_PKEY_paramgen_init(param_ctx) != 1 ||
      EVP_PKEY_CTX_set_dsa_paramgen_bits(param_ctx, modulus_length) != 1) {
    EVP_PKEY_CTX_free(param_ctx);
    *error_code = "ERR_CRYPTO_OPERATION_FAILED";
    *error_message = "DSA parameter generation initialization failed";
    return nullptr;
  }
  if (divisor_length > 0 && EVP_PKEY_CTX_set_dsa_paramgen_q_bits(param_ctx, divisor_length) != 1) {
    EVP_PKEY_CTX_free(param_ctx);
    *error_code = "ERR_CRYPTO_OPERATION_FAILED";
    *error_message = "DSA divisor length configuration failed";
    return nullptr;
  }

  EVP_PKEY* params = nullptr;
  if (EVP_PKEY_paramgen(param_ctx, &params) != 1 || params == nullptr) {
    EVP_PKEY_CTX_free(param_ctx);
    *error_code = "ERR_CRYPTO_OPERATION_FAILED";
    *error_message = "DSA parameter generation failed";
    return nullptr;
  }
  EVP_PKEY_CTX_free(param_ctx);

  EVP_PKEY_CTX* key_ctx = EVP_PKEY_CTX_new(params, nullptr);
  if (key_ctx == nullptr || EVP_PKEY_keygen_init(key_ctx) != 1) {
    if (key_ctx != nullptr) EVP_PKEY_CTX_free(key_ctx);
    EVP_PKEY_free(params);
    *error_code = "ERR_CRYPTO_OPERATION_FAILED";
    *error_message = "DSA key generation initialization failed";
    return nullptr;
  }
  EVP_PKEY* pkey = nullptr;
  if (EVP_PKEY_keygen(key_ctx, &pkey) != 1 || pkey == nullptr) {
    EVP_PKEY_CTX_free(key_ctx);
    EVP_PKEY_free(params);
    *error_code = "ERR_CRYPTO_OPERATION_FAILED";
    *error_message = "DSA key generation failed";
    return nullptr;
  }
  EVP_PKEY_CTX_free(key_ctx);
  EVP_PKEY_free(params);
  return pkey;
}

EVP_PKEY* WrapDhAsPkey(DH* dh, std::string* error_code, std::string* error_message) {
  if (dh == nullptr) {
    *error_code = "ERR_CRYPTO_OPERATION_FAILED";
    *error_message = "DH initialization failed";
    return nullptr;
  }
  if (DH_generate_key(dh) != 1) {
    DH_free(dh);
    *error_code = "ERR_CRYPTO_OPERATION_FAILED";
    *error_message = "DH key generation failed";
    return nullptr;
  }
  EVP_PKEY* pkey = EVP_PKEY_new();
  if (pkey == nullptr || EVP_PKEY_assign_DH(pkey, dh) != 1) {
    if (pkey != nullptr) EVP_PKEY_free(pkey);
    DH_free(dh);
    *error_code = "ERR_CRYPTO_OPERATION_FAILED";
    *error_message = "DH key wrapping failed";
    return nullptr;
  }
  return pkey;
}

EVP_PKEY* GenerateDhKeyPairFromGroupNative(const std::string& group_name,
                                           std::string* error_code,
                                           std::string* error_message) {
  int32_t verify_error = 0;
  DH* dh = CreateDhFromGroupName(group_name, &verify_error);
  if (dh == nullptr) {
    *error_code = "ERR_CRYPTO_UNKNOWN_DH_GROUP";
    *error_message = "Unknown DH group";
    return nullptr;
  }
  return WrapDhAsPkey(dh, error_code, error_message);
}

EVP_PKEY* GenerateDhKeyPairFromPrimeNative(const std::vector<uint8_t>& prime,
                                           int32_t generator,
                                           std::string* error_code,
                                           std::string* error_message) {
  int32_t verify_error = 0;
  DH* dh = CreateDhFromPrimeAndGenerator(prime, generator, {}, false, &verify_error);
  if (dh == nullptr) {
    *error_code = "ERR_CRYPTO_OPERATION_FAILED";
    *error_message = "DH parameter initialization failed";
    return nullptr;
  }
  return WrapDhAsPkey(dh, error_code, error_message);
}

EVP_PKEY* GenerateDhKeyPairFromPrimeLengthNative(int32_t prime_length,
                                                 int32_t generator,
                                                 std::string* error_code,
                                                 std::string* error_message) {
  int32_t verify_error = 0;
  DH* dh = CreateDhFromSize(prime_length, generator, {}, false, &verify_error);
  if (dh == nullptr) {
    *error_code = "ERR_CRYPTO_OPERATION_FAILED";
    *error_message = "DH parameter generation failed";
    return nullptr;
  }
  return WrapDhAsPkey(dh, error_code, error_message);
}

EVP_PKEY* GenerateEcKeyPairNative(const std::string& named_curve,
                                  int32_t param_encoding,
                                  std::string* error_code,
                                  std::string* error_message) {
  const int nid = CurveNidFromName(named_curve);
  if (nid == NID_undef) {
    *error_code = "ERR_CRYPTO_INVALID_CURVE";
    *error_message = "Invalid EC curve name";
    return nullptr;
  }
  EVP_PKEY_CTX* param_ctx = EVP_PKEY_CTX_new_id(EVP_PKEY_EC, nullptr);
  if (param_ctx == nullptr ||
      EVP_PKEY_paramgen_init(param_ctx) != 1 ||
      EVP_PKEY_CTX_set_ec_paramgen_curve_nid(param_ctx, nid) != 1 ||
      EVP_PKEY_CTX_set_ec_param_enc(param_ctx, param_encoding) != 1) {
    if (param_ctx != nullptr) EVP_PKEY_CTX_free(param_ctx);
    *error_code = "ERR_CRYPTO_OPERATION_FAILED";
    *error_message = "EC key generation initialization failed";
    return nullptr;
  }

  EVP_PKEY* params = nullptr;
  if (EVP_PKEY_paramgen(param_ctx, &params) != 1 || params == nullptr) {
    EVP_PKEY_CTX_free(param_ctx);
    *error_code = "ERR_CRYPTO_OPERATION_FAILED";
    *error_message = "EC parameter generation failed";
    return nullptr;
  }
  EVP_PKEY_CTX_free(param_ctx);

  EVP_PKEY_CTX* key_ctx = EVP_PKEY_CTX_new(params, nullptr);
  if (key_ctx == nullptr || EVP_PKEY_keygen_init(key_ctx) != 1) {
    if (key_ctx != nullptr) EVP_PKEY_CTX_free(key_ctx);
    EVP_PKEY_free(params);
    *error_code = "ERR_CRYPTO_OPERATION_FAILED";
    *error_message = "EC key generation initialization failed";
    return nullptr;
  }

  EVP_PKEY* pkey = nullptr;
  if (EVP_PKEY_keygen(key_ctx, &pkey) != 1 || pkey == nullptr) {
    EVP_PKEY_CTX_free(key_ctx);
    EVP_PKEY_free(params);
  } else {
    EVP_PKEY_CTX_free(key_ctx);
    EVP_PKEY_free(params);
    return pkey;
  }

  // Some legacy-but-valid curves still fail through OpenSSL's provider-based
  // EVP keygen path with "missing OID", while Node successfully generates
  // them. Fall back to direct EC_KEY generation in that case and preserve the
  // requested parameter encoding on the key.
  EC_KEY* ec = EC_KEY_new_by_curve_name(nid);
  if (ec == nullptr) {
    *error_code = "ERR_CRYPTO_OPERATION_FAILED";
    *error_message = "EC key generation failed";
    return nullptr;
  }
  EC_KEY_set_asn1_flag(ec, param_encoding);
  if (EC_KEY_generate_key(ec) != 1 || EC_KEY_check_key(ec) != 1) {
    EC_KEY_free(ec);
    *error_code = "ERR_CRYPTO_OPERATION_FAILED";
    *error_message = "EC key generation failed";
    return nullptr;
  }

  pkey = EVP_PKEY_new();
  if (pkey == nullptr || EVP_PKEY_assign_EC_KEY(pkey, ec) != 1) {
    if (pkey != nullptr) EVP_PKEY_free(pkey);
    else EC_KEY_free(ec);
    *error_code = "ERR_CRYPTO_OPERATION_FAILED";
    *error_message = "EC key generation failed";
    return nullptr;
  }
  return pkey;
}

EVP_PKEY* GenerateNidKeyPairNative(int32_t nid, std::string* error_code, std::string* error_message) {
  EVP_PKEY_CTX* ctx = EVP_PKEY_CTX_new_id(nid, nullptr);
  if (ctx == nullptr || EVP_PKEY_keygen_init(ctx) != 1) {
    if (ctx != nullptr) EVP_PKEY_CTX_free(ctx);
    *error_code = "ERR_CRYPTO_OPERATION_FAILED";
    *error_message = "Unsupported key type";
    return nullptr;
  }
  EVP_PKEY* pkey = nullptr;
  if (EVP_PKEY_keygen(ctx, &pkey) != 1 || pkey == nullptr) {
    EVP_PKEY_CTX_free(ctx);
    *error_code = "ERR_CRYPTO_OPERATION_FAILED";
    *error_message = "Key generation failed";
    return nullptr;
  }
  EVP_PKEY_CTX_free(ctx);
  return pkey;
}

napi_value BuildGeneratedKeyPairResult(napi_env env,
                                       EVP_PKEY* pkey,
                                       const KeyEncodingSelection& encoding,
                                       const std::string& curve_name_hint) {
  std::string error_code;
  std::string error_message;
  napi_value public_key = nullptr;
  napi_value private_key = nullptr;
  if (!ExportPublicKeyValue(env, pkey, encoding, curve_name_hint, &public_key, &error_code, &error_message)) {
    if (error_code.empty()) error_code = "ERR_CRYPTO_OPERATION_FAILED";
    if (error_message.empty()) error_message = "Public key export failed";
    return BuildJobErrorResult(env, error_code.c_str(), error_message);
  }
  if (!ExportPrivateKeyValue(env, pkey, encoding, curve_name_hint, &private_key, &error_code, &error_message)) {
    if (error_code.empty()) error_code = "ERR_CRYPTO_OPERATION_FAILED";
    if (error_message.empty()) error_message = "Private key export failed";
    return BuildJobErrorResult(env, error_code.c_str(), error_message);
  }

  napi_value keys = nullptr;
  napi_create_array_with_length(env, 2, &keys);
  if (keys == nullptr) return BuildJobErrorResult(env, "ERR_CRYPTO_OPERATION_FAILED", "Failed to allocate keys");
  napi_set_element(env, keys, 0, public_key != nullptr ? public_key : Undefined(env));
  napi_set_element(env, keys, 1, private_key != nullptr ? private_key : Undefined(env));
  return BuildJobResult(env, nullptr, keys);
}

bool GetSecretKeyFromHandle(napi_env env,
                            napi_value handle,
                            std::vector<uint8_t>* out_key,
                            std::string* error_code,
                            std::string* error_message) {
  if (out_key == nullptr || error_code == nullptr || error_message == nullptr) return false;
  KeyObjectWrap* wrap = UnwrapKeyObject(env, handle);
  if (wrap == nullptr) {
    *error_code = "ERR_INVALID_ARG_TYPE";
    *error_message = "Invalid key handle";
    return false;
  }
  if (wrap->key_type != kKeyTypeSecret) {
    *error_code = "ERR_CRYPTO_INVALID_KEYTYPE";
    *error_message = "Invalid key type";
    return false;
  }
  *out_key = wrap->key_data;
  return true;
}

EVP_PKEY* GetAsymmetricKeyFromValue(napi_env env,
                                    napi_value key_value,
                                    napi_value key_passphrase_value,
                                    bool require_private,
                                    std::string* error_code,
                                    std::string* error_message) {
  if (error_code == nullptr || error_message == nullptr) return nullptr;
  if (KeyObjectWrap* wrap = UnwrapKeyObject(env, key_value)) {
    if (require_private && wrap->key_type != kKeyTypePrivate) {
      *error_code = "ERR_CRYPTO_INVALID_KEYTYPE";
      *error_message = "Invalid key type";
      return nullptr;
    }
    EVP_PKEY* pkey = ParseKeyObjectAsymmetricKey(wrap);
    if (pkey == nullptr) {
      SetPreferredOpenSslKeyParseError(
          error_code, error_message, require_private, "ERR_CRYPTO_OPERATION_FAILED", "Failed to parse key");
    }
    return pkey;
  }

  const std::vector<uint8_t> key_bytes = ValueToBytes(env, key_value);
  if (key_bytes.empty()) {
    *error_code = "ERR_INVALID_ARG_TYPE";
    *error_message = "Invalid key";
    return nullptr;
  }

  const bool has_passphrase = !IsNullOrUndefinedValue(env, key_passphrase_value);
  const std::vector<uint8_t> passphrase = has_passphrase ? ValueToBytes(env, key_passphrase_value)
                                                         : std::vector<uint8_t>{};
  EVP_PKEY* pkey = nullptr;
  if (require_private) {
    pkey = ParsePrivateKeyBytesWithPassphrase(
        key_bytes.data(), key_bytes.size(), passphrase.data(), passphrase.size(), has_passphrase);
  } else {
    pkey = ParseAnyKeyBytes(key_bytes, passphrase, has_passphrase);
  }
  if (pkey == nullptr) {
    SetPreferredOpenSslKeyParseError(
        error_code, error_message, require_private, "ERR_CRYPTO_OPERATION_FAILED", "Failed to parse key");
  }
  return pkey;
}

bool CaptureCallFailureAsJobResult(napi_env env,
                                   const char* fallback_code,
                                   const char* fallback_message,
                                   napi_value* out_result) {
  if (out_result == nullptr) return false;
  bool pending = false;
  if (napi_is_exception_pending(env, &pending) == napi_ok && pending) {
    napi_value err = nullptr;
    napi_get_and_clear_last_exception(env, &err);
    *out_result = BuildJobResult(env, err, Undefined(env));
    return true;
  }
  *out_result = BuildJobErrorResult(env, fallback_code, fallback_message);
  return true;
}

std::string AesAlgorithmNameFromVariant(int32_t variant) {
  switch (variant) {
    case 0:
      return "aes-128-ctr";
    case 1:
      return "aes-192-ctr";
    case 2:
      return "aes-256-ctr";
    case 3:
      return "aes-128-cbc";
    case 4:
      return "aes-192-cbc";
    case 5:
      return "aes-256-cbc";
    case 6:
      return "aes-128-gcm";
    case 7:
      return "aes-192-gcm";
    case 8:
      return "aes-256-gcm";
    case 9:
      return "id-aes128-wrap";
    case 10:
      return "id-aes192-wrap";
    case 11:
      return "id-aes256-wrap";
    case 12:
      return "aes-128-ocb";
    case 13:
      return "aes-192-ocb";
    case 14:
      return "aes-256-ocb";
    default:
      return "";
  }
}

bool IsAesCtrVariant(int32_t variant) {
  return variant >= 0 && variant <= 2;
}

bool IsAesWrapVariant(int32_t variant) {
  return variant >= 9 && variant <= 11;
}

bool IsAesAeadVariant(int32_t variant) {
  return (variant >= 6 && variant <= 8) || (variant >= 12 && variant <= 14);
}

ncrypto::Cipher AesCtrCipherFromVariant(int32_t variant) {
  switch (variant) {
    case 0:
      return ncrypto::Cipher::AES_128_CTR;
    case 1:
      return ncrypto::Cipher::AES_192_CTR;
    case 2:
      return ncrypto::Cipher::AES_256_CTR;
    default:
      return ncrypto::Cipher();
  }
}

template <typename T>
T AesCeilDiv(T a, T b) {
  return a == 0 ? 0 : 1 + (a - 1) / b;
}

ncrypto::BignumPointer GetAesCtrCounter(const std::vector<uint8_t>& counter_block, uint32_t length_bits) {
  const unsigned int remainder = (length_bits % CHAR_BIT);
  const unsigned char* data = counter_block.data();
  if (remainder == 0) {
    const unsigned int byte_length = length_bits / CHAR_BIT;
    return ncrypto::BignumPointer(data + counter_block.size() - byte_length, byte_length);
  }

  const unsigned int byte_length = AesCeilDiv(length_bits, static_cast<uint32_t>(CHAR_BIT));
  std::vector<unsigned char> counter(data + counter_block.size() - byte_length, data + counter_block.size());
  counter[0] &= ~(0xFF << remainder);
  return ncrypto::BignumPointer(counter.data(), counter.size());
}

std::vector<unsigned char> GetAesCtrWrappedCounterBlock(const std::vector<uint8_t>& counter_block,
                                                        uint32_t length_bits) {
  const unsigned int length_bytes = length_bits / CHAR_BIT;
  const unsigned int remainder = length_bits % CHAR_BIT;
  std::vector<unsigned char> next(counter_block.begin(), counter_block.end());
  const size_t index = next.size() - length_bytes;
  std::memset(next.data() + index, 0, length_bytes);
  if (remainder) next[index - 1] &= 0xFF << remainder;
  return next;
}

bool RunAesCtrChunk(const std::vector<uint8_t>& key,
                    int32_t variant,
                    int32_t cipher_mode,
                    const uint8_t* counter,
                    const uint8_t* input,
                    size_t input_len,
                    uint8_t* output) {
  const ncrypto::Cipher cipher = AesCtrCipherFromVariant(variant);
  if (!cipher) return false;
  auto ctx = ncrypto::CipherCtxPointer::New();
  if (!ctx) return false;
  const bool encrypt = cipher_mode == 0;
  if (!ctx.init(cipher, encrypt, key.data(), counter)) return false;

  int out_len = 0;
  int final_len = 0;
  const ncrypto::Buffer<const unsigned char> in_buffer{input, input_len};
  if (input_len > 0 && !ctx.update(in_buffer, output, &out_len)) return false;
  if (!ctx.update({}, output + out_len, &final_len, true)) return false;
  return static_cast<size_t>(out_len + final_len) == input_len;
}

bool RunAesCtrCipher(const std::vector<uint8_t>& key,
                     int32_t variant,
                     int32_t cipher_mode,
                     const std::vector<uint8_t>& counter_block,
                     uint32_t length_bits,
                     const std::vector<uint8_t>& input,
                     std::vector<uint8_t>* output) {
  if (output == nullptr || counter_block.size() != 16 || length_bits == 0 || length_bits > 128) return false;
  auto num_counters = ncrypto::BignumPointer::NewLShift(length_bits);
  if (!num_counters) return false;
  auto current_counter = GetAesCtrCounter(counter_block, length_bits);
  if (!current_counter) return false;
  auto num_output = ncrypto::BignumPointer::New();
  if (!num_output || !num_output.setWord(AesCeilDiv(input.size(), static_cast<size_t>(16)))) return false;
  if (num_output > num_counters) return false;
  auto remaining_until_reset = ncrypto::BignumPointer::NewSub(num_counters, current_counter);
  if (!remaining_until_reset) return false;

  output->assign(input.size(), 0);
  if (remaining_until_reset >= num_output) {
    return RunAesCtrChunk(
        key, variant, cipher_mode, counter_block.data(), input.data(), input.size(), output->data());
  }

  const BN_ULONG input_size_part1 = remaining_until_reset.getWord() * 16;
  if (!RunAesCtrChunk(
          key, variant, cipher_mode, counter_block.data(), input.data(), input_size_part1, output->data())) {
    return false;
  }
  const std::vector<unsigned char> wrapped_counter = GetAesCtrWrappedCounterBlock(counter_block, length_bits);
  return RunAesCtrChunk(key,
                        variant,
                        cipher_mode,
                        wrapped_counter.data(),
                        input.data() + input_size_part1,
                        input.size() - input_size_part1,
                        output->data() + input_size_part1);
}

napi_value BuildArrayBufferJobResult(napi_env env, napi_value value) {
  return BuildJobResult(env, nullptr, CopyAsArrayBuffer(env, value));
}

napi_value BuildArrayBufferJobResult(napi_env env, const std::vector<uint8_t>& value) {
  return BuildJobResult(env, nullptr, CreateArrayBufferCopy(env, value));
}

napi_value FinalizeJobRunResult(napi_env env, napi_value this_arg, JobWrap* wrap, napi_value result) {
  if (wrap != nullptr && wrap->mode == kCryptoJobAsync) {
    InvokeJobOnDone(env, this_arg, result);
    return Undefined(env);
  }
  return result;
}

napi_value RsaKeyPairGenJobRun(napi_env env, napi_callback_info info) {
  napi_value this_arg = nullptr;
  size_t argc = 0;
  napi_get_cb_info(env, info, &argc, nullptr, &this_arg, nullptr);
  JobWrap* wrap = UnwrapJob(env, this_arg);
  if (wrap == nullptr) {
    return BuildJobErrorResult(env, "ERR_CRYPTO_OPERATION_FAILED", "Invalid RSA keygen arguments");
  }
  if (wrap->args.size() < 9) {
    napi_value result = BuildJobErrorResult(env, "ERR_CRYPTO_OPERATION_FAILED", "Invalid RSA keygen arguments");
    return FinalizeJobRunResult(env, this_arg, wrap, result);
  }

  const int32_t variant = GetInt32Value(env, GetRefValue(env, wrap->args[0]), kKeyVariantRSA_SSA_PKCS1_v1_5);
  const int32_t modulus_length = GetInt32Value(env, GetRefValue(env, wrap->args[1]), 0);
  const uint32_t public_exponent = static_cast<uint32_t>(GetInt32Value(env, GetRefValue(env, wrap->args[2]), 0x10001));

  std::string hash_algorithm;
  std::string mgf1_hash_algorithm;
  int32_t salt_length = -1;
  size_t encoding_index = 3;
  if (variant == kKeyVariantRSA_PSS && wrap->args.size() >= 12) {
    hash_algorithm = GetStringValue(env, GetRefValue(env, wrap->args[3]));
    mgf1_hash_algorithm = GetStringValue(env, GetRefValue(env, wrap->args[4]));
    salt_length = IsNullOrUndefinedValue(env, GetRefValue(env, wrap->args[5]))
                      ? -1
                      : GetInt32Value(env, GetRefValue(env, wrap->args[5]), -1);
    encoding_index = 6;
  }

  KeyEncodingSelection encoding;
  std::string error_code;
  std::string error_message;
  if (!ReadKeyEncodingSelection(env, wrap, encoding_index, &encoding, &error_code, &error_message)) {
    return FinalizeJobRunResult(env, this_arg, wrap, BuildJobErrorResult(env, error_code.c_str(), error_message));
  }

  EVP_PKEY* pkey = GenerateRsaKeyPairNative(
      variant, modulus_length, public_exponent, hash_algorithm, mgf1_hash_algorithm, salt_length, &error_code,
      &error_message);
  if (pkey == nullptr) {
    if (error_code.empty()) error_code = "ERR_CRYPTO_OPERATION_FAILED";
    if (error_message.empty()) error_message = "RSA key generation failed";
    return FinalizeJobRunResult(env, this_arg, wrap, BuildJobErrorResult(env, error_code.c_str(), error_message));
  }

  napi_value result = BuildGeneratedKeyPairResult(env, pkey, encoding, "");
  EVP_PKEY_free(pkey);
  return FinalizeJobRunResult(env, this_arg, wrap, result);
}

napi_value DsaKeyPairGenJobRun(napi_env env, napi_callback_info info) {
  napi_value this_arg = nullptr;
  size_t argc = 0;
  napi_get_cb_info(env, info, &argc, nullptr, &this_arg, nullptr);
  JobWrap* wrap = UnwrapJob(env, this_arg);
  if (wrap == nullptr) {
    return BuildJobErrorResult(env, "ERR_CRYPTO_OPERATION_FAILED", "Invalid DSA keygen arguments");
  }
  if (wrap->args.size() < 8) {
    napi_value result = BuildJobErrorResult(env, "ERR_CRYPTO_OPERATION_FAILED", "Invalid DSA keygen arguments");
    return FinalizeJobRunResult(env, this_arg, wrap, result);
  }

  const int32_t modulus_length = GetInt32Value(env, GetRefValue(env, wrap->args[0]), 0);
  const int32_t divisor_length = GetInt32Value(env, GetRefValue(env, wrap->args[1]), -1);
  KeyEncodingSelection encoding;
  std::string error_code;
  std::string error_message;
  if (!ReadKeyEncodingSelection(env, wrap, 2, &encoding, &error_code, &error_message)) {
    return FinalizeJobRunResult(env, this_arg, wrap, BuildJobErrorResult(env, error_code.c_str(), error_message));
  }

  EVP_PKEY* pkey = GenerateDsaKeyPairNative(modulus_length, divisor_length, &error_code, &error_message);
  if (pkey == nullptr) {
    if (error_code.empty()) error_code = "ERR_CRYPTO_OPERATION_FAILED";
    if (error_message.empty()) error_message = "DSA key generation failed";
    return FinalizeJobRunResult(env, this_arg, wrap, BuildJobErrorResult(env, error_code.c_str(), error_message));
  }

  napi_value result = BuildGeneratedKeyPairResult(env, pkey, encoding, "");
  EVP_PKEY_free(pkey);
  return FinalizeJobRunResult(env, this_arg, wrap, result);
}

napi_value DhKeyPairGenJobRun(napi_env env, napi_callback_info info) {
  napi_value this_arg = nullptr;
  size_t argc = 0;
  napi_get_cb_info(env, info, &argc, nullptr, &this_arg, nullptr);
  JobWrap* wrap = UnwrapJob(env, this_arg);
  if (wrap == nullptr) {
    return BuildJobErrorResult(env, "ERR_CRYPTO_OPERATION_FAILED", "Invalid DH keygen arguments");
  }
  if (wrap->args.size() < 7) {
    napi_value result = BuildJobErrorResult(env, "ERR_CRYPTO_OPERATION_FAILED", "Invalid DH keygen arguments");
    return FinalizeJobRunResult(env, this_arg, wrap, result);
  }

  napi_value source = GetRefValue(env, wrap->args[0]);
  napi_valuetype source_type = napi_undefined;
  if (source != nullptr) napi_typeof(env, source, &source_type);
  const bool group_mode = source_type == napi_string;
  const size_t encoding_index = group_mode ? 1 : 2;

  KeyEncodingSelection encoding;
  std::string error_code;
  std::string error_message;
  if (!ReadKeyEncodingSelection(env, wrap, encoding_index, &encoding, &error_code, &error_message)) {
    return FinalizeJobRunResult(env, this_arg, wrap, BuildJobErrorResult(env, error_code.c_str(), error_message));
  }

  EVP_PKEY* pkey = nullptr;
  if (group_mode) {
    const std::string group_name = GetStringValue(env, source);
    pkey = GenerateDhKeyPairFromGroupNative(group_name, &error_code, &error_message);
  } else if (source_type == napi_number) {
    const int32_t prime_length = GetInt32Value(env, source, 0);
    const int32_t generator = GetInt32Value(env, GetRefValue(env, wrap->args[1]), 2);
    pkey = GenerateDhKeyPairFromPrimeLengthNative(prime_length, generator, &error_code, &error_message);
  } else {
    const std::vector<uint8_t> prime = ValueToBytes(env, source);
    const int32_t generator = GetInt32Value(env, GetRefValue(env, wrap->args[1]), 2);
    pkey = GenerateDhKeyPairFromPrimeNative(prime, generator, &error_code, &error_message);
  }

  if (pkey == nullptr) {
    if (error_code.empty()) error_code = "ERR_CRYPTO_OPERATION_FAILED";
    if (error_message.empty()) error_message = "DH key generation failed";
    return FinalizeJobRunResult(env, this_arg, wrap, BuildJobErrorResult(env, error_code.c_str(), error_message));
  }

  napi_value result = BuildGeneratedKeyPairResult(env, pkey, encoding, "");
  EVP_PKEY_free(pkey);
  return FinalizeJobRunResult(env, this_arg, wrap, result);
}

napi_value EcKeyPairGenJobRun(napi_env env, napi_callback_info info) {
  napi_value this_arg = nullptr;
  size_t argc = 0;
  napi_get_cb_info(env, info, &argc, nullptr, &this_arg, nullptr);
  JobWrap* wrap = UnwrapJob(env, this_arg);
  if (wrap == nullptr) {
    return BuildJobErrorResult(env, "ERR_CRYPTO_OPERATION_FAILED", "Invalid EC keygen arguments");
  }
  if (wrap->args.size() < 8) {
    napi_value result = BuildJobErrorResult(env, "ERR_CRYPTO_OPERATION_FAILED", "Invalid EC keygen arguments");
    return FinalizeJobRunResult(env, this_arg, wrap, result);
  }
  const std::string named_curve = GetStringValue(env, GetRefValue(env, wrap->args[0]));
  const int32_t param_encoding = GetInt32Value(env, GetRefValue(env, wrap->args[1]), OPENSSL_EC_NAMED_CURVE);

  KeyEncodingSelection encoding;
  std::string error_code;
  std::string error_message;
  if (!ReadKeyEncodingSelection(env, wrap, 2, &encoding, &error_code, &error_message)) {
    return FinalizeJobRunResult(env, this_arg, wrap, BuildJobErrorResult(env, error_code.c_str(), error_message));
  }

  EVP_PKEY* pkey = GenerateEcKeyPairNative(named_curve, param_encoding, &error_code, &error_message);
  if (pkey == nullptr) {
    if (error_code.empty()) error_code = "ERR_CRYPTO_OPERATION_FAILED";
    if (error_message.empty()) error_message = "EC key generation failed";
    return FinalizeJobRunResult(env, this_arg, wrap, BuildJobErrorResult(env, error_code.c_str(), error_message));
  }

  napi_value result = BuildGeneratedKeyPairResult(env, pkey, encoding, named_curve);
  EVP_PKEY_free(pkey);
  return FinalizeJobRunResult(env, this_arg, wrap, result);
}

napi_value NidKeyPairGenJobRun(napi_env env, napi_callback_info info) {
  napi_value this_arg = nullptr;
  size_t argc = 0;
  napi_get_cb_info(env, info, &argc, nullptr, &this_arg, nullptr);
  JobWrap* wrap = UnwrapJob(env, this_arg);
  if (wrap == nullptr) {
    return BuildJobErrorResult(env, "ERR_CRYPTO_OPERATION_FAILED", "Invalid keygen arguments");
  }
  if (wrap->args.size() < 7) {
    napi_value result = BuildJobErrorResult(env, "ERR_CRYPTO_OPERATION_FAILED", "Invalid keygen arguments");
    return FinalizeJobRunResult(env, this_arg, wrap, result);
  }

  const int32_t nid = GetInt32Value(env, GetRefValue(env, wrap->args[0]), 0);
  KeyEncodingSelection encoding;
  std::string error_code;
  std::string error_message;
  if (!ReadKeyEncodingSelection(env, wrap, 1, &encoding, &error_code, &error_message)) {
    return FinalizeJobRunResult(env, this_arg, wrap, BuildJobErrorResult(env, error_code.c_str(), error_message));
  }

  EVP_PKEY* pkey = GenerateNidKeyPairNative(nid, &error_code, &error_message);
  if (pkey == nullptr) {
    if (error_code.empty()) error_code = "ERR_CRYPTO_OPERATION_FAILED";
    if (error_message.empty()) error_message = "Key generation failed";
    return FinalizeJobRunResult(env, this_arg, wrap, BuildJobErrorResult(env, error_code.c_str(), error_message));
  }

  napi_value result = BuildGeneratedKeyPairResult(env, pkey, encoding, "");
  EVP_PKEY_free(pkey);
  return FinalizeJobRunResult(env, this_arg, wrap, result);
}

napi_value SecretKeyGenJobRun(napi_env env, napi_callback_info info) {
  napi_value this_arg = nullptr;
  size_t argc = 0;
  napi_get_cb_info(env, info, &argc, nullptr, &this_arg, nullptr);
  JobWrap* wrap = UnwrapJob(env, this_arg);
  if (wrap == nullptr || wrap->args.empty()) {
    napi_value result = BuildJobErrorResult(env, "ERR_CRYPTO_OPERATION_FAILED", "Invalid secret keygen arguments");
    return FinalizeJobRunResult(env, this_arg, wrap, result);
  }

  uint32_t bits = 0;
  napi_value bits_value = GetRefValue(env, wrap->args[0]);
  if (bits_value == nullptr || napi_get_value_uint32(env, bits_value, &bits) != napi_ok) {
    napi_value result = BuildJobErrorResult(env, "ERR_CRYPTO_OPERATION_FAILED", "Invalid secret keygen arguments");
    return FinalizeJobRunResult(env, this_arg, wrap, result);
  }

  const size_t length = static_cast<size_t>(bits / CHAR_BIT);
  std::vector<uint8_t> key(length);
  if (length > 0 && !ncrypto::CSPRNG(key.data(), length)) {
    napi_value result = BuildJobErrorResult(env, "ERR_CRYPTO_OPERATION_FAILED", "Secret key generation failed");
    return FinalizeJobRunResult(env, this_arg, wrap, result);
  }

  napi_value value = CreateKeyObjectHandleValue(env, kKeyTypeSecret, key);
  if (value == nullptr || IsUndefined(env, value)) {
    napi_value result = BuildJobErrorResult(env, "ERR_CRYPTO_OPERATION_FAILED", "Secret key generation failed");
    return FinalizeJobRunResult(env, this_arg, wrap, result);
  }

  return FinalizeJobRunResult(env, this_arg, wrap, BuildJobResult(env, nullptr, value));
}

napi_value RandomPrimeJobRun(napi_env env, napi_callback_info info) {
  napi_value this_arg = nullptr;
  size_t argc = 0;
  napi_get_cb_info(env, info, &argc, nullptr, &this_arg, nullptr);
  JobWrap* wrap = UnwrapJob(env, this_arg);
  if (wrap == nullptr || wrap->args.size() < 2) {
    napi_value result = BuildJobErrorResult(env, "ERR_CRYPTO_OPERATION_FAILED", "Invalid random prime arguments");
    return FinalizeJobRunResult(env, this_arg, wrap, result);
  }

  const int32_t bits = GetInt32Value(env, GetRefValue(env, wrap->args[0]), 0);
  bool safe = false;
  napi_value safe_v = GetRefValue(env, wrap->args[1]);
  if (safe_v != nullptr) napi_get_value_bool(env, safe_v, &safe);

  ncrypto::BignumPointer add;
  ncrypto::BignumPointer rem;
  if (wrap->args.size() >= 3) {
    napi_value add_v = GetRefValue(env, wrap->args[2]);
    if (!IsNullOrUndefinedValue(env, add_v)) {
      const std::vector<uint8_t> add_bytes = ValueToBytes(env, add_v);
      add.reset(add_bytes.data(), add_bytes.size());
      if (!add) {
        napi_value result = BuildJobOpenSslErrorResult(env, "ERR_CRYPTO_OPERATION_FAILED", "could not generate prime");
        return FinalizeJobRunResult(env, this_arg, wrap, result);
      }
    }
  }
  if (wrap->args.size() >= 4) {
    napi_value rem_v = GetRefValue(env, wrap->args[3]);
    if (!IsNullOrUndefinedValue(env, rem_v)) {
      const std::vector<uint8_t> rem_bytes = ValueToBytes(env, rem_v);
      rem.reset(rem_bytes.data(), rem_bytes.size());
      if (!rem) {
        napi_value result = BuildJobOpenSslErrorResult(env, "ERR_CRYPTO_OPERATION_FAILED", "could not generate prime");
        return FinalizeJobRunResult(env, this_arg, wrap, result);
      }
    }
  }

  if (bits <= 0) {
    napi_value result = BuildJobErrorResult(env, "ERR_CRYPTO_OPERATION_FAILED", "Invalid random prime arguments");
    return FinalizeJobRunResult(env, this_arg, wrap, result);
  }
  if (add && ncrypto::BignumPointer::GetBitCount(add.get()) > bits) {
    napi_value result = BuildJobErrorResult(env, "ERR_OUT_OF_RANGE", "invalid options.add");
    return FinalizeJobRunResult(env, this_arg, wrap, result);
  }
  if (add && rem && BN_cmp(add.get(), rem.get()) <= 0) {
    napi_value result = BuildJobErrorResult(env, "ERR_OUT_OF_RANGE", "invalid options.rem");
    return FinalizeJobRunResult(env, this_arg, wrap, result);
  }

  ncrypto::BignumPointer prime = ncrypto::BignumPointer::NewSecure();
  if (!prime) {
    napi_value result = BuildJobErrorResult(env, "ERR_CRYPTO_OPERATION_FAILED", "could not generate prime");
    return FinalizeJobRunResult(env, this_arg, wrap, result);
  }
  if (!prime.generate({.bits = bits, .safe = safe, .add = add, .rem = rem})) {
    napi_value result = BuildJobOpenSslErrorResult(env, "ERR_CRYPTO_OPERATION_FAILED", "could not generate prime");
    return FinalizeJobRunResult(env, this_arg, wrap, result);
  }

  std::vector<uint8_t> prime_bytes(prime.byteLength());
  if (!prime_bytes.empty()) prime.encodeInto(prime_bytes.data());
  napi_value value = CreateArrayBufferCopy(env, prime_bytes);
  return FinalizeJobRunResult(env, this_arg, wrap, BuildJobResult(env, nullptr, value));
}

napi_value CheckPrimeJobRun(napi_env env, napi_callback_info info) {
  napi_value this_arg = nullptr;
  size_t argc = 0;
  napi_get_cb_info(env, info, &argc, nullptr, &this_arg, nullptr);
  JobWrap* wrap = UnwrapJob(env, this_arg);
  if (wrap == nullptr || wrap->args.size() < 2) {
    napi_value result = BuildJobErrorResult(env, "ERR_CRYPTO_OPERATION_FAILED", "Invalid check prime arguments");
    return FinalizeJobRunResult(env, this_arg, wrap, result);
  }

  const napi_value candidate_v = GetRefValue(env, wrap->args[0]);
  const uint8_t* candidate_bytes = nullptr;
  size_t candidate_len = 0;
  if (!GetByteSpan(env, candidate_v, &candidate_bytes, &candidate_len)) {
    napi_value result = BuildJobErrorResult(env, "ERR_CRYPTO_OPERATION_FAILED", "Invalid check prime arguments");
    return FinalizeJobRunResult(env, this_arg, wrap, result);
  }

  ncrypto::BignumPointer candidate(candidate_bytes, candidate_len);
  if (!candidate) {
    napi_value result = BuildJobOpenSslErrorResult(env, "ERR_CRYPTO_OPERATION_FAILED", "BignumPointer");
    return FinalizeJobRunResult(env, this_arg, wrap, result);
  }

  const int32_t checks = GetInt32Value(env, GetRefValue(env, wrap->args[1]), 0);
  const int ret = candidate.isPrime(checks);
  if (ret < 0) {
    napi_value result = BuildJobOpenSslErrorResult(env, "ERR_CRYPTO_OPERATION_FAILED", "check prime failed");
    return FinalizeJobRunResult(env, this_arg, wrap, result);
  }

  napi_value result_bool = nullptr;
  napi_get_boolean(env, ret != 0, &result_bool);
  return FinalizeJobRunResult(env, this_arg, wrap, BuildJobResult(env, nullptr, result_bool));
}

napi_value DeriveBitsJobRun(napi_env env,
                            napi_value this_arg,
                            JobWrap* wrap,
                            const char* invalid_message,
                            const char* failure_message) {
  if (wrap == nullptr || wrap->args.size() < 2) {
    napi_value result = BuildJobErrorResult(env, "ERR_CRYPTO_OPERATION_FAILED", invalid_message);
    return FinalizeJobRunResult(env, this_arg, wrap, result);
  }

  KeyObjectWrap* public_wrap = UnwrapKeyObject(env, GetRefValue(env, wrap->args[0]));
  KeyObjectWrap* private_wrap = UnwrapKeyObject(env, GetRefValue(env, wrap->args[1]));
  if (public_wrap == nullptr || private_wrap == nullptr) {
    napi_value result = BuildJobErrorResult(env, "ERR_CRYPTO_OPERATION_FAILED", invalid_message);
    return FinalizeJobRunResult(env, this_arg, wrap, result);
  }

  EVP_PKEY* public_pkey = ParseKeyObjectAsymmetricKey(public_wrap);
  EVP_PKEY* private_pkey = ParseKeyObjectAsymmetricKey(private_wrap);
  if (public_pkey == nullptr || private_pkey == nullptr) {
    if (public_pkey != nullptr) EVP_PKEY_free(public_pkey);
    if (private_pkey != nullptr) EVP_PKEY_free(private_pkey);
    napi_value result = BuildJobErrorResult(env, "ERR_CRYPTO_OPERATION_FAILED", failure_message);
    return FinalizeJobRunResult(env, this_arg, wrap, result);
  }

  const int public_key_type = EVP_PKEY_base_id(public_pkey);
  const int private_key_type = EVP_PKEY_base_id(private_pkey);
  const bool xdh_derive =
      public_key_type == EVP_PKEY_X25519 ||
      public_key_type == EVP_PKEY_X448 ||
      private_key_type == EVP_PKEY_X25519 ||
      private_key_type == EVP_PKEY_X448;
  EVP_PKEY_CTX* ctx = EVP_PKEY_CTX_new(private_pkey, nullptr);
  bool ok = ctx != nullptr && EVP_PKEY_derive_init(ctx) == 1 && EVP_PKEY_derive_set_peer(ctx, public_pkey) == 1;
  size_t secret_len = 0;
  if (ok) ok = EVP_PKEY_derive(ctx, nullptr, &secret_len) == 1;
  const int key_type = private_pkey != nullptr ? EVP_PKEY_base_id(private_pkey) : EVP_PKEY_NONE;
  size_t padded_secret_len = secret_len;
  if (key_type == EVP_PKEY_DH || key_type == EVP_PKEY_DHX) {
    const int pkey_size = private_pkey != nullptr ? EVP_PKEY_size(private_pkey) : 0;
    if (pkey_size > 0 && static_cast<size_t>(pkey_size) > padded_secret_len) {
      padded_secret_len = static_cast<size_t>(pkey_size);
    }
  }

  std::vector<uint8_t> secret;
  if (ok) {
    secret.resize(secret_len);
    ok = EVP_PKEY_derive(ctx, secret.data(), &secret_len) == 1;
    if (ok) {
      secret.resize(secret_len);
      if (padded_secret_len > secret_len) {
        std::vector<uint8_t> padded(padded_secret_len, 0);
        std::memcpy(padded.data() + (padded_secret_len - secret_len), secret.data(), secret_len);
        secret = std::move(padded);
      }
    }
  }

  if (ctx != nullptr) EVP_PKEY_CTX_free(ctx);
  EVP_PKEY_free(public_pkey);
  EVP_PKEY_free(private_pkey);

  if (!ok) {
    const unsigned long err = ConsumePreferredOpenSslError();
    const char* reason = err != 0 ? ERR_reason_error_string(err) : nullptr;
    napi_value result = nullptr;
    if (xdh_derive &&
        reason != nullptr &&
        (std::strcmp(reason, "unsupported") == 0 ||
         std::strcmp(reason, "failed during derivation") == 0)) {
      napi_value error =
          CreateErrorWithCode(env, "ERR_OSSL_FAILED_DURING_DERIVATION", "failed during derivation");
      SetErrorStringProperty(env, error, "library", "Provider routines");
      SetErrorStringProperty(env, error, "reason", "failed during derivation");
      result = BuildJobResult(env, error, Undefined(env));
    } else {
      napi_value error = err != 0 ? CreateOpenSslError(env, MapOpenSslErrorCode(err), err, failure_message)
                                  : CreateErrorWithCode(env, "ERR_CRYPTO_OPERATION_FAILED", failure_message);
      result = BuildJobResult(env, error, Undefined(env));
    }
    return FinalizeJobRunResult(env, this_arg, wrap, result);
  }

  napi_value value = nullptr;
  void* raw = nullptr;
  if (napi_create_arraybuffer(env, secret.size(), &raw, &value) != napi_ok || value == nullptr) {
    napi_value result = BuildJobErrorResult(env, "ERR_CRYPTO_OPERATION_FAILED", failure_message);
    return FinalizeJobRunResult(env, this_arg, wrap, result);
  }
  if (raw != nullptr && !secret.empty()) std::memcpy(raw, secret.data(), secret.size());
  return FinalizeJobRunResult(env, this_arg, wrap, BuildJobResult(env, nullptr, value));
}

napi_value DHBitsJobRun(napi_env env, napi_callback_info info) {
  napi_value this_arg = nullptr;
  size_t argc = 0;
  napi_get_cb_info(env, info, &argc, nullptr, &this_arg, nullptr);
  JobWrap* wrap = UnwrapJob(env, this_arg);
  return DeriveBitsJobRun(env, this_arg, wrap, "Invalid DH bits arguments", "DH bits generation failed");
}

napi_value ECDHBitsJobRun(napi_env env, napi_callback_info info) {
  napi_value this_arg = nullptr;
  size_t argc = 0;
  napi_get_cb_info(env, info, &argc, nullptr, &this_arg, nullptr);
  JobWrap* wrap = UnwrapJob(env, this_arg);
  return DeriveBitsJobRun(env, this_arg, wrap, "Invalid ECDH bits arguments", "ECDH bits generation failed");
}

napi_value HmacJobRun(napi_env env, napi_callback_info info) {
  napi_value this_arg = nullptr;
  size_t argc = 0;
  napi_get_cb_info(env, info, &argc, nullptr, &this_arg, nullptr);
  JobWrap* wrap = UnwrapJob(env, this_arg);
  if (wrap == nullptr || wrap->args.size() < 4) {
    napi_value result = BuildJobErrorResult(env, "ERR_CRYPTO_OPERATION_FAILED", "Invalid HMAC arguments");
    return FinalizeJobRunResult(env, this_arg, wrap, result);
  }

  const int32_t mode = GetInt32Value(env, GetRefValue(env, wrap->args[0]), kSignJobModeSign);
  const std::string digest = GetStringValue(env, GetRefValue(env, wrap->args[1]));
  std::vector<uint8_t> key;
  std::string error_code;
  std::string error_message;
  if (!GetSecretKeyFromHandle(env, GetRefValue(env, wrap->args[2]), &key, &error_code, &error_message)) {
    napi_value result = BuildJobErrorResult(env, error_code.c_str(), error_message);
    return FinalizeJobRunResult(env, this_arg, wrap, result);
  }

  napi_value binding = GetBinding(env);
  if (binding == nullptr) {
    napi_value result = BuildJobErrorResult(env, "ERR_CRYPTO_OPERATION_FAILED", "Crypto binding unavailable");
    return FinalizeJobRunResult(env, this_arg, wrap, result);
  }

  napi_value digest_value = nullptr;
  napi_create_string_utf8(env, digest.c_str(), NAPI_AUTO_LENGTH, &digest_value);
  napi_value key_value = BytesToBuffer(env, key);
  napi_value data_value = BytesToBuffer(env, ValueToBytes(env, GetRefValue(env, wrap->args[3])));
  napi_value call_argv[3] = {
      digest_value != nullptr ? digest_value : Undefined(env),
      key_value != nullptr ? key_value : Undefined(env),
      data_value != nullptr ? data_value : Undefined(env),
  };
  napi_value signature_value = nullptr;
  if (!CallBindingMethod(env, binding, "hmacOneShot", 3, call_argv, &signature_value) || signature_value == nullptr) {
    napi_value result = nullptr;
    CaptureCallFailureAsJobResult(env, "ERR_CRYPTO_OPERATION_FAILED", "HMAC operation failed", &result);
    return FinalizeJobRunResult(env, this_arg, wrap, result);
  }

  if (mode == kSignJobModeVerify) {
    const std::vector<uint8_t> expected = ValueToBytes(env, GetRefValue(env, wrap->args[4]));
    const std::vector<uint8_t> actual = ValueToBytes(env, signature_value);
    const bool ok = expected.size() == actual.size() &&
                    (expected.empty() || CRYPTO_memcmp(expected.data(), actual.data(), expected.size()) == 0);
    napi_value out = nullptr;
    napi_get_boolean(env, ok, &out);
    return FinalizeJobRunResult(env, this_arg, wrap, BuildJobResult(env, nullptr, out));
  }

  return FinalizeJobRunResult(env, this_arg, wrap, BuildArrayBufferJobResult(env, signature_value));
}

napi_value KmacJobRun(napi_env env, napi_callback_info info) {
  napi_value this_arg = nullptr;
  size_t argc = 0;
  napi_get_cb_info(env, info, &argc, nullptr, &this_arg, nullptr);
  JobWrap* wrap = UnwrapJob(env, this_arg);
  if (wrap == nullptr || wrap->args.size() < 6) {
    napi_value result = BuildJobErrorResult(env, "ERR_CRYPTO_OPERATION_FAILED", "Invalid KMAC arguments");
    return FinalizeJobRunResult(env, this_arg, wrap, result);
  }
#if OPENSSL_VERSION_MAJOR >= 3
  const int32_t mode = GetInt32Value(env, GetRefValue(env, wrap->args[0]), kSignJobModeSign);
  std::vector<uint8_t> key;
  std::string error_code;
  std::string error_message;
  if (!GetSecretKeyFromHandle(env, GetRefValue(env, wrap->args[1]), &key, &error_code, &error_message)) {
    napi_value result = BuildJobErrorResult(env, error_code.c_str(), error_message);
    return FinalizeJobRunResult(env, this_arg, wrap, result);
  }

  const std::string algorithm = GetStringValue(env, GetRefValue(env, wrap->args[2]));
  const std::vector<uint8_t> customization = ValueToBytes(env, GetRefValue(env, wrap->args[3]));
  const uint32_t output_len = static_cast<uint32_t>(GetInt32Value(env, GetRefValue(env, wrap->args[4]), 0));
  const std::vector<uint8_t> data = ValueToBytes(env, GetRefValue(env, wrap->args[5]));
  const std::vector<uint8_t> expected = ValueToBytes(env, GetRefValue(env, wrap->args[6]));

  if (output_len == 0) {
    if (mode == kSignJobModeVerify) {
      napi_value out = nullptr;
      napi_get_boolean(env, expected.empty(), &out);
      return FinalizeJobRunResult(env, this_arg, wrap, BuildJobResult(env, nullptr, out));
    }
    return FinalizeJobRunResult(env, this_arg, wrap, BuildArrayBufferJobResult(env, std::vector<uint8_t>{}));
  }

  const char* algorithm_name = nullptr;
  if (algorithm == "KMAC128") {
    algorithm_name = OSSL_MAC_NAME_KMAC128;
  } else if (algorithm == "KMAC256") {
    algorithm_name = OSSL_MAC_NAME_KMAC256;
  } else {
    napi_value result = BuildJobErrorResult(env, "ERR_CRYPTO_UNSUPPORTED_OPERATION", "Unsupported KMAC algorithm");
    return FinalizeJobRunResult(env, this_arg, wrap, result);
  }

  auto mac = ncrypto::EVPMacPointer::Fetch(algorithm_name);
  auto mac_ctx = mac ? ncrypto::EVPMacCtxPointer::New(mac.get()) : ncrypto::EVPMacCtxPointer();
  if (!mac_ctx) {
    napi_value result = BuildJobErrorResult(env, "ERR_CRYPTO_OPERATION_FAILED", "KMAC operation failed");
    return FinalizeJobRunResult(env, this_arg, wrap, result);
  }

  size_t length_param = output_len;
  OSSL_PARAM params[3];
  size_t params_count = 0;
  params[params_count++] = OSSL_PARAM_construct_size_t(OSSL_MAC_PARAM_SIZE, &length_param);
  if (!customization.empty()) {
    params[params_count++] = OSSL_PARAM_construct_octet_string(
        OSSL_MAC_PARAM_CUSTOM, const_cast<unsigned char*>(customization.data()), customization.size());
  }
  params[params_count] = OSSL_PARAM_construct_end();

  if (!mac_ctx.init({key.data(), key.size()}, params) || !mac_ctx.update({data.data(), data.size()})) {
    napi_value result = BuildJobOpenSslErrorResult(env, "ERR_CRYPTO_OPERATION_FAILED", "KMAC operation failed");
    return FinalizeJobRunResult(env, this_arg, wrap, result);
  }

  ncrypto::DataPointer mac_output = mac_ctx.final(output_len);
  if (!mac_output) {
    napi_value result = BuildJobOpenSslErrorResult(env, "ERR_CRYPTO_OPERATION_FAILED", "KMAC operation failed");
    return FinalizeJobRunResult(env, this_arg, wrap, result);
  }
  std::vector<uint8_t> actual(static_cast<const uint8_t*>(mac_output.get()),
                              static_cast<const uint8_t*>(mac_output.get()) + mac_output.size());

  if (mode == kSignJobModeVerify) {
    const bool ok = expected.size() == actual.size() &&
                    (expected.empty() || CRYPTO_memcmp(expected.data(), actual.data(), expected.size()) == 0);
    napi_value out = nullptr;
    napi_get_boolean(env, ok, &out);
    return FinalizeJobRunResult(env, this_arg, wrap, BuildJobResult(env, nullptr, out));
  }

  return FinalizeJobRunResult(env, this_arg, wrap, BuildArrayBufferJobResult(env, actual));
#else
  napi_value result = BuildJobErrorResult(env, "ERR_CRYPTO_UNSUPPORTED_OPERATION", "Unsupported crypto constructor");
  return FinalizeJobRunResult(env, this_arg, wrap, result);
#endif
}

napi_value DHKeyExportJobRun(napi_env env, napi_callback_info info) {
  napi_value this_arg = nullptr;
  size_t argc = 0;
  napi_get_cb_info(env, info, &argc, nullptr, &this_arg, nullptr);
  JobWrap* wrap = UnwrapJob(env, this_arg);
  if (wrap == nullptr || wrap->args.size() < 2) {
    napi_value result = BuildJobErrorResult(env, "ERR_CRYPTO_OPERATION_FAILED", "Invalid DH export arguments");
    return FinalizeJobRunResult(env, this_arg, wrap, result);
  }

  KeyObjectWrap* key_wrap = UnwrapKeyObject(env, GetRefValue(env, wrap->args[1]));
  if (key_wrap == nullptr || key_wrap->key_type == kKeyTypeSecret) {
    napi_value result = BuildJobErrorResult(env, "ERR_INVALID_ARG_TYPE", "Invalid key handle");
    return FinalizeJobRunResult(env, this_arg, wrap, result);
  }
  const int32_t format = GetInt32Value(env, GetRefValue(env, wrap->args[0]), 0);

  EVP_PKEY* pkey = ParseKeyObjectAsymmetricKey(key_wrap);
  if (pkey == nullptr) {
    napi_value result = BuildJobErrorResult(env, "ERR_CRYPTO_OPERATION_FAILED", "Failed to parse key");
    return FinalizeJobRunResult(env, this_arg, wrap, result);
  }

  KeyEncodingSelection encoding;
  napi_value exported = nullptr;
  std::string error_code;
  std::string error_message;
  if (format == 1) {
    if (key_wrap->key_type != kKeyTypePrivate) {
      EVP_PKEY_free(pkey);
      napi_value result = BuildJobErrorResult(env, "ERR_CRYPTO_INVALID_KEYTYPE", "Invalid key type");
      return FinalizeJobRunResult(env, this_arg, wrap, result);
    }
    encoding.has_private_encoding = true;
    encoding.private_format = kKeyFormatDER;
    encoding.private_type = kKeyEncodingPKCS8;
    if (!ExportPrivateKeyValue(env, pkey, encoding, "", &exported, &error_code, &error_message)) {
      EVP_PKEY_free(pkey);
      napi_value result = BuildJobErrorResult(
          env, error_code.empty() ? "ERR_CRYPTO_OPERATION_FAILED" : error_code.c_str(),
          error_message.empty() ? "DH export failed" : error_message);
      return FinalizeJobRunResult(env, this_arg, wrap, result);
    }
  } else if (format == 2) {
    if (key_wrap->key_type != kKeyTypePublic) {
      EVP_PKEY_free(pkey);
      napi_value result = BuildJobErrorResult(env, "ERR_CRYPTO_INVALID_KEYTYPE", "Invalid key type");
      return FinalizeJobRunResult(env, this_arg, wrap, result);
    }
    encoding.has_public_encoding = true;
    encoding.public_format = kKeyFormatDER;
    encoding.public_type = kKeyEncodingSPKI;
    if (!ExportPublicKeyValue(env, pkey, encoding, "", &exported, &error_code, &error_message)) {
      EVP_PKEY_free(pkey);
      napi_value result = BuildJobErrorResult(
          env, error_code.empty() ? "ERR_CRYPTO_OPERATION_FAILED" : error_code.c_str(),
          error_message.empty() ? "DH export failed" : error_message);
      return FinalizeJobRunResult(env, this_arg, wrap, result);
    }
  } else {
    EVP_PKEY_free(pkey);
    napi_value result = BuildJobErrorResult(env, "ERR_CRYPTO_UNSUPPORTED_OPERATION", "Unsupported export format");
    return FinalizeJobRunResult(env, this_arg, wrap, result);
  }
  EVP_PKEY_free(pkey);

  return FinalizeJobRunResult(env, this_arg, wrap, BuildArrayBufferJobResult(env, exported));
}

napi_value ECKeyExportJobRun(napi_env env, napi_callback_info info) {
  napi_value this_arg = nullptr;
  size_t argc = 0;
  napi_get_cb_info(env, info, &argc, nullptr, &this_arg, nullptr);
  JobWrap* wrap = UnwrapJob(env, this_arg);
  if (wrap == nullptr || wrap->args.size() < 2) {
    napi_value result = BuildJobErrorResult(env, "ERR_CRYPTO_OPERATION_FAILED", "Invalid EC export arguments");
    return FinalizeJobRunResult(env, this_arg, wrap, result);
  }

  KeyObjectWrap* key_wrap = UnwrapKeyObject(env, GetRefValue(env, wrap->args[1]));
  if (key_wrap == nullptr || key_wrap->key_type == kKeyTypeSecret) {
    napi_value result = BuildJobErrorResult(env, "ERR_INVALID_ARG_TYPE", "Invalid key handle");
    return FinalizeJobRunResult(env, this_arg, wrap, result);
  }
  const int32_t format = GetInt32Value(env, GetRefValue(env, wrap->args[0]), 0);

  EVP_PKEY* pkey = ParseKeyObjectAsymmetricKey(key_wrap);
  if (pkey == nullptr) {
    napi_value result = BuildJobErrorResult(env, "ERR_CRYPTO_OPERATION_FAILED", "Failed to parse key");
    return FinalizeJobRunResult(env, this_arg, wrap, result);
  }

  const int base = EVP_PKEY_base_id(pkey);
  std::string error_code;
  std::string error_message;
  if (format == 0) {
    if (key_wrap->key_type != kKeyTypePublic) {
      EVP_PKEY_free(pkey);
      napi_value result = BuildJobErrorResult(env, "ERR_CRYPTO_INVALID_KEYTYPE", "Invalid key type");
      return FinalizeJobRunResult(env, this_arg, wrap, result);
    }

    if (base == EVP_PKEY_EC) {
      EC_KEY* ec = EVP_PKEY_get1_EC_KEY(pkey);
      const EC_GROUP* group = ec != nullptr ? EC_KEY_get0_group(ec) : nullptr;
      const EC_POINT* point = ec != nullptr ? EC_KEY_get0_public_key(ec) : nullptr;
      const point_conversion_form_t form = POINT_CONVERSION_UNCOMPRESSED;
      const size_t need =
          (group != nullptr && point != nullptr) ? EC_POINT_point2oct(group, point, form, nullptr, 0, nullptr) : 0;
      std::vector<uint8_t> out(need);
      const size_t have = need == 0 ? 0 : EC_POINT_point2oct(group, point, form, out.data(), out.size(), nullptr);
      if (ec != nullptr) EC_KEY_free(ec);
      EVP_PKEY_free(pkey);
      if (have == 0) {
        napi_value result = BuildJobErrorResult(env, "ERR_CRYPTO_OPERATION_FAILED", "EC export failed");
        return FinalizeJobRunResult(env, this_arg, wrap, result);
      }
      out.resize(have);
      return FinalizeJobRunResult(env, this_arg, wrap, BuildArrayBufferJobResult(env, out));
    }

    if (base == EVP_PKEY_ED25519 || base == EVP_PKEY_ED448 || base == EVP_PKEY_X25519 || base == EVP_PKEY_X448) {
      size_t len = 0;
      if (EVP_PKEY_get_raw_public_key(pkey, nullptr, &len) != 1 || len == 0) {
        EVP_PKEY_free(pkey);
        napi_value result = BuildJobErrorResult(env, "ERR_CRYPTO_OPERATION_FAILED", "EC export failed");
        return FinalizeJobRunResult(env, this_arg, wrap, result);
      }
      std::vector<uint8_t> out(len);
      if (EVP_PKEY_get_raw_public_key(pkey, out.data(), &len) != 1) {
        EVP_PKEY_free(pkey);
        napi_value result = BuildJobErrorResult(env, "ERR_CRYPTO_OPERATION_FAILED", "EC export failed");
        return FinalizeJobRunResult(env, this_arg, wrap, result);
      }
      EVP_PKEY_free(pkey);
      out.resize(len);
      return FinalizeJobRunResult(env, this_arg, wrap, BuildArrayBufferJobResult(env, out));
    }

    EVP_PKEY_free(pkey);
    napi_value result = BuildJobErrorResult(env, "ERR_CRYPTO_INVALID_KEYTYPE", "Invalid key type");
    return FinalizeJobRunResult(env, this_arg, wrap, result);
  }

  if (format == 3) {
    napi_value exported = key_wrap->key_type == kKeyTypePrivate
                              ? ExportJwkPrivate(env, pkey, &error_code, &error_message, "")
                              : ExportJwkPublic(env, pkey, &error_code, &error_message, "");
    EVP_PKEY_free(pkey);
    if (exported == nullptr) {
      napi_value result = BuildJobErrorResult(
          env, error_code.empty() ? "ERR_CRYPTO_OPERATION_FAILED" : error_code.c_str(),
          error_message.empty() ? "EC export failed" : error_message);
      return FinalizeJobRunResult(env, this_arg, wrap, result);
    }
    return FinalizeJobRunResult(env, this_arg, wrap, BuildJobResult(env, nullptr, exported));
  }

  KeyEncodingSelection encoding;
  napi_value exported = nullptr;
  if (format == 1) {
    if (key_wrap->key_type != kKeyTypePrivate) {
      EVP_PKEY_free(pkey);
      napi_value result = BuildJobErrorResult(env, "ERR_CRYPTO_INVALID_KEYTYPE", "Invalid key type");
      return FinalizeJobRunResult(env, this_arg, wrap, result);
    }
    encoding.has_private_encoding = true;
    encoding.private_format = kKeyFormatDER;
    encoding.private_type = kKeyEncodingPKCS8;
    if (!ExportPrivateKeyValue(env, pkey, encoding, "", &exported, &error_code, &error_message)) {
      EVP_PKEY_free(pkey);
      napi_value result = BuildJobErrorResult(
          env, error_code.empty() ? "ERR_CRYPTO_OPERATION_FAILED" : error_code.c_str(),
          error_message.empty() ? "EC export failed" : error_message);
      return FinalizeJobRunResult(env, this_arg, wrap, result);
    }
  } else if (format == 2) {
    if (key_wrap->key_type != kKeyTypePublic) {
      EVP_PKEY_free(pkey);
      napi_value result = BuildJobErrorResult(env, "ERR_CRYPTO_INVALID_KEYTYPE", "Invalid key type");
      return FinalizeJobRunResult(env, this_arg, wrap, result);
    }
    if (base == EVP_PKEY_EC) {
      std::vector<uint8_t> out = ExportEcPublicDerSpkiUncompressed(pkey);
      EVP_PKEY_free(pkey);
      if (out.empty()) {
        napi_value result = BuildJobErrorResult(env, "ERR_CRYPTO_OPERATION_FAILED", "EC export failed");
        return FinalizeJobRunResult(env, this_arg, wrap, result);
      }
      return FinalizeJobRunResult(env, this_arg, wrap, BuildArrayBufferJobResult(env, out));
    }
    encoding.has_public_encoding = true;
    encoding.public_format = kKeyFormatDER;
    encoding.public_type = kKeyEncodingSPKI;
    if (!ExportPublicKeyValue(env, pkey, encoding, "", &exported, &error_code, &error_message)) {
      EVP_PKEY_free(pkey);
      napi_value result = BuildJobErrorResult(
          env, error_code.empty() ? "ERR_CRYPTO_OPERATION_FAILED" : error_code.c_str(),
          error_message.empty() ? "EC export failed" : error_message);
      return FinalizeJobRunResult(env, this_arg, wrap, result);
    }
  } else {
    EVP_PKEY_free(pkey);
    napi_value result = BuildJobErrorResult(env, "ERR_CRYPTO_UNSUPPORTED_OPERATION", "Unsupported export format");
    return FinalizeJobRunResult(env, this_arg, wrap, result);
  }
  EVP_PKEY_free(pkey);

  return FinalizeJobRunResult(env, this_arg, wrap, BuildArrayBufferJobResult(env, exported));
}

napi_value RSAKeyExportJobRun(napi_env env, napi_callback_info info) {
  napi_value this_arg = nullptr;
  size_t argc = 0;
  napi_get_cb_info(env, info, &argc, nullptr, &this_arg, nullptr);
  JobWrap* wrap = UnwrapJob(env, this_arg);
  if (wrap == nullptr || wrap->args.size() < 2) {
    napi_value result = BuildJobErrorResult(env, "ERR_CRYPTO_OPERATION_FAILED", "Invalid RSA export arguments");
    return FinalizeJobRunResult(env, this_arg, wrap, result);
  }

  KeyObjectWrap* key_wrap = UnwrapKeyObject(env, GetRefValue(env, wrap->args[1]));
  if (key_wrap == nullptr || key_wrap->key_type == kKeyTypeSecret) {
    napi_value result = BuildJobErrorResult(env, "ERR_INVALID_ARG_TYPE", "Invalid key handle");
    return FinalizeJobRunResult(env, this_arg, wrap, result);
  }
  const int32_t format = GetInt32Value(env, GetRefValue(env, wrap->args[0]), 0);
  const int32_t variant = GetInt32Value(env, GetRefValue(env, wrap->args[2]), kKeyVariantRSA_SSA_PKCS1_v1_5);

  EVP_PKEY* pkey = ParseKeyObjectAsymmetricKey(key_wrap);
  if (pkey == nullptr) {
    napi_value result = BuildJobErrorResult(env, "ERR_CRYPTO_OPERATION_FAILED", "Failed to parse key");
    return FinalizeJobRunResult(env, this_arg, wrap, result);
  }

  const int base = EVP_PKEY_base_id(pkey);
  std::string error_code;
  std::string error_message;
  if (format == 0) {
    EVP_PKEY_free(pkey);
    napi_value result = BuildJobErrorResult(env, "ERR_CRYPTO_UNSUPPORTED_OPERATION", "Unsupported export format");
    return FinalizeJobRunResult(env, this_arg, wrap, result);
  }
  if (format == 3) {
    if (base == EVP_PKEY_RSA_PSS && variant != kKeyVariantRSA_PSS) {
      EVP_PKEY_free(pkey);
      napi_value result =
          BuildJobErrorResult(env, "ERR_CRYPTO_JWK_UNSUPPORTED_KEY_TYPE", "Unsupported JWK Key Type.");
      return FinalizeJobRunResult(env, this_arg, wrap, result);
    }
    napi_value exported = key_wrap->key_type == kKeyTypePrivate
                              ? ExportJwkPrivate(env, pkey, &error_code, &error_message, "")
                              : ExportJwkPublic(env, pkey, &error_code, &error_message, "");
    EVP_PKEY_free(pkey);
    if (exported == nullptr) {
      napi_value result = BuildJobErrorResult(
          env, error_code.empty() ? "ERR_CRYPTO_OPERATION_FAILED" : error_code.c_str(),
          error_message.empty() ? "RSA export failed" : error_message);
      return FinalizeJobRunResult(env, this_arg, wrap, result);
    }
    return FinalizeJobRunResult(env, this_arg, wrap, BuildJobResult(env, nullptr, exported));
  }

  KeyEncodingSelection encoding;
  napi_value exported = nullptr;
  if (format == 1) {
    if (key_wrap->key_type != kKeyTypePrivate) {
      EVP_PKEY_free(pkey);
      napi_value result = BuildJobErrorResult(env, "ERR_CRYPTO_INVALID_KEYTYPE", "Invalid key type");
      return FinalizeJobRunResult(env, this_arg, wrap, result);
    }
    encoding.has_private_encoding = true;
    encoding.private_format = kKeyFormatDER;
    encoding.private_type = kKeyEncodingPKCS8;
    if (!ExportPrivateKeyValue(env, pkey, encoding, "", &exported, &error_code, &error_message)) {
      EVP_PKEY_free(pkey);
      napi_value result = BuildJobErrorResult(
          env, error_code.empty() ? "ERR_CRYPTO_OPERATION_FAILED" : error_code.c_str(),
          error_message.empty() ? "RSA export failed" : error_message);
      return FinalizeJobRunResult(env, this_arg, wrap, result);
    }
  } else if (format == 2) {
    if (key_wrap->key_type != kKeyTypePublic) {
      EVP_PKEY_free(pkey);
      napi_value result = BuildJobErrorResult(env, "ERR_CRYPTO_INVALID_KEYTYPE", "Invalid key type");
      return FinalizeJobRunResult(env, this_arg, wrap, result);
    }
    encoding.has_public_encoding = true;
    encoding.public_format = kKeyFormatDER;
    encoding.public_type = kKeyEncodingSPKI;
    if (!ExportPublicKeyValue(env, pkey, encoding, "", &exported, &error_code, &error_message)) {
      EVP_PKEY_free(pkey);
      napi_value result = BuildJobErrorResult(
          env, error_code.empty() ? "ERR_CRYPTO_OPERATION_FAILED" : error_code.c_str(),
          error_message.empty() ? "RSA export failed" : error_message);
      return FinalizeJobRunResult(env, this_arg, wrap, result);
    }
  } else {
    EVP_PKEY_free(pkey);
    napi_value result = BuildJobErrorResult(env, "ERR_CRYPTO_UNSUPPORTED_OPERATION", "Unsupported export format");
    return FinalizeJobRunResult(env, this_arg, wrap, result);
  }
  EVP_PKEY_free(pkey);

  return FinalizeJobRunResult(env, this_arg, wrap, BuildArrayBufferJobResult(env, exported));
}

napi_value AESCipherJobRun(napi_env env, napi_callback_info info) {
  napi_value this_arg = nullptr;
  size_t argc = 0;
  napi_get_cb_info(env, info, &argc, nullptr, &this_arg, nullptr);
  JobWrap* wrap = UnwrapJob(env, this_arg);
  if (wrap == nullptr || wrap->args.size() < 4) {
    napi_value result = BuildJobErrorResult(env, "ERR_CRYPTO_OPERATION_FAILED", "Invalid AES arguments");
    return FinalizeJobRunResult(env, this_arg, wrap, result);
  }

  const int32_t cipher_mode = GetInt32Value(env, GetRefValue(env, wrap->args[0]), 0);
  std::vector<uint8_t> key;
  std::string error_code;
  std::string error_message;
  if (!GetSecretKeyFromHandle(env, GetRefValue(env, wrap->args[1]), &key, &error_code, &error_message)) {
    napi_value result = BuildJobErrorResult(env, error_code.c_str(), error_message);
    return FinalizeJobRunResult(env, this_arg, wrap, result);
  }

  const std::vector<uint8_t> input = ValueToBytes(env, GetRefValue(env, wrap->args[2]));
  const int32_t variant = GetInt32Value(env, GetRefValue(env, wrap->args[3]), -1);
  if (variant < 0) {
    napi_value result = BuildJobErrorResult(env, "ERR_CRYPTO_UNSUPPORTED_OPERATION", "Unsupported AES variant");
    return FinalizeJobRunResult(env, this_arg, wrap, result);
  }

  if (IsAesCtrVariant(variant)) {
    const std::vector<uint8_t> counter = ValueToBytes(env, GetRefValue(env, wrap->args[4]));
    const uint32_t length_bits = static_cast<uint32_t>(GetInt32Value(env, GetRefValue(env, wrap->args[5]), 0));
    std::vector<uint8_t> output;
    if (!RunAesCtrCipher(key, variant, cipher_mode, counter, length_bits, input, &output)) {
      napi_value result = BuildJobErrorResult(env, "ERR_CRYPTO_OPERATION_FAILED", "AES-CTR operation failed");
      return FinalizeJobRunResult(env, this_arg, wrap, result);
    }
    return FinalizeJobRunResult(env, this_arg, wrap, BuildArrayBufferJobResult(env, output));
  }

  const std::string algorithm = AesAlgorithmNameFromVariant(variant);
  if (algorithm.empty()) {
    napi_value result = BuildJobErrorResult(env, "ERR_CRYPTO_UNSUPPORTED_OPERATION", "Unsupported AES variant");
    return FinalizeJobRunResult(env, this_arg, wrap, result);
  }

  napi_value binding = GetBinding(env);
  if (binding == nullptr) {
    napi_value result = BuildJobErrorResult(env, "ERR_CRYPTO_OPERATION_FAILED", "Crypto binding unavailable");
    return FinalizeJobRunResult(env, this_arg, wrap, result);
  }

  napi_value algorithm_value = nullptr;
  napi_create_string_utf8(env, algorithm.c_str(), NAPI_AUTO_LENGTH, &algorithm_value);
  napi_value key_value = BytesToBuffer(env, key);
  napi_value input_value = BytesToBuffer(env, input);
  napi_value decrypt_value = nullptr;
  napi_get_boolean(env, cipher_mode != 0, &decrypt_value);

  if (IsAesAeadVariant(variant)) {
    const std::vector<uint8_t> iv = ValueToBytes(env, GetRefValue(env, wrap->args[4]));
    const std::vector<uint8_t> aad = ValueToBytes(env, GetRefValue(env, wrap->args[6]));
    napi_value iv_value = BytesToBuffer(env, iv);
    napi_value aad_value = BytesToBuffer(env, aad);
    napi_value auth_tag_value = Undefined(env);
    int32_t auth_tag_len = 16;
    if (cipher_mode == 0) {
      auth_tag_len = GetInt32Value(env, GetRefValue(env, wrap->args[5]), 16);
    } else {
      const std::vector<uint8_t> auth_tag = ValueToBytes(env, GetRefValue(env, wrap->args[5]));
      auth_tag_value = BytesToBuffer(env, auth_tag);
      auth_tag_len = static_cast<int32_t>(auth_tag.size());
    }
    napi_value auth_tag_len_value = nullptr;
    napi_create_int32(env, auth_tag_len, &auth_tag_len_value);
    napi_value call_argv[8] = {
        algorithm_value != nullptr ? algorithm_value : Undefined(env),
        key_value != nullptr ? key_value : Undefined(env),
        iv_value != nullptr ? iv_value : Undefined(env),
        input_value != nullptr ? input_value : Undefined(env),
        decrypt_value != nullptr ? decrypt_value : Undefined(env),
        aad_value != nullptr ? aad_value : Undefined(env),
        auth_tag_value != nullptr ? auth_tag_value : Undefined(env),
        auth_tag_len_value != nullptr ? auth_tag_len_value : Undefined(env),
    };
    napi_value output_info = nullptr;
    if (!CallBindingMethod(env, binding, "cipherTransformAead", 8, call_argv, &output_info) ||
        output_info == nullptr) {
      napi_value result = nullptr;
      CaptureCallFailureAsJobResult(env, "ERR_CRYPTO_OPERATION_FAILED", "AES cipher operation failed", &result);
      return FinalizeJobRunResult(env, this_arg, wrap, result);
    }

    napi_value output_value = nullptr;
    if (napi_get_named_property(env, output_info, "output", &output_value) != napi_ok || output_value == nullptr) {
      napi_value result = BuildJobErrorResult(env, "ERR_CRYPTO_OPERATION_FAILED", "AES cipher operation failed");
      return FinalizeJobRunResult(env, this_arg, wrap, result);
    }
    std::vector<uint8_t> output = ValueToBytes(env, output_value);
    if (cipher_mode == 0) {
      napi_value tag_value = nullptr;
      if (napi_get_named_property(env, output_info, "authTag", &tag_value) == napi_ok && tag_value != nullptr &&
          !IsUndefined(env, tag_value)) {
        const std::vector<uint8_t> tag = ValueToBytes(env, tag_value);
        output.insert(output.end(), tag.begin(), tag.end());
      }
    }
    return FinalizeJobRunResult(env, this_arg, wrap, BuildArrayBufferJobResult(env, output));
  }

  const std::vector<uint8_t> iv = IsAesWrapVariant(variant)
                                      ? std::vector<uint8_t>{0xa6, 0xa6, 0xa6, 0xa6, 0xa6, 0xa6, 0xa6, 0xa6}
                                      : ValueToBytes(env, GetRefValue(env, wrap->args[4]));
  napi_value iv_value = BytesToBuffer(env, iv);
  napi_value auto_padding = nullptr;
  napi_get_boolean(env, true, &auto_padding);
  napi_value call_argv[7] = {
      algorithm_value != nullptr ? algorithm_value : Undefined(env),
      key_value != nullptr ? key_value : Undefined(env),
      iv_value != nullptr ? iv_value : Undefined(env),
      input_value != nullptr ? input_value : Undefined(env),
      decrypt_value != nullptr ? decrypt_value : Undefined(env),
      Undefined(env),
      auto_padding != nullptr ? auto_padding : Undefined(env),
  };
  napi_value output_value = nullptr;
  if (!CallBindingMethod(env, binding, "cipherTransform", 7, call_argv, &output_value) || output_value == nullptr) {
    napi_value result = nullptr;
    CaptureCallFailureAsJobResult(env, "ERR_CRYPTO_OPERATION_FAILED", "AES cipher operation failed", &result);
    return FinalizeJobRunResult(env, this_arg, wrap, result);
  }
  return FinalizeJobRunResult(env, this_arg, wrap, BuildArrayBufferJobResult(env, output_value));
}

napi_value ChaCha20Poly1305CipherJobRun(napi_env env, napi_callback_info info) {
  napi_value this_arg = nullptr;
  size_t argc = 0;
  napi_get_cb_info(env, info, &argc, nullptr, &this_arg, nullptr);
  JobWrap* wrap = UnwrapJob(env, this_arg);
  if (wrap == nullptr || wrap->args.size() < 5) {
    napi_value result = BuildJobErrorResult(
        env, "ERR_CRYPTO_OPERATION_FAILED", "Invalid ChaCha20-Poly1305 arguments");
    return FinalizeJobRunResult(env, this_arg, wrap, result);
  }

  const int32_t cipher_mode = GetInt32Value(env, GetRefValue(env, wrap->args[0]), 0);
  std::vector<uint8_t> key;
  std::string error_code;
  std::string error_message;
  if (!GetSecretKeyFromHandle(env, GetRefValue(env, wrap->args[1]), &key, &error_code, &error_message)) {
    napi_value result = BuildJobErrorResult(env, error_code.c_str(), error_message);
    return FinalizeJobRunResult(env, this_arg, wrap, result);
  }

  const std::vector<uint8_t> input = ValueToBytes(env, GetRefValue(env, wrap->args[2]));
  const std::vector<uint8_t> iv = ValueToBytes(env, GetRefValue(env, wrap->args[3]));
  const std::vector<uint8_t> aad = ValueToBytes(env, GetRefValue(env, wrap->args[5]));

  napi_value binding = GetBinding(env);
  if (binding == nullptr) {
    napi_value result = BuildJobErrorResult(env, "ERR_CRYPTO_OPERATION_FAILED", "Crypto binding unavailable");
    return FinalizeJobRunResult(env, this_arg, wrap, result);
  }

  napi_value algorithm_value = nullptr;
  napi_create_string_utf8(env, "chacha20-poly1305", NAPI_AUTO_LENGTH, &algorithm_value);
  napi_value key_value = BytesToBuffer(env, key);
  napi_value iv_value = BytesToBuffer(env, iv);
  napi_value input_value = BytesToBuffer(env, input);
  napi_value decrypt_value = nullptr;
  napi_get_boolean(env, cipher_mode != 0, &decrypt_value);
  napi_value aad_value = BytesToBuffer(env, aad);
  napi_value auth_tag_value = Undefined(env);
  int32_t auth_tag_len = 16;
  if (cipher_mode == 0) {
    auth_tag_len = GetInt32Value(env, GetRefValue(env, wrap->args[4]), 16);
  } else {
    const std::vector<uint8_t> auth_tag = ValueToBytes(env, GetRefValue(env, wrap->args[4]));
    auth_tag_value = BytesToBuffer(env, auth_tag);
    auth_tag_len = static_cast<int32_t>(auth_tag.size());
  }
  napi_value auth_tag_len_value = nullptr;
  napi_create_int32(env, auth_tag_len, &auth_tag_len_value);
  napi_value call_argv[8] = {
      algorithm_value != nullptr ? algorithm_value : Undefined(env),
      key_value != nullptr ? key_value : Undefined(env),
      iv_value != nullptr ? iv_value : Undefined(env),
      input_value != nullptr ? input_value : Undefined(env),
      decrypt_value != nullptr ? decrypt_value : Undefined(env),
      aad_value != nullptr ? aad_value : Undefined(env),
      auth_tag_value != nullptr ? auth_tag_value : Undefined(env),
      auth_tag_len_value != nullptr ? auth_tag_len_value : Undefined(env),
  };
  napi_value output_info = nullptr;
  if (!CallBindingMethod(env, binding, "cipherTransformAead", 8, call_argv, &output_info) || output_info == nullptr) {
    napi_value result = nullptr;
    CaptureCallFailureAsJobResult(
        env, "ERR_CRYPTO_OPERATION_FAILED", "ChaCha20-Poly1305 operation failed", &result);
    return FinalizeJobRunResult(env, this_arg, wrap, result);
  }

  napi_value output_value = nullptr;
  if (napi_get_named_property(env, output_info, "output", &output_value) != napi_ok || output_value == nullptr) {
    napi_value result =
        BuildJobErrorResult(env, "ERR_CRYPTO_OPERATION_FAILED", "ChaCha20-Poly1305 operation failed");
    return FinalizeJobRunResult(env, this_arg, wrap, result);
  }
  std::vector<uint8_t> output = ValueToBytes(env, output_value);
  if (cipher_mode == 0) {
    napi_value tag_value = nullptr;
    if (napi_get_named_property(env, output_info, "authTag", &tag_value) == napi_ok && tag_value != nullptr &&
        !IsUndefined(env, tag_value)) {
      const std::vector<uint8_t> tag = ValueToBytes(env, tag_value);
      output.insert(output.end(), tag.begin(), tag.end());
    }
  }
  return FinalizeJobRunResult(env, this_arg, wrap, BuildArrayBufferJobResult(env, output));
}

napi_value Argon2JobRun(napi_env env, napi_callback_info info) {
  napi_value this_arg = nullptr;
  size_t argc = 0;
  napi_get_cb_info(env, info, &argc, nullptr, &this_arg, nullptr);
  JobWrap* wrap = UnwrapJob(env, this_arg);
  if (wrap == nullptr || wrap->args.size() < 9) {
    napi_value result = BuildJobErrorResult(env, "ERR_CRYPTO_OPERATION_FAILED", "Invalid Argon2 arguments");
    return FinalizeJobRunResult(env, this_arg, wrap, result);
  }
#if OPENSSL_VERSION_NUMBER >= 0x30200000L && !defined(OPENSSL_NO_ARGON2)
  const std::vector<uint8_t> pass = ValueToBytes(env, GetRefValue(env, wrap->args[0]));
  const std::vector<uint8_t> salt = ValueToBytes(env, GetRefValue(env, wrap->args[1]));
  const uint32_t lanes = static_cast<uint32_t>(GetInt32Value(env, GetRefValue(env, wrap->args[2]), 0));
  const uint32_t keylen = static_cast<uint32_t>(GetInt32Value(env, GetRefValue(env, wrap->args[3]), 0));
  const uint32_t memcost = static_cast<uint32_t>(GetInt32Value(env, GetRefValue(env, wrap->args[4]), 0));
  const uint32_t iter = static_cast<uint32_t>(GetInt32Value(env, GetRefValue(env, wrap->args[5]), 0));
  const std::vector<uint8_t> secret = ValueToBytes(env, GetRefValue(env, wrap->args[6]));
  const std::vector<uint8_t> ad = ValueToBytes(env, GetRefValue(env, wrap->args[7]));
  const int32_t type = GetInt32Value(env, GetRefValue(env, wrap->args[8]), 2);

  ncrypto::Argon2Type argon_type = ncrypto::Argon2Type::ARGON2ID;
  if (type == 0) {
    argon_type = ncrypto::Argon2Type::ARGON2D;
  } else if (type == 1) {
    argon_type = ncrypto::Argon2Type::ARGON2I;
  }

  ncrypto::DataPointer output = ncrypto::argon2({reinterpret_cast<const char*>(pass.data()), pass.size()},
                                                {salt.data(), salt.size()},
                                                lanes,
                                                keylen,
                                                memcost,
                                                iter,
                                                0x13,
                                                {secret.data(), secret.size()},
                                                {ad.data(), ad.size()},
                                                argon_type);
  if (!output && keylen != 0) {
    napi_value result = BuildJobErrorResult(env, "ERR_CRYPTO_INVALID_ARGON2_PARAMS", "Invalid Argon2 parameters");
    return FinalizeJobRunResult(env, this_arg, wrap, result);
  }

  std::vector<uint8_t> out;
  if (output) {
    out.assign(static_cast<const uint8_t*>(output.get()),
               static_cast<const uint8_t*>(output.get()) + output.size());
  }
  return FinalizeJobRunResult(env, this_arg, wrap, BuildArrayBufferJobResult(env, out));
#else
  napi_value result = BuildJobErrorResult(env, "ERR_CRYPTO_ARGON2_NOT_SUPPORTED", "Argon2 is not supported");
  return FinalizeJobRunResult(env, this_arg, wrap, result);
#endif
}

napi_value KEMEncapsulateJobRun(napi_env env, napi_callback_info info) {
  napi_value this_arg = nullptr;
  size_t argc = 0;
  napi_get_cb_info(env, info, &argc, nullptr, &this_arg, nullptr);
  JobWrap* wrap = UnwrapJob(env, this_arg);
  if (wrap == nullptr || wrap->args.empty()) {
    napi_value result = BuildJobErrorResult(env, "ERR_CRYPTO_OPERATION_FAILED", "Invalid KEM arguments");
    return FinalizeJobRunResult(env, this_arg, wrap, result);
  }
#if OPENSSL_VERSION_MAJOR >= 3
  std::string error_code;
  std::string error_message;
  EVP_PKEY* pkey = GetAsymmetricKeyFromValue(
      env, GetRefValue(env, wrap->args[0]), GetRefValue(env, wrap->args[3]), false, &error_code, &error_message);
  if (pkey == nullptr) {
    napi_value result = BuildJobErrorResult(env, error_code.c_str(), error_message);
    return FinalizeJobRunResult(env, this_arg, wrap, result);
  }

  ncrypto::EVPKeyPointer key_ptr(pkey);
  auto kem_result = ncrypto::KEM::Encapsulate(key_ptr);
  if (!kem_result) {
    napi_value result =
        BuildJobOpenSslErrorResult(env, "ERR_CRYPTO_OPERATION_FAILED", "Failed to perform encapsulation");
    return FinalizeJobRunResult(env, this_arg, wrap, result);
  }

  napi_value out = nullptr;
  napi_create_array_with_length(env, 2, &out);
  if (out == nullptr) {
    napi_value result = BuildJobErrorResult(env, "ERR_CRYPTO_OPERATION_FAILED", "Failed to allocate result");
    return FinalizeJobRunResult(env, this_arg, wrap, result);
  }

  std::vector<uint8_t> shared_key(static_cast<const uint8_t*>(kem_result->shared_key.get()),
                                  static_cast<const uint8_t*>(kem_result->shared_key.get()) +
                                      kem_result->shared_key.size());
  std::vector<uint8_t> ciphertext(static_cast<const uint8_t*>(kem_result->ciphertext.get()),
                                  static_cast<const uint8_t*>(kem_result->ciphertext.get()) +
                                      kem_result->ciphertext.size());
  napi_set_element(env, out, 0, BytesToBuffer(env, shared_key));
  napi_set_element(env, out, 1, BytesToBuffer(env, ciphertext));
  return FinalizeJobRunResult(env, this_arg, wrap, BuildJobResult(env, nullptr, out));
#else
  napi_value result = BuildJobErrorResult(env, "ERR_CRYPTO_KEM_NOT_SUPPORTED", "KEM is not supported");
  return FinalizeJobRunResult(env, this_arg, wrap, result);
#endif
}

napi_value KEMDecapsulateJobRun(napi_env env, napi_callback_info info) {
  napi_value this_arg = nullptr;
  size_t argc = 0;
  napi_get_cb_info(env, info, &argc, nullptr, &this_arg, nullptr);
  JobWrap* wrap = UnwrapJob(env, this_arg);
  if (wrap == nullptr || wrap->args.size() < 5) {
    napi_value result = BuildJobErrorResult(env, "ERR_CRYPTO_OPERATION_FAILED", "Invalid KEM arguments");
    return FinalizeJobRunResult(env, this_arg, wrap, result);
  }
#if OPENSSL_VERSION_MAJOR >= 3
  std::string error_code;
  std::string error_message;
  EVP_PKEY* pkey = GetAsymmetricKeyFromValue(
      env, GetRefValue(env, wrap->args[0]), GetRefValue(env, wrap->args[3]), true, &error_code, &error_message);
  if (pkey == nullptr) {
    napi_value result = BuildJobErrorResult(env, error_code.c_str(), error_message);
    return FinalizeJobRunResult(env, this_arg, wrap, result);
  }

  const std::vector<uint8_t> ciphertext = ValueToBytes(env, GetRefValue(env, wrap->args[4]));
  ncrypto::EVPKeyPointer key_ptr(pkey);
  ncrypto::DataPointer output = ncrypto::KEM::Decapsulate(key_ptr, {ciphertext.data(), ciphertext.size()});
  if (!output) {
    napi_value result = wrap->mode == kCryptoJobAsync
                            ? BuildJobErrorResult(env, "ERR_CRYPTO_OPERATION_FAILED", "Deriving bits failed")
                            : BuildJobOpenSslErrorResult(
                                  env, "ERR_CRYPTO_OPERATION_FAILED", "Failed to perform decapsulation");
    return FinalizeJobRunResult(env, this_arg, wrap, result);
  }

  std::vector<uint8_t> out(static_cast<const uint8_t*>(output.get()),
                           static_cast<const uint8_t*>(output.get()) + output.size());
  return FinalizeJobRunResult(env, this_arg, wrap, BuildJobResult(env, nullptr, BytesToBuffer(env, out)));
#else
  napi_value result = BuildJobErrorResult(env, "ERR_CRYPTO_KEM_NOT_SUPPORTED", "KEM is not supported");
  return FinalizeJobRunResult(env, this_arg, wrap, result);
#endif
}

napi_value RandomBytesJobRun(napi_env env, napi_callback_info info) {
  napi_value this_arg = nullptr;
  size_t argc = 0;
  napi_get_cb_info(env, info, &argc, nullptr, &this_arg, nullptr);
  JobWrap* wrap = UnwrapJob(env, this_arg);
  if (wrap == nullptr || wrap->args.size() < 3) return BuildJobResult(env, nullptr, Undefined(env));
  napi_value buffer = GetRefValue(env, wrap->args[0]);
  napi_value offset = GetRefValue(env, wrap->args[1]);
  napi_value size = GetRefValue(env, wrap->args[2]);
  std::vector<napi_value> argsv = {buffer != nullptr ? buffer : Undefined(env),
                                   offset != nullptr ? offset : Undefined(env),
                                   size != nullptr ? size : Undefined(env)};
  napi_value result = RunSyncCall(env, this_arg, "randomFillSync", argsv);
  if (wrap->mode == kCryptoJobAsync) {
    InvokeJobOnDone(env, this_arg, result);
    return Undefined(env);
  }
  return result;
}

napi_value PBKDF2JobRun(napi_env env, napi_callback_info info) {
  napi_value this_arg = nullptr;
  size_t argc = 0;
  napi_get_cb_info(env, info, &argc, nullptr, &this_arg, nullptr);
  JobWrap* wrap = UnwrapJob(env, this_arg);
  if (wrap == nullptr || wrap->args.size() < 5) return BuildJobResult(env, nullptr, Undefined(env));
  napi_value password = NormalizeToBufferObject(env, GetRefValue(env, wrap->args[0]));
  napi_value salt = NormalizeToBufferObject(env, GetRefValue(env, wrap->args[1]));
  napi_value iterations = GetRefValue(env, wrap->args[2]);
  napi_value keylen = GetRefValue(env, wrap->args[3]);
  napi_value digest = GetRefValue(env, wrap->args[4]);
  std::vector<napi_value> argsv = {
      password != nullptr ? password : Undefined(env),
      salt != nullptr ? salt : Undefined(env),
      iterations != nullptr ? iterations : Undefined(env),
      keylen != nullptr ? keylen : Undefined(env),
      digest != nullptr ? digest : Undefined(env),
  };
  napi_value result = RunSyncCall(env, this_arg, "pbkdf2Sync", argsv);
  if (result != nullptr && !IsUndefined(env, result)) {
    napi_value err = nullptr;
    napi_value value = nullptr;
    if (napi_get_element(env, result, 0, &err) == napi_ok &&
        napi_get_element(env, result, 1, &value) == napi_ok &&
        err != nullptr) {
      if (value != nullptr && !IsUndefined(env, value)) {
        result = BuildJobResult(env, err, CopyAsArrayBuffer(env, value));
      }
    }
  }
  if (wrap->mode == kCryptoJobAsync) {
    InvokeJobOnDone(env, this_arg, result);
    return Undefined(env);
  }
  return result;
}

napi_value ScryptJobRun(napi_env env, napi_callback_info info) {
  napi_value this_arg = nullptr;
  size_t argc = 0;
  napi_get_cb_info(env, info, &argc, nullptr, &this_arg, nullptr);
  JobWrap* wrap = UnwrapJob(env, this_arg);
  if (wrap == nullptr || wrap->args.size() < 7) return BuildJobResult(env, nullptr, Undefined(env));
  napi_value password = NormalizeToBufferObject(env, GetRefValue(env, wrap->args[0]));
  napi_value salt = NormalizeToBufferObject(env, GetRefValue(env, wrap->args[1]));
  napi_value N = GetRefValue(env, wrap->args[2]);
  napi_value r = GetRefValue(env, wrap->args[3]);
  napi_value p = GetRefValue(env, wrap->args[4]);
  napi_value maxmem = GetRefValue(env, wrap->args[5]);
  napi_value keylen = GetRefValue(env, wrap->args[6]);
  std::vector<napi_value> argsv = {password, salt, keylen, N, r, p, maxmem};
  napi_value result = RunSyncCall(env, this_arg, "scryptSync", argsv);
  if (wrap->mode == kCryptoJobAsync) {
    InvokeJobOnDone(env, this_arg, result);
    return Undefined(env);
  }
  return result;
}

napi_value HKDFJobRun(napi_env env, napi_callback_info info) {
  napi_value this_arg = nullptr;
  size_t argc = 0;
  napi_get_cb_info(env, info, &argc, nullptr, &this_arg, nullptr);
  JobWrap* wrap = UnwrapJob(env, this_arg);
  if (wrap == nullptr || wrap->args.size() < 5) return BuildJobResult(env, nullptr, Undefined(env));
  napi_value digest = GetRefValue(env, wrap->args[0]);
  napi_value key = GetRefValue(env, wrap->args[1]);
  napi_value salt = EnsureBufferValue(env, GetRefValue(env, wrap->args[2]));
  napi_value info_arg = EnsureBufferValue(env, GetRefValue(env, wrap->args[3]));
  napi_value keylen = GetRefValue(env, wrap->args[4]);
  std::vector<napi_value> argsv = {
      digest != nullptr ? digest : Undefined(env),
      key != nullptr ? key : Undefined(env),
      salt != nullptr ? salt : Undefined(env),
      info_arg != nullptr ? info_arg : Undefined(env),
      keylen != nullptr ? keylen : Undefined(env),
  };
  napi_value result = RunSyncCall(env, this_arg, "hkdfSync", argsv);
  if (result != nullptr && !IsUndefined(env, result)) {
    napi_value err = nullptr;
    napi_value value = nullptr;
    if (napi_get_element(env, result, 0, &err) == napi_ok &&
        napi_get_element(env, result, 1, &value) == napi_ok &&
        err != nullptr) {
      if (!IsUndefined(env, err) && !IsNullOrUndefinedValue(env, err)) {
        napi_value code = nullptr;
        size_t code_len = 0;
        if (napi_get_named_property(env, err, "code", &code) == napi_ok &&
            code != nullptr &&
            napi_get_value_string_utf8(env, code, nullptr, 0, &code_len) == napi_ok) {
          std::string code_text(code_len + 1, '\0');
          size_t copied = 0;
          if (napi_get_value_string_utf8(env, code, code_text.data(), code_text.size(), &copied) == napi_ok) {
            code_text.resize(copied);
            if (code_text == "ERR_INVALID_ARG_TYPE") {
              napi_value binding = GetBinding(env);
              if (!IsSupportedDigestName(env, binding, digest)) {
                napi_value mapped = nullptr;
                if (napi_create_string_utf8(env, "ERR_CRYPTO_INVALID_DIGEST", NAPI_AUTO_LENGTH, &mapped) == napi_ok &&
                    mapped != nullptr) {
                  napi_set_named_property(env, err, "code", mapped);
                }
              }
            }
          }
        }
      }
      if (value != nullptr && !IsUndefined(env, value)) {
        const napi_value converted = CopyAsArrayBuffer(env, value);
        result = BuildJobResult(env, err, converted);
      } else {
        result = BuildJobResult(env, err, value);
      }
    } else if (value != nullptr && !IsUndefined(env, value)) {
      const napi_value converted = CopyAsArrayBuffer(env, value);
      result = BuildJobResult(env, err, converted);
    }
  }
  if (wrap->mode == kCryptoJobAsync) {
    InvokeJobOnDone(env, this_arg, result);
    return Undefined(env);
  }
  return result;
}

napi_value HashJobRun(napi_env env, napi_callback_info info) {
  napi_value this_arg = nullptr;
  size_t argc = 0;
  napi_get_cb_info(env, info, &argc, nullptr, &this_arg, nullptr);
  JobWrap* wrap = UnwrapJob(env, this_arg);
  if (wrap == nullptr || wrap->args.size() < 2) return BuildJobResult(env, nullptr, Undefined(env));
  std::vector<napi_value> argsv;
  bool has_output_length = false;
  for (size_t i = 0; i < wrap->args.size(); ++i) {
    napi_value value = GetRefValue(env, wrap->args[i]);
    if (i == 2 && value != nullptr && !IsUndefined(env, value) && !IsNullOrUndefinedValue(env, value)) {
      has_output_length = true;
      uint32_t length_bits = 0;
      if (napi_get_value_uint32(env, value, &length_bits) == napi_ok) {
        napi_value length_bytes = nullptr;
        if (napi_create_uint32(env, length_bits / CHAR_BIT, &length_bytes) == napi_ok && length_bytes != nullptr) {
          value = length_bytes;
        }
      }
    }
    argsv.push_back(value);
  }
  const char* method = has_output_length ? "hashOneShotXof" : "hashOneShot";
  napi_value result = RunSyncCall(env, this_arg, method, argsv);
  if (result != nullptr && !IsUndefined(env, result)) {
    napi_value err = nullptr;
    napi_value value = nullptr;
    if (napi_get_element(env, result, 0, &err) == napi_ok &&
        napi_get_element(env, result, 1, &value) == napi_ok &&
        value != nullptr && !IsUndefined(env, value)) {
      result = BuildJobResult(env, err, CopyAsArrayBuffer(env, value));
    }
  }
  if (wrap->mode == kCryptoJobAsync) {
    InvokeJobOnDone(env, this_arg, result);
    return Undefined(env);
  }
  return result;
}

napi_value SignJobRun(napi_env env, napi_callback_info info) {
  napi_value this_arg = nullptr;
  size_t argc = 0;
  napi_get_cb_info(env, info, &argc, nullptr, &this_arg, nullptr);
  JobWrap* wrap = UnwrapJob(env, this_arg);
  if (wrap == nullptr) return BuildJobResult(env, nullptr, Undefined(env));
  if (wrap->args.size() < 11) {
    napi_value result = BuildJobErrorResult(env, "ERR_CRYPTO_OPERATION_FAILED", "Invalid sign job arguments");
    return FinalizeJobRunResult(env, this_arg, wrap, result);
  }

  int32_t job_mode = kSignJobModeSign;
  napi_value mode_v = GetRefValue(env, wrap->args[0]);
  if (mode_v != nullptr) napi_get_value_int32(env, mode_v, &job_mode);

  napi_value key = GetRefValue(env, wrap->args[1]);
  napi_value key_format = GetRefValue(env, wrap->args[2]);
  napi_value key_type = GetRefValue(env, wrap->args[3]);
  napi_value key_passphrase = GetRefValue(env, wrap->args[4]);
  napi_value data = EnsureBufferValue(env, GetRefValue(env, wrap->args[5]));
  napi_value algorithm = GetRefValue(env, wrap->args[6]);
  napi_value pss_salt_len = GetRefValue(env, wrap->args[7]);
  napi_value rsa_padding = GetRefValue(env, wrap->args[8]);
  napi_value dsa_sig_enc = GetRefValue(env, wrap->args[9]);
  napi_value context = GetRefValue(env, wrap->args[10]);

  napi_value result = nullptr;
  if (job_mode == kSignJobModeVerify) {
    napi_value signature =
        wrap->args.size() >= 12 ? EnsureBufferValue(env, GetRefValue(env, wrap->args[11])) : Undefined(env);
    std::vector<napi_value> call_args = {
        algorithm != nullptr ? algorithm : Undefined(env),
        data != nullptr ? data : Undefined(env),
        key != nullptr ? key : Undefined(env),
        key_format != nullptr ? key_format : Undefined(env),
        key_type != nullptr ? key_type : Undefined(env),
        key_passphrase != nullptr ? key_passphrase : Undefined(env),
        signature != nullptr ? signature : Undefined(env),
        rsa_padding != nullptr ? rsa_padding : Undefined(env),
        pss_salt_len != nullptr ? pss_salt_len : Undefined(env),
        dsa_sig_enc != nullptr ? dsa_sig_enc : Undefined(env),
        context != nullptr ? context : Undefined(env),
    };
    result = RunSyncCall(env, this_arg, "verifyOneShot", call_args);
  } else {
    std::vector<napi_value> call_args = {
        algorithm != nullptr ? algorithm : Undefined(env),
        data != nullptr ? data : Undefined(env),
        key != nullptr ? key : Undefined(env),
        key_format != nullptr ? key_format : Undefined(env),
        key_type != nullptr ? key_type : Undefined(env),
        key_passphrase != nullptr ? key_passphrase : Undefined(env),
        rsa_padding != nullptr ? rsa_padding : Undefined(env),
        pss_salt_len != nullptr ? pss_salt_len : Undefined(env),
        dsa_sig_enc != nullptr ? dsa_sig_enc : Undefined(env),
        context != nullptr ? context : Undefined(env),
    };
    result = RunSyncCall(env, this_arg, "signOneShot", call_args);
  }

  if (wrap->mode == kCryptoJobAsync) {
    InvokeJobOnDone(env, this_arg, result);
    return Undefined(env);
  }
  if (job_mode != kSignJobModeSign) {
    return result;
  }

  napi_value err = nullptr;
  napi_value signature = nullptr;
  if (result != nullptr) {
    napi_get_element(env, result, 0, &err);
    napi_get_element(env, result, 1, &signature);
  }
  if (err == nullptr || IsUndefined(env, err)) {
    return BuildArrayBufferJobResult(env, signature);
  }
  return result;
}

napi_value RSACipherJobRun(napi_env env, napi_callback_info info) {
  napi_value this_arg = nullptr;
  size_t argc = 0;
  napi_get_cb_info(env, info, &argc, nullptr, &this_arg, nullptr);
  JobWrap* wrap = UnwrapJob(env, this_arg);
  if (wrap == nullptr || wrap->args.size() < 5) {
    napi_value result = BuildJobErrorResult(env, "ERR_CRYPTO_OPERATION_FAILED", "Invalid RSA cipher arguments");
    return FinalizeJobRunResult(env, this_arg, wrap, result);
  }

  const int32_t mode = GetInt32Value(env, GetRefValue(env, wrap->args[0]), 0);
  KeyObjectWrap* key_wrap = UnwrapKeyObject(env, GetRefValue(env, wrap->args[1]));
  if (key_wrap == nullptr) {
    napi_value result = BuildJobErrorResult(env, "ERR_CRYPTO_INVALID_KEY_OBJECT_TYPE", "Invalid key object");
    return FinalizeJobRunResult(env, this_arg, wrap, result);
  }

  const std::vector<uint8_t> data = ValueToBytes(env, GetRefValue(env, wrap->args[2]));
  const int32_t variant = GetInt32Value(env, GetRefValue(env, wrap->args[3]), kKeyVariantRSA_OAEP);
  const std::string oaep_hash = GetStringValue(env, GetRefValue(env, wrap->args[4]));
  const napi_value label_v = wrap->args.size() >= 6 ? GetRefValue(env, wrap->args[5]) : Undefined(env);
  const bool has_label = !IsNullOrUndefinedValue(env, label_v);
  const std::vector<uint8_t> label = has_label ? ValueToBytes(env, label_v) : std::vector<uint8_t>{};

  if (variant != kKeyVariantRSA_OAEP) {
    napi_value result = BuildJobErrorResult(env, "ERR_CRYPTO_OPERATION_FAILED", "Unsupported RSA cipher variant");
    return FinalizeJobRunResult(env, this_arg, wrap, result);
  }

  EVP_PKEY* pkey = ParseKeyObjectAsymmetricKey(key_wrap);
  if (pkey == nullptr) {
    napi_value result =
        BuildJobOpenSslErrorResult(env, "ERR_CRYPTO_OPERATION_FAILED", "Failed to parse key");
    return FinalizeJobRunResult(env, this_arg, wrap, result);
  }

  EVP_PKEY_CTX* ctx = EVP_PKEY_CTX_new(pkey, nullptr);
  const bool encrypt = (mode == 0);
  if (ctx == nullptr ||
      (encrypt ? EVP_PKEY_encrypt_init(ctx) : EVP_PKEY_decrypt_init(ctx)) != 1 ||
      EVP_PKEY_CTX_set_rsa_padding(ctx, RSA_PKCS1_OAEP_PADDING) != 1) {
    if (ctx != nullptr) EVP_PKEY_CTX_free(ctx);
    EVP_PKEY_free(pkey);
    napi_value result =
        BuildJobOpenSslErrorResult(env, "ERR_CRYPTO_OPERATION_FAILED", "RSA cipher initialization failed");
    return FinalizeJobRunResult(env, this_arg, wrap, result);
  }

  const EVP_MD* md = EVP_get_digestbyname(oaep_hash.empty() ? "sha1" : oaep_hash.c_str());
  if (md == nullptr ||
      EVP_PKEY_CTX_set_rsa_oaep_md(ctx, md) != 1 ||
      EVP_PKEY_CTX_set_rsa_mgf1_md(ctx, md) != 1) {
    EVP_PKEY_CTX_free(ctx);
    EVP_PKEY_free(pkey);
    napi_value result = BuildJobErrorResult(env, "ERR_CRYPTO_INVALID_DIGEST", "Invalid OAEP digest");
    return FinalizeJobRunResult(env, this_arg, wrap, result);
  }

  if (has_label && !label.empty()) {
    unsigned char* copied = reinterpret_cast<unsigned char*>(OPENSSL_malloc(label.size()));
    if (copied == nullptr) {
      EVP_PKEY_CTX_free(ctx);
      EVP_PKEY_free(pkey);
      napi_value result = BuildJobErrorResult(env, "ERR_CRYPTO_OPERATION_FAILED", "Failed to allocate OAEP label");
      return FinalizeJobRunResult(env, this_arg, wrap, result);
    }
    std::memcpy(copied, label.data(), label.size());
    if (EVP_PKEY_CTX_set0_rsa_oaep_label(ctx, copied, static_cast<int>(label.size())) != 1) {
      OPENSSL_free(copied);
      EVP_PKEY_CTX_free(ctx);
      EVP_PKEY_free(pkey);
      napi_value result = BuildJobOpenSslErrorResult(env, "ERR_CRYPTO_OPERATION_FAILED", "Failed to set OAEP label");
      return FinalizeJobRunResult(env, this_arg, wrap, result);
    }
  }

  const unsigned char* input = data.empty() ? nullptr : data.data();
  size_t out_len = 0;
  const int first_ok = encrypt ? EVP_PKEY_encrypt(ctx, nullptr, &out_len, input, data.size())
                               : EVP_PKEY_decrypt(ctx, nullptr, &out_len, input, data.size());
  if (first_ok != 1) {
    EVP_PKEY_CTX_free(ctx);
    EVP_PKEY_free(pkey);
    napi_value result = BuildJobOpenSslErrorResult(env, "ERR_CRYPTO_OPERATION_FAILED", "RSA cipher failed");
    return FinalizeJobRunResult(env, this_arg, wrap, result);
  }

  std::vector<uint8_t> out(out_len);
  const int second_ok = encrypt ? EVP_PKEY_encrypt(ctx, out.data(), &out_len, input, data.size())
                                : EVP_PKEY_decrypt(ctx, out.data(), &out_len, input, data.size());
  EVP_PKEY_CTX_free(ctx);
  EVP_PKEY_free(pkey);
  if (second_ok != 1) {
    napi_value result = BuildJobOpenSslErrorResult(env, "ERR_CRYPTO_OPERATION_FAILED", "RSA cipher failed");
    return FinalizeJobRunResult(env, this_arg, wrap, result);
  }

  napi_value value = CreateArrayBufferCopy(env, out.data(), out_len);
  return FinalizeJobRunResult(env, this_arg, wrap, BuildJobResult(env, nullptr, value));
}

napi_value CryptoOneShotDigest(napi_env env, napi_callback_info info) {
  size_t argc = 7;
  napi_value argv[7] = {nullptr, nullptr, nullptr, nullptr, nullptr, nullptr, nullptr};
  napi_get_cb_info(env, info, &argc, argv, nullptr, nullptr);
  napi_value binding = GetBinding(env);
  if (binding == nullptr || argc < 4) return Undefined(env);

  napi_value algorithm = argv[0];
  napi_value input = EnsureBufferValue(env, argv[3]);
  bool has_output_length = argc >= 7 && argv[6] != nullptr && !IsUndefined(env, argv[6]);

  napi_value out = nullptr;
  if (has_output_length) {
    napi_value call_argv[3] = {algorithm, input, argv[6]};
    CallBindingMethod(env, binding, "hashOneShotXof", 3, call_argv, &out);
  }
  if (out == nullptr) {
    napi_value call_argv[2] = {algorithm, input};
    if (!CallBindingMethod(env, binding, "hashOneShot", 2, call_argv, &out)) return Undefined(env);
  }
  napi_value as_buffer = EnsureBufferValue(env, out);
  return MaybeToEncodedOutput(env, as_buffer, argc >= 5 ? argv[4] : nullptr);
}

napi_value CryptoBindingPassthrough(napi_env env,
                                    napi_callback_info info,
                                    const char* method_name,
                                    size_t expected_argv) {
  std::vector<napi_value> argv(expected_argv, nullptr);
  size_t argc = expected_argv;
  napi_get_cb_info(env, info, &argc, argv.data(), nullptr, nullptr);
  napi_value binding = GetBinding(env);
  if (binding == nullptr) return Undefined(env);

  napi_value out = nullptr;
  if (CallBindingMethod(env, binding, method_name, argc, argv.data(), &out)) {
    return out != nullptr ? out : Undefined(env);
  }
  return nullptr;
}

napi_value CryptoPublicEncryptBridge(napi_env env, napi_callback_info info) {
  return CryptoBindingPassthrough(env, info, "publicEncrypt", 8);
}

napi_value CryptoPrivateDecryptBridge(napi_env env, napi_callback_info info) {
  return CryptoBindingPassthrough(env, info, "privateDecrypt", 8);
}

napi_value CryptoPrivateEncryptBridge(napi_env env, napi_callback_info info) {
  return CryptoBindingPassthrough(env, info, "privateEncrypt", 8);
}

napi_value CryptoPublicDecryptBridge(napi_env env, napi_callback_info info) {
  return CryptoBindingPassthrough(env, info, "publicDecrypt", 8);
}

napi_value CryptoTimingSafeEqual(napi_env env, napi_callback_info info) {
  size_t argc = 2;
  napi_value argv[2] = {nullptr, nullptr};
  napi_get_cb_info(env, info, &argc, argv, nullptr, nullptr);
  if (argc < 2) return Undefined(env);

  std::vector<uint8_t> left = ValueToBytes(env, argv[0]);
  std::vector<uint8_t> right = ValueToBytes(env, argv[1]);
  if (left.size() != right.size()) {
    napi_throw_range_error(env, nullptr, "Input buffers must have the same byte length");
    return nullptr;
  }
  uint8_t diff = 0;
  for (size_t i = 0; i < left.size(); ++i) diff |= static_cast<uint8_t>(left[i] ^ right[i]);
  napi_value out = nullptr;
  napi_get_boolean(env, diff == 0, &out);
  return out != nullptr ? out : Undefined(env);
}

napi_value CryptoSecureBuffer(napi_env env, napi_callback_info info) {
  size_t argc = 1;
  napi_value argv[1] = {nullptr};
  napi_get_cb_info(env, info, &argc, argv, nullptr, nullptr);
  int64_t size = 0;
  if (argc >= 1 && argv[0] != nullptr) napi_get_value_int64(env, argv[0], &size);
  if (size < 0) size = 0;
  napi_value ab = nullptr;
  void* data = nullptr;
  if (napi_create_arraybuffer(env, static_cast<size_t>(size), &data, &ab) != napi_ok || ab == nullptr) {
    return Undefined(env);
  }
  if (data != nullptr) std::memset(data, 0, static_cast<size_t>(size));
  napi_value out = nullptr;
  napi_create_typedarray(env, napi_uint8_array, static_cast<size_t>(size), ab, 0, &out);
  return out != nullptr ? out : Undefined(env);
}

napi_value CryptoSecureHeapUsed(napi_env env, napi_callback_info info) {
  (void)info;
#if OPENSSL_VERSION_NUMBER >= 0x10100000L
  if (CRYPTO_secure_malloc_initialized() != 1) {
    return Undefined(env);
  }
  napi_value out = nullptr;
  if (napi_create_double(env, static_cast<double>(CRYPTO_secure_used()), &out) != napi_ok || out == nullptr) {
    return Undefined(env);
  }
  return out;
#else
  return Undefined(env);
#endif
}

napi_value CryptoGetCachedAliases(napi_env env, napi_callback_info info) {
  napi_value out = nullptr;
  if (napi_create_object(env, &out) != napi_ok || out == nullptr) return Undefined(env);
  napi_value binding = GetBinding(env);
  if (binding == nullptr) return out;
  napi_value hashes = nullptr;
  if (!CallBindingMethod(env, binding, "getHashes", 0, nullptr, &hashes) || hashes == nullptr) return out;
  bool is_array = false;
  if (napi_is_array(env, hashes, &is_array) != napi_ok || !is_array) return out;
  uint32_t len = 0;
  napi_get_array_length(env, hashes, &len);
  for (uint32_t i = 0; i < len; ++i) {
    napi_value key = nullptr;
    if (napi_get_element(env, hashes, i, &key) != napi_ok || key == nullptr) continue;
    napi_value value = nullptr;
    napi_create_uint32(env, i, &value);
    if (value != nullptr) napi_set_property(env, out, key, value);
  }
  return out;
}

napi_value CryptoGetFips(napi_env env, napi_callback_info info) {
  CryptoBindingState* state = GetState(env);
  napi_value out = nullptr;
  napi_create_int32(env, state == nullptr ? 0 : state->fips_mode, &out);
  return out != nullptr ? out : Undefined(env);
}

napi_value CryptoSetFips(napi_env env, napi_callback_info info) {
  CryptoBindingState* state = GetState(env);
  if (state == nullptr) return Undefined(env);
  size_t argc = 1;
  napi_value argv[1] = {nullptr};
  napi_get_cb_info(env, info, &argc, argv, nullptr, nullptr);
  bool enabled = false;
  if (argc >= 1 && argv[0] != nullptr) napi_get_value_bool(env, argv[0], &enabled);
  state->fips_mode = enabled ? 1 : 0;
  return Undefined(env);
}

napi_value CryptoTestFips(napi_env env, napi_callback_info info) {
  napi_value out = nullptr;
  napi_get_boolean(env, true, &out);
  return out != nullptr ? out : Undefined(env);
}

napi_value CryptoGetOpenSSLSecLevel(napi_env env, napi_callback_info info) {
  napi_value out = nullptr;
  napi_create_int32(env, 0, &out);
  return out != nullptr ? out : Undefined(env);
}

napi_value CryptoGetEmptyArray(napi_env env, napi_callback_info info) {
  napi_value out = nullptr;
  napi_create_array_with_length(env, 0, &out);
  return out != nullptr ? out : Undefined(env);
}

napi_value CryptoNoop(napi_env env, napi_callback_info info) {
  return Undefined(env);
}

struct X509HandleWrap {
  napi_env env = nullptr;
  napi_ref wrapper_ref = nullptr;
  ncrypto::X509Pointer cert;
  ncrypto::X509Pointer issuer;
};

void X509HandleFinalize(napi_env env, void* data, void* /*hint*/) {
  auto* wrap = static_cast<X509HandleWrap*>(data);
  if (wrap == nullptr) return;
  if (wrap->wrapper_ref != nullptr) {
    napi_delete_reference(env, wrap->wrapper_ref);
    wrap->wrapper_ref = nullptr;
  }
  delete wrap;
}

X509HandleWrap* UnwrapX509Handle(napi_env env, napi_value value) {
  if (value == nullptr) return nullptr;
  X509HandleWrap* wrap = nullptr;
  if (napi_unwrap(env, value, reinterpret_cast<void**>(&wrap)) != napi_ok) return nullptr;
  return wrap;
}

X509HandleWrap* UnwrapX509This(napi_env env, napi_callback_info info, size_t* argc, napi_value* argv) {
  napi_value this_arg = nullptr;
  size_t actual_argc = argc != nullptr ? *argc : 0;
  napi_get_cb_info(env, info, argc != nullptr ? &actual_argc : nullptr, argv, &this_arg, nullptr);
  if (argc != nullptr) *argc = actual_argc;
  return UnwrapX509Handle(env, this_arg);
}

bool CreateNullPrototypeObjectLocal(napi_env env, napi_value* out) {
  if (out == nullptr) return false;
  *out = nullptr;
  napi_value global = GetGlobal(env);
  if (global == nullptr) return false;
  napi_value object_ctor = nullptr;
  if (napi_get_named_property(env, global, "Object", &object_ctor) != napi_ok || object_ctor == nullptr) return false;
  napi_value create_fn = nullptr;
  if (napi_get_named_property(env, object_ctor, "create", &create_fn) != napi_ok || create_fn == nullptr) return false;
  napi_value null_value = nullptr;
  if (napi_get_null(env, &null_value) != napi_ok || null_value == nullptr) return false;
  napi_value argv[1] = {null_value};
  return napi_call_function(env, object_ctor, create_fn, 1, argv, out) == napi_ok && *out != nullptr;
}

std::string BioToString(ncrypto::BIOPointer&& bio) {
  if (!bio) return {};
  BUF_MEM* mem = nullptr;
  BIO_get_mem_ptr(bio.get(), &mem);
  if (mem == nullptr || mem->data == nullptr || mem->length == 0) return {};
  return std::string(mem->data, mem->length);
}

std::vector<uint8_t> BioToBytes(ncrypto::BIOPointer&& bio) {
  if (!bio) return {};
  BUF_MEM* mem = nullptr;
  BIO_get_mem_ptr(bio.get(), &mem);
  if (mem == nullptr || mem->data == nullptr || mem->length == 0) return {};
  const auto* begin = reinterpret_cast<const uint8_t*>(mem->data);
  return std::vector<uint8_t>(begin, begin + mem->length);
}

napi_value StringValueFromUtf8(napi_env env, const std::string& text) {
  napi_value out = nullptr;
  if (napi_create_string_utf8(env, text.data(), text.size(), &out) != napi_ok) return Undefined(env);
  return out != nullptr ? out : Undefined(env);
}

napi_value StringValueFromBio(napi_env env, ncrypto::BIOPointer&& bio) {
  const std::string text = BioToString(std::move(bio));
  if (text.empty()) return Undefined(env);
  return StringValueFromUtf8(env, text);
}

napi_value BufferValueFromBio(napi_env env, ncrypto::BIOPointer&& bio) {
  return BytesToBuffer(env, BioToBytes(std::move(bio)));
}

napi_value FingerprintValue(napi_env env, const ncrypto::Digest& digest, const ncrypto::X509View& cert) {
  auto fp = cert.getFingerprint(digest);
  if (!fp.has_value()) return Undefined(env);
  return StringValueFromUtf8(env, fp.value());
}

napi_value CreateX509NameObjectValue(napi_env env, const ncrypto::X509Name& name) {
  if (!name) return Undefined(env);
  napi_value out = nullptr;
  if (!CreateNullPrototypeObjectLocal(env, &out) || out == nullptr) return Undefined(env);

  for (const auto& [key_text, value_text] : name) {
    napi_value key = StringValueFromUtf8(env, key_text);
    napi_value value = StringValueFromUtf8(env, value_text);
    if (key == nullptr || value == nullptr) return Undefined(env);

    bool has_prop = false;
    if (napi_has_property(env, out, key, &has_prop) != napi_ok) return Undefined(env);
    if (!has_prop) {
      if (napi_set_property(env, out, key, value) != napi_ok) return Undefined(env);
      continue;
    }

    napi_value current = nullptr;
    if (napi_get_property(env, out, key, &current) != napi_ok || current == nullptr) return Undefined(env);
    bool is_array = false;
    if (napi_is_array(env, current, &is_array) == napi_ok && is_array) {
      uint32_t length = 0;
      napi_get_array_length(env, current, &length);
      napi_set_element(env, current, length, value);
      continue;
    }

    napi_value array = nullptr;
    if (napi_create_array_with_length(env, 2, &array) != napi_ok || array == nullptr) return Undefined(env);
    napi_set_element(env, array, 0, current);
    napi_set_element(env, array, 1, value);
    if (napi_set_property(env, out, key, array) != napi_ok) return Undefined(env);
  }

  return out;
}

napi_value CreateKeyUsageArrayValue(napi_env env, const ncrypto::X509View& cert) {
  napi_value out = nullptr;
  if (napi_create_array(env, &out) != napi_ok || out == nullptr) return Undefined(env);
  uint32_t index = 0;
  const bool ok = cert.enumUsages([&](std::string_view usage) {
    napi_value value = StringValueFromUtf8(env, std::string(usage));
    if (value != nullptr) {
      napi_set_element(env, out, index++, value);
    }
  });
  if (!ok) return Undefined(env);
  return out;
}

napi_value CreateDateValue(napi_env env, int64_t seconds_since_epoch) {
  napi_value out = nullptr;
  if (napi_create_date(env, static_cast<double>(seconds_since_epoch) * 1000.0, &out) != napi_ok) {
    return Undefined(env);
  }
  return out != nullptr ? out : Undefined(env);
}

napi_value CreateHexStringValue(napi_env env, const BIGNUM* bn) {
  if (bn == nullptr) return Undefined(env);
  char* hex = BN_bn2hex(bn);
  if (hex == nullptr) return Undefined(env);
  napi_value out = nullptr;
  napi_create_string_utf8(env, hex, NAPI_AUTO_LENGTH, &out);
  OPENSSL_free(hex);
  return out != nullptr ? out : Undefined(env);
}

napi_value CreateExponentStringValue(napi_env env, const BIGNUM* bn) {
  if (bn == nullptr) return Undefined(env);
  char text[32] = {0};
  std::snprintf(text, sizeof(text), "0x%" PRIx64, static_cast<uint64_t>(ncrypto::BignumPointer::GetWord(bn)));
  return StringValueFromUtf8(env, text);
}

napi_value CreateRsaPublicKeyBuffer(napi_env env, const ncrypto::Rsa& rsa) {
  const int size = i2d_RSA_PUBKEY(rsa, nullptr);
  if (size <= 0) return Undefined(env);
  std::vector<uint8_t> out(static_cast<size_t>(size));
  unsigned char* write_ptr = out.data();
  if (i2d_RSA_PUBKEY(rsa, &write_ptr) != size) return Undefined(env);
  return BytesToBuffer(env, out);
}

napi_value CreateEcPublicKeyBuffer(napi_env env, const ncrypto::Ec& ec) {
  const EC_GROUP* group = ec.getGroup();
  const EC_POINT* point = ncrypto::ECKeyPointer::GetPublicKey(ec);
  if (group == nullptr || point == nullptr) return Undefined(env);
  const point_conversion_form_t form = EC_KEY_get_conv_form(ec);
  const size_t size = EC_POINT_point2oct(group, point, form, nullptr, 0, nullptr);
  if (size == 0) return Undefined(env);
  std::vector<uint8_t> out(size);
  if (EC_POINT_point2oct(group, point, form, out.data(), out.size(), nullptr) != size) return Undefined(env);
  return BytesToBuffer(env, out);
}

napi_value CreateCurveNameValue(napi_env env, const char* (*nid_to_name)(int), int nid) {
  if (nid == 0 || nid_to_name == nullptr) return Undefined(env);
  const char* name = nid_to_name(nid);
  if (name == nullptr || name[0] == '\0') return Undefined(env);
  return StringValueFromUtf8(env, name);
}

bool SetNamedPropertyIfDefined(napi_env env, napi_value target, const char* key, napi_value value) {
  if (target == nullptr || key == nullptr || value == nullptr || IsUndefined(env, value)) return true;
  return napi_set_named_property(env, target, key, value) == napi_ok;
}

napi_value CreateLegacyX509ObjectValue(napi_env env, const ncrypto::X509View& cert) {
  napi_value out = nullptr;
  if (!CreateNullPrototypeObjectLocal(env, &out) || out == nullptr) return Undefined(env);

  auto serial = cert.getSerialNumber();

  if (!SetNamedPropertyIfDefined(env, out, "subject", CreateX509NameObjectValue(env, cert.getSubjectName())) ||
      !SetNamedPropertyIfDefined(env, out, "issuer", CreateX509NameObjectValue(env, cert.getIssuerName())) ||
      !SetNamedPropertyIfDefined(env, out, "subjectaltname", StringValueFromBio(env, cert.getSubjectAltName())) ||
      !SetNamedPropertyIfDefined(env, out, "infoAccess", StringValueFromBio(env, cert.getInfoAccess())) ||
      !SetNamedPropertyIfDefined(env, out, "valid_from", StringValueFromBio(env, cert.getValidFrom())) ||
      !SetNamedPropertyIfDefined(env, out, "valid_to", StringValueFromBio(env, cert.getValidTo())) ||
      !SetNamedPropertyIfDefined(env, out, "fingerprint", FingerprintValue(env, ncrypto::Digest::SHA1, cert)) ||
      !SetNamedPropertyIfDefined(env, out, "fingerprint256", FingerprintValue(env, ncrypto::Digest::SHA256, cert)) ||
      !SetNamedPropertyIfDefined(env, out, "fingerprint512", FingerprintValue(env, ncrypto::Digest::SHA512, cert)) ||
      !SetNamedPropertyIfDefined(env, out, "ext_key_usage", CreateKeyUsageArrayValue(env, cert)) ||
      !SetNamedPropertyIfDefined(env,
                                 out,
                                 "serialNumber",
                                 serial ? StringValueFromUtf8(env, static_cast<const char*>(serial.get())) : Undefined(env))) {
    return Undefined(env);
  }

  napi_value ca = nullptr;
  napi_get_boolean(env, cert.isCA(), &ca);
  if (!SetNamedPropertyIfDefined(env, out, "ca", ca)) return Undefined(env);
  if (!SetNamedPropertyIfDefined(env, out, "raw", BufferValueFromBio(env, cert.toDER()))) return Undefined(env);

  cert.ifRsa([&](const ncrypto::Rsa& rsa) {
    const auto pub = rsa.getPublicKey();
    SetNamedPropertyIfDefined(env, out, "modulus", CreateHexStringValue(env, pub.n));
    SetNamedPropertyIfDefined(env, out, "exponent", CreateExponentStringValue(env, pub.e));
    SetNamedPropertyIfDefined(env, out, "pubkey", CreateRsaPublicKeyBuffer(env, rsa));
    if (pub.n != nullptr) {
      napi_value bits = nullptr;
      napi_create_int32(env, BN_num_bits(pub.n), &bits);
      SetNamedPropertyIfDefined(env, out, "bits", bits);
    }
    return true;
  });

  cert.ifEc([&](const ncrypto::Ec& ec) {
    SetNamedPropertyIfDefined(env, out, "pubkey", CreateEcPublicKeyBuffer(env, ec));
    const EC_GROUP* group = ec.getGroup();
    if (group != nullptr) {
      napi_value bits = nullptr;
      napi_create_int32(env, EC_GROUP_order_bits(group), &bits);
      SetNamedPropertyIfDefined(env, out, "bits", bits);
    }
    const int nid = ec.getCurve();
    SetNamedPropertyIfDefined(env, out, "asn1Curve", CreateCurveNameValue(env, OBJ_nid2sn, nid));
    SetNamedPropertyIfDefined(env, out, "nistCurve", CreateCurveNameValue(env, EC_curve_nid2nist, nid));
    return true;
  });

  return out;
}

napi_value CreateX509HandleValue(napi_env env,
                                 ncrypto::X509Pointer&& cert,
                                 ncrypto::X509Pointer&& issuer = ncrypto::X509Pointer()) {
  napi_value binding = GetBinding(env);
  if (binding == nullptr) return Undefined(env);
  napi_value ctor = nullptr;
  if (napi_get_named_property(env, binding, "X509CertificateHandle", &ctor) != napi_ok || ctor == nullptr) {
    return Undefined(env);
  }
  napi_value handle = nullptr;
  if (napi_new_instance(env, ctor, 0, nullptr, &handle) != napi_ok || handle == nullptr) return Undefined(env);
  X509HandleWrap* wrap = UnwrapX509Handle(env, handle);
  if (wrap == nullptr) return Undefined(env);
  wrap->cert = std::move(cert);
  wrap->issuer = std::move(issuer);
  return handle;
}

napi_value X509HandleConstructor(napi_env env, napi_callback_info info) {
  napi_value this_arg = nullptr;
  napi_get_cb_info(env, info, nullptr, nullptr, &this_arg, nullptr);
  if (this_arg == nullptr) return Undefined(env);
  auto* wrap = new X509HandleWrap();
  wrap->env = env;
  if (napi_wrap(env, this_arg, wrap, X509HandleFinalize, nullptr, &wrap->wrapper_ref) != napi_ok) {
    delete wrap;
    return Undefined(env);
  }
  return this_arg;
}

napi_value X509HandleSubject(napi_env env, napi_callback_info info) {
  X509HandleWrap* wrap = UnwrapX509This(env, info, nullptr, nullptr);
  return wrap != nullptr ? StringValueFromBio(env, wrap->cert.view().getSubject()) : Undefined(env);
}

napi_value X509HandleSubjectAltName(napi_env env, napi_callback_info info) {
  X509HandleWrap* wrap = UnwrapX509This(env, info, nullptr, nullptr);
  return wrap != nullptr ? StringValueFromBio(env, wrap->cert.view().getSubjectAltName()) : Undefined(env);
}

napi_value X509HandleIssuer(napi_env env, napi_callback_info info) {
  X509HandleWrap* wrap = UnwrapX509This(env, info, nullptr, nullptr);
  return wrap != nullptr ? StringValueFromBio(env, wrap->cert.view().getIssuer()) : Undefined(env);
}

napi_value X509HandleInfoAccess(napi_env env, napi_callback_info info) {
  X509HandleWrap* wrap = UnwrapX509This(env, info, nullptr, nullptr);
  return wrap != nullptr ? StringValueFromBio(env, wrap->cert.view().getInfoAccess()) : Undefined(env);
}

napi_value X509HandleValidFrom(napi_env env, napi_callback_info info) {
  X509HandleWrap* wrap = UnwrapX509This(env, info, nullptr, nullptr);
  return wrap != nullptr ? StringValueFromBio(env, wrap->cert.view().getValidFrom()) : Undefined(env);
}

napi_value X509HandleValidTo(napi_env env, napi_callback_info info) {
  X509HandleWrap* wrap = UnwrapX509This(env, info, nullptr, nullptr);
  return wrap != nullptr ? StringValueFromBio(env, wrap->cert.view().getValidTo()) : Undefined(env);
}

napi_value X509HandleValidFromDate(napi_env env, napi_callback_info info) {
  X509HandleWrap* wrap = UnwrapX509This(env, info, nullptr, nullptr);
  return wrap != nullptr ? CreateDateValue(env, wrap->cert.view().getValidFromTime()) : Undefined(env);
}

napi_value X509HandleValidToDate(napi_env env, napi_callback_info info) {
  X509HandleWrap* wrap = UnwrapX509This(env, info, nullptr, nullptr);
  return wrap != nullptr ? CreateDateValue(env, wrap->cert.view().getValidToTime()) : Undefined(env);
}

napi_value X509HandleFingerprint(napi_env env, napi_callback_info info) {
  X509HandleWrap* wrap = UnwrapX509This(env, info, nullptr, nullptr);
  return wrap != nullptr ? FingerprintValue(env, ncrypto::Digest::SHA1, wrap->cert.view()) : Undefined(env);
}

napi_value X509HandleFingerprint256(napi_env env, napi_callback_info info) {
  X509HandleWrap* wrap = UnwrapX509This(env, info, nullptr, nullptr);
  return wrap != nullptr ? FingerprintValue(env, ncrypto::Digest::SHA256, wrap->cert.view()) : Undefined(env);
}

napi_value X509HandleFingerprint512(napi_env env, napi_callback_info info) {
  X509HandleWrap* wrap = UnwrapX509This(env, info, nullptr, nullptr);
  return wrap != nullptr ? FingerprintValue(env, ncrypto::Digest::SHA512, wrap->cert.view()) : Undefined(env);
}

napi_value X509HandleKeyUsage(napi_env env, napi_callback_info info) {
  X509HandleWrap* wrap = UnwrapX509This(env, info, nullptr, nullptr);
  return wrap != nullptr ? CreateKeyUsageArrayValue(env, wrap->cert.view()) : Undefined(env);
}

napi_value X509HandleSerialNumber(napi_env env, napi_callback_info info) {
  X509HandleWrap* wrap = UnwrapX509This(env, info, nullptr, nullptr);
  if (wrap == nullptr) return Undefined(env);
  auto serial = wrap->cert.view().getSerialNumber();
  if (!serial) return Undefined(env);
  return StringValueFromUtf8(env, static_cast<const char*>(serial.get()));
}

napi_value X509HandleSignatureAlgorithm(napi_env env, napi_callback_info info) {
  X509HandleWrap* wrap = UnwrapX509This(env, info, nullptr, nullptr);
  if (wrap == nullptr) return Undefined(env);
  auto value = wrap->cert.view().getSignatureAlgorithm();
  return value.has_value() ? StringValueFromUtf8(env, std::string(value.value())) : Undefined(env);
}

napi_value X509HandleSignatureAlgorithmOid(napi_env env, napi_callback_info info) {
  X509HandleWrap* wrap = UnwrapX509This(env, info, nullptr, nullptr);
  if (wrap == nullptr) return Undefined(env);
  auto value = wrap->cert.view().getSignatureAlgorithmOID();
  return value.has_value() ? StringValueFromUtf8(env, value.value()) : Undefined(env);
}

napi_value X509HandleRaw(napi_env env, napi_callback_info info) {
  X509HandleWrap* wrap = UnwrapX509This(env, info, nullptr, nullptr);
  return wrap != nullptr ? BufferValueFromBio(env, wrap->cert.view().toDER()) : Undefined(env);
}

napi_value X509HandlePem(napi_env env, napi_callback_info info) {
  X509HandleWrap* wrap = UnwrapX509This(env, info, nullptr, nullptr);
  return wrap != nullptr ? StringValueFromBio(env, wrap->cert.view().toPEM()) : Undefined(env);
}

napi_value X509HandlePublicKey(napi_env env, napi_callback_info info) {
  X509HandleWrap* wrap = UnwrapX509This(env, info, nullptr, nullptr);
  if (wrap == nullptr) return Undefined(env);
  auto result = wrap->cert.view().getPublicKey();
  if (!result.value) {
    napi_throw(env, CreateErrorWithCode(env, "ERR_CRYPTO_OPERATION_FAILED", "Failed to get public key"));
    return nullptr;
  }
  return CreateNativeKeyObjectHandleValue(env, kKeyTypePublic, result.value.get());
}

napi_value X509HandleCheckCA(napi_env env, napi_callback_info info) {
  X509HandleWrap* wrap = UnwrapX509This(env, info, nullptr, nullptr);
  napi_value out = nullptr;
  napi_get_boolean(env, wrap != nullptr && wrap->cert.view().isCA(), &out);
  return out != nullptr ? out : Undefined(env);
}

napi_value X509HandleCheckHost(napi_env env, napi_callback_info info) {
  size_t argc = 2;
  napi_value argv[2] = {nullptr, nullptr};
  X509HandleWrap* wrap = UnwrapX509This(env, info, &argc, argv);
  if (wrap == nullptr || argc < 2) return Undefined(env);
  const std::string name = GetStringValue(env, argv[0]);
  uint32_t flags = 0;
  napi_get_value_uint32(env, argv[1], &flags);
  ncrypto::DataPointer peer_name;
  switch (wrap->cert.view().checkHost(name, static_cast<int>(flags), &peer_name)) {
    case ncrypto::X509View::CheckMatch::MATCH:
      return peer_name ? StringValueFromUtf8(env, std::string(static_cast<const char*>(peer_name.get()), peer_name.size()))
                       : argv[0];
    case ncrypto::X509View::CheckMatch::NO_MATCH:
      return Undefined(env);
    case ncrypto::X509View::CheckMatch::INVALID_NAME:
      napi_throw(env, CreateErrorWithCode(env, "ERR_INVALID_ARG_VALUE", "Invalid name"));
      return nullptr;
    default:
      napi_throw(env, CreateErrorWithCode(env, "ERR_CRYPTO_OPERATION_FAILED", "Failed to check host"));
      return nullptr;
  }
}

napi_value X509HandleCheckEmail(napi_env env, napi_callback_info info) {
  size_t argc = 2;
  napi_value argv[2] = {nullptr, nullptr};
  X509HandleWrap* wrap = UnwrapX509This(env, info, &argc, argv);
  if (wrap == nullptr || argc < 2) return Undefined(env);
  const std::string name = GetStringValue(env, argv[0]);
  uint32_t flags = 0;
  napi_get_value_uint32(env, argv[1], &flags);
  switch (wrap->cert.view().checkEmail(name, static_cast<int>(flags))) {
    case ncrypto::X509View::CheckMatch::MATCH:
      return argv[0];
    case ncrypto::X509View::CheckMatch::NO_MATCH:
      return Undefined(env);
    case ncrypto::X509View::CheckMatch::INVALID_NAME:
      napi_throw(env, CreateErrorWithCode(env, "ERR_INVALID_ARG_VALUE", "Invalid name"));
      return nullptr;
    default:
      napi_throw(env, CreateErrorWithCode(env, "ERR_CRYPTO_OPERATION_FAILED", "Failed to check email"));
      return nullptr;
  }
}

napi_value X509HandleCheckIP(napi_env env, napi_callback_info info) {
  size_t argc = 2;
  napi_value argv[2] = {nullptr, nullptr};
  X509HandleWrap* wrap = UnwrapX509This(env, info, &argc, argv);
  if (wrap == nullptr || argc < 2) return Undefined(env);
  const std::string ip = GetStringValue(env, argv[0]);
  uint32_t flags = 0;
  napi_get_value_uint32(env, argv[1], &flags);
  switch (wrap->cert.view().checkIp(ip, static_cast<int>(flags))) {
    case ncrypto::X509View::CheckMatch::MATCH:
      return argv[0];
    case ncrypto::X509View::CheckMatch::NO_MATCH:
      return Undefined(env);
    case ncrypto::X509View::CheckMatch::INVALID_NAME:
      napi_throw(env, CreateErrorWithCode(env, "ERR_INVALID_ARG_VALUE", "Invalid IP"));
      return nullptr;
    default:
      napi_throw(env, CreateErrorWithCode(env, "ERR_CRYPTO_OPERATION_FAILED", "Failed to check IP"));
      return nullptr;
  }
}

napi_value X509HandleCheckIssued(napi_env env, napi_callback_info info) {
  size_t argc = 1;
  napi_value argv[1] = {nullptr};
  X509HandleWrap* wrap = UnwrapX509This(env, info, &argc, argv);
  if (wrap == nullptr || argc < 1) return Undefined(env);
  X509HandleWrap* issuer = UnwrapX509Handle(env, argv[0]);
  napi_value out = nullptr;
  napi_get_boolean(env, issuer != nullptr && wrap->cert.view().isIssuedBy(issuer->cert.view()), &out);
  return out != nullptr ? out : Undefined(env);
}

napi_value X509HandleCheckPrivateKey(napi_env env, napi_callback_info info) {
  size_t argc = 1;
  napi_value argv[1] = {nullptr};
  X509HandleWrap* wrap = UnwrapX509This(env, info, &argc, argv);
  if (wrap == nullptr || argc < 1) return Undefined(env);
  std::string error_code;
  std::string error_message;
  EVP_PKEY* pkey = GetAsymmetricKeyFromValue(env, nullptr, argv[0], true, &error_code, &error_message);
  if (pkey == nullptr) return Undefined(env);
  ncrypto::EVPKeyPointer key(pkey);
  napi_value out = nullptr;
  napi_get_boolean(env, wrap->cert.view().checkPrivateKey(key), &out);
  return out != nullptr ? out : Undefined(env);
}

napi_value X509HandleVerify(napi_env env, napi_callback_info info) {
  size_t argc = 1;
  napi_value argv[1] = {nullptr};
  X509HandleWrap* wrap = UnwrapX509This(env, info, &argc, argv);
  if (wrap == nullptr || argc < 1) return Undefined(env);
  std::string error_code;
  std::string error_message;
  EVP_PKEY* pkey = GetAsymmetricKeyFromValue(env, nullptr, argv[0], true, &error_code, &error_message);
  if (pkey == nullptr) return Undefined(env);
  ncrypto::EVPKeyPointer key(pkey);
  napi_value out = nullptr;
  napi_get_boolean(env, wrap->cert.view().checkPublicKey(key), &out);
  return out != nullptr ? out : Undefined(env);
}

napi_value X509HandleToLegacy(napi_env env, napi_callback_info info) {
  X509HandleWrap* wrap = UnwrapX509This(env, info, nullptr, nullptr);
  return wrap != nullptr ? CreateLegacyX509ObjectValue(env, wrap->cert.view()) : Undefined(env);
}

napi_value X509HandleGetIssuerCert(napi_env env, napi_callback_info info) {
  X509HandleWrap* wrap = UnwrapX509This(env, info, nullptr, nullptr);
  if (wrap == nullptr || !wrap->issuer) return Undefined(env);
  return CreateX509HandleValue(env, wrap->issuer.view().clone());
}

napi_value CryptoGetSSLCiphers(napi_env env, napi_callback_info info) {
  napi_value binding = GetBinding(env);
  if (binding != nullptr) {
    napi_value out = nullptr;
    if (CallBindingMethod(env, binding, "getSSLCiphers", 0, nullptr, &out) && out != nullptr) return out;
    if (CallBindingMethod(env, binding, "getCiphers", 0, nullptr, &out) && out != nullptr) return out;
  }
  napi_value empty = nullptr;
  napi_create_array_with_length(env, 0, &empty);
  return empty != nullptr ? empty : Undefined(env);
}

napi_value CryptoParseX509(napi_env env, napi_callback_info info) {
  size_t argc = 2;
  napi_value argv[2] = {nullptr, nullptr};
  napi_get_cb_info(env, info, &argc, argv, nullptr, nullptr);
  if (argc < 1 || argv[0] == nullptr) return Undefined(env);
  const std::vector<uint8_t> data = ValueToBytes(env, argv[0]);
  auto result = ncrypto::X509Pointer::Parse({data.data(), data.size()});
  if (!result.value) {
    napi_throw(env, CreateErrorWithCode(env, "ERR_CRYPTO_OPERATION_FAILED", "Failed to parse X509 certificate"));
    return nullptr;
  }
  ncrypto::X509Pointer issuer;
  if (argc >= 2 && argv[1] != nullptr && !IsUndefined(env, argv[1]) && !IsNullOrUndefinedValue(env, argv[1])) {
    const std::vector<uint8_t> issuer_data = ValueToBytes(env, argv[1]);
    auto issuer_result = ncrypto::X509Pointer::Parse({issuer_data.data(), issuer_data.size()});
    if (!issuer_result.value) {
      napi_throw(env, CreateErrorWithCode(env, "ERR_CRYPTO_OPERATION_FAILED", "Failed to parse issuer certificate"));
      return nullptr;
    }
    issuer = std::move(issuer_result.value);
  }
  return CreateX509HandleValue(env, std::move(result.value), std::move(issuer));
}

napi_value CryptoCreateNativeKeyObjectClass(napi_env env, napi_callback_info info) {
  size_t argc = 1;
  napi_value argv[1] = {nullptr};
  napi_get_cb_info(env, info, &argc, argv, nullptr, nullptr);
  if (argc < 1 || argv[0] == nullptr) return Undefined(env);
  napi_valuetype type = napi_undefined;
  if (napi_typeof(env, argv[0], &type) != napi_ok || type != napi_function) return Undefined(env);

  napi_property_descriptor props[] = {
      {"constructor", nullptr, nullptr, nullptr, nullptr, nullptr, napi_default, nullptr},
  };
  napi_value native_ctor = nullptr;
  auto ctor = [](napi_env env, napi_callback_info info) -> napi_value {
    size_t argc = 1;
    napi_value argv[1] = {nullptr};
    napi_value this_arg = nullptr;
    napi_get_cb_info(env, info, &argc, argv, &this_arg, nullptr);
    if (this_arg == nullptr) return Undefined(env);
    auto* wrap = new NativeKeyObjectWrap();
    if (napi_wrap(env, this_arg, wrap, NativeKeyObjectFinalize, nullptr, &wrap->wrapper_ref) != napi_ok) {
      delete wrap;
      return Undefined(env);
    }

    if (argc >= 1 && argv[0] != nullptr) {
      KeyObjectWrap* handle = UnwrapKeyObject(env, argv[0]);
      if (handle == nullptr) {
        napi_throw(env, CreateErrorWithCode(env, "ERR_CRYPTO_OPERATION_FAILED", "Failed to clone key object"));
        return nullptr;
      }
      wrap->key_type = handle->key_type;
      wrap->key_data = handle->key_data;
      wrap->key_passphrase = handle->key_passphrase;
      wrap->has_key_passphrase = handle->has_key_passphrase;
      if (handle->native_pkey != nullptr && EVP_PKEY_up_ref(handle->native_pkey) == 1) {
        wrap->native_pkey = handle->native_pkey;
      }
    }
    return this_arg != nullptr ? this_arg : Undefined(env);
  };
  if (napi_define_class(env,
                        "NativeKeyObject",
                        NAPI_AUTO_LENGTH,
                        ctor,
                        nullptr,
                        sizeof(props) / sizeof(props[0]),
                        props,
                        &native_ctor) != napi_ok ||
      native_ctor == nullptr) {
    return Undefined(env);
  }

  napi_value global = GetGlobal(env);
  napi_value call_argv[1] = {native_ctor};
  napi_value out = nullptr;
  if (napi_call_function(env, global, argv[0], 1, call_argv, &out) != napi_ok || out == nullptr) {
    return Undefined(env);
  }
  (void)StoreNativeKeyObjectConstructorsFromExports(env, out);
  return out;
}

void EnsurePropertyMethod(napi_env env, napi_value binding, const char* name, napi_callback cb) {
  bool has = false;
  if (napi_has_named_property(env, binding, name, &has) != napi_ok || has) return;
  SetNamedMethod(env, binding, name, cb);
}

napi_value CryptoUnsupportedMethod(napi_env env, napi_callback_info info) {
  napi_throw_error(env, "ERR_CRYPTO_UNSUPPORTED_OPERATION", "Unsupported crypto operation");
  return nullptr;
}

napi_value CryptoUnsupportedCtor(napi_env env, napi_callback_info info) {
  napi_throw_error(env, "ERR_CRYPTO_UNSUPPORTED_OPERATION", "Unsupported crypto constructor");
  return nullptr;
}

void EnsureUndefinedProperty(napi_env env, napi_value binding, const char* name) {
  bool has = false;
  if (napi_has_named_property(env, binding, name, &has) != napi_ok || has) return;
  napi_set_named_property(env, binding, name, Undefined(env));
}

void EnsureStubMethod(napi_env env, napi_value binding, const char* name) {
  bool has = false;
  if (napi_has_named_property(env, binding, name, &has) != napi_ok || has) return;
  SetNamedMethod(env, binding, name, CryptoUnsupportedMethod);
}

void EnsureStubClass(napi_env env, napi_value binding, const char* name) {
  bool has = false;
  if (napi_has_named_property(env, binding, name, &has) == napi_ok && has) return;
  napi_value cls = nullptr;
  if (napi_define_class(env, name, NAPI_AUTO_LENGTH, CryptoUnsupportedCtor, nullptr, 0, nullptr, &cls) == napi_ok &&
      cls != nullptr) {
    napi_set_named_property(env, binding, name, cls);
  }
}

void EnsureClass(napi_env env,
                 napi_value binding,
                 const char* name,
                 napi_callback ctor,
                 const std::vector<napi_property_descriptor>& methods) {
  bool has = false;
  if (napi_has_named_property(env, binding, name, &has) == napi_ok && has) return;
  std::vector<napi_property_descriptor> descriptors = methods;
  for (auto& desc : descriptors) {
    if (desc.method != nullptr) {
      desc.attributes = static_cast<napi_property_attributes>(napi_writable | napi_configurable);
    }
  }
  napi_value cls = nullptr;
  if (napi_define_class(env,
                        name,
                        NAPI_AUTO_LENGTH,
                        ctor,
                        nullptr,
                        descriptors.size(),
                        descriptors.data(),
                        &cls) == napi_ok &&
      cls != nullptr) {
    napi_set_named_property(env, binding, name, cls);
  }
}

}  // namespace

napi_value InitCrypto(napi_env env) {
  napi_value out = EdgeInstallCryptoBinding(env);
  if (out == nullptr || IsUndefined(env, out)) return Undefined(env);

  auto& st = EnsureState(env);
  if (st.binding_ref == nullptr) napi_create_reference(env, out, 1, &st.binding_ref);

  // Core constants.
  const std::pair<const char*, int32_t> constants[] = {
      {"kCryptoJobAsync", kCryptoJobAsync},
      {"kCryptoJobSync", kCryptoJobSync},
      {"kSignJobModeSign", kSignJobModeSign},
      {"kSignJobModeVerify", kSignJobModeVerify},
      {"kWebCryptoCipherEncrypt", 0},
      {"kWebCryptoCipherDecrypt", 1},
      {"kKeyTypeSecret", kKeyTypeSecret},
      {"kKeyTypePublic", kKeyTypePublic},
      {"kKeyTypePrivate", kKeyTypePrivate},
      {"kKeyFormatDER", 0},
      {"kKeyFormatPEM", 1},
      {"kKeyFormatJWK", 2},
      {"kKeyEncodingPKCS1", 0},
      {"kKeyEncodingPKCS8", 1},
      {"kKeyEncodingSPKI", 2},
      {"kKeyEncodingSEC1", 3},
      {"kSigEncDER", 0},
      {"kSigEncP1363", 1},
      {"kKEMEncapsulate", 0},
      {"kKEMDecapsulate", 1},
      {"kTypeArgon2d", 0},
      {"kTypeArgon2i", 1},
      {"kTypeArgon2id", 2},
      {"kWebCryptoKeyFormatRaw", 0},
      {"kWebCryptoKeyFormatPKCS8", 1},
      {"kWebCryptoKeyFormatSPKI", 2},
      {"kWebCryptoKeyFormatJWK", 3},
      {"kKeyVariantRSA_SSA_PKCS1_v1_5", 0},
      {"kKeyVariantRSA_PSS", 1},
      {"kKeyVariantRSA_OAEP", 2},
      {"kKeyVariantAES_CTR_128", 0},
      {"kKeyVariantAES_CTR_192", 1},
      {"kKeyVariantAES_CTR_256", 2},
      {"kKeyVariantAES_CBC_128", 3},
      {"kKeyVariantAES_CBC_192", 4},
      {"kKeyVariantAES_CBC_256", 5},
      {"kKeyVariantAES_GCM_128", 6},
      {"kKeyVariantAES_GCM_192", 7},
      {"kKeyVariantAES_GCM_256", 8},
      {"kKeyVariantAES_KW_128", 9},
      {"kKeyVariantAES_KW_192", 10},
      {"kKeyVariantAES_KW_256", 11},
      {"kKeyVariantAES_OCB_128", 12},
      {"kKeyVariantAES_OCB_192", 13},
      {"kKeyVariantAES_OCB_256", 14},
      {"OPENSSL_EC_EXPLICIT_CURVE", 0},
      {"OPENSSL_EC_NAMED_CURVE", 1},
      {"RSA_PKCS1_PADDING", RSA_PKCS1_PADDING},
      {"RSA_NO_PADDING", RSA_NO_PADDING},
      {"RSA_PKCS1_OAEP_PADDING", RSA_PKCS1_OAEP_PADDING},
      {"RSA_X931_PADDING", RSA_X931_PADDING},
      {"RSA_PKCS1_PSS_PADDING", RSA_PKCS1_PSS_PADDING},
      {"RSA_PSS_SALTLEN_DIGEST", RSA_PSS_SALTLEN_DIGEST},
      {"RSA_PSS_SALTLEN_MAX_SIGN", RSA_PSS_SALTLEN_MAX_SIGN},
      {"RSA_PSS_SALTLEN_AUTO", RSA_PSS_SALTLEN_AUTO},
      {"POINT_CONVERSION_COMPRESSED", POINT_CONVERSION_COMPRESSED},
      {"POINT_CONVERSION_UNCOMPRESSED", POINT_CONVERSION_UNCOMPRESSED},
      {"POINT_CONVERSION_HYBRID", POINT_CONVERSION_HYBRID},
      {"X509_CHECK_FLAG_ALWAYS_CHECK_SUBJECT", 1},
      {"X509_CHECK_FLAG_NO_WILDCARDS", 2},
      {"X509_CHECK_FLAG_NO_PARTIAL_WILDCARDS", 4},
      {"X509_CHECK_FLAG_MULTI_LABEL_WILDCARDS", 8},
      {"X509_CHECK_FLAG_SINGLE_LABEL_SUBDOMAINS", 16},
      {"X509_CHECK_FLAG_NEVER_CHECK_SUBJECT", 32},
      {"EVP_PKEY_ED25519", 1087},
      {"EVP_PKEY_ED448", 1088},
      {"EVP_PKEY_X25519", 1034},
      {"EVP_PKEY_X448", 1035},
  };
  for (const auto& [name, value] : constants) {
    bool has = false;
    if (napi_has_named_property(env, out, name, &has) == napi_ok && !has) SetNamedInt(env, out, name, value);
  }
#if OPENSSL_WITH_PQC
  const std::pair<const char*, int32_t> pqc_constants[] = {
      {"EVP_PKEY_ML_DSA_44", 1457},
      {"EVP_PKEY_ML_DSA_65", 1458},
      {"EVP_PKEY_ML_DSA_87", 1459},
      {"EVP_PKEY_ML_KEM_512", 1454},
      {"EVP_PKEY_ML_KEM_768", 1455},
      {"EVP_PKEY_ML_KEM_1024", 1456},
      {"EVP_PKEY_SLH_DSA_SHA2_128S", 1460},
      {"EVP_PKEY_SLH_DSA_SHA2_128F", 1461},
      {"EVP_PKEY_SLH_DSA_SHA2_192S", 1462},
      {"EVP_PKEY_SLH_DSA_SHA2_192F", 1463},
      {"EVP_PKEY_SLH_DSA_SHA2_256S", 1464},
      {"EVP_PKEY_SLH_DSA_SHA2_256F", 1465},
      {"EVP_PKEY_SLH_DSA_SHAKE_128S", 1466},
      {"EVP_PKEY_SLH_DSA_SHAKE_128F", 1467},
      {"EVP_PKEY_SLH_DSA_SHAKE_192S", 1468},
      {"EVP_PKEY_SLH_DSA_SHAKE_192F", 1469},
      {"EVP_PKEY_SLH_DSA_SHAKE_256S", 1470},
      {"EVP_PKEY_SLH_DSA_SHAKE_256F", 1471},
  };
  for (const auto& [name, value] : pqc_constants) {
    bool has = false;
    if (napi_has_named_property(env, out, name, &has) == napi_ok && !has) SetNamedInt(env, out, name, value);
  }
#endif

  EnsurePropertyMethod(env, out, "oneShotDigest", CryptoOneShotDigest);
  EnsurePropertyMethod(env, out, "timingSafeEqual", CryptoTimingSafeEqual);
  EnsurePropertyMethod(env, out, "secureBuffer", CryptoSecureBuffer);
  EnsurePropertyMethod(env, out, "secureHeapUsed", CryptoSecureHeapUsed);
  EnsurePropertyMethod(env, out, "getCachedAliases", CryptoGetCachedAliases);
  EnsurePropertyMethod(env, out, "getFipsCrypto", CryptoGetFips);
  EnsurePropertyMethod(env, out, "setFipsCrypto", CryptoSetFips);
  EnsurePropertyMethod(env, out, "testFipsCrypto", CryptoTestFips);
  EnsurePropertyMethod(env, out, "getOpenSSLSecLevelCrypto", CryptoGetOpenSSLSecLevel);
  EnsurePropertyMethod(env, out, "getBundledRootCertificates", edge::crypto::CryptoGetBundledRootCertificates);
  EnsurePropertyMethod(env, out, "getExtraCACertificates", edge::crypto::CryptoGetExtraCACertificates);
  EnsurePropertyMethod(env, out, "getSystemCACertificates", edge::crypto::CryptoGetSystemCACertificates);
  EnsurePropertyMethod(env, out, "getUserRootCertificates", edge::crypto::CryptoGetUserRootCertificates);
  EnsurePropertyMethod(env, out, "resetRootCertStore", edge::crypto::CryptoResetRootCertStore);
  EnsurePropertyMethod(env, out, "startLoadingCertificatesOffThread", edge::crypto::CryptoStartLoadingCertificatesOffThread);
  EnsurePropertyMethod(env, out, "createNativeKeyObjectClass", CryptoCreateNativeKeyObjectClass);
  EnsurePropertyMethod(env, out, "ECDHConvertKey", EcdhConvertKey);
  EnsureStubMethod(env, out, "setEngine");
  EnsurePropertyMethod(env, out, "publicEncrypt", CryptoPublicEncryptBridge);
  EnsurePropertyMethod(env, out, "privateDecrypt", CryptoPrivateDecryptBridge);
  EnsurePropertyMethod(env, out, "privateEncrypt", CryptoPrivateEncryptBridge);
  EnsurePropertyMethod(env, out, "publicDecrypt", CryptoPublicDecryptBridge);
  EnsurePropertyMethod(env, out, "certExportChallenge", CertExportChallenge);
  EnsurePropertyMethod(env, out, "certExportPublicKey", CertExportPublicKey);
  EnsurePropertyMethod(env, out, "certVerifySpkac", CertVerifySpkac);
  EnsurePropertyMethod(env, out, "parseX509", CryptoParseX509);
  EnsurePropertyMethod(env, out, "getSSLCiphers", CryptoGetSSLCiphers);

  EnsureClass(env,
              out,
              "Hash",
              HashCtor,
              {
                  {"update", nullptr, HashUpdate, nullptr, nullptr, nullptr, napi_default, nullptr},
                  {"digest", nullptr, HashDigest, nullptr, nullptr, nullptr, napi_default, nullptr},
              });

  EnsureClass(env,
              out,
              "Hmac",
              HmacCtor,
              {
                  {"init", nullptr, HmacInit, nullptr, nullptr, nullptr, napi_default, nullptr},
                  {"update", nullptr, HmacUpdate, nullptr, nullptr, nullptr, napi_default, nullptr},
                  {"digest", nullptr, HmacDigest, nullptr, nullptr, nullptr, napi_default, nullptr},
              });

  EnsureClass(env,
              out,
              "Sign",
              SignCtor,
              {
                  {"init", nullptr, SignVerifyInit, nullptr, nullptr, nullptr, napi_default, nullptr},
                  {"update", nullptr, SignVerifyUpdate, nullptr, nullptr, nullptr, napi_default, nullptr},
                  {"sign", nullptr, SignSign, nullptr, nullptr, nullptr, napi_default, nullptr},
              });

  EnsureClass(env,
              out,
              "Verify",
              VerifyCtor,
              {
                  {"init", nullptr, SignVerifyInit, nullptr, nullptr, nullptr, napi_default, nullptr},
                  {"update", nullptr, SignVerifyUpdate, nullptr, nullptr, nullptr, napi_default, nullptr},
                  {"verify", nullptr, VerifyVerify, nullptr, nullptr, nullptr, napi_default, nullptr},
              });

  EnsureClass(env,
              out,
              "CipherBase",
              CipherBaseCtor,
              {
                  {"update", nullptr, CipherBaseUpdate, nullptr, nullptr, nullptr, napi_default, nullptr},
                  {"final", nullptr, CipherBaseFinal, nullptr, nullptr, nullptr, napi_default, nullptr},
                  {"setAutoPadding", nullptr, CipherBaseSetAutoPadding, nullptr, nullptr, nullptr, napi_default,
                   nullptr},
                  {"setAAD", nullptr, CipherBaseSetAAD, nullptr, nullptr, nullptr, napi_default, nullptr},
                  {"getAuthTag", nullptr, CipherBaseGetAuthTag, nullptr, nullptr, nullptr, napi_default, nullptr},
                  {"setAuthTag", nullptr, CipherBaseSetAuthTag, nullptr, nullptr, nullptr, napi_default, nullptr},
              });

  EnsureClass(env,
              out,
              "SecureContext",
              SecureContextCtor,
              {
                  {"init", nullptr, SecureContextInit, nullptr, nullptr, nullptr, napi_default, nullptr},
                  {"setOptions", nullptr, SecureContextSetOptions, nullptr, nullptr, nullptr, napi_default, nullptr},
                  {"setCiphers", nullptr, SecureContextSetCiphers, nullptr, nullptr, nullptr, napi_default, nullptr},
                  {"setCipherSuites", nullptr, SecureContextSetCipherSuites, nullptr, nullptr, nullptr, napi_default,
                   nullptr},
                  {"setCert", nullptr, SecureContextSetCert, nullptr, nullptr, nullptr, napi_default, nullptr},
                  {"setKey", nullptr, SecureContextSetKey, nullptr, nullptr, nullptr, napi_default, nullptr},
                  {"addCACert", nullptr, SecureContextAddCACert, nullptr, nullptr, nullptr, napi_default, nullptr},
                  {"addCRL", nullptr, SecureContextAddCrl, nullptr, nullptr, nullptr, napi_default, nullptr},
                  {"addRootCerts", nullptr, SecureContextAddRootCerts, nullptr, nullptr, nullptr, napi_default,
                   nullptr},
                  {"setECDHCurve", nullptr, SecureContextSetECDHCurve, nullptr, nullptr, nullptr, napi_default,
                   nullptr},
                  {"setDHParam", nullptr, SecureContextSetDHParam, nullptr, nullptr, nullptr, napi_default, nullptr},
                  {"setSigalgs", nullptr, SecureContextSetSigalgs, nullptr, nullptr, nullptr, napi_default, nullptr},
                  {"setEngineKey", nullptr, SecureContextNoop, nullptr, nullptr, nullptr, napi_default, nullptr},
                  {"setClientCertEngine", nullptr, SecureContextSetClientCertEngine, nullptr, nullptr, nullptr,
                   napi_default, nullptr},
                  {"setSessionIdContext", nullptr, SecureContextSetSessionIdContext, nullptr, nullptr, nullptr,
                   napi_default, nullptr},
                  {"setSessionTimeout", nullptr, SecureContextSetSessionTimeout, nullptr, nullptr, nullptr,
                   napi_default, nullptr},
                  {"setTicketKeys", nullptr, SecureContextSetTicketKeys, nullptr, nullptr, nullptr, napi_default,
                   nullptr},
                  {"getTicketKeys", nullptr, SecureContextGetTicketKeys, nullptr, nullptr, nullptr, napi_default,
                   nullptr},
                  {"enableTicketKeyCallback", nullptr, SecureContextEnableTicketKeyCallback, nullptr, nullptr,
                   nullptr, napi_default, nullptr},
                  {"setAllowPartialTrustChain", nullptr, SecureContextSetAllowPartialTrustChain, nullptr, nullptr,
                   nullptr, napi_default, nullptr},
                  {"getCertificate", nullptr, SecureContextGetCertificate, nullptr, nullptr, nullptr, napi_default,
                   nullptr},
                  {"getIssuer", nullptr, SecureContextGetIssuer, nullptr, nullptr, nullptr, napi_default, nullptr},
                  {"setMinProto", nullptr, SecureContextSetMinProto, nullptr, nullptr, nullptr, napi_default, nullptr},
                  {"setMaxProto", nullptr, SecureContextSetMaxProto, nullptr, nullptr, nullptr, napi_default, nullptr},
                  {"loadPKCS12", nullptr, SecureContextLoadPKCS12, nullptr, nullptr, nullptr, napi_default, nullptr},
                  {"getMinProto", nullptr, SecureContextGetMinProto, nullptr, nullptr, nullptr, napi_default, nullptr},
                  {"getMaxProto", nullptr, SecureContextGetMaxProto, nullptr, nullptr, nullptr, napi_default, nullptr},
                  {"close", nullptr, SecureContextClose, nullptr, nullptr, nullptr, napi_default, nullptr},
              });
  napi_value secure_context_ctor = nullptr;
  if (napi_get_named_property(env, out, "SecureContext", &secure_context_ctor) == napi_ok &&
      secure_context_ctor != nullptr) {
    napi_value secure_context_proto = nullptr;
    if (napi_get_named_property(env, secure_context_ctor, "prototype", &secure_context_proto) == napi_ok &&
        secure_context_proto != nullptr) {
      const napi_property_descriptor external_desc = {
          "_external",
          nullptr,
          nullptr,
          SecureContextExternalGetter,
          nullptr,
          nullptr,
          napi_default,
          nullptr,
      };
      (void)napi_define_properties(env, secure_context_proto, 1, &external_desc);
    }
  }

  EnsureClass(env,
              out,
              "KeyObjectHandle",
              KeyObjectCtor,
              {
                  {"init", nullptr, KeyObjectInit, nullptr, nullptr, nullptr, napi_default, nullptr},
                  {"initJwk", nullptr, KeyObjectInitJwk, nullptr, nullptr, nullptr, napi_default, nullptr},
                  {"exportJwk", nullptr, KeyObjectExportJwk, nullptr, nullptr, nullptr, napi_default, nullptr},
                  {"export", nullptr, KeyObjectExport, nullptr, nullptr, nullptr, napi_default, nullptr},
                  {"getAsymmetricKeyType", nullptr, KeyObjectGetAsymmetricKeyType, nullptr, nullptr, nullptr,
                   napi_default, nullptr},
                  {"checkEcKeyData", nullptr, KeyObjectCheckEcKeyData, nullptr, nullptr, nullptr, napi_default,
                   nullptr},
                  {"initECRaw", nullptr, KeyObjectInitECRaw, nullptr, nullptr, nullptr, napi_default, nullptr},
                  {"initEDRaw", nullptr, KeyObjectInitEDRaw, nullptr, nullptr, nullptr, napi_default, nullptr},
                  {"initPqcRaw", nullptr, KeyObjectInitPqcRaw, nullptr, nullptr, nullptr, napi_default, nullptr},
                  {"rawPublicKey", nullptr, KeyObjectRawPublicKey, nullptr, nullptr, nullptr, napi_default, nullptr},
                  {"rawSeed", nullptr, KeyObjectRawSeed, nullptr, nullptr, nullptr, napi_default, nullptr},
                  {"keyDetail", nullptr, KeyObjectGetAsymmetricKeyDetails, nullptr, nullptr, nullptr, napi_default,
                   nullptr},
                  {"getSymmetricKeySize", nullptr, KeyObjectGetSymmetricKeySize, nullptr, nullptr, nullptr, napi_default,
                   nullptr},
                  {"equals", nullptr, KeyObjectEquals, nullptr, nullptr, nullptr, napi_default, nullptr},
              });

  EnsureClass(env,
              out,
              "DiffieHellman",
              DiffieHellmanCtor,
              {
                  {"generateKeys", nullptr, DiffieHellmanGenerateKeys, nullptr, nullptr, nullptr, napi_default,
                   nullptr},
                  {"computeSecret", nullptr, DiffieHellmanComputeSecret, nullptr, nullptr, nullptr, napi_default,
                   nullptr},
                  {"getPrime", nullptr, DiffieHellmanGetPrime, nullptr, nullptr, nullptr, napi_default, nullptr},
                  {"getGenerator", nullptr, DiffieHellmanGetGenerator, nullptr, nullptr, nullptr, napi_default,
                   nullptr},
                  {"getPublicKey", nullptr, DiffieHellmanGetPublicKey, nullptr, nullptr, nullptr, napi_default,
                   nullptr},
                  {"getPrivateKey", nullptr, DiffieHellmanGetPrivateKey, nullptr, nullptr, nullptr, napi_default,
                   nullptr},
                  {"setPublicKey", nullptr, DiffieHellmanSetPublicKey, nullptr, nullptr, nullptr, napi_default,
                   nullptr},
                  {"setPrivateKey", nullptr, DiffieHellmanSetPrivateKey, nullptr, nullptr, nullptr, napi_default,
                   nullptr},
              });

  EnsureClass(env,
              out,
              "DiffieHellmanGroup",
              DiffieHellmanGroupCtor,
              {
                  {"generateKeys", nullptr, DiffieHellmanGenerateKeys, nullptr, nullptr, nullptr, napi_default,
                   nullptr},
                  {"computeSecret", nullptr, DiffieHellmanComputeSecret, nullptr, nullptr, nullptr, napi_default,
                   nullptr},
                  {"getPrime", nullptr, DiffieHellmanGetPrime, nullptr, nullptr, nullptr, napi_default, nullptr},
                  {"getGenerator", nullptr, DiffieHellmanGetGenerator, nullptr, nullptr, nullptr, napi_default,
                   nullptr},
                  {"getPublicKey", nullptr, DiffieHellmanGetPublicKey, nullptr, nullptr, nullptr, napi_default,
                   nullptr},
                  {"getPrivateKey", nullptr, DiffieHellmanGetPrivateKey, nullptr, nullptr, nullptr, napi_default,
                   nullptr},
              });

  EnsureClass(env,
              out,
              "ECDH",
              EcdhCtor,
              {
                  {"generateKeys", nullptr, EcdhGenerateKeys, nullptr, nullptr, nullptr, napi_default, nullptr},
                  {"computeSecret", nullptr, EcdhComputeSecret, nullptr, nullptr, nullptr, napi_default, nullptr},
                  {"getPublicKey", nullptr, EcdhGetPublicKey, nullptr, nullptr, nullptr, napi_default, nullptr},
                  {"getPrivateKey", nullptr, EcdhGetPrivateKey, nullptr, nullptr, nullptr, napi_default, nullptr},
                  {"setPublicKey", nullptr, EcdhSetPublicKey, nullptr, nullptr, nullptr, napi_default, nullptr},
                  {"setPrivateKey", nullptr, EcdhSetPrivateKey, nullptr, nullptr, nullptr, napi_default, nullptr},
              });

  EnsureClass(env,
              out,
              "RandomBytesJob",
              JobCtor,
              {
                  {"run", nullptr, RandomBytesJobRun, nullptr, nullptr, nullptr, napi_default, nullptr},
              });
  EnsureClass(env,
              out,
              "PBKDF2Job",
              PBKDF2JobCtor,
              {
                  {"run", nullptr, PBKDF2JobRun, nullptr, nullptr, nullptr, napi_default, nullptr},
              });
  EnsureClass(env,
              out,
              "ScryptJob",
              ScryptJobCtor,
              {
                  {"run", nullptr, ScryptJobRun, nullptr, nullptr, nullptr, napi_default, nullptr},
              });
  EnsureClass(env,
              out,
              "HKDFJob",
              HKDFJobCtor,
              {
                  {"run", nullptr, HKDFJobRun, nullptr, nullptr, nullptr, napi_default, nullptr},
              });
  SetClassPrototypeMethod(env, out, "HKDFJob", "run", HKDFJobRun);
  EnsureClass(env,
              out,
              "HashJob",
              JobCtor,
              {
                  {"run", nullptr, HashJobRun, nullptr, nullptr, nullptr, napi_default, nullptr},
              });
  EnsureClass(env,
              out,
              "RandomPrimeJob",
              JobCtor,
              {
                  {"run", nullptr, RandomPrimeJobRun, nullptr, nullptr, nullptr, napi_default, nullptr},
              });
  EnsureClass(env,
              out,
              "CheckPrimeJob",
              CheckPrimeJobCtor,
              {
                  {"run", nullptr, CheckPrimeJobRun, nullptr, nullptr, nullptr, napi_default, nullptr},
              });
  EnsureClass(env,
              out,
              "SignJob",
              JobCtor,
              {
                  {"run", nullptr, SignJobRun, nullptr, nullptr, nullptr, napi_default, nullptr},
              });
  EnsureClass(env,
              out,
              "RSACipherJob",
              JobCtor,
              {
                  {"run", nullptr, RSACipherJobRun, nullptr, nullptr, nullptr, napi_default, nullptr},
              });
  EnsureClass(env,
              out,
              "RsaKeyPairGenJob",
              RsaKeyPairGenJobCtor,
              {
                  {"run", nullptr, RsaKeyPairGenJobRun, nullptr, nullptr, nullptr, napi_default, nullptr},
              });
  EnsureClass(env,
              out,
              "DsaKeyPairGenJob",
              JobCtor,
              {
                  {"run", nullptr, DsaKeyPairGenJobRun, nullptr, nullptr, nullptr, napi_default, nullptr},
              });
  EnsureClass(env,
              out,
              "DhKeyPairGenJob",
              DhKeyPairGenJobCtor,
              {
                  {"run", nullptr, DhKeyPairGenJobRun, nullptr, nullptr, nullptr, napi_default, nullptr},
              });
  EnsureClass(env,
              out,
              "EcKeyPairGenJob",
              JobCtor,
              {
                  {"run", nullptr, EcKeyPairGenJobRun, nullptr, nullptr, nullptr, napi_default, nullptr},
              });
  EnsureClass(env,
              out,
              "NidKeyPairGenJob",
              JobCtor,
              {
                  {"run", nullptr, NidKeyPairGenJobRun, nullptr, nullptr, nullptr, napi_default, nullptr},
              });
  EnsureClass(env,
              out,
              "SecretKeyGenJob",
              JobCtor,
              {
                  {"run", nullptr, SecretKeyGenJobRun, nullptr, nullptr, nullptr, napi_default, nullptr},
              });
  EnsureClass(env,
              out,
              "DHBitsJob",
              JobCtor,
              {
                  {"run", nullptr, DHBitsJobRun, nullptr, nullptr, nullptr, napi_default, nullptr},
              });
  EnsureClass(env,
              out,
              "ECDHBitsJob",
              JobCtor,
              {
                  {"run", nullptr, ECDHBitsJobRun, nullptr, nullptr, nullptr, napi_default, nullptr},
              });
  EnsureClass(env,
              out,
              "X509CertificateHandle",
              X509HandleConstructor,
              {
                  {"subject", nullptr, X509HandleSubject, nullptr, nullptr, nullptr, napi_default, nullptr},
                  {"subjectAltName", nullptr, X509HandleSubjectAltName, nullptr, nullptr, nullptr, napi_default, nullptr},
                  {"issuer", nullptr, X509HandleIssuer, nullptr, nullptr, nullptr, napi_default, nullptr},
                  {"infoAccess", nullptr, X509HandleInfoAccess, nullptr, nullptr, nullptr, napi_default, nullptr},
                  {"validFrom", nullptr, X509HandleValidFrom, nullptr, nullptr, nullptr, napi_default, nullptr},
                  {"validTo", nullptr, X509HandleValidTo, nullptr, nullptr, nullptr, napi_default, nullptr},
                  {"validFromDate", nullptr, X509HandleValidFromDate, nullptr, nullptr, nullptr, napi_default, nullptr},
                  {"validToDate", nullptr, X509HandleValidToDate, nullptr, nullptr, nullptr, napi_default, nullptr},
                  {"fingerprint", nullptr, X509HandleFingerprint, nullptr, nullptr, nullptr, napi_default, nullptr},
                  {"fingerprint256", nullptr, X509HandleFingerprint256, nullptr, nullptr, nullptr, napi_default, nullptr},
                  {"fingerprint512", nullptr, X509HandleFingerprint512, nullptr, nullptr, nullptr, napi_default, nullptr},
                  {"keyUsage", nullptr, X509HandleKeyUsage, nullptr, nullptr, nullptr, napi_default, nullptr},
                  {"serialNumber", nullptr, X509HandleSerialNumber, nullptr, nullptr, nullptr, napi_default, nullptr},
                  {"signatureAlgorithm", nullptr, X509HandleSignatureAlgorithm, nullptr, nullptr, nullptr, napi_default, nullptr},
                  {"signatureAlgorithmOid", nullptr, X509HandleSignatureAlgorithmOid, nullptr, nullptr, nullptr, napi_default, nullptr},
                  {"raw", nullptr, X509HandleRaw, nullptr, nullptr, nullptr, napi_default, nullptr},
                  {"pem", nullptr, X509HandlePem, nullptr, nullptr, nullptr, napi_default, nullptr},
                  {"publicKey", nullptr, X509HandlePublicKey, nullptr, nullptr, nullptr, napi_default, nullptr},
                  {"checkCA", nullptr, X509HandleCheckCA, nullptr, nullptr, nullptr, napi_default, nullptr},
                  {"checkHost", nullptr, X509HandleCheckHost, nullptr, nullptr, nullptr, napi_default, nullptr},
                  {"checkEmail", nullptr, X509HandleCheckEmail, nullptr, nullptr, nullptr, napi_default, nullptr},
                  {"checkIP", nullptr, X509HandleCheckIP, nullptr, nullptr, nullptr, napi_default, nullptr},
                  {"checkIssued", nullptr, X509HandleCheckIssued, nullptr, nullptr, nullptr, napi_default, nullptr},
                  {"checkPrivateKey", nullptr, X509HandleCheckPrivateKey, nullptr, nullptr, nullptr, napi_default, nullptr},
                  {"verify", nullptr, X509HandleVerify, nullptr, nullptr, nullptr, napi_default, nullptr},
                  {"toLegacy", nullptr, X509HandleToLegacy, nullptr, nullptr, nullptr, napi_default, nullptr},
                  {"getIssuerCert", nullptr, X509HandleGetIssuerCert, nullptr, nullptr, nullptr, napi_default, nullptr},
              });
  EnsureClass(env,
              out,
              "AESCipherJob",
              JobCtor,
              {
                  {"run", nullptr, AESCipherJobRun, nullptr, nullptr, nullptr, napi_default, nullptr},
              });
#if OPENSSL_VERSION_NUMBER >= 0x30200000L && !defined(OPENSSL_NO_ARGON2)
  EnsureClass(env,
              out,
              "Argon2Job",
              JobCtor,
              {
                  {"run", nullptr, Argon2JobRun, nullptr, nullptr, nullptr, napi_default, nullptr},
              });
#endif
  EnsureClass(env,
              out,
              "ChaCha20Poly1305CipherJob",
              JobCtor,
              {
                  {"run", nullptr, ChaCha20Poly1305CipherJobRun, nullptr, nullptr, nullptr, napi_default, nullptr},
              });
  EnsureClass(env,
              out,
              "DHKeyExportJob",
              JobCtor,
              {
                  {"run", nullptr, DHKeyExportJobRun, nullptr, nullptr, nullptr, napi_default, nullptr},
              });
  EnsureClass(env,
              out,
              "ECKeyExportJob",
              JobCtor,
              {
                  {"run", nullptr, ECKeyExportJobRun, nullptr, nullptr, nullptr, napi_default, nullptr},
              });
  EnsureClass(env,
              out,
              "HmacJob",
              JobCtor,
              {
                  {"run", nullptr, HmacJobRun, nullptr, nullptr, nullptr, napi_default, nullptr},
              });
#if OPENSSL_VERSION_MAJOR >= 3
  EnsureClass(env,
              out,
              "KEMDecapsulateJob",
              JobCtor,
              {
                  {"run", nullptr, KEMDecapsulateJobRun, nullptr, nullptr, nullptr, napi_default, nullptr},
              });
  EnsureClass(env,
              out,
              "KEMEncapsulateJob",
              JobCtor,
              {
                  {"run", nullptr, KEMEncapsulateJobRun, nullptr, nullptr, nullptr, napi_default, nullptr},
              });
#endif
  EnsureClass(env,
              out,
              "KmacJob",
              JobCtor,
              {
                  {"run", nullptr, KmacJobRun, nullptr, nullptr, nullptr, napi_default, nullptr},
              });
  EnsureClass(env,
              out,
              "RSAKeyExportJob",
              JobCtor,
              {
                  {"run", nullptr, RSAKeyExportJobRun, nullptr, nullptr, nullptr, napi_default, nullptr},
              });

  return out;
}

bool EdgeCryptoGetSecureContextHolderFromObject(napi_env env,
                                               napi_value value,
                                               edge::crypto::SecureContextHolder** out) {
  if (out == nullptr) return false;
  *out = nullptr;
  SecureContextWrap* wrap = UnwrapSecureContext(env, value);
  if (wrap == nullptr) return false;
  *out = GetSecureContextHolderFromWrap(env, wrap);
  return *out != nullptr;
}

napi_value EdgeCryptoCreateNativeKeyObjectCloneData(napi_env env, napi_value value) {
  NativeKeyObjectWrap* wrap = UnwrapNativeKeyObject(env, value);
  if (wrap == nullptr) return nullptr;

  int32_t key_type_value = kKeyTypeSecret;
  std::vector<uint8_t> key_bytes;
  if (!ExtractNormalizedKeyCloneData(wrap->key_type,
                                     wrap->native_pkey,
                                     wrap->key_data,
                                     wrap->key_passphrase,
                                     wrap->has_key_passphrase,
                                     &key_type_value,
                                     &key_bytes)) {
    return nullptr;
  }

  napi_value data = nullptr;
  napi_value key_type = nullptr;
  napi_value key_data = nullptr;
  if (napi_create_object(env, &data) != napi_ok || data == nullptr ||
      napi_create_int32(env, key_type_value, &key_type) != napi_ok || key_type == nullptr ||
      napi_set_named_property(env, data, "type", key_type) != napi_ok) {
    return nullptr;
  }

  key_data = BytesToBuffer(env, key_bytes);
  if (key_data == nullptr || napi_set_named_property(env, data, "data", key_data) != napi_ok) {
    return nullptr;
  }

  return data;
}

napi_value EdgeCryptoCreateKeyObjectFromCloneData(napi_env env, napi_value data) {
  if (data == nullptr) return nullptr;

  napi_value type_value = nullptr;
  napi_value key_value = nullptr;
  int32_t key_type = kKeyTypeSecret;
  if (napi_get_named_property(env, data, "type", &type_value) != napi_ok ||
      type_value == nullptr ||
      napi_get_value_int32(env, type_value, &key_type) != napi_ok ||
      napi_get_named_property(env, data, "data", &key_value) != napi_ok ||
      key_value == nullptr) {
    return nullptr;
  }

  return InstantiateKeyObjectFromCloneData(env, key_type, ValueToBytes(env, key_value));
}

}  // namespace internal_binding
