// String utilities for Deno inspector protocol

#include "src/deno_inspector/string_util.h"

#include "v8/third_party/inspector_protocol/crdtp/json.h"

namespace v8_crdtp {

bool ProtocolTypeTraits<std::string>::Deserialize(DeserializerState* state,
                                                  std::string* value) {
  if (state->tokenizer()->TokenTag() == cbor::CBORTokenTag::STRING8) {
    span<uint8_t> cbor_span = state->tokenizer()->GetString8();
    value->assign(reinterpret_cast<const char*>(cbor_span.data()),
                  cbor_span.size());
    return true;
  }
  if (state->tokenizer()->TokenTag() == cbor::CBORTokenTag::STRING16) {
    span<uint8_t> utf16le = state->tokenizer()->GetString16WireRep();
    *value = deno_inspector::protocol::StringUtil::fromUTF16LE(
        reinterpret_cast<const uint16_t*>(utf16le.data()),
        utf16le.size() / sizeof(uint16_t));
    return true;
  }
  return false;
}

void ProtocolTypeTraits<std::string>::Serialize(const std::string& value,
                                                std::vector<uint8_t>* bytes) {
  cbor::EncodeString8(SpanFrom(value), bytes);
}

bool ProtocolTypeTraits<deno_inspector::protocol::Binary>::Deserialize(
    DeserializerState* state, deno_inspector::protocol::Binary* value) {
  if (state->tokenizer()->TokenTag() != cbor::CBORTokenTag::BINARY) {
    return false;
  }
  span<uint8_t> cbor_span = state->tokenizer()->GetBinary();
  *value = deno_inspector::protocol::Binary::fromSpan(cbor_span.data(),
                                                      cbor_span.size());
  return true;
}

void ProtocolTypeTraits<deno_inspector::protocol::Binary>::Serialize(
    const deno_inspector::protocol::Binary& value,
    std::vector<uint8_t>* bytes) {
  cbor::EncodeString8(SpanFrom(value.toBase64()), bytes);
}

}  // namespace v8_crdtp

namespace deno_inspector {
namespace protocol {

String StringUtil::StringViewToUtf8(v8_inspector::StringView view) {
  if (view.length() == 0) return "";
  if (view.is8Bit()) {
    return std::string(reinterpret_cast<const char*>(view.characters8()),
                       view.length());
  }
  return fromUTF16(view.characters16(), view.length());
}

String StringUtil::fromUTF16(const uint16_t* data, size_t length) {
  // Simple UTF-16 to UTF-8 conversion
  std::string result;
  result.reserve(length * 3);  // Worst case
  for (size_t i = 0; i < length; i++) {
    uint32_t code_point = data[i];
    // Handle surrogate pairs
    if (code_point >= 0xD800 && code_point <= 0xDBFF && i + 1 < length) {
      uint32_t low = data[i + 1];
      if (low >= 0xDC00 && low <= 0xDFFF) {
        code_point = 0x10000 + ((code_point - 0xD800) << 10) + (low - 0xDC00);
        i++;
      }
    }
    // Encode to UTF-8
    if (code_point < 0x80) {
      result.push_back(static_cast<char>(code_point));
    } else if (code_point < 0x800) {
      result.push_back(static_cast<char>(0xC0 | (code_point >> 6)));
      result.push_back(static_cast<char>(0x80 | (code_point & 0x3F)));
    } else if (code_point < 0x10000) {
      result.push_back(static_cast<char>(0xE0 | (code_point >> 12)));
      result.push_back(static_cast<char>(0x80 | ((code_point >> 6) & 0x3F)));
      result.push_back(static_cast<char>(0x80 | (code_point & 0x3F)));
    } else {
      result.push_back(static_cast<char>(0xF0 | (code_point >> 18)));
      result.push_back(static_cast<char>(0x80 | ((code_point >> 12) & 0x3F)));
      result.push_back(static_cast<char>(0x80 | ((code_point >> 6) & 0x3F)));
      result.push_back(static_cast<char>(0x80 | (code_point & 0x3F)));
    }
  }
  return result;
}

String StringUtil::fromUTF8(const uint8_t* data, size_t length) {
  return std::string(reinterpret_cast<const char*>(data), length);
}

String StringUtil::fromUTF16LE(const uint16_t* data, size_t length) {
  return fromUTF16(data, length);  // Assuming host is little-endian
}

const uint8_t* StringUtil::CharactersUTF8(const String& s) {
  return reinterpret_cast<const uint8_t*>(s.data());
}

size_t StringUtil::CharacterCount(const String& s) { return s.length(); }

// Base64 encoding table
static const char kBase64Chars[] =
    "ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";

String Binary::toBase64() const {
  std::string result;
  size_t len = bytes_->size();
  const uint8_t* data = bytes_->data();
  result.reserve(((len + 2) / 3) * 4);

  for (size_t i = 0; i < len; i += 3) {
    uint32_t n = static_cast<uint32_t>(data[i]) << 16;
    if (i + 1 < len) n |= static_cast<uint32_t>(data[i + 1]) << 8;
    if (i + 2 < len) n |= static_cast<uint32_t>(data[i + 2]);

    result.push_back(kBase64Chars[(n >> 18) & 0x3F]);
    result.push_back(kBase64Chars[(n >> 12) & 0x3F]);
    result.push_back(i + 1 < len ? kBase64Chars[(n >> 6) & 0x3F] : '=');
    result.push_back(i + 2 < len ? kBase64Chars[n & 0x3F] : '=');
  }
  return result;
}

Binary Binary::concat(const std::vector<Binary>& binaries) {
  size_t total_size = 0;
  for (const auto& binary : binaries) {
    total_size += binary.size();
  }
  auto bytes = std::make_shared<std::vector<uint8_t>>(total_size);
  uint8_t* data_ptr = bytes->data();
  for (const auto& binary : binaries) {
    memcpy(data_ptr, binary.data(), binary.size());
    data_ptr += binary.size();
  }
  return Binary(bytes);
}

// Base64 decoding table
static const uint8_t kBase64DecodeTable[256] = {
    255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255,
    255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255,
    255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 62,  255,
    255, 255, 63,  52,  53,  54,  55,  56,  57,  58,  59,  60,  61,  255, 255,
    255, 255, 255, 255, 255, 0,   1,   2,   3,   4,   5,   6,   7,   8,   9,
    10,  11,  12,  13,  14,  15,  16,  17,  18,  19,  20,  21,  22,  23,  24,
    25,  255, 255, 255, 255, 255, 255, 26,  27,  28,  29,  30,  31,  32,  33,
    34,  35,  36,  37,  38,  39,  40,  41,  42,  43,  44,  45,  46,  47,  48,
    49,  50,  51,  255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255,
    255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255,
    255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255,
    255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255,
    255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255,
    255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255,
    255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255,
    255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255,
    255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255,
    255,
};

Binary Binary::fromBase64(const String& base64, bool* success) {
  *success = true;
  auto bytes = std::make_shared<std::vector<uint8_t>>();
  bytes->reserve((base64.size() * 3) / 4);

  uint32_t buffer = 0;
  int bits_collected = 0;

  for (char c : base64) {
    if (c == '=') break;
    uint8_t val = kBase64DecodeTable[static_cast<uint8_t>(c)];
    if (val == 255) {
      *success = false;
      return Binary();
    }
    buffer = (buffer << 6) | val;
    bits_collected += 6;
    if (bits_collected >= 8) {
      bits_collected -= 8;
      bytes->push_back(static_cast<uint8_t>((buffer >> bits_collected) & 0xFF));
    }
  }

  return Binary(bytes);
}

}  // namespace protocol
}  // namespace deno_inspector
