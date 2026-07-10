#ifndef NAPI_V8_MODULE_WRAP_RECORD_H_
#define NAPI_V8_MODULE_WRAP_RECORD_H_

#include <cstdint>
#include <string>
#include <unordered_map>
#include <vector>

#include <v8.h>

#include "js_native_api.h"

namespace v8impl::detail {

struct ModuleImportAttributeRecord {
  std::string key;
  std::string value;
};

struct ModuleRequestRecord {
  std::string specifier;
  std::vector<ModuleImportAttributeRecord> attributes;
  int32_t phase = 2;
};

struct ModuleWrapRecord {
  napi_env env = nullptr;
  napi_ref wrapper_ref = nullptr;
  napi_ref synthetic_eval_steps_ref = nullptr;
  napi_ref source_object_ref = nullptr;
  napi_ref host_defined_option_ref = nullptr;
  v8::Global<v8::Context> context;
  v8::Global<v8::Module> module;
  std::vector<ModuleRequestRecord> module_requests;
  std::unordered_map<std::string, uint32_t> resolve_cache;
  std::vector<ModuleWrapRecord*> linked_requests;
};

}  // namespace v8impl::detail

#endif  // NAPI_V8_MODULE_WRAP_RECORD_H_
