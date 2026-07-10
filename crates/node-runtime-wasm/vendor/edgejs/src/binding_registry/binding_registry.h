#ifndef EDGE_BINDING_REGISTRY_BINDING_REGISTRY_H_
#define EDGE_BINDING_REGISTRY_BINDING_REGISTRY_H_

#include <string_view>
#include <vector>

#include "node_api.h"

namespace edge::binding_registry {

using BindingInit = napi_value (*)(napi_env env);

bool Has(std::string_view name);
std::vector<std::string_view> Names();

napi_value Get(napi_env env, std::string_view name);

void FinalizeEnv(napi_env env);

}  // namespace edge::binding_registry

#endif  // EDGE_BINDING_REGISTRY_BINDING_REGISTRY_H_
