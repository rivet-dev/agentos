#include "edge_string_decoder.h"

#include <algorithm>
#include <cstddef>
#include <cstdint>
#include <cstring>
#include <string>
#include <vector>

#include "edge_encoding_ids.h"
#include "edge_module_loader.h"

namespace {

constexpr int kIncompleteCharactersStart = 0;
constexpr int kIncompleteCharactersEnd = 4;
constexpr int kMissingBytes = 4;
constexpr int kBufferedBytes = 5;
constexpr int kEncodingField = 6;
constexpr int kSize = 7;
constexpr int kNumFields = 7;
constexpr size_t kEdgeStringMaxLength =
    sizeof(void*) == 4 ? static_cast<size_t>(0x18ffffe8) : static_cast<size_t>(0x1fffffe8);

using edge::encoding_ids::kEncAscii;
using edge::encoding_ids::kEncBase64;
using edge::encoding_ids::kEncBase64Url;
using edge::encoding_ids::kEncHex;
using edge::encoding_ids::kEncLatin1;
using edge::encoding_ids::kEncUtf16Le;
using edge::encoding_ids::kEncUtf8;

void ClearPendingException(napi_env env) {
  if (env == nullptr) return;
  // TODO: Make napi_get_and_clear_last_exception() match upstream Node so
  // this helper does not need a separate pending-exception check first.
  bool pending = false;
  if (napi_is_exception_pending(env, &pending) != napi_ok || !pending) return;
  napi_value ignored = nullptr;
  napi_get_and_clear_last_exception(env, &ignored);
}

void SetMethod(napi_env env, napi_value obj, const char* name, napi_callback cb) {
  napi_value fn = nullptr;
  if (napi_create_function(env, name, NAPI_AUTO_LENGTH, cb, nullptr, &fn) == napi_ok && fn != nullptr) {
    napi_set_named_property(env, obj, name, fn);
  }
}

size_t TypedArrayElementSize(napi_typedarray_type t) {
  switch (t) {
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

bool ReadState(napi_env env, napi_value state_value, uint8_t** state, size_t* state_len) {
  void* data = nullptr;
  size_t len = 0;
  bool is_buffer = false;
  if (napi_is_buffer(env, state_value, &is_buffer) == napi_ok && is_buffer) {
    if (napi_get_buffer_info(env, state_value, &data, &len) == napi_ok &&
        data != nullptr &&
        len >= kSize) {
      *state = static_cast<uint8_t*>(data);
      *state_len = len;
      return true;
    }
  }

  bool is_typedarray = false;
  if (napi_is_typedarray(env, state_value, &is_typedarray) == napi_ok && is_typedarray) {
    napi_typedarray_type t;
    size_t element_len = 0;
    napi_value ab = nullptr;
    size_t byte_offset = 0;
    if (napi_get_typedarray_info(env, state_value, &t, &element_len, &data, &ab, &byte_offset) == napi_ok &&
        data != nullptr) {
      const size_t byte_len = element_len * TypedArrayElementSize(t);
      if (byte_len < kSize) return false;
      *state = static_cast<uint8_t*>(data);
      *state_len = byte_len;
      return true;
    }
  }

  bool is_dataview = false;
  if (napi_is_dataview(env, state_value, &is_dataview) == napi_ok && is_dataview) {
    napi_value ab = nullptr;
    size_t byte_offset = 0;
    if (napi_get_dataview_info(env, state_value, &len, &data, &ab, &byte_offset) == napi_ok &&
        len >= kSize) {
      if (ab != nullptr) {
        void* ab_data = nullptr;
        size_t ab_len = 0;
        if (napi_get_arraybuffer_info(env, ab, &ab_data, &ab_len) == napi_ok &&
            ab_data != nullptr &&
            byte_offset <= ab_len &&
            len <= (ab_len - byte_offset)) {
          data = static_cast<uint8_t*>(ab_data) + byte_offset;
        }
      }
      if (data == nullptr) return false;
      *state = static_cast<uint8_t*>(data);
      *state_len = len;
      return true;
    }
  }

  return false;
}

bool ReadView(napi_env env, napi_value value, const uint8_t** data, size_t* len) {
  bool is_buffer = false;
  if (napi_is_buffer(env, value, &is_buffer) == napi_ok && is_buffer) {
    void* ptr = nullptr;
    size_t blen = 0;
    if (napi_get_buffer_info(env, value, &ptr, &blen) != napi_ok) return false;
    *data = static_cast<const uint8_t*>(ptr);
    *len = blen;
    return true;
  }

  bool is_typedarray = false;
  if (napi_is_typedarray(env, value, &is_typedarray) == napi_ok && is_typedarray) {
    napi_typedarray_type t;
    size_t element_len = 0;
    void* ptr = nullptr;
    napi_value ab = nullptr;
    size_t byte_offset = 0;
    if (napi_get_typedarray_info(env, value, &t, &element_len, &ptr, &ab, &byte_offset) != napi_ok) return false;
    *data = static_cast<const uint8_t*>(ptr);
    *len = element_len * TypedArrayElementSize(t);
    return true;
  }

  bool is_dataview = false;
  if (napi_is_dataview(env, value, &is_dataview) == napi_ok && is_dataview) {
    size_t byte_len = 0;
    void* ptr = nullptr;
    napi_value ab = nullptr;
    size_t byte_offset = 0;
    if (napi_get_dataview_info(env, value, &byte_len, &ptr, &ab, &byte_offset) != napi_ok) return false;
    if (ab != nullptr) {
      void* ab_data = nullptr;
      size_t ab_len = 0;
      if (napi_get_arraybuffer_info(env, ab, &ab_data, &ab_len) == napi_ok &&
          ab_data != nullptr &&
          byte_offset <= ab_len &&
          byte_len <= (ab_len - byte_offset)) {
        ptr = static_cast<uint8_t*>(ab_data) + byte_offset;
      }
    }
    if (ptr == nullptr) return false;
    *data = static_cast<const uint8_t*>(ptr);
    *len = byte_len;
    return true;
  }

  return false;
}

const char* EncodingName(uint8_t enc) {
  switch (enc) {
    case kEncUtf8: return "utf8";
    case kEncUtf16Le: return "utf16le";
    case kEncLatin1: return "latin1";
    case kEncAscii: return "ascii";
    case kEncBase64: return "base64";
    case kEncBase64Url: return "base64url";
    case kEncHex: return "hex";
    default: return "utf8";
  }
}

bool ThrowIfStringTooLong(napi_env env, size_t length) {
  if (length <= kEdgeStringMaxLength) return false;
  napi_throw_error(env, "ERR_STRING_TOO_LONG", "Cannot create a string longer than 0x1fffffe8 characters");
  return true;
}

napi_value MakeStringFromBytes(napi_env env, const uint8_t* data, size_t len, uint8_t enc) {
  napi_value out = nullptr;
  if (len == 0) {
    napi_create_string_utf8(env, "", 0, &out);
    return out;
  }

  size_t max_decoded_length = len;
  if (enc == kEncUtf16Le) {
    max_decoded_length = len / 2;
  } else if (enc == kEncBase64 || enc == kEncBase64Url) {
    max_decoded_length = ((len + 2) / 3) * 4;
  } else if (enc == kEncHex) {
    max_decoded_length = len * 2;
  }
  if (ThrowIfStringTooLong(env, max_decoded_length)) return nullptr;

  // First try global Buffer.from(...).toString(enc) so we match the runtime's
  // exact Buffer decoding behavior used by tests for comparison. UTF-8 is
  // decoded below because StringDecoder has streaming boundary rules that are
  // stricter than a raw Buffer decode fallback.
  if (enc != kEncUtf8) {
    napi_value global = nullptr;
    napi_value buffer_ctor = nullptr;
    napi_value from_fn = nullptr;
    napi_valuetype ctor_type = napi_undefined;
    if (napi_get_global(env, &global) == napi_ok &&
        napi_get_named_property(env, global, "Buffer", &buffer_ctor) == napi_ok &&
        napi_typeof(env, buffer_ctor, &ctor_type) == napi_ok &&
        ctor_type == napi_function &&
        napi_get_named_property(env, buffer_ctor, "from", &from_fn) == napi_ok) {
      void* ab_data = nullptr;
      napi_value array_buffer = nullptr;
      if (napi_create_arraybuffer(env, len, &ab_data, &array_buffer) == napi_ok && ab_data != nullptr) {
        std::memcpy(ab_data, data, len);
        napi_value uint8_view = nullptr;
        if (napi_create_typedarray(env,
                                   napi_uint8_array,
                                   len,
                                   array_buffer,
                                   0,
                                   &uint8_view) == napi_ok &&
            uint8_view != nullptr) {
          napi_value from_argv[1] = { uint8_view };
          napi_value buffer_value = nullptr;
          if (napi_call_function(env, buffer_ctor, from_fn, 1, from_argv, &buffer_value) == napi_ok &&
              buffer_value != nullptr) {
            napi_value to_string_fn = nullptr;
            napi_value encoding = nullptr;
            if (napi_get_named_property(env, buffer_value, "toString", &to_string_fn) == napi_ok &&
                napi_create_string_utf8(env, EncodingName(enc), NAPI_AUTO_LENGTH, &encoding) == napi_ok) {
              napi_value to_string_argv[1] = { encoding };
              if (napi_call_function(env, buffer_value, to_string_fn, 1, to_string_argv, &out) == napi_ok &&
                  out != nullptr) {
                return out;
              }
            }
          }
          ClearPendingException(env);
        }
      }
    }
  }

  // Prefer using the JS Buffer module decoding path for exact parity with
  // Buffer#toString() behavior that Node's tests compare against.
  if (enc != kEncUtf8) {
    napi_value global = nullptr;
    napi_value require_fn = EdgeGetRequireFunction(env);
    napi_valuetype require_type = napi_undefined;
    if (napi_get_global(env, &global) == napi_ok &&
        (require_fn != nullptr ||
         napi_get_named_property(env, global, "require", &require_fn) == napi_ok) &&
        napi_typeof(env, require_fn, &require_type) == napi_ok &&
        require_type == napi_function) {
      napi_value buffer_mod_name = nullptr;
      napi_value buffer_mod = nullptr;
      if (napi_create_string_utf8(env, "buffer", NAPI_AUTO_LENGTH, &buffer_mod_name) == napi_ok &&
          napi_call_function(env, global, require_fn, 1, &buffer_mod_name, &buffer_mod) == napi_ok &&
          buffer_mod != nullptr) {
        napi_value buffer_ctor = nullptr;
        napi_value from_fn = nullptr;
        if (napi_get_named_property(env, buffer_mod, "Buffer", &buffer_ctor) == napi_ok &&
            napi_get_named_property(env, buffer_ctor, "from", &from_fn) == napi_ok) {
          void* ab_data = nullptr;
          napi_value array_buffer = nullptr;
          if (napi_create_arraybuffer(env, len, &ab_data, &array_buffer) == napi_ok &&
              ab_data != nullptr) {
            std::memcpy(ab_data, data, len);
            napi_value uint8_view = nullptr;
            if (napi_create_typedarray(env,
                                       napi_uint8_array,
                                       len,
                                       array_buffer,
                                       0,
                                       &uint8_view) == napi_ok &&
                uint8_view != nullptr) {
              napi_value from_argv[1] = { uint8_view };
              napi_value buffer_value = nullptr;
              if (napi_call_function(env, buffer_ctor, from_fn, 1, from_argv, &buffer_value) == napi_ok &&
                  buffer_value != nullptr) {
                napi_value to_string_fn = nullptr;
                napi_value encoding = nullptr;
                if (napi_get_named_property(env, buffer_value, "toString", &to_string_fn) == napi_ok &&
                    napi_create_string_utf8(env, EncodingName(enc), NAPI_AUTO_LENGTH, &encoding) == napi_ok) {
                  napi_value to_string_argv[1] = { encoding };
                  if (napi_call_function(env, buffer_value, to_string_fn, 1, to_string_argv, &out) == napi_ok &&
                      out != nullptr) {
                    return out;
                  }
                }
              }
              ClearPendingException(env);
            }
          }
        }
      }
    }
  }

  if (enc == kEncUtf8) {
    std::u16string s;
    s.reserve(len);
    size_t i = 0;
    auto append_replacement = [&]() {
      s.push_back(static_cast<char16_t>(0xFFFD));
    };
    auto append_code_point = [&](uint32_t cp) {
      if (cp <= 0xFFFF) {
        s.push_back(static_cast<char16_t>(cp));
      } else {
        cp -= 0x10000;
        s.push_back(static_cast<char16_t>(0xD800 + ((cp >> 10) & 0x3FF)));
        s.push_back(static_cast<char16_t>(0xDC00 + (cp & 0x3FF)));
      }
    };

    while (i < len) {
      const uint8_t b0 = data[i];
      if (b0 < 0x80) {
        s.push_back(static_cast<char16_t>(b0));
        ++i;
        continue;
      }

      size_t needed = 0;
      uint32_t cp = 0;
      uint32_t min_cp = 0;
      if (b0 >= 0xC2 && b0 <= 0xDF) {
        needed = 1;
        cp = b0 & 0x1F;
        min_cp = 0x80;
      } else if (b0 >= 0xE0 && b0 <= 0xEF) {
        needed = 2;
        cp = b0 & 0x0F;
        min_cp = 0x800;
      } else if (b0 >= 0xF0 && b0 <= 0xF4) {
        needed = 3;
        cp = b0 & 0x07;
        min_cp = 0x10000;
      } else {
        append_replacement();
        ++i;
        continue;
      }

      size_t j = 1;
      for (; j <= needed && i + j < len; ++j) {
        const uint8_t bx = data[i + j];
        if ((bx & 0xC0) != 0x80) break;
        cp = (cp << 6) | (bx & 0x3F);
      }

      if (j <= needed) {
        append_replacement();
        bool prefix_invalid = false;
        if (j > 1) {
          const uint8_t b1 = data[i + 1];
          if (needed == 2 && b0 == 0xE0 && b1 < 0xA0) prefix_invalid = true;
          if (needed == 2 && b0 == 0xED && b1 > 0x9F) prefix_invalid = true;
          if (needed == 3 && b0 == 0xF0 && b1 < 0x90) prefix_invalid = true;
          if (needed == 3 && b0 == 0xF4 && b1 > 0x8F) prefix_invalid = true;
        }
        if (prefix_invalid) {
          ++i;
        } else {
          i += j;
        }
        continue;
      }

      bool valid = true;
      if (cp < min_cp || cp > 0x10FFFF || (cp >= 0xD800 && cp <= 0xDFFF)) valid = false;
      if (needed == 2 && b0 == 0xE0 && data[i + 1] < 0xA0) valid = false;
      if (needed == 2 && b0 == 0xED && data[i + 1] > 0x9F) valid = false;
      if (needed == 3 && b0 == 0xF0 && data[i + 1] < 0x90) valid = false;
      if (needed == 3 && b0 == 0xF4 && data[i + 1] > 0x8F) valid = false;

      if (!valid) {
        append_replacement();
        ++i;
        continue;
      }

      append_code_point(cp);
      i += needed + 1;
    }

    if (napi_create_string_utf16(env,
                                 reinterpret_cast<const char16_t*>(s.data()),
                                 s.size(),
                                 &out) != napi_ok) {
      return nullptr;
    }
    return out;
  }

  if (enc == kEncAscii) {
    std::string s;
    s.resize(len);
    for (size_t i = 0; i < len; ++i) s[i] = static_cast<char>(data[i] & 0x7F);
    if (napi_create_string_utf8(env, s.c_str(), s.size(), &out) != napi_ok) return nullptr;
    return out;
  }

  if (enc == kEncLatin1) {
    std::u16string s;
    s.resize(len);
    for (size_t i = 0; i < len; ++i) s[i] = static_cast<char16_t>(data[i]);
    if (napi_create_string_utf16(env, reinterpret_cast<const char16_t*>(s.data()), s.size(), &out) != napi_ok) return nullptr;
    return out;
  }

  if (enc == kEncUtf16Le) {
    const size_t n = len / 2;
    std::u16string s;
    s.resize(n);
    for (size_t i = 0; i < n; ++i) {
      s[i] = static_cast<char16_t>(data[2 * i] | (static_cast<uint16_t>(data[2 * i + 1]) << 8));
    }
    if (napi_create_string_utf16(env, reinterpret_cast<const char16_t*>(s.data()), s.size(), &out) != napi_ok) return nullptr;
    return out;
  }

  if (enc == kEncHex) {
    static const char* kHex = "0123456789abcdef";
    std::string s;
    s.resize(len * 2);
    for (size_t i = 0; i < len; ++i) {
      s[2 * i] = kHex[(data[i] >> 4) & 0xF];
      s[2 * i + 1] = kHex[data[i] & 0xF];
    }
    if (napi_create_string_utf8(env, s.c_str(), s.size(), &out) != napi_ok) return nullptr;
    return out;
  }

  if (enc == kEncBase64 || enc == kEncBase64Url) {
    static const char* kStd = "ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    static const char* kUrl = "ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-_";
    const char* table = (enc == kEncBase64Url) ? kUrl : kStd;
    std::string s;
    s.reserve(((len + 2) / 3) * 4);
    size_t i = 0;
    for (; i + 2 < len; i += 3) {
      const uint32_t n = (static_cast<uint32_t>(data[i]) << 16) |
                         (static_cast<uint32_t>(data[i + 1]) << 8) |
                         static_cast<uint32_t>(data[i + 2]);
      s.push_back(table[(n >> 18) & 0x3F]);
      s.push_back(table[(n >> 12) & 0x3F]);
      s.push_back(table[(n >> 6) & 0x3F]);
      s.push_back(table[n & 0x3F]);
    }
    if (i < len) {
      const uint8_t a = data[i];
      const bool has_b = (i + 1 < len);
      const uint8_t b = has_b ? data[i + 1] : 0;
      s.push_back(table[(a >> 2) & 0x3F]);
      s.push_back(table[((a & 0x03) << 4) | ((b >> 4) & 0x0F)]);
      if (has_b) {
        s.push_back(table[(b & 0x0F) << 2]);
        if (enc == kEncBase64) s.push_back('=');
      } else if (enc == kEncBase64) {
        s.push_back('=');
        s.push_back('=');
      }
    }
    if (napi_create_string_utf8(env, s.c_str(), s.size(), &out) != napi_ok) return nullptr;
    return out;
  }

  if (napi_create_string_utf8(env, reinterpret_cast<const char*>(data), len, &out) != napi_ok) return nullptr;
  return out;
}

napi_value ConcatStrings(napi_env env, napi_value a, napi_value b) {
  napi_value sa = nullptr;
  napi_value sb = nullptr;
  if (napi_coerce_to_string(env, a, &sa) != napi_ok) return nullptr;
  if (napi_coerce_to_string(env, b, &sb) != napi_ok) return nullptr;
  napi_value global = nullptr;
  napi_value string_ctor = nullptr;
  napi_value string_proto = nullptr;
  napi_value concat_fn = nullptr;
  if (napi_get_global(env, &global) != napi_ok ||
      napi_get_named_property(env, global, "String", &string_ctor) != napi_ok ||
      napi_get_named_property(env, string_ctor, "prototype", &string_proto) != napi_ok ||
      napi_get_named_property(env, string_proto, "concat", &concat_fn) != napi_ok) {
    return nullptr;
  }
  napi_value out = nullptr;
  napi_value argv[1] = { sb };
  if (napi_call_function(env, sa, concat_fn, 1, argv, &out) != napi_ok) return nullptr;
  return out;
}

napi_value DecodeBinding(napi_env env, napi_callback_info info) {
  size_t argc = 2;
  napi_value argv[2] = {nullptr, nullptr};
  if (napi_get_cb_info(env, info, &argc, argv, nullptr, nullptr) != napi_ok || argc < 2) return nullptr;

  uint8_t* state = nullptr;
  size_t state_len = 0;
  if (!ReadState(env, argv[0], &state, &state_len)) return nullptr;
  (void)state_len;

  const uint8_t* data = nullptr;
  size_t nread = 0;
  if (!ReadView(env, argv[1], &data, &nread)) return nullptr;

  const uint8_t enc = state[kEncodingField];
  const bool variable = (enc == kEncUtf8 || enc == kEncUtf16Le || enc == kEncBase64 || enc == kEncBase64Url);
  const size_t max_input_for_string =
      enc == kEncUtf16Le ? kEdgeStringMaxLength * 2 : kEdgeStringMaxLength;
  if (nread > max_input_for_string) {
    napi_throw_range_error(env, "ERR_STRING_TOO_LONG", "Cannot create a string longer than the maximum allowed length");
    return nullptr;
  }
  if (!variable) {
    return MakeStringFromBytes(env, data, nread, enc);
  }

  napi_value prepend = nullptr;
  bool has_prepend = false;

  if (state[kMissingBytes] > 0) {
    if (enc == kEncUtf8) {
      for (size_t i = 0; i < nread && i < state[kMissingBytes]; ++i) {
        if ((data[i] & 0xC0) != 0x80) {
          state[kMissingBytes] = 0;
          std::memcpy(state + kIncompleteCharactersStart + state[kBufferedBytes], data, i);
          state[kBufferedBytes] = static_cast<uint8_t>(state[kBufferedBytes] + i);
          data += i;
          nread -= i;
          break;
        }
      }
    }

    const size_t found = std::min(nread, static_cast<size_t>(state[kMissingBytes]));
    std::memcpy(state + kIncompleteCharactersStart + state[kBufferedBytes], data, found);
    data += found;
    nread -= found;
    state[kMissingBytes] = static_cast<uint8_t>(state[kMissingBytes] - found);
    state[kBufferedBytes] = static_cast<uint8_t>(state[kBufferedBytes] + found);

    if (state[kMissingBytes] == 0) {
      prepend = MakeStringFromBytes(env, state + kIncompleteCharactersStart, state[kBufferedBytes], enc);
      has_prepend = prepend != nullptr;
      state[kBufferedBytes] = 0;
    }
  }

  napi_value body = nullptr;
  if (nread == 0) {
    if (has_prepend) return prepend;
    napi_create_string_utf8(env, "", 0, &body);
    return body;
  }

  if (enc == kEncUtf8 && (data[nread - 1] & 0x80)) {
    for (size_t i = nread - 1;; --i) {
      state[kBufferedBytes] = static_cast<uint8_t>(state[kBufferedBytes] + 1);
      if ((data[i] & 0xC0) == 0x80) {
        if (state[kBufferedBytes] >= 4 || i == 0) {
          state[kBufferedBytes] = 0;
          break;
        }
      } else {
        if ((data[i] & 0xE0) == 0xC0) state[kMissingBytes] = 2;
        else if ((data[i] & 0xF0) == 0xE0) state[kMissingBytes] = 3;
        else if ((data[i] & 0xF8) == 0xF0) state[kMissingBytes] = 4;
        else {
          state[kBufferedBytes] = 0;
          break;
        }
        if (state[kBufferedBytes] >= state[kMissingBytes]) {
          state[kMissingBytes] = 0;
          state[kBufferedBytes] = 0;
        } else {
          state[kMissingBytes] = static_cast<uint8_t>(state[kMissingBytes] - state[kBufferedBytes]);
        }
        break;
      }
      if (i == 0) break;
    }
  } else if (enc == kEncUtf16Le) {
    if ((nread % 2) == 1) {
      state[kBufferedBytes] = 1;
      state[kMissingBytes] = 1;
    } else if ((data[nread - 1] & 0xFC) == 0xD8) {
      state[kBufferedBytes] = 2;
      state[kMissingBytes] = 2;
    }
  } else if (enc == kEncBase64 || enc == kEncBase64Url) {
    state[kBufferedBytes] = static_cast<uint8_t>(nread % 3);
    if (state[kBufferedBytes] > 0) state[kMissingBytes] = static_cast<uint8_t>(3 - state[kBufferedBytes]);
  }

  if (state[kBufferedBytes] > 0) {
    nread -= state[kBufferedBytes];
    std::memcpy(state + kIncompleteCharactersStart, data + nread, state[kBufferedBytes]);
  }

  body = MakeStringFromBytes(env, data, nread, enc);
  if (!has_prepend) return body;
  return ConcatStrings(env, prepend, body);
}

napi_value FlushBinding(napi_env env, napi_callback_info info) {
  size_t argc = 1;
  napi_value argv[1] = {nullptr};
  if (napi_get_cb_info(env, info, &argc, argv, nullptr, nullptr) != napi_ok || argc < 1) return nullptr;

  uint8_t* state = nullptr;
  size_t state_len = 0;
  if (!ReadState(env, argv[0], &state, &state_len)) return nullptr;
  (void)state_len;

  const uint8_t enc = state[kEncodingField];
  if (enc == kEncUtf16Le && (state[kBufferedBytes] % 2) == 1) {
    state[kMissingBytes] = static_cast<uint8_t>(state[kMissingBytes] - 1);
    state[kBufferedBytes] = static_cast<uint8_t>(state[kBufferedBytes] - 1);
  }
  if (state[kBufferedBytes] == 0) {
    napi_value empty = nullptr;
    napi_create_string_utf8(env, "", 0, &empty);
    return empty;
  }

  napi_value ret = MakeStringFromBytes(env, state + kIncompleteCharactersStart, state[kBufferedBytes], enc);
  state[kMissingBytes] = 0;
  state[kBufferedBytes] = 0;
  return ret;
}

}  // namespace

napi_value EdgeInstallStringDecoderBinding(napi_env env) {
  napi_value binding = nullptr;
  if (napi_create_object(env, &binding) != napi_ok || binding == nullptr) return nullptr;

  auto set_i32 = [&](const char* name, int32_t v) {
    napi_value n = nullptr;
    if (napi_create_int32(env, v, &n) == napi_ok && n != nullptr) {
      napi_set_named_property(env, binding, name, n);
    }
  };

  set_i32("kIncompleteCharactersStart", kIncompleteCharactersStart);
  set_i32("kIncompleteCharactersEnd", kIncompleteCharactersEnd);
  set_i32("kMissingBytes", kMissingBytes);
  set_i32("kBufferedBytes", kBufferedBytes);
  set_i32("kEncodingField", kEncodingField);
  set_i32("kSize", kSize);
  set_i32("kNumFields", kNumFields);

  // Node's internal/util expects internalBinding('string_decoder').encodings.
  {
    const char* names[] = {"ascii", "utf8", "base64", "base64url", "utf16le", "hex", "buffer", "latin1"};
    napi_value enc_arr = nullptr;
    if (napi_create_array_with_length(env, sizeof(names) / sizeof(names[0]), &enc_arr) == napi_ok &&
        enc_arr != nullptr) {
      for (uint32_t i = 0; i < sizeof(names) / sizeof(names[0]); ++i) {
        napi_value s = nullptr;
        if (napi_create_string_utf8(env, names[i], NAPI_AUTO_LENGTH, &s) == napi_ok && s != nullptr) {
          napi_set_element(env, enc_arr, i, s);
        }
      }
      napi_set_named_property(env, binding, "encodings", enc_arr);
    }
  }

  SetMethod(env, binding, "decode", DecodeBinding);
  SetMethod(env, binding, "flush", FlushBinding);

  return binding;
}
