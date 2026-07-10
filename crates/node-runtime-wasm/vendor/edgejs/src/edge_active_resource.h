#ifndef EDGE_ACTIVE_RESOURCE_H_
#define EDGE_ACTIVE_RESOURCE_H_

#include "node_api.h"

using EdgeActiveHandleHasRef = bool (*)(void* data);
using EdgeActiveHandleGetOwner = napi_value (*)(napi_env env, void* data);
using EdgeActiveHandleClose = void (*)(void* data);
using EdgeActiveRequestCancel = void (*)(void* data);
using EdgeActiveRequestGetOwner = napi_value (*)(napi_env env, void* data);

void* EdgeRegisterActiveHandle(napi_env env,
                              napi_value keepalive_owner,
                              const char* resource_name,
                              EdgeActiveHandleHasRef has_ref,
                              EdgeActiveHandleGetOwner get_owner,
                              void* data,
                              EdgeActiveHandleClose close_callback = nullptr);
void EdgeUnregisterActiveHandle(napi_env env, void* token);

void* EdgeRegisterActiveRequest(napi_env env,
                               napi_value req,
                               const char* resource_name,
                               void* data = nullptr,
                               EdgeActiveRequestCancel cancel = nullptr,
                               EdgeActiveRequestGetOwner get_owner = nullptr);
void EdgeUnregisterActiveRequestToken(napi_env env, void* token);
void EdgeTrackActiveRequest(napi_env env, napi_value req, const char* resource_name);
void EdgeUntrackActiveRequest(napi_env env, napi_value req);

napi_value EdgeGetActiveHandlesArray(napi_env env);
napi_value EdgeGetActiveRequestsArray(napi_env env);
napi_value EdgeGetActiveResourcesInfoArray(napi_env env);

#endif  // EDGE_ACTIVE_RESOURCE_H_
