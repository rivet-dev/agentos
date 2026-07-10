include("${CMAKE_CURRENT_LIST_DIR}/binding_registry/CMakeLists.txt")
include("${CMAKE_CURRENT_LIST_DIR}/bindings/CMakeLists.txt")
include("${CMAKE_CURRENT_LIST_DIR}/crypto/CMakeLists.txt")

set(EDGE_RUNTIME_CORE_SOURCES
  "${CMAKE_CURRENT_LIST_DIR}/edge_environment_runtime.cc"
  "${CMAKE_CURRENT_LIST_DIR}/edge_icu_data.cc"
  "${CMAKE_CURRENT_LIST_DIR}/edge_intl.cc"
  "${CMAKE_CURRENT_LIST_DIR}/edge_runtime.cc"
  "${CMAKE_CURRENT_LIST_DIR}/edge_runtime_platform_v8.cc"
  "${CMAKE_CURRENT_LIST_DIR}/edge_node_compat.cc"
  "${CMAKE_CURRENT_LIST_DIR}/edge_process.cc"
  "${CMAKE_CURRENT_LIST_DIR}/edge_active_resource.cc"
  "${CMAKE_CURRENT_LIST_DIR}/edge_worker_env.cc"
)

set(EDGE_LOADER_SOURCES
  "${CMAKE_CURRENT_LIST_DIR}/builtin_catalog.cc"
  "${CMAKE_CURRENT_LIST_DIR}/edge_builtin_bytecode.cc"
  "${CMAKE_CURRENT_LIST_DIR}/edge_bytecode_cache.cc"
  "${CMAKE_CURRENT_LIST_DIR}/edge_module_loader.cc"
  "${CMAKE_CURRENT_LIST_DIR}/edge_precompile.cc"
)

set(EDGE_LLHTTP_SOURCES
  "${PROJECT_ROOT}/deps/llhttp/src/api.c"
  "${PROJECT_ROOT}/deps/llhttp/src/http.c"
  "${PROJECT_ROOT}/deps/llhttp/src/llhttp.c"
)

set(EDGE_RUNTIME_SOURCES
  ${EDGE_RUNTIME_CORE_SOURCES}
  ${EDGE_LOADER_SOURCES}
  ${EDGE_BINDING_REGISTRY_SOURCES}
  ${EDGE_INTERNAL_BINDING_SOURCES}
  ${EDGE_NATIVE_BINDING_SOURCES}
  ${EDGE_CRYPTO_SOURCES}
  ${EDGE_LLHTTP_SOURCES}
)
