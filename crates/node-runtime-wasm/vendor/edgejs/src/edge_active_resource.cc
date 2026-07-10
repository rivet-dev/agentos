#include "edge_active_resource.h"

#include "edge_environment.h"

namespace {

}  // namespace

void* EdgeRegisterActiveHandle(napi_env env,
                              napi_value keepalive_owner,
                              const char* resource_name,
                              EdgeActiveHandleHasRef has_ref,
                              EdgeActiveHandleGetOwner get_owner,
                              void* data,
                              EdgeActiveHandleClose close_callback) {
  if (auto* environment = EdgeEnvironmentGet(env); environment != nullptr) {
    return environment->RegisterActiveHandle(
        keepalive_owner, resource_name, has_ref, get_owner, data, close_callback);
  }
  return nullptr;
}

void EdgeUnregisterActiveHandle(napi_env env, void* token) {
  if (auto* environment = EdgeEnvironmentGet(env); environment != nullptr) {
    environment->UnregisterActiveHandle(token);
  }
}

void* EdgeRegisterActiveRequest(napi_env env,
                               napi_value req,
                               const char* resource_name,
                               void* data,
                               EdgeActiveRequestCancel cancel,
                               EdgeActiveRequestGetOwner get_owner) {
  if (auto* environment = EdgeEnvironmentGet(env); environment != nullptr) {
    return environment->RegisterActiveRequest(req, resource_name, data, cancel, get_owner);
  }
  return nullptr;
}

void EdgeUnregisterActiveRequestToken(napi_env env, void* token) {
  if (auto* environment = EdgeEnvironmentGet(env); environment != nullptr) {
    environment->UnregisterActiveRequest(token);
  }
}

void EdgeTrackActiveRequest(napi_env env, napi_value req, const char* resource_name) {
  (void)EdgeRegisterActiveRequest(env, req, resource_name);
}

void EdgeUntrackActiveRequest(napi_env env, napi_value req) {
  if (env == nullptr || req == nullptr) return;
  if (auto* environment = EdgeEnvironmentGet(env); environment != nullptr) {
    environment->UnregisterActiveRequestByOwner(req);
  }
}

napi_value EdgeGetActiveHandlesArray(napi_env env) {
  if (auto* environment = EdgeEnvironmentGet(env); environment != nullptr) {
    return environment->GetActiveHandlesArray();
  }
  napi_value out = nullptr;
  napi_create_array(env, &out);
  return out;
}

napi_value EdgeGetActiveRequestsArray(napi_env env) {
  if (auto* environment = EdgeEnvironmentGet(env); environment != nullptr) {
    return environment->GetActiveRequestsArray();
  }
  napi_value out = nullptr;
  napi_create_array(env, &out);
  return out;
}

napi_value EdgeGetActiveResourcesInfoArray(napi_env env) {
  if (auto* environment = EdgeEnvironmentGet(env); environment != nullptr) {
    return environment->GetActiveResourcesInfoArray();
  }
  napi_value out = nullptr;
  napi_create_array(env, &out);
  return out;
}
