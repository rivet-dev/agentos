#include "napi_text.h"

#include <cstring>

namespace napi::text {

bool decimal_digits_fit(const char* value, const char* max) {
  while (*value == '0' && value[1] != '\0') {
    ++value;
  }
  std::size_t value_len = std::strlen(value);
  std::size_t max_len = std::strlen(max);
  if (value_len != max_len) {
    return value_len < max_len;
  }
  return std::strcmp(value, max) <= 0;
}

std::vector<uint64_t> bigint_words_from_decimal_string(const char* decimal,
                                                       bool* negative) {
  std::vector<uint64_t> words;
  const char* cursor = decimal;
  *negative = cursor[0] == '-';
  if (*negative) {
    ++cursor;
  }

  for (; *cursor != '\0'; ++cursor) {
    if (*cursor < '0' || *cursor > '9') {
      continue;
    }

    unsigned carry = static_cast<unsigned>(*cursor - '0');
    for (std::size_t i = 0; i < words.size(); ++i) {
      unsigned __int128 next =
          static_cast<unsigned __int128>(words[i]) * 10 + carry;
      words[i] = static_cast<uint64_t>(next);
      carry = static_cast<unsigned>(next >> 64);
    }
    if (carry != 0 || words.empty()) {
      words.push_back(carry);
    }
  }

  while (words.size() > 1 && words.back() == 0) {
    words.pop_back();
  }
  return words;
}

std::vector<char> utf8_to_latin1(const char* str, std::size_t len) {
  std::vector<char> out;
  for (std::size_t i = 0; i < len;) {
    unsigned char c = static_cast<unsigned char>(str[i]);
    uint32_t cp = c;
    std::size_t advance = 1;
    if ((c & 0xe0) == 0xc0 && i + 1 < len) {
      cp = ((c & 0x1f) << 6) |
           (static_cast<unsigned char>(str[i + 1]) & 0x3f);
      advance = 2;
    } else if ((c & 0xf0) == 0xe0 && i + 2 < len) {
      cp = ((c & 0x0f) << 12) |
           ((static_cast<unsigned char>(str[i + 1]) & 0x3f) << 6) |
           (static_cast<unsigned char>(str[i + 2]) & 0x3f);
      advance = 3;
    } else if ((c & 0xf8) == 0xf0 && i + 3 < len) {
      cp = '?';
      advance = 4;
    }
    out.push_back(static_cast<char>(cp <= 0xff ? cp : '?'));
    i += advance;
  }
  return out;
}

std::size_t complete_utf8_prefix_length(const char* str, std::size_t len) {
  std::size_t i = 0;
  while (i < len) {
    unsigned char c = static_cast<unsigned char>(str[i]);
    std::size_t width = 1;
    if ((c & 0x80) == 0) {
      width = 1;
    } else if ((c & 0xe0) == 0xc0) {
      width = 2;
    } else if ((c & 0xf0) == 0xe0) {
      width = 3;
    } else if ((c & 0xf8) == 0xf0) {
      width = 4;
    } else {
      break;
    }
    if (i + width > len) {
      break;
    }
    i += width;
  }
  return i;
}

}  // namespace napi::text
