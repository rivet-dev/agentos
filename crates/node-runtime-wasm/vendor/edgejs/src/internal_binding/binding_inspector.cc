#include "internal_binding/binding_initializers.h"

#include "internal_binding/helpers.h"

namespace internal_binding {
namespace {

void DefineMethod(napi_env env, napi_value obj, const char* name, napi_callback cb) {
  napi_value fn = nullptr;
  if (napi_create_function(env, name, NAPI_AUTO_LENGTH, cb, nullptr, &fn) == napi_ok &&
      fn != nullptr) {
    napi_set_named_property(env, obj, name, fn);
  }
}

napi_value InspectorUnavailable(napi_env env, napi_callback_info /*info*/) {
  napi_throw_error(env,
                   "ERR_INSPECTOR_NOT_AVAILABLE",
                   "Inspector is not available in this EdgeJS build");
  return Undefined(env);
}

napi_value InspectorNotConnected(napi_env env, napi_callback_info /*info*/) {
  napi_throw_error(env, "ERR_INSPECTOR_NOT_CONNECTED", "Inspector session is not connected");
  return Undefined(env);
}

napi_value ReturnUndefined(napi_env env, napi_callback_info /*info*/) {
  return Undefined(env);
}

napi_value ReturnThis(napi_env env, napi_callback_info info) {
  napi_value receiver = nullptr;
  if (napi_get_cb_info(env, info, nullptr, nullptr, &receiver, nullptr) != napi_ok ||
      receiver == nullptr) {
    return Undefined(env);
  }
  return receiver;
}

napi_value ReturnFalse(napi_env env, napi_callback_info /*info*/) {
  napi_value out = nullptr;
  napi_get_boolean(env, false, &out);
  return out != nullptr ? out : Undefined(env);
}

napi_value ReturnZero(napi_env env, napi_callback_info /*info*/) {
  napi_value out = nullptr;
  napi_create_uint32(env, 0, &out);
  return out != nullptr ? out : Undefined(env);
}

napi_value ReturnEmptyArray(napi_env env, napi_callback_info /*info*/) {
  napi_value out = nullptr;
  napi_create_array(env, &out);
  return out != nullptr ? out : Undefined(env);
}

napi_value InspectorIsEnabled(napi_env env, napi_callback_info /*info*/) {
  napi_value out = nullptr;
  napi_get_boolean(env, false, &out);
  return out != nullptr ? out : Undefined(env);
}

napi_value InspectorUrl(napi_env env, napi_callback_info /*info*/) {
  return Undefined(env);
}

napi_value InspectorEmitProtocolEvent(napi_env env, napi_callback_info /*info*/) {
  return Undefined(env);
}

napi_value InspectorConnectionConstructor(napi_env env, napi_callback_info /*info*/) {
  napi_throw_error(env,
                   "ERR_INSPECTOR_NOT_AVAILABLE",
                   "Inspector sessions are not available in this EdgeJS build");
  return nullptr;
}

napi_value DefineClass(napi_env env,
                       const char* name,
                       napi_callback constructor,
                       const napi_property_descriptor* properties,
                       size_t property_count) {
  napi_value cls = nullptr;
  if (napi_define_class(env,
                        name,
                        NAPI_AUTO_LENGTH,
                        constructor,
                        nullptr,
                        property_count,
                        properties,
                        &cls) != napi_ok ||
      cls == nullptr) {
    return nullptr;
  }
  return cls;
}

napi_value DefineInspectorConnection(napi_env env, const char* name) {
  return DefineClass(env, name, InspectorConnectionConstructor, nullptr, 0);
}

napi_value DefineInspectorSession(napi_env env) {
  const napi_property_attributes method_attributes = napi_default_method;
  napi_property_descriptor methods[] = {
      {"connect", nullptr, InspectorUnavailable, nullptr, nullptr, nullptr, method_attributes, nullptr},
      {"connectToMainThread", nullptr, InspectorUnavailable, nullptr, nullptr, nullptr, method_attributes, nullptr},
      {"post", nullptr, InspectorNotConnected, nullptr, nullptr, nullptr, method_attributes, nullptr},
      {"disconnect", nullptr, ReturnUndefined, nullptr, nullptr, nullptr, method_attributes, nullptr},
      {"addListener", nullptr, ReturnThis, nullptr, nullptr, nullptr, method_attributes, nullptr},
      {"on", nullptr, ReturnThis, nullptr, nullptr, nullptr, method_attributes, nullptr},
      {"once", nullptr, ReturnThis, nullptr, nullptr, nullptr, method_attributes, nullptr},
      {"prependListener", nullptr, ReturnThis, nullptr, nullptr, nullptr, method_attributes, nullptr},
      {"prependOnceListener", nullptr, ReturnThis, nullptr, nullptr, nullptr, method_attributes, nullptr},
      {"removeListener", nullptr, ReturnThis, nullptr, nullptr, nullptr, method_attributes, nullptr},
      {"off", nullptr, ReturnThis, nullptr, nullptr, nullptr, method_attributes, nullptr},
      {"removeAllListeners", nullptr, ReturnThis, nullptr, nullptr, nullptr, method_attributes, nullptr},
      {"setMaxListeners", nullptr, ReturnThis, nullptr, nullptr, nullptr, method_attributes, nullptr},
      {"getMaxListeners", nullptr, ReturnZero, nullptr, nullptr, nullptr, method_attributes, nullptr},
      {"emit", nullptr, ReturnFalse, nullptr, nullptr, nullptr, method_attributes, nullptr},
      {"listenerCount", nullptr, ReturnZero, nullptr, nullptr, nullptr, method_attributes, nullptr},
      {"listeners", nullptr, ReturnEmptyArray, nullptr, nullptr, nullptr, method_attributes, nullptr},
      {"rawListeners", nullptr, ReturnEmptyArray, nullptr, nullptr, nullptr, method_attributes, nullptr},
      {"eventNames", nullptr, ReturnEmptyArray, nullptr, nullptr, nullptr, method_attributes, nullptr},
  };
  return DefineClass(env,
                     "Session",
                     [](napi_env /*env*/, napi_callback_info /*info*/) -> napi_value { return nullptr; },
                     methods,
                     sizeof(methods) / sizeof(methods[0]));
}

napi_value CreateNoopMethodObject(napi_env env, const char* const* names, size_t count) {
  napi_value out = nullptr;
  if (napi_create_object(env, &out) != napi_ok || out == nullptr) return nullptr;
  for (size_t i = 0; i < count; ++i) {
    DefineMethod(env, out, names[i], ReturnUndefined);
  }
  return out;
}

}  // namespace

napi_value InitInspector(napi_env env) {
  napi_value out = nullptr;
  if (napi_create_object(env, &out) != napi_ok || out == nullptr) return Undefined(env);

  DefineMethod(env, out, "open", InspectorUnavailable);
  DefineMethod(env, out, "close", ReturnUndefined);
  DefineMethod(env, out, "url", InspectorUrl);
  DefineMethod(env, out, "isEnabled", InspectorIsEnabled);
  DefineMethod(env, out, "waitForDebugger", InspectorUnavailable);
  DefineMethod(env, out, "emitProtocolEvent", InspectorEmitProtocolEvent);
  DefineMethod(env, out, "setConsoleExtensionInstaller", ReturnUndefined);
  DefineMethod(env, out, "registerAsyncHook", ReturnUndefined);
  DefineMethod(env, out, "setupNetworkTracking", ReturnUndefined);
  DefineMethod(env, out, "putNetworkResource", ReturnUndefined);
  DefineMethod(env, out, "consoleCall", ReturnUndefined);
  DefineMethod(env, out, "callAndPauseOnStart", ReturnUndefined);

  napi_value console = GetGlobalNamed(env, "console");
  if (console == nullptr || IsUndefined(env, console)) {
    napi_create_object(env, &console);
  }
  if (console != nullptr) {
    napi_set_named_property(env, out, "console", console);
  }

  napi_value connection = DefineInspectorConnection(env, "Connection");
  if (connection != nullptr) napi_set_named_property(env, out, "Connection", connection);

  napi_value main_thread_connection = DefineInspectorConnection(env, "MainThreadConnection");
  if (main_thread_connection != nullptr) {
    napi_set_named_property(env, out, "MainThreadConnection", main_thread_connection);
  }

  napi_value session = DefineInspectorSession(env);
  if (session != nullptr) napi_set_named_property(env, out, "Session", session);

  const char* network_methods[] = {
      "requestWillBeSent",
      "responseReceived",
      "loadingFinished",
      "loadingFailed",
      "dataSent",
      "dataReceived",
      "webSocketCreated",
      "webSocketClosed",
      "webSocketHandshakeResponseReceived",
  };
  napi_value network =
      CreateNoopMethodObject(env, network_methods, sizeof(network_methods) / sizeof(network_methods[0]));
  if (network != nullptr) napi_set_named_property(env, out, "Network", network);

  const char* network_resource_methods[] = {"put"};
  napi_value network_resources = CreateNoopMethodObject(env, network_resource_methods, 1);
  if (network_resources != nullptr) {
    napi_set_named_property(env, out, "NetworkResources", network_resources);
  }

  return out;
}

}  // namespace internal_binding
