#ifndef NAPI_TYPEDARRAY_METADATA_H_
#define NAPI_TYPEDARRAY_METADATA_H_

#include "js_native_api.h"

namespace napi {

const char* typedarray_constructor_name(napi_typedarray_type type);

}  // namespace napi

#endif  // NAPI_TYPEDARRAY_METADATA_H_
