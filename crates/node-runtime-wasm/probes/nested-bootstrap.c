#include <stdint.h>

__attribute__((import_module("agentos_napi_v1"), import_name("call_js")))
int32_t agentos_napi_call_js(int32_t value);

__attribute__((export_name("start")))
int32_t agentos_node_probe_start(int32_t value) {
  return agentos_napi_call_js(value) + 1;
}

__attribute__((export_name("reenter")))
int32_t agentos_node_probe_reenter(int32_t value) {
  return value * 2;
}

__attribute__((export_name("grow_memory")))
int32_t agentos_node_probe_grow_memory(int32_t pages) {
  return __builtin_wasm_memory_grow(0, pages);
}

__attribute__((export_name("trap"), noreturn))
void agentos_node_probe_trap(void) {
  __builtin_trap();
}

