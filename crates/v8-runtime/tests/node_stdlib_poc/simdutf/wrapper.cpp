// POC wrapper exporting a flat C ABI over simdutf for the node-stdlib POC.
// Compiled to a wasm32-wasip1 reactor and instantiated inside the V8 isolate.
#include <cstdint>
#include <cstdlib>

#include "simdutf.h"

extern "C" {

uint8_t* poc_alloc(size_t len) {
  return static_cast<uint8_t*>(malloc(len));
}

void poc_free(uint8_t* ptr) {
  free(ptr);
}

int32_t poc_is_utf8(const uint8_t* buf, size_t len) {
  return simdutf::validate_utf8(reinterpret_cast<const char*>(buf), len) ? 1
                                                                         : 0;
}

int32_t poc_is_ascii(const uint8_t* buf, size_t len) {
  return simdutf::validate_ascii(reinterpret_cast<const char*>(buf), len) ? 1
                                                                          : 0;
}

// Input is UTF-16LE code units; caller guarantees well-formed UTF-16
// (the JS side falls back to its own implementation for lone surrogates,
// matching node's replacement-character semantics).
double poc_utf8_len_from_utf16le(const uint8_t* buf, size_t u16len) {
  return static_cast<double>(simdutf::utf8_length_from_utf16le(
      reinterpret_cast<const char16_t*>(buf), u16len));
}

}  // extern "C"
