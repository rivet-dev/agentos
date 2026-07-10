cmake_minimum_required(VERSION 3.20)
project(edge LANGUAGES C CXX)

set(CMAKE_CXX_STANDARD 20)
set(CMAKE_CXX_STANDARD_REQUIRED ON)
set(CMAKE_CXX_EXTENSIONS OFF)

set(PROJECT_ROOT "${CMAKE_CURRENT_SOURCE_DIR}")
set(EDGE_ROOT "${PROJECT_ROOT}")
set(NAPI_V8_ROOT "${PROJECT_ROOT}/napi/v8")
set(NAPI_INCLUDE_ROOT "${PROJECT_ROOT}/napi/include")
find_package(Python3 REQUIRED COMPONENTS Interpreter)
find_package(Git QUIET)
include("${PROJECT_ROOT}/cmake/EdgeBuiltinCatalog.cmake")
include("${PROJECT_ROOT}/cmake/EdgeICU.cmake")
include("${PROJECT_ROOT}/cmake/EdgeNapiProvider.cmake")
include("${PROJECT_ROOT}/cmake/EdgeOpenSSL.cmake")
include("${PROJECT_ROOT}/cmake/EdgeOptions.cmake")
include("${PROJECT_ROOT}/cmake/EdgeVendoredDeps.cmake")
include("${PROJECT_ROOT}/cmake/EdgeWasmer.cmake")

set(EDGE_VERSION_COMMIT "unknown")
if(GIT_FOUND)
  execute_process(
    COMMAND "${GIT_EXECUTABLE}" rev-parse --short=7 HEAD
    WORKING_DIRECTORY "${PROJECT_ROOT}"
    OUTPUT_VARIABLE EDGE_VERSION_COMMIT
    OUTPUT_STRIP_TRAILING_WHITESPACE
    ERROR_QUIET
  )
endif()
if(EDGE_VERSION_COMMIT STREQUAL "")
  set(EDGE_VERSION_COMMIT "unknown")
endif()
set(EDGE_VERSION_SUFFIX "")
if(NOT EDGE_VERSION_COMMIT STREQUAL "unknown")
  set(EDGE_VERSION_SUFFIX "-${EDGE_VERSION_COMMIT}")
endif()
if(EXISTS "${PROJECT_ROOT}/napi/v8/cmake/NapiBuildCache.cmake")
  include("${PROJECT_ROOT}/napi/v8/cmake/NapiBuildCache.cmake")
  napi_enable_sccache_if_available()
endif()

edge_configure_options()
edge_configure_napi_provider()
edge_configure_quickjs_webassembly()

# Hard gate: edge runtime/library sources must never directly reference V8.
file(GLOB_RECURSE EDGE_PURE_SRC_FILES
  "${EDGE_ROOT}/src/*.h"
  "${EDGE_ROOT}/src/*.cc"
  "${EDGE_ROOT}/src/*.c"
)
foreach(EDGE_CODE_FILE IN LISTS EDGE_PURE_SRC_FILES)
  file(READ "${EDGE_CODE_FILE}" EDGE_CODE_CONTENTS)
  if(EDGE_CODE_CONTENTS MATCHES "#include <v8\\.h>" OR
     EDGE_CODE_CONTENTS MATCHES "#include <libplatform/libplatform\\.h>" OR
     EDGE_CODE_CONTENTS MATCHES "v8::")
    message(FATAL_ERROR
      "V8 usage is forbidden by policy in src. "
      "Found in: ${EDGE_CODE_FILE}")
  endif()
endforeach()

if(EDGE_NAPI_PROVIDER STREQUAL "bundled-v8")
  if(EDGE_PREFER_REPO_LOCAL_V8)
    set(_edge_repo_local_v8_include "${EDGE_REPO_LOCAL_V8_DIST_ROOT}/include")
    set(_edge_repo_local_v8_library "${EDGE_REPO_LOCAL_V8_DIST_ROOT}/lib/libv8.a")
    if(EXISTS "${_edge_repo_local_v8_include}" AND EXISTS "${_edge_repo_local_v8_library}")
      message(STATUS "EDGE: preferring repo-local V8 bundle at ${EDGE_REPO_LOCAL_V8_DIST_ROOT}")
      set(NAPI_V8_BUILD_METHOD "local" CACHE STRING
        "V8 resolution method: prebuilt|source|local" FORCE)
      set(NAPI_V8_INCLUDE_DIR "${_edge_repo_local_v8_include}" CACHE PATH
        "Path to V8 headers" FORCE)
      set(NAPI_V8_LIBRARY "${_edge_repo_local_v8_library}" CACHE FILEPATH
        "Path to V8 runtime library (monolith/static/dynamic)" FORCE)
      set(NAPI_V8_DEFINES "V8_COMPRESS_POINTERS" CACHE STRING
        "Semicolon-separated V8 embedder compile definitions" FORCE)
      if(APPLE)
        set(NAPI_V8_EXTRA_LIBS "/System/Library/Frameworks/CoreFoundation.framework" CACHE STRING
          "Semicolon-separated extra V8 libraries (e.g. libplatform/libbase)" FORCE)
      endif()
    endif()
  endif()
  set(NAPI_V8_BUILD_TESTS "${EDGE_BUILD_NAPI_TESTS}" CACHE BOOL "" FORCE)
  add_subdirectory("${NAPI_V8_ROOT}" "${CMAKE_CURRENT_BINARY_DIR}/napi-v8")
elseif(EDGE_NAPI_PROVIDER STREQUAL "imports")
  add_library(edge_napi_headers INTERFACE)
  target_include_directories(edge_napi_headers INTERFACE "${NAPI_INCLUDE_ROOT}")
elseif(EDGE_NAPI_PROVIDER STREQUAL "quickjs")
  set(EDGE_QUICKJS_ROOT "${PROJECT_ROOT}/napi/quickjs/deps/quickjs")
  set(QJS_ENABLE_INSTALL OFF CACHE BOOL "" FORCE)
  set(QJS_BUILD_EXAMPLES OFF CACHE BOOL "" FORCE)
  set(QJS_BUILD_CLI_STATIC OFF CACHE BOOL "" FORCE)
  add_subdirectory("${EDGE_QUICKJS_ROOT}" "${CMAKE_CURRENT_BINARY_DIR}/quickjs" EXCLUDE_FROM_ALL)
  set(NAPI_QUICKJS_INCLUDE_DIR "${EDGE_QUICKJS_ROOT}" CACHE PATH
    "Path to QuickJS headers" FORCE)
  set(NAPI_QUICKJS_BUILD_TESTS "${EDGE_BUILD_NAPI_TESTS}" CACHE BOOL "" FORCE)
  add_subdirectory("${PROJECT_ROOT}/napi/quickjs" "${CMAKE_CURRENT_BINARY_DIR}/napi-quickjs")
endif()

edge_add_vendored_deps()
edge_configure_openssl()

set(EDGE_EMBEDDED_V8_VERSION "0.0.0-node.0")
if(DEFINED NAPI_V8_INCLUDE_DIR AND EXISTS "${NAPI_V8_INCLUDE_DIR}/v8-version.h")
  file(STRINGS "${NAPI_V8_INCLUDE_DIR}/v8-version.h" _v8_major_line
       REGEX "^#define V8_MAJOR_VERSION [0-9]+$")
  file(STRINGS "${NAPI_V8_INCLUDE_DIR}/v8-version.h" _v8_minor_line
       REGEX "^#define V8_MINOR_VERSION [0-9]+$")
  file(STRINGS "${NAPI_V8_INCLUDE_DIR}/v8-version.h" _v8_build_line
       REGEX "^#define V8_BUILD_NUMBER [0-9]+$")
  file(STRINGS "${NAPI_V8_INCLUDE_DIR}/v8-version.h" _v8_patch_line
       REGEX "^#define V8_PATCH_LEVEL [0-9]+$")
  if(_v8_major_line AND _v8_minor_line AND _v8_build_line AND _v8_patch_line)
    string(REGEX REPLACE "^#define V8_MAJOR_VERSION ([0-9]+)$" "\\1"
           _v8_major "${_v8_major_line}")
    string(REGEX REPLACE "^#define V8_MINOR_VERSION ([0-9]+)$" "\\1"
           _v8_minor "${_v8_minor_line}")
    string(REGEX REPLACE "^#define V8_BUILD_NUMBER ([0-9]+)$" "\\1"
           _v8_build "${_v8_build_line}")
    string(REGEX REPLACE "^#define V8_PATCH_LEVEL ([0-9]+)$" "\\1"
           _v8_patch "${_v8_patch_line}")
    set(EDGE_EMBEDDED_V8_VERSION
      "${_v8_major}.${_v8_minor}.${_v8_build}.${_v8_patch}-node.0")
  endif()
endif()

set(EDGE_EMBEDDED_QUICKJS_VERSION "0.0.0")
if(EDGE_NAPI_PROVIDER STREQUAL "quickjs" AND EXISTS "${EDGE_QUICKJS_ROOT}/quickjs.h")
  file(STRINGS "${EDGE_QUICKJS_ROOT}/quickjs.h" _qjs_major_line
       REGEX "^#define QJS_VERSION_MAJOR [0-9]+$")
  file(STRINGS "${EDGE_QUICKJS_ROOT}/quickjs.h" _qjs_minor_line
       REGEX "^#define QJS_VERSION_MINOR [0-9]+$")
  file(STRINGS "${EDGE_QUICKJS_ROOT}/quickjs.h" _qjs_patch_line
       REGEX "^#define QJS_VERSION_PATCH [0-9]+$")
  if(_qjs_major_line AND _qjs_minor_line AND _qjs_patch_line)
    string(REGEX REPLACE "^#define QJS_VERSION_MAJOR ([0-9]+)$" "\\1"
           _qjs_major "${_qjs_major_line}")
    string(REGEX REPLACE "^#define QJS_VERSION_MINOR ([0-9]+)$" "\\1"
           _qjs_minor "${_qjs_minor_line}")
    string(REGEX REPLACE "^#define QJS_VERSION_PATCH ([0-9]+)$" "\\1"
           _qjs_patch "${_qjs_patch_line}")
    set(EDGE_EMBEDDED_QUICKJS_VERSION "${_qjs_major}.${_qjs_minor}.${_qjs_patch}")
  endif()
endif()

add_library(edge_node_api STATIC
  src/node_api.cc
  src/edge_napi_embedder_hooks.cc
  src/unofficial_napi_testing_until_gc.cc
)

add_library(edge_environment_core STATIC
  src/edge_environment.cc
)
target_include_directories(edge_environment_core
  PUBLIC
    "${EDGE_ROOT}/src"
  PRIVATE
    "${NAPI_INCLUDE_ROOT}"
)
target_link_libraries(edge_environment_core PUBLIC uv_a)
if(EDGE_NAPI_PROVIDER STREQUAL "bundled-v8")
  target_compile_definitions(edge_environment_core PUBLIC EDGE_BUNDLED_NAPI_V8=1 EDGE_EMBEDDED_NAPI_PROVIDER=1)
elseif(EDGE_NAPI_PROVIDER STREQUAL "quickjs")
  target_compile_definitions(edge_environment_core PUBLIC EDGE_EMBEDDED_NAPI_PROVIDER=1 NAPI_EXTERN=)
endif()

edge_configure_icu()
edge_add_runtime_support_libraries()

target_include_directories(edge_node_api
  PRIVATE
    "${NAPI_INCLUDE_ROOT}"
)

target_link_libraries(edge_node_api
  PUBLIC
    edge_environment_core
  PRIVATE
    uv_a
)
if(EDGE_NAPI_PROVIDER STREQUAL "bundled-v8")
  target_compile_definitions(napi_v8 PUBLIC EDGE_BUNDLED_NAPI_V8=1 EDGE_EMBEDDED_NAPI_PROVIDER=1)
  target_link_libraries(edge_node_api PUBLIC napi_v8)
elseif(EDGE_NAPI_PROVIDER STREQUAL "quickjs")
  target_link_libraries(edge_node_api PUBLIC napi_quickjs)
endif()

include("${PROJECT_ROOT}/src/CMakeLists.txt")
set(EDGE_NODE_SHARED_OPENSSL 0)
if(NOT EDGE_IS_WASIX_TARGET AND EDGE_SHARED_OPENSSL)
  set(EDGE_NODE_SHARED_OPENSSL 1)
endif()

function(edge_apply_runtime_compile_surface target_name)
  target_include_directories("${target_name}"
    PUBLIC
      "${EDGE_ROOT}/src"
    PRIVATE
      "${NAPI_INCLUDE_ROOT}"
      "${CMAKE_CURRENT_BINARY_DIR}"
      "${PROJECT_ROOT}/deps"
      "${PROJECT_ROOT}/deps/llhttp/include"
      "${PROJECT_ROOT}/deps/ncrypto"
      ${EDGE_ICU_INCLUDE_DIRS}
  )
  target_link_libraries("${target_name}"
    PUBLIC
      edge_environment_core
      edge_node_api
      ${EDGE_ICU_LINK_LIBS}
    PRIVATE
      edge_simdjson
      edge_simdutf
      edge_ada
      edge_ncrypto
      edge_brotli
      edge_nghttp2
      edge_zstd
      uv_a
      c-ares::cares
      OpenSSL::Crypto
      zlibstatic
  )
  target_compile_definitions("${target_name}"
    PRIVATE
      EDGE_HAS_ICU=1
      EDGE_HAS_SMALL_ICU=1
      EDGE_ENABLE_TRACE_DIAGNOSTICS=$<IF:$<CONFIG:Debug>,1,0>
      EDGE_NODE_SHARED_OPENSSL=${EDGE_NODE_SHARED_OPENSSL}
      EDGE_EMBEDDED_V8_VERSION="${EDGE_EMBEDDED_V8_VERSION}"
      EDGE_EMBEDDED_QUICKJS_VERSION="${EDGE_EMBEDDED_QUICKJS_VERSION}"
      EDGE_VERSION_COMMIT="${EDGE_VERSION_COMMIT}"
      EDGE_VERSION_SUFFIX="${EDGE_VERSION_SUFFIX}"
  )
  if(ENABLE_TRACING)
    target_compile_definitions("${target_name}" PRIVATE ENABLE_TRACING=1)
  endif()
  if(EDGE_NAPI_PROVIDER STREQUAL "bundled-v8")
    target_compile_definitions("${target_name}" PRIVATE EDGE_BUNDLED_NAPI_V8=1 EDGE_EMBEDDED_NAPI_PROVIDER=1)
  elseif(EDGE_NAPI_PROVIDER STREQUAL "quickjs")
    target_compile_definitions("${target_name}" PRIVATE EDGE_EMBEDDED_NAPI_PROVIDER=1 EDGE_NAPI_QUICKJS=1)
  endif()
endfunction()

add_library(edge_binding_registry OBJECT EXCLUDE_FROM_ALL
  ${EDGE_BINDING_REGISTRY_SOURCES}
)
edge_apply_runtime_compile_surface(edge_binding_registry)

add_library(edge_loader OBJECT EXCLUDE_FROM_ALL
  ${EDGE_LOADER_SOURCES}
)
edge_apply_runtime_compile_surface(edge_loader)
edge_add_builtin_catalog(edge_loader)

add_library(edge_runtime_core OBJECT EXCLUDE_FROM_ALL
  ${EDGE_RUNTIME_CORE_SOURCES}
)
edge_apply_runtime_compile_surface(edge_runtime_core)
if(EDGE_QUICKJS_WEBASSEMBLY_ENABLED)
  target_compile_definitions(edge_runtime_core PRIVATE EDGE_QUICKJS_WEBASSEMBLY=1)
endif()

add_library(edge_bindings OBJECT EXCLUDE_FROM_ALL
  ${EDGE_INTERNAL_BINDING_SOURCES}
  ${EDGE_NATIVE_BINDING_SOURCES}
)
edge_apply_runtime_compile_surface(edge_bindings)

add_library(edge_crypto OBJECT EXCLUDE_FROM_ALL
  ${EDGE_CRYPTO_SOURCES}
)
edge_apply_runtime_compile_surface(edge_crypto)

add_library(edge_runtime STATIC
  $<TARGET_OBJECTS:edge_runtime_core>
  $<TARGET_OBJECTS:edge_loader>
  $<TARGET_OBJECTS:edge_binding_registry>
  $<TARGET_OBJECTS:edge_bindings>
  $<TARGET_OBJECTS:edge_crypto>
  ${EDGE_LLHTTP_SOURCES}
)
edge_add_builtin_catalog(edge_runtime)
edge_apply_runtime_compile_surface(edge_runtime)
if(EDGE_QUICKJS_WEBASSEMBLY_ENABLED)
  target_sources(edge_runtime PRIVATE
    src/webassembly/edge_wasm.cc
  )
  target_link_libraries(edge_runtime PRIVATE edge_quickjs_wasmer_c_api)
  target_compile_definitions(edge_runtime PRIVATE
    EDGE_QUICKJS_WEBASSEMBLY=1
    $<$<BOOL:${EDGE_IS_WASIX_TARGET}>:EDGE_QUICKJS_WEBASSEMBLY_IMPORTS=1>
  )
endif()
if(EDGE_NAPI_PROVIDER STREQUAL "bundled-v8")
  target_link_libraries(edge_runtime PUBLIC napi_v8)
elseif(EDGE_NAPI_PROVIDER STREQUAL "quickjs")
  target_link_libraries(edge_runtime PUBLIC napi_quickjs)
endif()
if(EDGE_IS_WASIX_TARGET)
  target_sources(edge_runtime PRIVATE
    wasix/src/wasix_compat.cc
  )
  target_compile_definitions(edge_runtime PRIVATE NODE_OPENSSL_CERT_STORE=1)
endif()

add_library(edge_cli STATIC
  src/edge_cli.cc
  src/edge_compat_exec.cc
)
target_include_directories(edge_cli
  PUBLIC
    "${EDGE_ROOT}/src"
  PRIVATE
    "${NAPI_INCLUDE_ROOT}"
)
target_link_libraries(edge_cli
  PUBLIC
    edge_runtime
  PRIVATE
    uv_a
)
target_compile_definitions(edge_cli
  PRIVATE
    EDGE_VERSION_COMMIT="${EDGE_VERSION_COMMIT}"
    EDGE_VERSION_SUFFIX="${EDGE_VERSION_SUFFIX}"
)
if(ENABLE_TRACING)
  target_compile_definitions(edge_cli PRIVATE ENABLE_TRACING=1)
endif()
if(NOT EDGE_DEFAULT_WASMER_PACKAGE STREQUAL "")
  target_compile_definitions(edge_cli
    PRIVATE
      EDGE_DEFAULT_WASMER_PACKAGE="${EDGE_DEFAULT_WASMER_PACKAGE}"
  )
endif()

option(EDGE_BUILD_CLI "Build edge CLI binary" ON)
if(EDGE_BUILD_CLI)
  set(EDGE_CLI_SOURCES
    "${EDGE_ROOT}/src/edge_main.cc"
  )
  add_executable(edge
    ${EDGE_CLI_SOURCES}
  )
  set_target_properties(edge PROPERTIES ENABLE_EXPORTS ON)
  target_include_directories(edge
    PRIVATE
      "${EDGE_ROOT}/src"
      "${NAPI_INCLUDE_ROOT}"
  )
  target_link_libraries(edge
    PRIVATE
      edge_cli
  )
  if(EDGE_NAPI_PROVIDER STREQUAL "bundled-v8" AND NAPI_V8_DEFINES)
    target_compile_definitions(edge PRIVATE ${NAPI_V8_DEFINES})
  endif()
  if(EDGE_ALLOW_UNDEFINED_IMPORTS)
    target_link_options(edge PRIVATE
      "-Wl,--allow-undefined"
      "-Wl,--import-undefined"
    )
  endif()

  if(NOT EDGE_IS_WASIX_TARGET)
    add_executable(edgeenv
      "${EDGE_ROOT}/src/edgeenv_main.cc"
    )
    target_include_directories(edgeenv
      PRIVATE
        "${EDGE_ROOT}/src"
    )
  endif()
endif()

if(NOT EDGE_IS_WASIX_TARGET)
  enable_testing()
  add_subdirectory(tests)
endif()
