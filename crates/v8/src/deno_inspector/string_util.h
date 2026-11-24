// Based on Node's node_string.h
#ifndef SRC_DENO_INSPECTOR_STRING_UTIL_H_
#define SRC_DENO_INSPECTOR_STRING_UTIL_H_

#include <cassert>
#include <cstring>
#include <memory>
#include <sstream>
#include <string>
#include <vector>

#include "v8-inspector.h"
#include "v8/third_party/inspector_protocol/crdtp/protocol_core.h"
#include "v8/third_party/inspector_protocol/crdtp/span.h"

// Provide DCHECK macros that the generated protocol code expects
#ifndef DCHECK
#define DCHECK(condition) assert(condition)
#endif
#ifndef DCHECK_LT
#define DCHECK_LT(a, b) assert((a) < (b))
#endif

namespace deno_inspector::protocol {
class Binary;
}

namespace v8_crdtp {

template <>
struct ProtocolTypeTraits<std::string> {
  static bool Deserialize(DeserializerState* state, std::string* value);
  static void Serialize(const std::string& value, std::vector<uint8_t>* bytes);
};

template <>
struct ProtocolTypeTraits<deno_inspector::protocol::Binary> {
  static bool Deserialize(DeserializerState* state,
                          deno_inspector::protocol::Binary* value);
  static void Serialize(const deno_inspector::protocol::Binary& value,
                        std::vector<uint8_t>* bytes);
};

}  // namespace v8_crdtp

namespace deno_inspector {
namespace protocol {

class Value;

using String = std::string;
using StringBuilder = std::ostringstream;
using ProtocolMessage = std::string;

struct StringUtil {
  static String StringViewToUtf8(v8_inspector::StringView view);
  static String fromUTF16(const uint16_t* data, size_t length);
  static String fromUTF8(const uint8_t* data, size_t length);
  static String fromUTF16LE(const uint16_t* data, size_t length);
  static const uint8_t* CharactersUTF8(const String& s);
  static size_t CharacterCount(const String& s);

  inline static uint8_t* CharactersLatin1(const String& s) { return nullptr; }
  inline static const uint16_t* CharactersUTF16(const String& s) {
    return nullptr;
  }
};

// A read-only sequence of uninterpreted bytes with reference-counted storage.
class Binary {
 public:
  Binary() : bytes_(std::make_shared<std::vector<uint8_t>>()) {}

  const uint8_t* data() const { return bytes_->data(); }
  size_t size() const { return bytes_->size(); }

  String toBase64() const;

  static Binary concat(const std::vector<Binary>& binaries);
  static Binary fromBase64(const String& base64, bool* success);
  static Binary fromSpan(const uint8_t* data, size_t size) {
    return Binary(std::make_shared<std::vector<uint8_t>>(data, data + size));
  }
  // Overload for v8_crdtp::span used by generated protocol code
  static Binary fromSpan(v8_crdtp::span<uint8_t> bytes) {
    return fromSpan(bytes.data(), bytes.size());
  }

 private:
  std::shared_ptr<std::vector<uint8_t>> bytes_;

  explicit Binary(std::shared_ptr<std::vector<uint8_t>> bytes)
      : bytes_(bytes) {}
};

}  // namespace protocol
}  // namespace deno_inspector

#endif  // SRC_DENO_INSPECTOR_STRING_UTIL_H_
