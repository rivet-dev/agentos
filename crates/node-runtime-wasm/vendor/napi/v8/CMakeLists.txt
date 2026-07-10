cmake_minimum_required(VERSION 3.20)
project(napi_v8 LANGUAGES C CXX)

set(CMAKE_CXX_STANDARD 20)
set(CMAKE_CXX_STANDARD_REQUIRED ON)
set(CMAKE_CXX_EXTENSIONS OFF)

include("${CMAKE_CURRENT_SOURCE_DIR}/../lib/cmake/NapiLifetimeOptions.cmake")

xoption(NAPI_V8_BUILD_TESTS "Build napi-v8 tier1 tests" ON)
napi_define_lifetime_options("V8")

set(NAPI_V8_ROOT "${CMAKE_CURRENT_SOURCE_DIR}")
include("${NAPI_V8_ROOT}/cmake/NapiBuildCache.cmake")
napi_enable_sccache_if_available()
include("${NAPI_V8_ROOT}/cmake/NapiEngine.cmake")
include("${NAPI_V8_ROOT}/cmake/NapiV8Resolver.cmake")
napi_engine_set_layout("${NAPI_V8_ROOT}")
napi_v8_resolve_configuration()

file(GLOB_RECURSE NAPI_V8_SRC_FILES
  "${NAPI_V8_ROOT}/src/*.h"
  "${NAPI_V8_ROOT}/src/*.cc"
  "${NAPI_V8_ROOT}/src/*.c"
)
foreach(NAPI_V8_SRC_FILE IN LISTS NAPI_V8_SRC_FILES)
  file(READ "${NAPI_V8_SRC_FILE}" NAPI_V8_SRC_CONTENTS)
  if(NAPI_V8_SRC_CONTENTS MATCHES "#include <uv\\.h>" OR
     NAPI_V8_SRC_CONTENTS MATCHES "#include \"uv\\.h\"")
    message(FATAL_ERROR
      "libuv usage is forbidden in napi/v8/src. "
      "Found in: ${NAPI_V8_SRC_FILE}")
  endif()
endforeach()

add_library(napi_v8 STATIC
  ../lib/src/napi_error_state.cc
  ../lib/src/napi_lifetime_tracker.cc
  ../lib/src/napi_periodic_gate.cc
  ../lib/src/napi_text.cc
  ../lib/src/napi_typedarray_metadata.cc
  src/js_native_api_v8.cc
  src/internal/napi_escapable_handle_scope_wrapper.cc
  src/internal/napi_external_wrapper.cc
  src/internal/napi_function_callback_info.cc
  src/internal/napi_getter_callback_info.cc
  src/internal/napi_handle_scope_wrapper.cc
  src/internal/napi_lifetime_tracker.cc
  src/internal/napi_ref.cc
  src/internal/napi_ref_tracker.cc
  src/internal/napi_ref_with_data.cc
  src/internal/napi_ref_with_finalizer.cc
  src/internal/napi_setter_callback_info.cc
  src/unofficial_napi.cc
  src/unofficial_napi_error_utils.cc
  src/unofficial_napi_contextify.cc
  src/edge_v8_platform.cc
)

target_include_directories(napi_v8
  PUBLIC
    "${NAPI_INCLUDE_ROOT}"
    "${NAPI_ROOT}/lib/src"
  PRIVATE
    "${NAPI_V8_ROOT}/src"
    "${NAPI_V8_INCLUDE_DIR}"
)

target_compile_definitions(napi_v8
  PUBLIC
    NAPI_EXTERN=
    EDGE_BUNDLED_NAPI_V8=1
  PRIVATE
    $<$<BOOL:${NAPI_ENABLE_LIFETIME_TRACKER}>:NAPI_ENABLE_LIFETIME_TRACKER=1>
    $<$<BOOL:${NAPI_ENABLE_LIFETIME_PERIODIC_STATS}>:NAPI_ENABLE_LIFETIME_PERIODIC_STATS=1>
    $<$<BOOL:${NAPI_ENABLE_LIFETIME_TAG_STATS}>:NAPI_ENABLE_LIFETIME_TAG_STATS=1>
    $<$<BOOL:${NAPI_ENABLE_LIFETIME_STRING_SYMBOL_DUMP}>:NAPI_ENABLE_LIFETIME_STRING_SYMBOL_DUMP=1>
)
if(CMAKE_CXX_COMPILER_ID MATCHES "Clang|GNU")
  target_compile_options(napi_v8 PRIVATE -fno-rtti)
endif()
if(NAPI_V8_DEFINES)
  target_compile_definitions(napi_v8 PRIVATE ${NAPI_V8_DEFINES})
endif()
if(NAPI_V8_LIBRARY)
  target_link_libraries(napi_v8 PUBLIC "${NAPI_V8_LIBRARY}")
endif()
if(NAPI_V8_EXTRA_LIBS)
  target_link_libraries(napi_v8 PUBLIC ${NAPI_V8_EXTRA_LIBS})
endif()
if(NAPI_V8_BUILD_TESTS)
  enable_testing()
  set(NAPI_TEST_ENGINE v8)
  add_subdirectory("${NAPI_TESTS_ROOT}" "${CMAKE_CURRENT_BINARY_DIR}/tests")
endif()
