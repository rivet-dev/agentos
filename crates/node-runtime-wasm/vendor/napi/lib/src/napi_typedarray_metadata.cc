#include "napi_typedarray_metadata.h"

namespace napi {

const char* typedarray_constructor_name(napi_typedarray_type type) {
  switch (type) {
    case napi_int8_array:
      return "Int8Array";
    case napi_uint8_array:
      return "Uint8Array";
    case napi_uint8_clamped_array:
      return "Uint8ClampedArray";
    case napi_int16_array:
      return "Int16Array";
    case napi_uint16_array:
      return "Uint16Array";
    case napi_int32_array:
      return "Int32Array";
    case napi_uint32_array:
      return "Uint32Array";
    case napi_float16_array:
      return "Float16Array";
    case napi_float32_array:
      return "Float32Array";
    case napi_float64_array:
      return "Float64Array";
    case napi_bigint64_array:
      return "BigInt64Array";
    case napi_biguint64_array:
      return "BigUint64Array";
    default:
      return nullptr;
  }
}

}  // namespace napi
