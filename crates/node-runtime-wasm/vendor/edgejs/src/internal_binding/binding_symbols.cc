#include "internal_binding/binding_initializers.h"

#include "internal_binding/helpers.h"
#include "edge_module_loader.h"
#include "edge_util.h"

namespace internal_binding {

namespace {

napi_value GetPerIsolateSymbolSource(napi_env env) {
  napi_value source = EdgeGetPerIsolateSymbols(env);
  if (source != nullptr && !IsUndefined(env, source)) return source;

  source = EdgeCreatePerIsolateSymbolsObject(env);
  if (source != nullptr && !IsUndefined(env, source)) {
    EdgeSetPerIsolateSymbols(env, source);
  }
  return source;
}

}  // namespace

napi_value InitSymbols(napi_env env) {
  const napi_value undefined = Undefined(env);
  napi_value source = GetPerIsolateSymbolSource(env);
  if (source == nullptr || IsUndefined(env, source)) return undefined;

  napi_value out = nullptr;
  if (napi_create_object(env, &out) != napi_ok || out == nullptr) return undefined;

  auto set_symbol = [&](const char* target_key, const char* source_key) {
    napi_value sym = nullptr;
    if (napi_get_named_property(env, source, source_key, &sym) == napi_ok && sym != nullptr) {
      napi_set_named_property(env, out, target_key, sym);
    }
  };

  set_symbol("vm_dynamic_import_default_internal", "vm_dynamic_import_default_internal");
  set_symbol("vm_dynamic_import_main_context_default", "vm_dynamic_import_main_context_default");
  set_symbol("vm_dynamic_import_no_callback", "vm_dynamic_import_no_callback");
  set_symbol("vm_dynamic_import_missing_flag", "vm_dynamic_import_missing_flag");
  set_symbol("vm_context_no_contextify", "vm_context_no_contextify");
  set_symbol("source_text_module_default_hdo", "source_text_module_default_hdo");
  set_symbol("constructor_key_symbol", "constructor_key_symbol");
  set_symbol("fs_use_promises_symbol", "fs_use_promises_symbol");
  set_symbol("handle_onclose", "handle_onclose_symbol");
  set_symbol("resource_symbol", "resource_symbol");
  set_symbol("owner_symbol", "owner_symbol");
  set_symbol("async_id_symbol", "async_id_symbol");
  set_symbol("trigger_async_id_symbol", "trigger_async_id_symbol");
  set_symbol("oninit", "oninit_symbol");
  set_symbol("onpskexchange", "onpskexchange_symbol");
  set_symbol("messaging_deserialize_symbol", "messaging_deserialize_symbol");
  set_symbol("messaging_transfer_symbol", "messaging_transfer_symbol");
  set_symbol("messaging_clone_symbol", "messaging_clone_symbol");
  set_symbol("messaging_transfer_list_symbol", "messaging_transfer_list_symbol");
  set_symbol("no_message_symbol", "no_message_symbol");
  set_symbol("imported_cjs_symbol", "imported_cjs_symbol");

  return out;
}

}  // namespace internal_binding
