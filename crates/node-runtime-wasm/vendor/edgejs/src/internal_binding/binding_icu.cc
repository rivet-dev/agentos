#include "internal_binding/binding_initializers.h"

#include <algorithm>
#include <cctype>
#include <cstdint>
#include <cstring>
#include <memory>
#include <string>
#include <string_view>
#include <unordered_map>
#include <unordered_set>
#include <vector>

#define U_DISABLE_RENAMING 1
#include <unicode/uchar.h>
#include <unicode/ucnv.h>
#include <unicode/ucnv_cb.h>
#include <unicode/utf16.h>
#include <unicode/utypes.h>

#include "edge_environment.h"
#include "internal_binding/helpers.h"

namespace internal_binding {

namespace {

const char* ZeroLengthByteSentinel() {
  static const char sentinel = 0;
  return &sentinel;
}

constexpr uint32_t kConverterFlagsFlush = 0x1;
constexpr uint32_t kConverterFlagsFatal = 0x2;
constexpr uint32_t kConverterFlagsIgnoreBom = 0x4;

void DeleteRefIfPresent(napi_env env, napi_ref* ref);

struct IcuBindingState {
  explicit IcuBindingState(napi_env env_in) : env(env_in) {}
  ~IcuBindingState() {
    DeleteRefIfPresent(env, &binding_ref);
  }

  napi_env env = nullptr;
  napi_ref binding_ref = nullptr;
};

struct ConverterWrap {
  UConverter* converter = nullptr;
  uint32_t flags = 0;
  bool bom_seen = false;
  bool unicode = false;
};

void DeleteRefIfPresent(napi_env env, napi_ref* ref) {
  if (env == nullptr || ref == nullptr || *ref == nullptr) return;
  napi_delete_reference(env, *ref);
  *ref = nullptr;
}

IcuBindingState* GetIcuState(napi_env env) {
  return EdgeEnvironmentGetSlotData<IcuBindingState>(env, kEdgeEnvironmentSlotIcuBindingState);
}

IcuBindingState& EnsureIcuState(napi_env env) {
  return EdgeEnvironmentGetOrCreateSlotData<IcuBindingState>(
      env, kEdgeEnvironmentSlotIcuBindingState);
}

bool IsBigEndian() {
  const uint16_t marker = 0x0102;
  return *(reinterpret_cast<const uint8_t*>(&marker)) == 0x01;
}

void LowerAscii(std::string* text) {
  if (text == nullptr) return;
  for (char& ch : *text) {
    ch = static_cast<char>(std::tolower(static_cast<unsigned char>(ch)));
  }
}

bool ReadUtf8(napi_env env, napi_value value, std::string* out) {
  if (value == nullptr || out == nullptr) return false;
  size_t length = 0;
  if (napi_get_value_string_utf8(env, value, nullptr, 0, &length) != napi_ok) return false;
  std::string text(length + 1, '\0');
  size_t written = 0;
  if (napi_get_value_string_utf8(env, value, text.data(), text.size(), &written) != napi_ok) return false;
  text.resize(written);
  *out = std::move(text);
  return true;
}

bool ReadUtf16(napi_env env, napi_value value, std::vector<char16_t>* out) {
  if (value == nullptr || out == nullptr) return false;
  size_t length = 0;
  if (napi_get_value_string_utf16(env, value, nullptr, 0, &length) != napi_ok) return false;
  std::vector<char16_t> buffer(length + 1, 0);
  size_t written = 0;
  if (napi_get_value_string_utf16(env, value, buffer.data(), buffer.size(), &written) != napi_ok) return false;
  buffer.resize(written);
  *out = std::move(buffer);
  return true;
}

bool ReadByteSpan(napi_env env, napi_value value, const char** data, size_t* length) {
  if (value == nullptr || data == nullptr || length == nullptr) return false;

  bool is_buffer = false;
  if (napi_is_buffer(env, value, &is_buffer) == napi_ok && is_buffer) {
    void* raw = nullptr;
    if (napi_get_buffer_info(env, value, &raw, length) != napi_ok) return false;
    if (*length > 0 && raw == nullptr) return false;
    *data = static_cast<const char*>(raw);
    return true;
  }

  bool is_typedarray = false;
  if (napi_is_typedarray(env, value, &is_typedarray) == napi_ok && is_typedarray) {
    napi_typedarray_type type = napi_uint8_array;
    size_t element_length = 0;
    void* raw = nullptr;
    napi_value arraybuffer = nullptr;
    size_t byte_offset = 0;
    if (napi_get_typedarray_info(
            env, value, &type, &element_length, &raw, &arraybuffer, &byte_offset) != napi_ok) {
      return false;
    }

    size_t element_size = 1;
    switch (type) {
      case napi_int16_array:
      case napi_uint16_array:
      case napi_float16_array:
        element_size = 2;
        break;
      case napi_int32_array:
      case napi_uint32_array:
      case napi_float32_array:
        element_size = 4;
        break;
      case napi_float64_array:
      case napi_bigint64_array:
      case napi_biguint64_array:
        element_size = 8;
        break;
      default:
        element_size = 1;
        break;
    }

    *length = element_length * element_size;
    if (*length > 0 && raw == nullptr) return false;
    *data = static_cast<const char*>(raw);
    return true;
  }

  bool is_dataview = false;
  if (napi_is_dataview(env, value, &is_dataview) == napi_ok && is_dataview) {
    void* raw = nullptr;
    napi_value arraybuffer = nullptr;
    size_t byte_offset = 0;
    if (napi_get_dataview_info(env, value, length, &raw, &arraybuffer, &byte_offset) != napi_ok) {
      return false;
    }
    if (*length > 0 && raw == nullptr) return false;
    *data = static_cast<const char*>(raw);
    return true;
  }

  bool is_arraybuffer = false;
  if (napi_is_arraybuffer(env, value, &is_arraybuffer) == napi_ok && is_arraybuffer) {
    void* raw = nullptr;
    if (napi_get_arraybuffer_info(env, value, &raw, length) != napi_ok) return false;
    if (*length > 0 && raw == nullptr) return false;
    *data = raw != nullptr ? static_cast<const char*>(raw) : ZeroLengthByteSentinel();
    return true;
  }

  {
    void* raw = nullptr;
    size_t byte_length = 0;
    if (napi_get_arraybuffer_info(env, value, &raw, &byte_length) == napi_ok) {
      if (byte_length > 0 && raw == nullptr) return false;
      *data = raw != nullptr ? static_cast<const char*>(raw) : ZeroLengthByteSentinel();
      *length = byte_length;
      return true;
    }
  }

  return false;
}

bool CreateBufferCopy(napi_env env, const char* data, size_t length, napi_value* out) {
  if (out == nullptr) return false;
  void* copied = nullptr;
  return napi_create_buffer_copy(env, length, data, &copied, out) == napi_ok && *out != nullptr;
}

bool CreateStringFromUtf16(napi_env env, const UChar* data, size_t length, napi_value* out) {
  if (out == nullptr) return false;
  if (length == 0) {
    return napi_create_string_utf8(env, "", 0, out) == napi_ok && *out != nullptr;
  }

  if (!IsBigEndian()) {
    return napi_create_string_utf16(env, reinterpret_cast<const char16_t*>(data), length, out) == napi_ok &&
           *out != nullptr;
  }

  std::vector<char16_t> swapped(length);
  for (size_t i = 0; i < length; ++i) {
    const uint16_t raw = static_cast<uint16_t>(data[i]);
    swapped[i] = static_cast<char16_t>((raw >> 8) | (raw << 8));
  }
  return napi_create_string_utf16(env, swapped.data(), swapped.size(), out) == napi_ok && *out != nullptr;
}

enum class BufferEncoding {
  kAscii,
  kLatin1,
  kUcs2,
  kUtf8,
  kUnsupported,
};

BufferEncoding ParseBufferEncoding(std::string encoding) {
  LowerAscii(&encoding);
  if (encoding == "ascii") return BufferEncoding::kAscii;
  if (encoding == "latin1" || encoding == "binary") return BufferEncoding::kLatin1;
  if (encoding == "ucs2" || encoding == "ucs-2" || encoding == "utf16le" || encoding == "utf-16le") {
    return BufferEncoding::kUcs2;
  }
  if (encoding == "utf8" || encoding == "utf-8") return BufferEncoding::kUtf8;
  return BufferEncoding::kUnsupported;
}

const char* BufferEncodingToIcuName(BufferEncoding encoding) {
  switch (encoding) {
    case BufferEncoding::kAscii:
      return "us-ascii";
    case BufferEncoding::kLatin1:
      return "iso8859-1";
    case BufferEncoding::kUcs2:
      return "utf16le";
    case BufferEncoding::kUtf8:
      return "utf-8";
    case BufferEncoding::kUnsupported:
      return nullptr;
  }
  return nullptr;
}

bool WrapConverter(napi_env env, napi_value object, ConverterWrap* wrap) {
  return napi_wrap(
             env,
             object,
             wrap,
             [](napi_env env, void* data, void* /*hint*/) {
               auto* wrap = static_cast<ConverterWrap*>(data);
               if (wrap == nullptr) return;
               if (wrap->converter != nullptr) {
                 ucnv_close(wrap->converter);
                 wrap->converter = nullptr;
               }
               delete wrap;
             },
             nullptr,
             nullptr) == napi_ok;
}

ConverterWrap* UnwrapConverter(napi_env env, napi_value value) {
  if (value == nullptr) return nullptr;
  void* data = nullptr;
  if (napi_unwrap(env, value, &data) != napi_ok || data == nullptr) return nullptr;
  return static_cast<ConverterWrap*>(data);
}

bool IsTruthyBool(napi_env env, napi_value value, bool* out) {
  if (out == nullptr) return false;
  napi_valuetype type = napi_undefined;
  if (value == nullptr || napi_typeof(env, value, &type) != napi_ok || type != napi_boolean) return false;
  return napi_get_value_bool(env, value, out) == napi_ok;
}

int GetColumnWidth(UChar32 codepoint, bool ambiguous_as_full_width) {
  const int east_asian_width = u_getIntPropertyValue(codepoint, UCHAR_EAST_ASIAN_WIDTH);
  switch (east_asian_width) {
    case U_EA_FULLWIDTH:
    case U_EA_WIDE:
      return 2;
    case U_EA_AMBIGUOUS:
      if (ambiguous_as_full_width) return 2;
      [[fallthrough]];
    case U_EA_NEUTRAL:
      if (u_hasBinaryProperty(codepoint, UCHAR_EMOJI_PRESENTATION)) return 2;
      [[fallthrough]];
    case U_EA_HALFWIDTH:
    case U_EA_NARROW:
    default: {
      const auto zero_width_mask =
          U_GC_CC_MASK | U_GC_CF_MASK | U_GC_ME_MASK | U_GC_MN_MASK;
      if (codepoint != 0x00AD &&
          ((U_MASK(u_charType(codepoint)) & zero_width_mask) ||
           u_hasBinaryProperty(codepoint, UCHAR_EMOJI_MODIFIER))) {
        return 0;
      }
      return 1;
    }
  }
}

void MaybeResetConverter(ConverterWrap* wrap, bool flush) {
  if (wrap == nullptr || wrap->converter == nullptr || !flush) return;
  wrap->bom_seen = false;
  ucnv_reset(wrap->converter);
}

napi_value ICUErrorNameCallback(napi_env env, napi_callback_info info) {
  size_t argc = 1;
  napi_value argv[1] = {nullptr};
  if (napi_get_cb_info(env, info, &argc, argv, nullptr, nullptr) != napi_ok || argc < 1) {
    return Undefined(env);
  }

  int32_t status_value = 0;
  if (napi_get_value_int32(env, argv[0], &status_value) != napi_ok) return Undefined(env);

  napi_value out = nullptr;
  napi_create_string_utf8(env, u_errorName(static_cast<UErrorCode>(status_value)), NAPI_AUTO_LENGTH, &out);
  return out != nullptr ? out : Undefined(env);
}

napi_value TranscodeCallback(napi_env env, napi_callback_info info) {
  size_t argc = 3;
  napi_value argv[3] = {nullptr, nullptr, nullptr};
  if (napi_get_cb_info(env, info, &argc, argv, nullptr, nullptr) != napi_ok || argc < 3) {
    return Undefined(env);
  }

  const char* source = nullptr;
  size_t source_length = 0;
  if (!ReadByteSpan(env, argv[0], &source, &source_length)) {
    napi_throw_type_error(env,
                          "ERR_INVALID_ARG_TYPE",
                          "The \"source\" argument must be an instance of Buffer or Uint8Array.");
    return nullptr;
  }

  std::string from_encoding;
  std::string to_encoding;
  if (!ReadUtf8(env, argv[1], &from_encoding) || !ReadUtf8(env, argv[2], &to_encoding)) {
    return Undefined(env);
  }

  const BufferEncoding from = ParseBufferEncoding(from_encoding);
  const BufferEncoding to = ParseBufferEncoding(to_encoding);
  const char* from_name = BufferEncodingToIcuName(from);
  const char* to_name = BufferEncodingToIcuName(to);

  if (from_name == nullptr || to_name == nullptr) {
    napi_value status = nullptr;
    napi_create_int32(env, static_cast<int32_t>(U_ILLEGAL_ARGUMENT_ERROR), &status);
    return status != nullptr ? status : Undefined(env);
  }

  if (source_length == 0) {
    napi_value empty = nullptr;
    CreateBufferCopy(env, nullptr, 0, &empty);
    return empty != nullptr ? empty : Undefined(env);
  }

  UErrorCode status = U_ZERO_ERROR;
  UConverter* to_converter = ucnv_open(to_name, &status);
  if (U_FAILURE(status) || to_converter == nullptr) {
    napi_value status_value = nullptr;
    napi_create_int32(env, static_cast<int32_t>(status), &status_value);
    return status_value != nullptr ? status_value : Undefined(env);
  }

  status = U_ZERO_ERROR;
  UConverter* from_converter = ucnv_open(from_name, &status);
  if (U_FAILURE(status) || from_converter == nullptr) {
    if (to_converter != nullptr) ucnv_close(to_converter);
    napi_value status_value = nullptr;
    napi_create_int32(env, static_cast<int32_t>(status), &status_value);
    return status_value != nullptr ? status_value : Undefined(env);
  }

  const int32_t substitute_length = std::max<int32_t>(ucnv_getMinCharSize(to_converter), 1);
  std::string substitute(static_cast<size_t>(substitute_length), '?');
  status = U_ZERO_ERROR;
  ucnv_setSubstChars(to_converter, substitute.data(), substitute_length, &status);
  if (U_FAILURE(status)) {
    ucnv_close(from_converter);
    ucnv_close(to_converter);
    napi_value status_value = nullptr;
    napi_create_int32(env, static_cast<int32_t>(status), &status_value);
    return status_value != nullptr ? status_value : Undefined(env);
  }

  const int max_char_size = std::max<int>(static_cast<int>(ucnv_getMaxCharSize(to_converter)), 1);
  size_t capacity = std::max<size_t>(source_length * static_cast<size_t>(max_char_size), 32);
  std::vector<char> output(capacity);
  int32_t written = 0;

  for (;;) {
    status = U_ZERO_ERROR;
    const char* source_cursor = source;
    char* target = output.data();
    ucnv_convertEx(to_converter,
                   from_converter,
                   &target,
                   target + output.size(),
                   &source_cursor,
                   source + source_length,
                   nullptr,
                   nullptr,
                   nullptr,
                   nullptr,
                   true,
                   true,
                   &status);
    written = static_cast<int32_t>(target - output.data());
    if (status != U_BUFFER_OVERFLOW_ERROR) break;
    capacity = std::max<size_t>(output.size() * 2, static_cast<size_t>(written) + 16);
    output.resize(capacity);
  }

  ucnv_close(from_converter);
  ucnv_close(to_converter);

  if (U_FAILURE(status)) {
    napi_value status_value = nullptr;
    napi_create_int32(env, static_cast<int32_t>(status), &status_value);
    return status_value != nullptr ? status_value : Undefined(env);
  }

  napi_value out = nullptr;
  if (!CreateBufferCopy(env, output.data(), static_cast<size_t>(written), &out)) return Undefined(env);
  return out;
}

napi_value HasConverterCallback(napi_env env, napi_callback_info info) {
  size_t argc = 1;
  napi_value argv[1] = {nullptr};
  if (napi_get_cb_info(env, info, &argc, argv, nullptr, nullptr) != napi_ok || argc < 1) {
    return Undefined(env);
  }

  std::string label;
  if (!ReadUtf8(env, argv[0], &label)) return Undefined(env);

  UErrorCode status = U_ZERO_ERROR;
  UConverter* converter = ucnv_open(label.c_str(), &status);
  if (U_SUCCESS(status) && converter != nullptr) ucnv_close(converter);

  napi_value out = nullptr;
  napi_get_boolean(env, U_SUCCESS(status), &out);
  return out != nullptr ? out : Undefined(env);
}

napi_value GetConverterCallback(napi_env env, napi_callback_info info) {
  size_t argc = 2;
  napi_value argv[2] = {nullptr, nullptr};
  napi_value this_arg = nullptr;
  if (napi_get_cb_info(env, info, &argc, argv, &this_arg, nullptr) != napi_ok || argc < 2) {
    return Undefined(env);
  }

  std::string label;
  if (!ReadUtf8(env, argv[0], &label)) return Undefined(env);

  uint32_t flags = 0;
  if (napi_get_value_uint32(env, argv[1], &flags) != napi_ok) return Undefined(env);

  UErrorCode status = U_ZERO_ERROR;
  UConverter* converter = ucnv_open(label.c_str(), &status);
  if (U_FAILURE(status) || converter == nullptr) return Undefined(env);

  if ((flags & kConverterFlagsFatal) == kConverterFlagsFatal) {
    status = U_ZERO_ERROR;
    ucnv_setToUCallBack(
        converter, UCNV_TO_U_CALLBACK_STOP, nullptr, nullptr, nullptr, &status);
    if (U_FAILURE(status)) {
      ucnv_close(converter);
      return Undefined(env);
    }
  }

  napi_value out = nullptr;
  if (napi_create_object(env, &out) != napi_ok || out == nullptr) {
    ucnv_close(converter);
    return Undefined(env);
  }

  auto* wrap = new ConverterWrap();
  wrap->converter = converter;
  wrap->flags = flags;
  switch (ucnv_getType(converter)) {
    case UCNV_UTF8:
    case UCNV_UTF16_BigEndian:
    case UCNV_UTF16_LittleEndian:
      wrap->unicode = true;
      break;
    default:
      wrap->unicode = false;
      break;
  }

  if (!WrapConverter(env, out, wrap)) {
    ucnv_close(converter);
    delete wrap;
    return Undefined(env);
  }

  return out;
}

napi_value DecodeCallback(napi_env env, napi_callback_info info) {
  size_t argc = 4;
  napi_value argv[4] = {nullptr, nullptr, nullptr, nullptr};
  if (napi_get_cb_info(env, info, &argc, argv, nullptr, nullptr) != napi_ok || argc < 4) {
    return Undefined(env);
  }

  ConverterWrap* wrap = UnwrapConverter(env, argv[0]);
  if (wrap == nullptr || wrap->converter == nullptr) {
    napi_throw_type_error(env, "ERR_INVALID_ARG_TYPE", "The \"converter\" argument must be a converter object.");
    return nullptr;
  }

  const char* input = nullptr;
  size_t input_length = 0;
  if (!ReadByteSpan(env, argv[1], &input, &input_length)) {
    napi_throw_type_error(env,
                          "ERR_INVALID_ARG_TYPE",
                          "The \"input\" argument must be an instance of SharedArrayBuffer, ArrayBuffer or ArrayBufferView.");
    return nullptr;
  }

  uint32_t flags = 0;
  if (napi_get_value_uint32(env, argv[2], &flags) != napi_ok) return Undefined(env);

  std::string from_encoding;
  if (!ReadUtf8(env, argv[3], &from_encoding)) from_encoding = "unknown";

  UErrorCode status = U_ZERO_ERROR;
  const bool flush = (flags & kConverterFlagsFlush) == kConverterFlagsFlush;
  const size_t pending = flush ? static_cast<size_t>(ucnv_toUCountPending(wrap->converter, &status)) : input_length;
  status = U_ZERO_ERROR;
  const size_t limit = 2 * static_cast<size_t>(ucnv_getMinCharSize(wrap->converter)) *
                       (!flush ? input_length : std::max(input_length, pending));

  std::vector<UChar> output(limit > 0 ? limit : 1);
  UChar* target = output.data();
  const char* source = input;
  ucnv_toUnicode(wrap->converter,
                 &target,
                 output.data() + output.size(),
                 &source,
                 input + input_length,
                 nullptr,
                 flush,
                 &status);

  if (U_FAILURE(status)) {
    MaybeResetConverter(wrap, flush);
    std::string message = "The encoded data was not valid for encoding ";
    message += from_encoding;
    napi_throw_error(env, "ERR_ENCODING_INVALID_ENCODED_DATA", message.c_str());
    return nullptr;
  }

  size_t output_length = static_cast<size_t>(target - output.data());
  bool omit_initial_bom = false;
  if (output_length > 0 &&
      wrap->unicode &&
      (wrap->flags & kConverterFlagsIgnoreBom) == 0 &&
      !wrap->bom_seen) {
    if (output[0] == 0xFEFF) omit_initial_bom = true;
    wrap->bom_seen = true;
  }

  napi_value out = nullptr;
  const size_t start = omit_initial_bom ? 1 : 0;
  const size_t length = output_length >= start ? output_length - start : 0;
  if (!CreateStringFromUtf16(env, output.data() + start, length, &out)) {
    MaybeResetConverter(wrap, flush);
    return Undefined(env);
  }

  MaybeResetConverter(wrap, flush);
  return out;
}

napi_value GetStringWidthCallback(napi_env env, napi_callback_info info) {
  size_t argc = 3;
  napi_value argv[3] = {nullptr, nullptr, nullptr};
  if (napi_get_cb_info(env, info, &argc, argv, nullptr, nullptr) != napi_ok || argc < 1 || argv[0] == nullptr) {
    return Undefined(env);
  }

  std::vector<char16_t> value;
  if (!ReadUtf16(env, argv[0], &value)) return Undefined(env);

  bool ambiguous_as_full_width = false;
  if (argc >= 2 && argv[1] != nullptr) {
    IsTruthyBool(env, argv[1], &ambiguous_as_full_width);
  }

  bool expand_emoji_sequence = true;
  if (argc >= 3 && argv[2] != nullptr) {
    bool explicit_value = true;
    if (IsTruthyBool(env, argv[2], &explicit_value)) {
      expand_emoji_sequence = explicit_value;
    }
  }

  UChar32 current = 0;
  UChar32 previous = 0;
  size_t index = 0;
  uint32_t width = 0;
  UChar* string = reinterpret_cast<UChar*>(value.data());

  while (index < value.size()) {
    previous = current;
    U16_NEXT(string, index, value.size(), current);
    if (!expand_emoji_sequence &&
        index > 0 &&
        previous == 0x200d &&
        (u_hasBinaryProperty(current, UCHAR_EMOJI_PRESENTATION) ||
         u_hasBinaryProperty(current, UCHAR_EMOJI_MODIFIER))) {
      continue;
    }
    width += static_cast<uint32_t>(GetColumnWidth(current, ambiguous_as_full_width));
  }

  napi_value out = nullptr;
  napi_create_uint32(env, width, &out);
  return out != nullptr ? out : Undefined(env);
}

bool SetFunction(napi_env env, napi_value object, const char* name, napi_callback callback) {
  napi_value fn = nullptr;
  return napi_create_function(env, name, NAPI_AUTO_LENGTH, callback, nullptr, &fn) == napi_ok &&
         fn != nullptr &&
         napi_set_named_property(env, object, name, fn) == napi_ok;
}

}  // namespace

napi_value InitIcu(napi_env env) {
  auto* state = GetIcuState(env);
  if (state != nullptr && state->binding_ref != nullptr) {
    napi_value cached = nullptr;
    if (napi_get_reference_value(env, state->binding_ref, &cached) == napi_ok && cached != nullptr) {
      return cached;
    }
  }

  napi_value out = nullptr;
  if (napi_create_object(env, &out) != napi_ok || out == nullptr) return Undefined(env);

  if (!SetFunction(env, out, "icuErrName", ICUErrorNameCallback) ||
      !SetFunction(env, out, "transcode", TranscodeCallback) ||
      !SetFunction(env, out, "hasConverter", HasConverterCallback) ||
      !SetFunction(env, out, "getConverter", GetConverterCallback) ||
      !SetFunction(env, out, "decode", DecodeCallback) ||
      !SetFunction(env, out, "getStringWidth", GetStringWidthCallback)) {
    return Undefined(env);
  }

  auto& state_ref = EnsureIcuState(env);
  DeleteRefIfPresent(env, &state_ref.binding_ref);
  napi_create_reference(env, out, 1, &state_ref.binding_ref);
  return out;
}

}  // namespace internal_binding
