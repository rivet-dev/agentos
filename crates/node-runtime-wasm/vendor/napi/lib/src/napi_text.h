#ifndef NAPI_TEXT_H_
#define NAPI_TEXT_H_

#include <cstddef>
#include <cstdint>
#include <vector>

namespace napi::text {

bool decimal_digits_fit(const char* value, const char* max);
std::vector<uint64_t> bigint_words_from_decimal_string(const char* decimal,
                                                       bool* negative);
std::vector<char> utf8_to_latin1(const char* str, std::size_t len);
std::size_t complete_utf8_prefix_length(const char* str, std::size_t len);

}  // namespace napi::text

#endif  // NAPI_TEXT_H_
