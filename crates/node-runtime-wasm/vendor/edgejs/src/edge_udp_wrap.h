#ifndef EDGE_UDP_WRAP_H_
#define EDGE_UDP_WRAP_H_

#include <uv.h>

#include "node_api.h"
#include "edge_udp_listener.h"

napi_value EdgeInstallUdpWrapBinding(napi_env env);
napi_value EdgeGetUdpWrapConstructor(napi_env env);
uv_handle_t* EdgeUdpWrapGetHandle(napi_env env, napi_value value);
EdgeUdpSendWrap* EdgeUdpWrapUnwrapSendWrap(napi_env env, napi_value value);

#endif  // EDGE_UDP_WRAP_H_
