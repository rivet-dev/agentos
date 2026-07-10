#include "edge_buffer.h"

#include <algorithm>
#include <cctype>
#include <climits>
#include <cmath>
#include <cstdint>
#include <cstdlib>
#include <cstring>
#include <limits>
#include <string>
#include <unordered_map>
#include <vector>

#include "edge_environment.h"
#include "edge_encoding_ids.h"
#include "edge_runtime.h"
#include "simdutf.h"

namespace {
using edge::encoding_ids::kEncAscii;
using edge::encoding_ids::kEncBase64;
using edge::encoding_ids::kEncBase64Url;
using edge::encoding_ids::kEncBuffer;
using edge::encoding_ids::kEncHex;
using edge::encoding_ids::kEncLatin1;
using edge::encoding_ids::kEncUtf16Le;
using edge::encoding_ids::kEncUtf8;

constexpr double kEdgeBufferMaxLength = 9007199254740991.0;
constexpr double kEdgeUnsafeArrayBufferAllocCap = 2147483647.0;
#if defined(EDGE_NAPI_QUICKJS)
constexpr size_t kEdgeStringMaxLength = 0x3fffffff;
constexpr const char* kEdgeStringMaxLengthHex = "0x3fffffff";
#else
constexpr size_t kEdgeStringMaxLength =
    sizeof(void*) == 4 ? static_cast<size_t>(0x18ffffe8) : static_cast<size_t>(0x1fffffe8);
constexpr const char* kEdgeStringMaxLengthHex =
    sizeof(void*) == 4 ? "0x18ffffe8" : "0x1fffffe8";
#endif

std::string GetUtf8String(napi_env env, napi_value value);

struct BufferBindingState {
  explicit BufferBindingState(napi_env env_in) : env(env_in) {}
  ~BufferBindingState() {
    if (zero_fill_toggle_ref != nullptr) {
      napi_delete_reference(env, zero_fill_toggle_ref);
      zero_fill_toggle_ref = nullptr;
    }
    if (buffer_prototype_ref != nullptr) {
      napi_delete_reference(env, buffer_prototype_ref);
      buffer_prototype_ref = nullptr;
    }
  }

  napi_env env = nullptr;
  napi_ref zero_fill_toggle_ref = nullptr;
  uint32_t* zero_fill_toggle_data = nullptr;
  napi_ref buffer_prototype_ref = nullptr;
};

BufferBindingState& EnsureBufferBindingState(napi_env env) {
  return EdgeEnvironmentGetOrCreateSlotData<BufferBindingState>(
      env, kEdgeEnvironmentSlotBufferBindingState);
}

bool DefaultZeroFillBuffersEnabled() {
  return EdgeExecArgvHasFlag("--zero-fill-buffers");
}

napi_value MakeUndefined(napi_env env) {
  napi_value out = nullptr;
  napi_get_undefined(env, &out);
  return out;
}

napi_value GetZeroFillToggleValue(napi_env env) {
  auto& state = EnsureBufferBindingState(env);
  if (state.zero_fill_toggle_ref != nullptr) {
    napi_value current = nullptr;
    if (napi_get_reference_value(env, state.zero_fill_toggle_ref, &current) == napi_ok && current != nullptr) {
      return current;
    }
    napi_delete_reference(env, state.zero_fill_toggle_ref);
    state.zero_fill_toggle_ref = nullptr;
    state.zero_fill_toggle_data = nullptr;
  }

  void* data = nullptr;
  napi_value ab = nullptr;
  if (napi_create_arraybuffer(env, sizeof(uint32_t), &data, &ab) != napi_ok || ab == nullptr || data == nullptr) {
    return nullptr;
  }
  state.zero_fill_toggle_data = static_cast<uint32_t*>(data);
  state.zero_fill_toggle_data[0] = DefaultZeroFillBuffersEnabled() ? 1U : 0U;

  napi_value ta = nullptr;
  if (napi_create_typedarray(env, napi_uint32_array, 1, ab, 0, &ta) != napi_ok || ta == nullptr) {
    state.zero_fill_toggle_data = nullptr;
    return nullptr;
  }
  if (napi_create_reference(env, ta, 1, &state.zero_fill_toggle_ref) != napi_ok) {
    state.zero_fill_toggle_ref = nullptr;
    state.zero_fill_toggle_data = nullptr;
    return nullptr;
  }
  return ta;
}

bool ShouldZeroFillBuffers(napi_env env) {
  auto& state = EnsureBufferBindingState(env);
  if (state.zero_fill_toggle_data == nullptr) {
    (void)GetZeroFillToggleValue(env);
  }
  return state.zero_fill_toggle_data != nullptr ? (state.zero_fill_toggle_data[0] != 0) : DefaultZeroFillBuffersEnabled();
}

void ExternalArrayBufferFinalize(napi_env /*env*/, void* data, void* /*hint*/) {
  free(data);
}

int32_t ParseEncodingArg(napi_env env, napi_value value, int32_t fallback) {
  if (value == nullptr) return fallback;
  napi_valuetype type = napi_undefined;
  if (napi_typeof(env, value, &type) != napi_ok) return fallback;
  if (type == napi_number) {
    int32_t enc = fallback;
    if (napi_get_value_int32(env, value, &enc) == napi_ok) return enc;
    return fallback;
  }
  if (type != napi_string) return fallback;
  std::string enc = GetUtf8String(env, value);
  std::transform(enc.begin(), enc.end(), enc.begin(), [](unsigned char c) { return static_cast<char>(std::tolower(c)); });
  if (enc == "utf8" || enc == "utf-8") return kEncUtf8;
  if (enc == "ucs2" || enc == "ucs-2" || enc == "utf16le" || enc == "utf-16le") return kEncUtf16Le;
  if (enc == "latin1" || enc == "binary") return kEncLatin1;
  if (enc == "ascii") return kEncAscii;
  if (enc == "base64") return kEncBase64;
  if (enc == "base64url") return kEncBase64Url;
  if (enc == "hex") return kEncHex;
  return fallback;
}

bool GetInt32(napi_env env, napi_value value, int32_t* out) {
  return value != nullptr && out != nullptr && napi_get_value_int32(env, value, out) == napi_ok;
}

int32_t GetIndexOffsetArg(napi_env env, napi_value value) {
  if (value == nullptr) return 0;
  double d = 0;
  if (napi_get_value_double(env, value, &d) == napi_ok) {
    if (std::isnan(d) || d == 0) return 0;
    if (!std::isfinite(d)) return d > 0 ? INT32_MAX : INT32_MIN;
    if (d >= static_cast<double>(INT32_MAX)) return INT32_MAX;
    if (d <= static_cast<double>(INT32_MIN)) return INT32_MIN;
    return static_cast<int32_t>(std::trunc(d));
  }
  int32_t out = 0;
  if (napi_get_value_int32(env, value, &out) == napi_ok) return out;
  return 0;
}

int32_t ClampOffset(int32_t offset, size_t len, bool dir);

bool GetBool(napi_env env, napi_value value, bool* out) {
  return value != nullptr && out != nullptr && napi_get_value_bool(env, value, out) == napi_ok;
}

uint8_t* ZeroLengthDataSentinel() {
  static uint8_t sentinel = 0;
  return &sentinel;
}

bool ExtractBytesFromValue(napi_env env, napi_value value, uint8_t** data, size_t* len) {
  if (value == nullptr || data == nullptr || len == nullptr) return false;
  bool is_buffer = false;
  if (napi_is_buffer(env, value, &is_buffer) != napi_ok) return false;
  if (is_buffer) {
    void* ptr = nullptr;
    if (napi_get_buffer_info(env, value, &ptr, len) != napi_ok) return false;
    if (ptr == nullptr && *len != 0) return false;
    *data = (ptr != nullptr) ? static_cast<uint8_t*>(ptr) : ZeroLengthDataSentinel();
    return true;
  }

  bool is_typed = false;
  if (napi_is_typedarray(env, value, &is_typed) == napi_ok && is_typed) {
    napi_typedarray_type type = napi_uint8_array;
    size_t element_len = 0;
    void* ptr = nullptr;
    napi_value arraybuffer = nullptr;
    size_t byte_offset = 0;
    if (napi_get_typedarray_info(
            env, value, &type, &element_len, &ptr, &arraybuffer, &byte_offset) != napi_ok) {
      return false;
    }

    size_t bytes_per_element = 1;
    switch (type) {
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
    *len = element_len * bytes_per_element;
    if (*len == 0) {
      *data = ZeroLengthDataSentinel();
      return true;
    }

    void* ab_data = nullptr;
    size_t ab_len = 0;
    if (arraybuffer != nullptr &&
        napi_get_arraybuffer_info(env, arraybuffer, &ab_data, &ab_len) == napi_ok &&
        ab_data != nullptr &&
        byte_offset <= ab_len) {
      *data = static_cast<uint8_t*>(ab_data) + byte_offset;
    } else if (ptr != nullptr) {
      *data = static_cast<uint8_t*>(ptr);
    } else {
      return false;
    }
    return true;
  }

  bool is_dataview = false;
  if (napi_is_dataview(env, value, &is_dataview) == napi_ok && is_dataview) {
    void* ptr = nullptr;
    size_t byte_len = 0;
    napi_value arraybuffer = nullptr;
    size_t byte_offset = 0;
    if (napi_get_dataview_info(env, value, &byte_len, &ptr, &arraybuffer, &byte_offset) != napi_ok) {
      return false;
    }
    if (byte_len == 0) {
      *data = ZeroLengthDataSentinel();
      *len = 0;
      return true;
    }
    if (ptr != nullptr) {
      *data = static_cast<uint8_t*>(ptr);
      *len = byte_len;
      return true;
    }
    void* ab_data = nullptr;
    size_t ab_len = 0;
    if (arraybuffer != nullptr &&
        napi_get_arraybuffer_info(env, arraybuffer, &ab_data, &ab_len) == napi_ok &&
        ab_data != nullptr &&
        byte_offset <= ab_len) {
      *data = static_cast<uint8_t*>(ab_data) + byte_offset;
      *len = byte_len;
      return true;
    }
  }

  return false;
}

bool ExtractArrayBufferParts(napi_env env, napi_value value, uint8_t** data, size_t* len) {
  if (value == nullptr || data == nullptr || len == nullptr) return false;

  bool is_ab = false;
  bool is_typed = false;
  bool is_dataview = false;
  bool is_buffer = false;
  if (napi_is_arraybuffer(env, value, &is_ab) == napi_ok && is_ab) {
    // continue below
  } else {
    if (napi_is_typedarray(env, value, &is_typed) == napi_ok && is_typed) return false;
    if (napi_is_dataview(env, value, &is_dataview) == napi_ok && is_dataview) return false;
    if (napi_is_buffer(env, value, &is_buffer) == napi_ok && is_buffer) return false;
  }

  void* ptr = nullptr;
  size_t byte_len = 0;
  if (napi_get_arraybuffer_info(env, value, &ptr, &byte_len) != napi_ok) return false;
  if (ptr == nullptr && byte_len != 0) return false;
  *data = (ptr != nullptr) ? static_cast<uint8_t*>(ptr) : ZeroLengthDataSentinel();
  *len = byte_len;
  return true;
}

bool IsValueTrackedDetachedArrayBuffer(napi_env env, napi_value value) {
  if (value == nullptr) return false;
  auto get_symbol_for = [&](const char* key) -> napi_value {
    napi_value global = nullptr;
    if (napi_get_global(env, &global) != napi_ok || global == nullptr) return nullptr;
    napi_value symbol_obj = nullptr;
    if (napi_get_named_property(env, global, "Symbol", &symbol_obj) != napi_ok || symbol_obj == nullptr) {
      return nullptr;
    }
    napi_value for_fn = nullptr;
    if (napi_get_named_property(env, symbol_obj, "for", &for_fn) != napi_ok || for_fn == nullptr) {
      return nullptr;
    }
    napi_valuetype t = napi_undefined;
    if (napi_typeof(env, for_fn, &t) != napi_ok || t != napi_function) return nullptr;
    napi_value key_v = nullptr;
    if (napi_create_string_utf8(env, key, NAPI_AUTO_LENGTH, &key_v) != napi_ok || key_v == nullptr) {
      return nullptr;
    }
    napi_value symbol_key = nullptr;
    napi_value argv[1] = {key_v};
    if (napi_call_function(env, symbol_obj, for_fn, 1, argv, &symbol_key) != napi_ok || symbol_key == nullptr) {
      return nullptr;
    }
    return symbol_key;
  };

  napi_value marker_symbol = get_symbol_for("node.detachedArrayBufferMarker");
  if (marker_symbol != nullptr) {
    bool has_marker_symbol = false;
    if (napi_has_property(env, value, marker_symbol, &has_marker_symbol) == napi_ok && has_marker_symbol) {
      return true;
    }
  }

  napi_value global = nullptr;
  if (napi_get_global(env, &global) != napi_ok || global == nullptr) return false;
  napi_value detached_set = nullptr;
  napi_value detached_set_symbol = get_symbol_for("node.detachedArrayBuffers");
  bool found_set = false;
  if (detached_set_symbol != nullptr) {
    bool has_symbol_set = false;
    if (napi_has_property(env, global, detached_set_symbol, &has_symbol_set) == napi_ok && has_symbol_set) {
      if (napi_get_property(env, global, detached_set_symbol, &detached_set) == napi_ok &&
          detached_set != nullptr) {
        found_set = true;
      }
    }
  }
  if (!found_set) return false;
  if (detached_set == nullptr) {
    return false;
  }
  napi_value has_fn = nullptr;
  if (napi_get_named_property(env, detached_set, "has", &has_fn) != napi_ok || has_fn == nullptr) return false;
  napi_valuetype t = napi_undefined;
  if (napi_typeof(env, has_fn, &t) != napi_ok || t != napi_function) return false;
  napi_value argv[1] = {value};
  napi_value result = nullptr;
  if (napi_call_function(env, detached_set, has_fn, 1, argv, &result) != napi_ok || result == nullptr) return false;
  bool detached = false;
  if (napi_get_value_bool(env, result, &detached) != napi_ok) return false;
  return detached;
}

bool IsArrayBufferDetached(napi_env env, napi_value value) {
  bool detached = false;
  if (value != nullptr && napi_is_detached_arraybuffer(env, value, &detached) == napi_ok && detached) {
    return true;
  }
  return IsValueTrackedDetachedArrayBuffer(env, value);
}

bool ExtractValidationBytesOrThrow(napi_env env, napi_value value, uint8_t** data, size_t* len) {
  if (value == nullptr || data == nullptr || len == nullptr) return false;

  bool is_ab = false;
  if (napi_is_arraybuffer(env, value, &is_ab) == napi_ok && is_ab) {
    if (IsArrayBufferDetached(env, value)) {
      napi_throw_error(env, "ERR_INVALID_STATE", "Cannot validate on a detached buffer");
      return false;
    }
    return ExtractArrayBufferParts(env, value, data, len);
  }

  bool is_typed = false;
  if (napi_is_typedarray(env, value, &is_typed) == napi_ok && is_typed) {
    napi_typedarray_type type = napi_uint8_array;
    size_t element_len = 0;
    void* ptr = nullptr;
    napi_value arraybuffer = nullptr;
    size_t byte_offset = 0;
    if (napi_get_typedarray_info(env, value, &type, &element_len, &ptr, &arraybuffer, &byte_offset) == napi_ok &&
        arraybuffer != nullptr &&
        IsArrayBufferDetached(env, arraybuffer)) {
      napi_throw_error(env, "ERR_INVALID_STATE", "Cannot validate on a detached buffer");
      return false;
    }
    return ExtractBytesFromValue(env, value, data, len);
  }

  bool is_dataview = false;
  if (napi_is_dataview(env, value, &is_dataview) == napi_ok && is_dataview) {
    void* ptr = nullptr;
    size_t byte_len = 0;
    napi_value arraybuffer = nullptr;
    size_t byte_offset = 0;
    if (napi_get_dataview_info(env, value, &byte_len, &ptr, &arraybuffer, &byte_offset) == napi_ok &&
        arraybuffer != nullptr &&
        IsArrayBufferDetached(env, arraybuffer)) {
      napi_throw_error(env, "ERR_INVALID_STATE", "Cannot validate on a detached buffer");
      return false;
    }
    return ExtractBytesFromValue(env, value, data, len);
  }

  if (ExtractArrayBufferParts(env, value, data, len)) {
    return true;
  }

  napi_throw_type_error(env,
                        "ERR_INVALID_ARG_TYPE",
                        "The \"input\" argument must be an instance of ArrayBuffer, Buffer, TypedArray, or DataView.");
  return false;
}

std::string GetUtf8String(napi_env env, napi_value value) {
  if (value == nullptr) return "";
  size_t len = 0;
  if (napi_get_value_string_utf8(env, value, nullptr, 0, &len) != napi_ok) return "";
  std::string out(len + 1, '\0');
  size_t copied = 0;
  if (napi_get_value_string_utf8(env, value, out.data(), out.size(), &copied) != napi_ok) return "";
  out.resize(copied);
  return out;
}

napi_value MakeInt32(napi_env env, int32_t value) {
  napi_value out = nullptr;
  napi_create_int32(env, value, &out);
  return out;
}

napi_value MakeStringUtf8(napi_env env, const std::string& s) {
  napi_value out = nullptr;
  napi_create_string_utf8(env, s.c_str(), s.size(), &out);
  return out;
}

napi_value MakeLatin1String(napi_env env, const uint8_t* data, size_t len) {
  if (len == 0) return MakeStringUtf8(env, "");
  std::vector<char16_t> utf16(len);
  for (size_t i = 0; i < len; ++i) {
    utf16[i] = static_cast<char16_t>(data[i]);
  }
  napi_value out = nullptr;
  napi_create_string_utf16(env, utf16.data(), utf16.size(), &out);
  return out;
}

std::string HexSlice(const uint8_t* data, size_t len) {
  static const char* kHex = "0123456789abcdef";
  std::string out;
  out.reserve(len * 2);
  for (size_t i = 0; i < len; i++) {
    out.push_back(kHex[(data[i] >> 4) & 0x0f]);
    out.push_back(kHex[data[i] & 0x0f]);
  }
  return out;
}

bool DecodeHex(const std::string& s, std::vector<uint8_t>* out) {
  if (out == nullptr) return false;
  out->clear();
  auto hex_val = [](char c) -> int {
    if (c >= '0' && c <= '9') return c - '0';
    if (c >= 'a' && c <= 'f') return c - 'a' + 10;
    if (c >= 'A' && c <= 'F') return c - 'A' + 10;
    return -1;
  };
  const size_t max_pairs = s.size() / 2;
  out->reserve(max_pairs);
  for (size_t i = 0; i + 1 < s.size(); i += 2) {
    const int hi = hex_val(s[i]);
    const int lo = hex_val(s[i + 1]);
    if (hi < 0 || lo < 0) break;
    out->push_back(static_cast<uint8_t>((hi << 4) | lo));
  }
  return true;
}

bool IsBase64Char(char c, bool url_mode) {
  if ((c >= 'A' && c <= 'Z') || (c >= 'a' && c <= 'z') || (c >= '0' && c <= '9')) return true;
  if (c == '=') return true;
  if (c == '+' || c == '/') return true;
  if (c == '-' || c == '_') return true;
  return false;
}

std::string NormalizeForgivingBase64(const std::string& input, bool url_mode) {
  std::string compact;
  compact.reserve(input.size());
  for (char c : input) {
    if (c == '=') break;
    if (!IsBase64Char(c, url_mode)) continue;
    if (c == '-') c = '+';
    else if (c == '_') c = '/';
    compact.push_back(c);
  }
  const size_t rem = compact.size() % 4;
  if (rem == 1) return "";
  if (rem == 2) compact.append("==");
  else if (rem == 3) compact.push_back('=');
  return compact;
}

bool DecodeBase64(const std::string& input, bool is_url, std::vector<uint8_t>* out) {
  if (out == nullptr) return false;
  const size_t max_len = simdutf::maximal_binary_length_from_base64(input.data(), input.size());
  out->assign(max_len, 0);
  size_t out_len = max_len;
  simdutf::result result = is_url
                               ? simdutf::base64_to_binary_safe(
                                     input.data(), input.size(),
                                     reinterpret_cast<char*>(out->data()), out_len, simdutf::base64_url)
                               : simdutf::base64_to_binary_safe(
                                     input.data(), input.size(),
                                     reinterpret_cast<char*>(out->data()), out_len);
  if (result.error == simdutf::error_code::SUCCESS) {
    out->resize(out_len);
    return true;
  }

  const std::string forgiving = NormalizeForgivingBase64(input, is_url);
  if (forgiving.empty()) {
    out->clear();
    return true;
  }
  const size_t fallback_max = simdutf::maximal_binary_length_from_base64(forgiving.data(), forgiving.size());
  out->assign(fallback_max, 0);
  size_t fallback_len = fallback_max;
  simdutf::result fallback = simdutf::base64_to_binary_safe(
      forgiving.data(), forgiving.size(), reinterpret_cast<char*>(out->data()), fallback_len);
  if (fallback.error != simdutf::error_code::SUCCESS) {
    out->clear();
    return true;
  }
  out->resize(fallback_len);
  return true;
}

bool EncodeStringToBytes(napi_env env, napi_value str_value, int32_t enc, std::vector<uint8_t>* out) {
  if (out == nullptr || str_value == nullptr) return false;
  if (enc == kEncHex) {
    return DecodeHex(GetUtf8String(env, str_value), out);
  }
  if (enc == kEncBase64 || enc == kEncBase64Url) {
    return DecodeBase64(GetUtf8String(env, str_value), enc == kEncBase64Url, out);
  }
  if (enc == kEncAscii || enc == kEncLatin1) {
    size_t utf16_len = 0;
    if (napi_get_value_string_utf16(env, str_value, nullptr, 0, &utf16_len) != napi_ok) return false;
    std::vector<char16_t> utf16(utf16_len + 1);
    size_t copied = 0;
    if (napi_get_value_string_utf16(env, str_value, utf16.data(), utf16.size(), &copied) != napi_ok) return false;
    out->resize(copied);
    for (size_t i = 0; i < copied; i++) {
      (*out)[i] = static_cast<uint8_t>(utf16[i] & 0xff);
    }
    return true;
  }
  if (enc == kEncUtf16Le) {
    size_t utf16_len = 0;
    if (napi_get_value_string_utf16(env, str_value, nullptr, 0, &utf16_len) != napi_ok) return false;
    std::vector<char16_t> utf16(utf16_len + 1);
    size_t copied = 0;
    if (napi_get_value_string_utf16(env, str_value, utf16.data(), utf16.size(), &copied) != napi_ok) return false;
    out->resize(copied * 2);
    for (size_t i = 0; i < copied; i++) {
      const char16_t ch = utf16[i];
      (*out)[i * 2] = static_cast<uint8_t>(ch & 0xff);
      (*out)[i * 2 + 1] = static_cast<uint8_t>((ch >> 8) & 0xff);
    }
    return true;
  }

  // utf8 default
  const std::string utf8 = GetUtf8String(env, str_value);
  out->assign(utf8.begin(), utf8.end());
  return true;
}

napi_value BindingByteLengthUtf8(napi_env env, napi_callback_info info) {
  size_t argc = 1;
  napi_value argv[1] = {nullptr};
  if (napi_get_cb_info(env, info, &argc, argv, nullptr, nullptr) != napi_ok || argc < 1) return nullptr;
  const std::string utf8 = GetUtf8String(env, argv[0]);
  const size_t out_len = utf8.size();
  napi_value out = nullptr;
  napi_create_uint32(env, static_cast<uint32_t>(out_len), &out);
  return out;
}

napi_value BindingCompare(napi_env env, napi_callback_info info) {
  size_t argc = 2;
  napi_value argv[2] = {nullptr, nullptr};
  if (napi_get_cb_info(env, info, &argc, argv, nullptr, nullptr) != napi_ok || argc < 2) return nullptr;
  uint8_t* a = nullptr;
  uint8_t* b = nullptr;
  size_t a_len = 0;
  size_t b_len = 0;
  if (!ExtractBytesFromValue(env, argv[0], &a, &a_len) || !ExtractBytesFromValue(env, argv[1], &b, &b_len)) {
    return MakeInt32(env, 0);
  }
  const size_t min_len = std::min(a_len, b_len);
  int cmp = (min_len == 0) ? 0 : std::memcmp(a, b, min_len);
  if (cmp < 0) return MakeInt32(env, -1);
  if (cmp > 0) return MakeInt32(env, 1);
  if (a_len < b_len) return MakeInt32(env, -1);
  if (a_len > b_len) return MakeInt32(env, 1);
  return MakeInt32(env, 0);
}

napi_value BindingCompareOffset(napi_env env, napi_callback_info info) {
  size_t argc = 6;
  napi_value argv[6] = {nullptr, nullptr, nullptr, nullptr, nullptr, nullptr};
  if (napi_get_cb_info(env, info, &argc, argv, nullptr, nullptr) != napi_ok || argc < 6) return nullptr;
  uint8_t* src = nullptr;
  uint8_t* dst = nullptr;
  size_t src_len = 0;
  size_t dst_len = 0;
  if (!ExtractBytesFromValue(env, argv[0], &src, &src_len) || !ExtractBytesFromValue(env, argv[1], &dst, &dst_len)) {
    return MakeInt32(env, 0);
  }
  int32_t dst_start = 0;
  int32_t src_start = 0;
  int32_t dst_end = 0;
  int32_t src_end = 0;
  GetInt32(env, argv[2], &dst_start);
  GetInt32(env, argv[3], &src_start);
  GetInt32(env, argv[4], &dst_end);
  GetInt32(env, argv[5], &src_end);
  if (dst_start < 0) dst_start = 0;
  if (src_start < 0) src_start = 0;
  dst_end = std::min<int32_t>(dst_end, static_cast<int32_t>(dst_len));
  src_end = std::min<int32_t>(src_end, static_cast<int32_t>(src_len));
  const size_t d_len = dst_end > dst_start ? static_cast<size_t>(dst_end - dst_start) : 0;
  const size_t s_len = src_end > src_start ? static_cast<size_t>(src_end - src_start) : 0;
  const size_t min_len = std::min(d_len, s_len);
  int cmp = (min_len == 0) ? 0 : std::memcmp(src + src_start, dst + dst_start, min_len);
  if (cmp < 0) return MakeInt32(env, -1);
  if (cmp > 0) return MakeInt32(env, 1);
  if (s_len < d_len) return MakeInt32(env, -1);
  if (s_len > d_len) return MakeInt32(env, 1);
  return MakeInt32(env, 0);
}

napi_value BindingCopy(napi_env env, napi_callback_info info) {
  size_t argc = 5;
  napi_value argv[5] = {nullptr, nullptr, nullptr, nullptr, nullptr};
  if (napi_get_cb_info(env, info, &argc, argv, nullptr, nullptr) != napi_ok || argc < 5) return nullptr;
  uint8_t* src = nullptr;
  uint8_t* dst = nullptr;
  size_t src_len = 0;
  size_t dst_len = 0;
  if (!ExtractBytesFromValue(env, argv[0], &src, &src_len) || !ExtractBytesFromValue(env, argv[1], &dst, &dst_len)) {
    return MakeInt32(env, 0);
  }
  int32_t dst_start = 0;
  int32_t src_start = 0;
  int32_t count = 0;
  GetInt32(env, argv[2], &dst_start);
  GetInt32(env, argv[3], &src_start);
  GetInt32(env, argv[4], &count);
  if (dst_start < 0 || src_start < 0 || count <= 0) return MakeInt32(env, 0);
  const size_t available_src = src_start < static_cast<int32_t>(src_len) ? src_len - static_cast<size_t>(src_start) : 0;
  const size_t available_dst = dst_start < static_cast<int32_t>(dst_len) ? dst_len - static_cast<size_t>(dst_start) : 0;
  size_t to_copy = std::min<size_t>(static_cast<size_t>(count), std::min(available_src, available_dst));
  if (to_copy == 0) return MakeInt32(env, 0);
  std::memmove(dst + dst_start, src + src_start, to_copy);
  return MakeInt32(env, static_cast<int32_t>(to_copy));
}

napi_value BindingFill(napi_env env, napi_callback_info info) {
  size_t argc = 5;
  napi_value argv[5] = {nullptr, nullptr, nullptr, nullptr, nullptr};
  if (napi_get_cb_info(env, info, &argc, argv, nullptr, nullptr) != napi_ok || argc < 4) return nullptr;
  uint8_t* dst = nullptr;
  size_t dst_len = 0;
  if (!ExtractBytesFromValue(env, argv[0], &dst, &dst_len)) return MakeInt32(env, -2);
  int32_t start = 0;
  int32_t end = 0;
  GetInt32(env, argv[2], &start);
  GetInt32(env, argv[3], &end);
  if (start < 0 || end < 0 || start > end || end > static_cast<int32_t>(dst_len)) return MakeInt32(env, -2);
  const size_t fill_len = static_cast<size_t>(end - start);
  if (fill_len == 0) return MakeUndefined(env);

  napi_valuetype value_type = napi_undefined;
  napi_typeof(env, argv[1], &value_type);
  if (value_type == napi_null || value_type == napi_undefined) {
    std::memset(dst + start, 0, fill_len);
    return MakeUndefined(env);
  }
  if (value_type == napi_number) {
    int32_t n = 0;
    napi_get_value_int32(env, argv[1], &n);
    std::memset(dst + start, static_cast<uint8_t>(n & 0xff), fill_len);
    return MakeUndefined(env);
  }

  std::vector<uint8_t> bytes;
  bool from_buffer_source = false;
  if (value_type == napi_string) {
    const std::string raw = GetUtf8String(env, argv[1]);
    if (raw.empty()) return MakeUndefined(env);
    int32_t enc = kEncUtf8;
    if (argc >= 5) enc = ParseEncodingArg(env, argv[4], kEncUtf8);
    if (!EncodeStringToBytes(env, argv[1], enc, &bytes)) return MakeInt32(env, -1);
  } else {
    uint8_t* src = nullptr;
    size_t src_len = 0;
    if (!ExtractBytesFromValue(env, argv[1], &src, &src_len)) {
      napi_value coerced = nullptr;
      double d = 0;
      if (napi_coerce_to_number(env, argv[1], &coerced) == napi_ok &&
          coerced != nullptr &&
          napi_get_value_double(env, coerced, &d) == napi_ok) {
        int32_t n = 0;
        if (std::isfinite(d)) n = static_cast<int32_t>(d);
        std::memset(dst + start, static_cast<uint8_t>(n & 0xff), fill_len);
        return MakeUndefined(env);
      }
      return MakeInt32(env, -1);
    }
    from_buffer_source = true;
    bytes.assign(src, src + src_len);
  }
  if (bytes.empty()) {
    if (value_type == napi_string) return MakeInt32(env, -1);
    if (from_buffer_source) return MakeInt32(env, -1);
    return MakeUndefined(env);
  }
  for (size_t i = 0; i < fill_len; i++) {
    dst[start + i] = bytes[i % bytes.size()];
  }
  return MakeUndefined(env);
}

napi_value BindingIsAscii(napi_env env, napi_callback_info info) {
  size_t argc = 1;
  napi_value argv[1] = {nullptr};
  if (napi_get_cb_info(env, info, &argc, argv, nullptr, nullptr) != napi_ok || argc < 1) return nullptr;
  uint8_t* data = nullptr;
  size_t len = 0;
  if (!ExtractValidationBytesOrThrow(env, argv[0], &data, &len)) return nullptr;
  bool ok = true;
  for (size_t i = 0; i < len; i++) {
    if (data[i] > 0x7f) {
      ok = false;
      break;
    }
  }
  napi_value out = nullptr;
  napi_get_boolean(env, ok, &out);
  return out;
}

napi_value BindingIsUtf8(napi_env env, napi_callback_info info) {
  size_t argc = 1;
  napi_value argv[1] = {nullptr};
  if (napi_get_cb_info(env, info, &argc, argv, nullptr, nullptr) != napi_ok || argc < 1) return nullptr;
  uint8_t* data = nullptr;
  size_t len = 0;
  if (!ExtractValidationBytesOrThrow(env, argv[0], &data, &len)) return nullptr;
  napi_value out = nullptr;
  if (len == 0) {
    napi_get_boolean(env, true, &out);
    return out;
  }
  napi_get_boolean(env, simdutf::validate_utf8(reinterpret_cast<const char*>(data), len), &out);
  return out;
}

napi_value BindingIndexOfNumber(napi_env env, napi_callback_info info) {
  size_t argc = 4;
  napi_value argv[4] = {nullptr, nullptr, nullptr, nullptr};
  if (napi_get_cb_info(env, info, &argc, argv, nullptr, nullptr) != napi_ok || argc < 4) return nullptr;
  uint8_t* data = nullptr;
  size_t len = 0;
  if (!ExtractBytesFromValue(env, argv[0], &data, &len)) return MakeInt32(env, -1);
  int32_t value = 0;
  int32_t offset = 0;
  bool dir = true;
  GetInt32(env, argv[1], &value);
  offset = GetIndexOffsetArg(env, argv[2]);
  GetBool(env, argv[3], &dir);
  uint8_t needle = static_cast<uint8_t>(value & 0xff);
  if (dir) {
    int32_t start = ClampOffset(offset, len, true);
    for (size_t i = static_cast<size_t>(start); i < len; i++) {
      if (data[i] == needle) return MakeInt32(env, static_cast<int32_t>(i));
    }
  } else {
    int32_t start = offset < 0 ? static_cast<int32_t>(len) + offset : offset;
    if (start >= static_cast<int32_t>(len)) start = static_cast<int32_t>(len) - 1;
    for (int32_t i = start; i >= 0; i--) {
      if (data[i] == needle) return MakeInt32(env, i);
    }
  }
  return MakeInt32(env, -1);
}

int32_t ClampOffset(int32_t offset, size_t len, bool dir) {
  const int64_t len64 = static_cast<int64_t>(len);
  if (dir) {
    if (offset < 0) {
      const int64_t pos = len64 + static_cast<int64_t>(offset);
      return pos < 0 ? 0 : static_cast<int32_t>(pos);
    }
    if (offset > static_cast<int32_t>(len)) return static_cast<int32_t>(len);
    return offset;
  }
  if (offset < 0) {
    const int64_t pos = len64 + static_cast<int64_t>(offset);
    return static_cast<int32_t>(pos);
  }
  if (offset >= static_cast<int32_t>(len)) return static_cast<int32_t>(len) - 1;
  return offset;
}

int32_t FindSubsequence(const uint8_t* hay, size_t hay_len, const uint8_t* needle, size_t needle_len,
                        int32_t offset, bool dir) {
  if (needle_len == 0) return std::max<int32_t>(0, std::min<int32_t>(offset, static_cast<int32_t>(hay_len)));
  if (hay_len < needle_len) return -1;
  if (dir) {
    const int32_t start = ClampOffset(offset, hay_len, true);
    for (size_t i = static_cast<size_t>(start); i + needle_len <= hay_len; i++) {
      if (std::memcmp(hay + i, needle, needle_len) == 0) return static_cast<int32_t>(i);
    }
    return -1;
  }
  int32_t start = ClampOffset(offset, hay_len, false);
  if (start > static_cast<int32_t>(hay_len - needle_len)) start = static_cast<int32_t>(hay_len - needle_len);
  for (int32_t i = start; i >= 0; i--) {
    if (std::memcmp(hay + i, needle, needle_len) == 0) return i;
  }
  return -1;
}

napi_value BindingIndexOfBuffer(napi_env env, napi_callback_info info) {
  size_t argc = 5;
  napi_value argv[5] = {nullptr, nullptr, nullptr, nullptr, nullptr};
  if (napi_get_cb_info(env, info, &argc, argv, nullptr, nullptr) != napi_ok || argc < 5) return nullptr;
  uint8_t* hay = nullptr;
  uint8_t* needle = nullptr;
  size_t hay_len = 0;
  size_t needle_len = 0;
  if (!ExtractBytesFromValue(env, argv[0], &hay, &hay_len) ||
      !ExtractBytesFromValue(env, argv[1], &needle, &needle_len)) {
    return MakeInt32(env, -1);
  }
  int32_t offset = 0;
  int32_t enc = kEncUtf8;
  bool dir = true;
  offset = GetIndexOffsetArg(env, argv[2]);
  GetInt32(env, argv[3], &enc);
  GetBool(env, argv[4], &dir);
  if (enc == kEncUtf16Le) {
    if ((needle_len & 1) != 0 || (hay_len & 1) != 0) return MakeInt32(env, -1);
    const size_t hay_units = hay_len / 2;
    const size_t needle_units = needle_len / 2;
    if (needle_units == 0) {
      const int32_t pos = std::max<int32_t>(0, std::min<int32_t>(offset, static_cast<int32_t>(hay_len)));
      return MakeInt32(env, pos);
    }
    if (hay_units < needle_units) return MakeInt32(env, -1);
    int32_t unit_offset = offset / 2;
    if (dir) {
      if (unit_offset < 0) unit_offset = std::max<int32_t>(0, static_cast<int32_t>(hay_units) + unit_offset);
      if (unit_offset > static_cast<int32_t>(hay_units)) unit_offset = static_cast<int32_t>(hay_units);
      for (size_t i = static_cast<size_t>(unit_offset); i + needle_units <= hay_units; i++) {
        if (std::memcmp(hay + (i * 2), needle, needle_len) == 0) return MakeInt32(env, static_cast<int32_t>(i * 2));
      }
      return MakeInt32(env, -1);
    }
    if (unit_offset < 0) unit_offset = static_cast<int32_t>(hay_units) + unit_offset;
    if (unit_offset >= static_cast<int32_t>(hay_units)) unit_offset = static_cast<int32_t>(hay_units) - 1;
    int32_t start = unit_offset;
    if (start > static_cast<int32_t>(hay_units - needle_units)) {
      start = static_cast<int32_t>(hay_units - needle_units);
    }
    for (int32_t i = start; i >= 0; i--) {
      if (std::memcmp(hay + (static_cast<size_t>(i) * 2), needle, needle_len) == 0) return MakeInt32(env, i * 2);
    }
    return MakeInt32(env, -1);
  }
  return MakeInt32(env, FindSubsequence(hay, hay_len, needle, needle_len, offset, dir));
}

napi_value BindingIndexOfString(napi_env env, napi_callback_info info) {
  size_t argc = 5;
  napi_value argv[5] = {nullptr, nullptr, nullptr, nullptr, nullptr};
  if (napi_get_cb_info(env, info, &argc, argv, nullptr, nullptr) != napi_ok || argc < 5) return nullptr;
  uint8_t* hay = nullptr;
  size_t hay_len = 0;
  if (!ExtractBytesFromValue(env, argv[0], &hay, &hay_len)) return MakeInt32(env, -1);
  int32_t offset = 0;
  int32_t enc = kEncUtf8;
  bool dir = true;
  offset = GetIndexOffsetArg(env, argv[2]);
  GetInt32(env, argv[3], &enc);
  GetBool(env, argv[4], &dir);
  std::vector<uint8_t> needle;
  if (!EncodeStringToBytes(env, argv[1], enc, &needle)) return MakeInt32(env, -1);
  if (enc == kEncUtf16Le) {
    const size_t needle_len = needle.size();
    if ((needle_len & 1) != 0 || (hay_len & 1) != 0) return MakeInt32(env, -1);
    const size_t hay_units = hay_len / 2;
    const size_t needle_units = needle_len / 2;
    if (needle_units == 0) {
      const int32_t pos = std::max<int32_t>(0, std::min<int32_t>(offset, static_cast<int32_t>(hay_len)));
      return MakeInt32(env, pos);
    }
    if (hay_units < needle_units) return MakeInt32(env, -1);
    int32_t unit_offset = offset / 2;
    if (dir) {
      if (unit_offset < 0) unit_offset = std::max<int32_t>(0, static_cast<int32_t>(hay_units) + unit_offset);
      if (unit_offset > static_cast<int32_t>(hay_units)) unit_offset = static_cast<int32_t>(hay_units);
      for (size_t i = static_cast<size_t>(unit_offset); i + needle_units <= hay_units; i++) {
        if (std::memcmp(hay + (i * 2), needle.data(), needle_len) == 0) return MakeInt32(env, static_cast<int32_t>(i * 2));
      }
      return MakeInt32(env, -1);
    }
    if (unit_offset < 0) unit_offset = static_cast<int32_t>(hay_units) + unit_offset;
    if (unit_offset >= static_cast<int32_t>(hay_units)) unit_offset = static_cast<int32_t>(hay_units) - 1;
    int32_t start = unit_offset;
    if (start > static_cast<int32_t>(hay_units - needle_units)) {
      start = static_cast<int32_t>(hay_units - needle_units);
    }
    for (int32_t i = start; i >= 0; i--) {
      if (std::memcmp(hay + (static_cast<size_t>(i) * 2), needle.data(), needle_len) == 0) return MakeInt32(env, i * 2);
    }
    return MakeInt32(env, -1);
  }
  return MakeInt32(env, FindSubsequence(hay, hay_len, needle.data(), needle.size(), offset, dir));
}

napi_value BindingSwap16(napi_env env, napi_callback_info info) {
  size_t argc = 1;
  napi_value argv[1] = {nullptr};
  if (napi_get_cb_info(env, info, &argc, argv, nullptr, nullptr) != napi_ok || argc < 1) return nullptr;
  uint8_t* data = nullptr;
  size_t len = 0;
  if (!ExtractBytesFromValue(env, argv[0], &data, &len)) return nullptr;
  for (size_t i = 0; i + 1 < len; i += 2) std::swap(data[i], data[i + 1]);
  return argv[0];
}

napi_value BindingSwap32(napi_env env, napi_callback_info info) {
  size_t argc = 1;
  napi_value argv[1] = {nullptr};
  if (napi_get_cb_info(env, info, &argc, argv, nullptr, nullptr) != napi_ok || argc < 1) return nullptr;
  uint8_t* data = nullptr;
  size_t len = 0;
  if (!ExtractBytesFromValue(env, argv[0], &data, &len)) return nullptr;
  for (size_t i = 0; i + 3 < len; i += 4) {
    std::swap(data[i], data[i + 3]);
    std::swap(data[i + 1], data[i + 2]);
  }
  return argv[0];
}

napi_value BindingSwap64(napi_env env, napi_callback_info info) {
  size_t argc = 1;
  napi_value argv[1] = {nullptr};
  if (napi_get_cb_info(env, info, &argc, argv, nullptr, nullptr) != napi_ok || argc < 1) return nullptr;
  uint8_t* data = nullptr;
  size_t len = 0;
  if (!ExtractBytesFromValue(env, argv[0], &data, &len)) return nullptr;
  for (size_t i = 0; i + 7 < len; i += 8) {
    std::swap(data[i], data[i + 7]);
    std::swap(data[i + 1], data[i + 6]);
    std::swap(data[i + 2], data[i + 5]);
    std::swap(data[i + 3], data[i + 4]);
  }
  return argv[0];
}

napi_value SliceByEncoding(napi_env env, napi_callback_info info, int32_t enc) {
  size_t argc = 3;
  napi_value argv[3] = {nullptr, nullptr, nullptr};
  if (napi_get_cb_info(env, info, &argc, argv, nullptr, nullptr) != napi_ok || argc < 3) return nullptr;
  uint8_t* data = nullptr;
  size_t len = 0;
  if (!ExtractBytesFromValue(env, argv[0], &data, &len)) return MakeStringUtf8(env, "");
  int32_t start = 0;
  int32_t end = static_cast<int32_t>(len);
  GetInt32(env, argv[1], &start);
  GetInt32(env, argv[2], &end);
  start = std::max<int32_t>(0, std::min<int32_t>(start, static_cast<int32_t>(len)));
  end = std::max<int32_t>(start, std::min<int32_t>(end, static_cast<int32_t>(len)));
  const uint8_t* p = data + start;
  const size_t n = static_cast<size_t>(end - start);

  if ((enc == kEncUtf8 || enc == kEncAscii || enc == kEncLatin1) && n > kEdgeStringMaxLength) {
    std::string message = "Cannot create a string longer than ";
    message += kEdgeStringMaxLengthHex;
    message += " characters";
    napi_throw_error(env, "ERR_STRING_TOO_LONG", message.c_str());
    return nullptr;
  }
  if (enc == kEncUtf16Le && (n / 2) > kEdgeStringMaxLength) {
    std::string message = "Cannot create a string longer than ";
    message += kEdgeStringMaxLengthHex;
    message += " characters";
    napi_throw_error(env, "ERR_STRING_TOO_LONG", message.c_str());
    return nullptr;
  }

  if (enc == kEncHex) return MakeStringUtf8(env, HexSlice(p, n));
  if (enc == kEncBase64 || enc == kEncBase64Url) {
    const size_t out_len = (enc == kEncBase64Url)
                               ? simdutf::base64_length_from_binary(n, simdutf::base64_url)
                               : simdutf::base64_length_from_binary(n);
    std::string out(out_len, '\0');
    const size_t written = (enc == kEncBase64Url)
                               ? simdutf::binary_to_base64(reinterpret_cast<const char*>(p), n, out.data(),
                                                           simdutf::base64_url)
                               : simdutf::binary_to_base64(reinterpret_cast<const char*>(p), n, out.data());
    out.resize(written);
    if (enc == kEncBase64Url) {
      while (!out.empty() && out.back() == '=') out.pop_back();
    }
    return MakeStringUtf8(env, out);
  }
  if (enc == kEncAscii) {
    std::string out(n, '\0');
    for (size_t i = 0; i < n; i++) {
      uint8_t c = p[i] & 0x7f;
      out[i] = static_cast<char>(c);
    }
    return MakeStringUtf8(env, out);
  }
  if (enc == kEncLatin1) {
    std::vector<char16_t> u16(n);
    for (size_t i = 0; i < n; i++) {
      u16[i] = static_cast<char16_t>(p[i]);
    }
    napi_value out = nullptr;
    napi_create_string_utf16(env, u16.data(), u16.size(), &out);
    return out;
  }
  if (enc == kEncUtf16Le) {
    const size_t pairs = n / 2;
    std::vector<char16_t> u16(pairs);
    for (size_t i = 0; i < pairs; i++) {
      u16[i] = static_cast<char16_t>(p[i * 2] | (static_cast<uint16_t>(p[i * 2 + 1]) << 8));
    }
    napi_value out = nullptr;
    napi_create_string_utf16(env, u16.data(), u16.size(), &out);
    return out;
  }

  napi_value out = nullptr;
  napi_create_string_utf8(env, reinterpret_cast<const char*>(p), n, &out);
  return out;
}

napi_value BindingAsciiSlice(napi_env env, napi_callback_info info) { return SliceByEncoding(env, info, kEncAscii); }
napi_value BindingBase64Slice(napi_env env, napi_callback_info info) { return SliceByEncoding(env, info, kEncBase64); }
napi_value BindingBase64UrlSlice(napi_env env, napi_callback_info info) { return SliceByEncoding(env, info, kEncBase64Url); }
napi_value BindingLatin1Slice(napi_env env, napi_callback_info info) { return SliceByEncoding(env, info, kEncLatin1); }
napi_value BindingHexSlice(napi_env env, napi_callback_info info) { return SliceByEncoding(env, info, kEncHex); }
napi_value BindingUcs2Slice(napi_env env, napi_callback_info info) { return SliceByEncoding(env, info, kEncUtf16Le); }
napi_value BindingUtf8Slice(napi_env env, napi_callback_info info) { return SliceByEncoding(env, info, kEncUtf8); }

napi_value WriteByEncoding(napi_env env, napi_callback_info info, int32_t enc) {
  size_t argc = 4;
  napi_value argv[4] = {nullptr, nullptr, nullptr, nullptr};
  if (napi_get_cb_info(env, info, &argc, argv, nullptr, nullptr) != napi_ok || argc < 4) return nullptr;
  uint8_t* dst = nullptr;
  size_t dst_len = 0;
  if (!ExtractBytesFromValue(env, argv[0], &dst, &dst_len)) return MakeInt32(env, 0);
  int32_t offset = 0;
  int32_t max_len = 0;
  GetInt32(env, argv[2], &offset);
  GetInt32(env, argv[3], &max_len);
  if (offset < 0 || max_len < 0 || offset > static_cast<int32_t>(dst_len)) return MakeInt32(env, 0);
  std::vector<uint8_t> bytes;
  if (!EncodeStringToBytes(env, argv[1], enc, &bytes)) return MakeInt32(env, 0);
  const size_t writable = std::min<size_t>(static_cast<size_t>(max_len), dst_len - static_cast<size_t>(offset));
  size_t to_write = std::min(writable, bytes.size());
  if (enc == kEncUtf16Le) {
    to_write &= ~static_cast<size_t>(1);
  } else if (enc == kEncUtf8 && to_write < bytes.size()) {
    while (to_write > 0 &&
           !simdutf::validate_utf8(reinterpret_cast<const char*>(bytes.data()), to_write)) {
      to_write--;
    }
  }
  if (to_write > 0) std::memcpy(dst + offset, bytes.data(), to_write);
  return MakeInt32(env, static_cast<int32_t>(to_write));
}

napi_value BindingAsciiWriteStatic(napi_env env, napi_callback_info info) { return WriteByEncoding(env, info, kEncAscii); }
napi_value BindingLatin1WriteStatic(napi_env env, napi_callback_info info) { return WriteByEncoding(env, info, kEncLatin1); }
napi_value BindingUtf8WriteStatic(napi_env env, napi_callback_info info) { return WriteByEncoding(env, info, kEncUtf8); }
napi_value BindingBase64Write(napi_env env, napi_callback_info info) { return WriteByEncoding(env, info, kEncBase64); }
napi_value BindingBase64UrlWrite(napi_env env, napi_callback_info info) { return WriteByEncoding(env, info, kEncBase64Url); }
napi_value BindingHexWrite(napi_env env, napi_callback_info info) { return WriteByEncoding(env, info, kEncHex); }
napi_value BindingUcs2Write(napi_env env, napi_callback_info info) { return WriteByEncoding(env, info, kEncUtf16Le); }

napi_value BindingCreateUnsafeArrayBuffer(napi_env env, napi_callback_info info) {
  size_t argc = 1;
  napi_value argv[1] = {nullptr};
  if (napi_get_cb_info(env, info, &argc, argv, nullptr, nullptr) != napi_ok || argc < 1) return nullptr;
  double size_double = 0;
  if (napi_get_value_double(env, argv[0], &size_double) != napi_ok ||
      !std::isfinite(size_double) ||
      size_double < 0 ||
      size_double > std::min(kEdgeBufferMaxLength, static_cast<double>(std::numeric_limits<size_t>::max()))) {
    napi_throw_range_error(env, "ERR_OUT_OF_RANGE", "The value is out of range");
    return nullptr;
  }
  if (size_double >= kEdgeUnsafeArrayBufferAllocCap) {
    napi_throw_error(env, "ERR_MEMORY_ALLOCATION_FAILED", "Array buffer allocation failed");
    return nullptr;
  }

  const size_t size = static_cast<size_t>(std::trunc(size_double));
  if (size == 0) {
    napi_value ab = nullptr;
    void* data = nullptr;
    if (napi_create_arraybuffer(env, 0, &data, &ab) != napi_ok || ab == nullptr) {
      napi_throw_error(env, "ERR_MEMORY_ALLOCATION_FAILED", "Array buffer allocation failed");
      return nullptr;
    }
    return ab;
  }

  void* data = ShouldZeroFillBuffers(env) ? std::calloc(size, 1) : std::malloc(size);
  if (data == nullptr) {
    napi_throw_error(env, "ERR_MEMORY_ALLOCATION_FAILED", "Array buffer allocation failed");
    return nullptr;
  }
  napi_value ab = nullptr;
  if (napi_create_external_arraybuffer(env, data, size, ExternalArrayBufferFinalize, nullptr, &ab) != napi_ok ||
      ab == nullptr) {
    std::free(data);
    napi_throw_error(env, "ERR_MEMORY_ALLOCATION_FAILED", "Array buffer allocation failed");
    return nullptr;
  }
  return ab;
}

napi_value BindingSetBufferPrototype(napi_env env, napi_callback_info info) {
  size_t argc = 1;
  napi_value argv[1] = {nullptr};
  if (napi_get_cb_info(env, info, &argc, argv, nullptr, nullptr) != napi_ok || argc < 1) return nullptr;
  napi_valuetype t = napi_undefined;
  if (napi_typeof(env, argv[0], &t) != napi_ok || t != napi_object) {
    napi_throw_type_error(env, "ERR_INVALID_ARG_TYPE", "The \"prototype\" argument must be an object");
    return nullptr;
  }
  auto& state = EnsureBufferBindingState(env);
  if (state.buffer_prototype_ref != nullptr) {
    napi_delete_reference(env, state.buffer_prototype_ref);
    state.buffer_prototype_ref = nullptr;
  }
  if (napi_create_reference(env, argv[0], 1, &state.buffer_prototype_ref) != napi_ok) {
    return nullptr;
  }
  return MakeUndefined(env);
}

napi_value BindingCopyArrayBuffer(napi_env env, napi_callback_info info) {
  size_t argc = 5;
  napi_value argv[5] = {nullptr, nullptr, nullptr, nullptr, nullptr};
  if (napi_get_cb_info(env, info, &argc, argv, nullptr, nullptr) != napi_ok || argc < 5) return nullptr;

  uint8_t* dst = nullptr;
  uint8_t* src = nullptr;
  size_t dst_len = 0;
  size_t src_len = 0;
  if (!ExtractArrayBufferParts(env, argv[0], &dst, &dst_len) ||
      !ExtractArrayBufferParts(env, argv[2], &src, &src_len)) {
    napi_throw_type_error(env, "ERR_INVALID_ARG_TYPE", "copyArrayBuffer expects (ArrayBuffer, ...)");
    return nullptr;
  }

  uint32_t dst_offset = 0;
  uint32_t src_offset = 0;
  uint32_t bytes_to_copy = 0;
  if (napi_get_value_uint32(env, argv[1], &dst_offset) != napi_ok ||
      napi_get_value_uint32(env, argv[3], &src_offset) != napi_ok ||
      napi_get_value_uint32(env, argv[4], &bytes_to_copy) != napi_ok) {
    napi_throw_type_error(env, "ERR_INVALID_ARG_TYPE", "copyArrayBuffer offsets and length must be uint32");
    return nullptr;
  }

  const size_t dst_off = static_cast<size_t>(dst_offset);
  const size_t src_off = static_cast<size_t>(src_offset);
  const size_t count = static_cast<size_t>(bytes_to_copy);
  if (dst_off > dst_len || src_off > src_len || count > (dst_len - dst_off) || count > (src_len - src_off)) {
    napi_throw_range_error(env, "ERR_OUT_OF_RANGE", "copyArrayBuffer range is out of bounds");
    return nullptr;
  }
  if (count > 0) std::memcpy(dst + dst_off, src + src_off, count);

  napi_value undef = nullptr;
  napi_get_undefined(env, &undef);
  return undef;
}

napi_value BindingAtob(napi_env env, napi_callback_info info) {
  size_t argc = 1;
  napi_value argv[1] = {nullptr};
  if (napi_get_cb_info(env, info, &argc, argv, nullptr, nullptr) != napi_ok || argc < 1) return nullptr;
  napi_value input = nullptr;
  if (napi_coerce_to_string(env, argv[0], &input) != napi_ok || input == nullptr) return nullptr;
  size_t utf16_len = 0;
  if (napi_get_value_string_utf16(env, input, nullptr, 0, &utf16_len) != napi_ok) return nullptr;
  std::vector<char16_t> utf16(utf16_len + 1, 0);
  size_t copied = 0;
  if (napi_get_value_string_utf16(env, input, utf16.data(), utf16.size(), &copied) != napi_ok) return nullptr;

  const size_t max_len =
      simdutf::maximal_binary_length_from_base64(reinterpret_cast<const char16_t*>(utf16.data()), copied);
  std::vector<char> out(max_len);
  simdutf::result result =
      simdutf::base64_to_binary(reinterpret_cast<const char16_t*>(utf16.data()), copied, out.data());
  if (result.error == simdutf::error_code::SUCCESS) {
    return MakeLatin1String(env, reinterpret_cast<const uint8_t*>(out.data()), result.count);
  }

  int32_t error_code = -3;
  if (result.error == simdutf::error_code::INVALID_BASE64_CHARACTER) {
    error_code = -2;
  } else if (result.error == simdutf::error_code::BASE64_INPUT_REMAINDER) {
    error_code = -1;
  }
  return MakeInt32(env, error_code);
}

napi_value BindingBtoa(napi_env env, napi_callback_info info) {
  size_t argc = 1;
  napi_value argv[1] = {nullptr};
  if (napi_get_cb_info(env, info, &argc, argv, nullptr, nullptr) != napi_ok || argc < 1) return nullptr;
  napi_value input_value = nullptr;
  if (napi_coerce_to_string(env, argv[0], &input_value) != napi_ok || input_value == nullptr) return nullptr;
  size_t utf16_len = 0;
  if (napi_get_value_string_utf16(env, input_value, nullptr, 0, &utf16_len) != napi_ok) return nullptr;
  std::vector<char16_t> utf16(utf16_len + 1, 0);
  size_t copied = 0;
  if (napi_get_value_string_utf16(env, input_value, utf16.data(), utf16.size(), &copied) != napi_ok) return nullptr;
  std::string latin1;
  latin1.reserve(copied);
  for (size_t i = 0; i < copied; ++i) {
    if (utf16[i] > 0xff) {
      return MakeInt32(env, -1);
    }
    latin1.push_back(static_cast<char>(utf16[i] & 0xff));
  }
  const size_t out_len = simdutf::base64_length_from_binary(latin1.size());
  std::string out(out_len, '\0');
  const size_t written = simdutf::binary_to_base64(latin1.data(), latin1.size(), out.data());
  out.resize(written);
  return MakeStringUtf8(env, out);
}

napi_value BindingGetZeroFillToggle(napi_env env, napi_callback_info /*info*/) {
  napi_value toggle = GetZeroFillToggleValue(env);
  return toggle != nullptr ? toggle : MakeUndefined(env);
}

void SetMethod(napi_env env, napi_value obj, const char* name, napi_callback cb) {
  napi_value fn = nullptr;
  if (napi_create_function(env, name, NAPI_AUTO_LENGTH, cb, nullptr, &fn) == napi_ok && fn != nullptr) {
    napi_set_named_property(env, obj, name, fn);
  }
}

void SetIntConst(napi_env env, napi_value obj, const char* name, int32_t value) {
  napi_value v = nullptr;
  if (napi_create_int32(env, value, &v) == napi_ok && v != nullptr) {
    napi_set_named_property(env, obj, name, v);
  }
}

void SetNumberConst(napi_env env, napi_value obj, const char* name, double value) {
  napi_value v = nullptr;
  if (napi_create_double(env, value, &v) == napi_ok && v != nullptr) {
    napi_set_named_property(env, obj, name, v);
  }
}

}  // namespace

napi_value EdgeInstallBufferBinding(napi_env env) {
  napi_value binding = nullptr;
  if (napi_create_object(env, &binding) != napi_ok || binding == nullptr) return nullptr;

  SetMethod(env, binding, "byteLengthUtf8", BindingByteLengthUtf8);
  SetMethod(env, binding, "compare", BindingCompare);
  SetMethod(env, binding, "compareOffset", BindingCompareOffset);
  SetMethod(env, binding, "copy", BindingCopy);
  SetMethod(env, binding, "fill", BindingFill);
  SetMethod(env, binding, "isAscii", BindingIsAscii);
  SetMethod(env, binding, "isUtf8", BindingIsUtf8);
  SetMethod(env, binding, "indexOfBuffer", BindingIndexOfBuffer);
  SetMethod(env, binding, "indexOfNumber", BindingIndexOfNumber);
  SetMethod(env, binding, "indexOfString", BindingIndexOfString);
  SetMethod(env, binding, "swap16", BindingSwap16);
  SetMethod(env, binding, "swap32", BindingSwap32);
  SetMethod(env, binding, "swap64", BindingSwap64);
  SetMethod(env, binding, "atob", BindingAtob);
  SetMethod(env, binding, "btoa", BindingBtoa);
  SetMethod(env, binding, "setBufferPrototype", BindingSetBufferPrototype);
  SetMethod(env, binding, "getZeroFillToggle", BindingGetZeroFillToggle);

  SetMethod(env, binding, "asciiSlice", BindingAsciiSlice);
  SetMethod(env, binding, "base64Slice", BindingBase64Slice);
  SetMethod(env, binding, "base64urlSlice", BindingBase64UrlSlice);
  SetMethod(env, binding, "latin1Slice", BindingLatin1Slice);
  SetMethod(env, binding, "hexSlice", BindingHexSlice);
  SetMethod(env, binding, "ucs2Slice", BindingUcs2Slice);
  SetMethod(env, binding, "utf8Slice", BindingUtf8Slice);

  SetMethod(env, binding, "asciiWriteStatic", BindingAsciiWriteStatic);
  SetMethod(env, binding, "latin1WriteStatic", BindingLatin1WriteStatic);
  SetMethod(env, binding, "utf8WriteStatic", BindingUtf8WriteStatic);
  SetMethod(env, binding, "base64Write", BindingBase64Write);
  SetMethod(env, binding, "base64urlWrite", BindingBase64UrlWrite);
  SetMethod(env, binding, "hexWrite", BindingHexWrite);
  SetMethod(env, binding, "ucs2Write", BindingUcs2Write);
  SetMethod(env, binding, "copyArrayBuffer", BindingCopyArrayBuffer);
  SetMethod(env, binding, "createUnsafeArrayBuffer", BindingCreateUnsafeArrayBuffer);

  SetNumberConst(env, binding, "kMaxLength", kEdgeBufferMaxLength);
  SetIntConst(env, binding, "kStringMaxLength", static_cast<int32_t>(kEdgeStringMaxLength));

  return binding;
}
