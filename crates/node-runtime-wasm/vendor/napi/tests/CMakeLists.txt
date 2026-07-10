include(GoogleTest)
include(FetchContent)

set(NAPI_TEST_ENGINE "${NAPI_TEST_ENGINE}" CACHE STRING "N-API engine to test: quickjs or v8")
set_property(CACHE NAPI_TEST_ENGINE PROPERTY STRINGS quickjs v8)

if(NOT NAPI_TEST_ENGINE)
  if(TARGET napi_quickjs AND NOT TARGET napi_v8)
    set(NAPI_TEST_ENGINE quickjs)
  elseif(TARGET napi_v8 AND NOT TARGET napi_quickjs)
    set(NAPI_TEST_ENGINE v8)
  else()
    message(FATAL_ERROR
      "Set NAPI_TEST_ENGINE to quickjs or v8 before adding shared N-API tests")
  endif()
endif()

string(TOLOWER "${NAPI_TEST_ENGINE}" _napi_test_engine)
if(NOT _napi_test_engine MATCHES "^(quickjs|v8)$")
  message(FATAL_ERROR "Unsupported NAPI_TEST_ENGINE: ${NAPI_TEST_ENGINE}")
endif()

set(_napi_test_prefix "napi_${_napi_test_engine}")
set(_napi_test_engine_target "${_napi_test_prefix}")
string(TOUPPER "${_napi_test_engine}" _napi_test_engine_upper)

if(NOT TARGET "${_napi_test_engine_target}")
  message(FATAL_ERROR
    "Shared N-API tests for ${_napi_test_engine} require target ${_napi_test_engine_target}")
endif()

if(NOT NAPI_ROOT)
  get_filename_component(NAPI_ROOT "${CMAKE_CURRENT_SOURCE_DIR}/.." ABSOLUTE)
endif()
if(NOT NAPI_TESTS_ROOT)
  set(NAPI_TESTS_ROOT "${CMAKE_CURRENT_SOURCE_DIR}")
endif()
if(NOT NAPI_INCLUDE_ROOT)
  set(NAPI_INCLUDE_ROOT "${NAPI_ROOT}/include")
endif()

set(_napi_gtest_root_default "${PROJECT_ROOT}/deps/googletest")
set(_napi_gtest_root_var "NAPI_${_napi_test_engine_upper}_GTEST_ROOT")
if(DEFINED ${_napi_gtest_root_var})
  set(_napi_gtest_root "${${_napi_gtest_root_var}}")
else()
  set(_napi_gtest_root "${_napi_gtest_root_default}")
endif()
if(NOT EXISTS "${_napi_gtest_root}/src/gtest-all.cc" AND
   EXISTS "${_napi_gtest_root_default}/src/gtest-all.cc")
  set(_napi_gtest_root "${_napi_gtest_root_default}")
endif()
if(NOT EXISTS "${_napi_gtest_root}/src/gtest-all.cc")
  message(STATUS
    "googletest not found at ${_napi_gtest_root}; fetching googletest for ${_napi_test_prefix} shared tests")
  FetchContent_Declare(
    napi_googletest
    URL https://github.com/google/googletest/archive/refs/tags/v1.14.0.tar.gz
    URL_HASH SHA256=8ad598c73ad796e0d8280b082cebd82a630d73e73cd3c70057938a6501bba5d7
    DOWNLOAD_EXTRACT_TIMESTAMP TRUE
  )
  FetchContent_MakeAvailable(napi_googletest)
  FetchContent_GetProperties(napi_googletest SOURCE_DIR _napi_gtest_root)
  if(EXISTS "${_napi_gtest_root}/googletest/src/gtest-all.cc")
    set(_napi_gtest_root "${_napi_gtest_root}/googletest")
  endif()
endif()

set(_napi_engine_include_var "NAPI_${_napi_test_engine_upper}_INCLUDE_DIR")
set(_napi_engine_defines_var "NAPI_${_napi_test_engine_upper}_DEFINES")
set(_napi_engine_extra_libs_var "NAPI_${_napi_test_engine_upper}_EXTRA_LIBS")
set(_napi_engine_library_var "NAPI_${_napi_test_engine_upper}_LIBRARY")

add_library(${_napi_test_prefix}_gtest STATIC
  "${_napi_gtest_root}/src/gtest-all.cc"
  "${_napi_gtest_root}/src/gtest_main.cc"
)

target_include_directories(${_napi_test_prefix}_gtest
  PRIVATE
    "${_napi_gtest_root}"
    "${_napi_gtest_root}/include"
    "${NAPI_TESTS_ROOT}/js-native-api"
    "${NAPI_TESTS_ROOT}/node-api"
    "${CMAKE_CURRENT_SOURCE_DIR}/runners"
    "${NAPI_ROOT}/lib/src"
    "${NAPI_INCLUDE_ROOT}"
    "${${_napi_engine_include_var}}"
)

function(add_fixture_test target_name)
  foreach(_src IN LISTS ARGN)
    if(_src MATCHES "^runners/")
      set(_runner_path "${CMAKE_CURRENT_SOURCE_DIR}/${_src}")
      if(NOT EXISTS "${_runner_path}")
        message(STATUS
          "Skipping ${target_name}: ${_src} is not tracked as shared N-API coverage")
        return()
      endif()
    endif()
  endforeach()

  add_executable(${target_name} ${ARGN})
  target_include_directories(${target_name}
    PRIVATE
      "${_napi_gtest_root}"
      "${_napi_gtest_root}/include"
      "${NAPI_TESTS_ROOT}/js-native-api"
      "${NAPI_TESTS_ROOT}/node-api"
      "${CMAKE_CURRENT_SOURCE_DIR}/runners"
      "${NAPI_ROOT}/lib/src"
      "${NAPI_INCLUDE_ROOT}"
      "${${_napi_engine_include_var}}"
  )
  target_link_libraries(${target_name}
    PRIVATE
      ${_napi_test_prefix}_gtest
      ${_napi_test_engine_target}
  )
  if(_napi_test_engine STREQUAL "quickjs")
    if(TARGET qjs)
      target_link_libraries(${target_name} PRIVATE qjs)
    elseif(${_napi_engine_library_var})
      target_link_libraries(${target_name} PRIVATE "${${_napi_engine_library_var}}")
    endif()
  elseif(_napi_test_engine STREQUAL "v8" AND ${_napi_engine_library_var})
    target_link_libraries(${target_name} PRIVATE "${${_napi_engine_library_var}}")
  endif()
  if(TARGET edge_environment_core)
    target_link_libraries(${target_name} PRIVATE edge_environment_core)
  endif()
  target_compile_definitions(${target_name}
    PRIVATE
      NAPI_TESTS_ROOT_PATH="${NAPI_TESTS_ROOT}"
      NAPI_TEST_ENGINE_${_napi_test_engine_upper}=1
  )
  target_compile_options(${target_name}
    PRIVATE
      "-DNAPI_MODULE(modname,regfunc)="
      "-DNODE_GYP_MODULE_NAME=napi_test_module"
  )
  if(${_napi_engine_defines_var})
    target_compile_definitions(${target_name} PRIVATE ${${_napi_engine_defines_var}})
  endif()
  if(${_napi_engine_extra_libs_var})
    target_link_libraries(${target_name} PRIVATE ${${_napi_engine_extra_libs_var}})
  endif()
  if(${_napi_engine_library_var})
    get_filename_component(_napi_engine_lib_dir "${${_napi_engine_library_var}}" DIRECTORY)
    set_target_properties(${target_name} PROPERTIES
      BUILD_RPATH "${_napi_engine_lib_dir}"
      INSTALL_RPATH "${_napi_engine_lib_dir}"
    )
  endif()
  gtest_discover_tests(
    ${target_name}
    DISCOVERY_TIMEOUT 30
    TEST_PREFIX "${_napi_test_prefix}.${target_name}."
  )
endfunction()

add_fixture_test(${_napi_test_prefix}_test_2
  runners/test_2_function_arguments.cc
  ${NAPI_TESTS_ROOT}/js-native-api/2_function_arguments/2_function_arguments.c
)
add_fixture_test(${_napi_test_prefix}_test_3
  runners/test_3_callbacks.cc
  ${NAPI_TESTS_ROOT}/js-native-api/3_callbacks/3_callbacks.c
)
add_fixture_test(${_napi_test_prefix}_test_4
  runners/test_4_object_factory.cc
  ${NAPI_TESTS_ROOT}/js-native-api/4_object_factory/4_object_factory.c
)
add_fixture_test(${_napi_test_prefix}_test_5
  runners/test_5_function_factory.cc
  ${NAPI_TESTS_ROOT}/js-native-api/5_function_factory/5_function_factory.c
)
add_fixture_test(${_napi_test_prefix}_test_6
  runners/test_6_object_wrap.cc
  ${NAPI_TESTS_ROOT}/js-native-api/6_object_wrap/myobject.cc
)
add_fixture_test(${_napi_test_prefix}_test_7
  runners/test_7_factory_wrap.cc
  ${NAPI_TESTS_ROOT}/js-native-api/7_factory_wrap/7_factory_wrap.cc
  ${NAPI_TESTS_ROOT}/js-native-api/7_factory_wrap/myobject.cc
)
add_fixture_test(${_napi_test_prefix}_test_8
  runners/test_8_passing_wrapped.cc
  ${NAPI_TESTS_ROOT}/js-native-api/8_passing_wrapped/8_passing_wrapped.cc
  ${NAPI_TESTS_ROOT}/js-native-api/8_passing_wrapped/myobject.cc
)
add_fixture_test(${_napi_test_prefix}_test_9_array
  runners/test_9_array.cc
  ${NAPI_TESTS_ROOT}/js-native-api/test_array/test_array.c
)
add_fixture_test(${_napi_test_prefix}_test_10_number
  runners/test_10_number.cc
  ${NAPI_TESTS_ROOT}/js-native-api/test_number/test_number.c
  ${NAPI_TESTS_ROOT}/js-native-api/test_number/test_null.c
)
add_fixture_test(${_napi_test_prefix}_test_11_constructor
  runners/test_11_constructor.cc
  ${NAPI_TESTS_ROOT}/js-native-api/test_constructor/test_constructor.c
  ${NAPI_TESTS_ROOT}/js-native-api/test_constructor/test_null.c
)
add_fixture_test(${_napi_test_prefix}_test_12_error
  runners/test_12_error.cc
  ${NAPI_TESTS_ROOT}/js-native-api/test_error/test_error.c
)
add_fixture_test(${_napi_test_prefix}_test_13_symbol
  runners/test_13_symbol.cc
  ${NAPI_TESTS_ROOT}/js-native-api/test_symbol/test_symbol.c
)
add_fixture_test(${_napi_test_prefix}_test_14_exception
  runners/test_14_exception.cc
  ${NAPI_TESTS_ROOT}/js-native-api/test_exception/test_exception.c
)
add_fixture_test(${_napi_test_prefix}_test_15_function
  runners/test_15_function.cc
  ${NAPI_TESTS_ROOT}/js-native-api/test_function/test_function.c
)
add_fixture_test(${_napi_test_prefix}_test_16_reference
  runners/test_16_reference.cc
  ${NAPI_TESTS_ROOT}/js-native-api/test_reference/test_reference.c
)
add_fixture_test(${_napi_test_prefix}_test_17_string
  runners/test_17_string.cc
  ${NAPI_TESTS_ROOT}/js-native-api/test_string/test_string.c
  ${NAPI_TESTS_ROOT}/js-native-api/test_string/test_null.c
)
target_compile_definitions(${_napi_test_prefix}_test_17_string PRIVATE NAPI_VERSION=10)
add_fixture_test(${_napi_test_prefix}_test_18_conversions
  runners/test_18_conversions.cc
  ${NAPI_TESTS_ROOT}/js-native-api/test_conversions/test_conversions.c
  ${NAPI_TESTS_ROOT}/js-native-api/test_conversions/test_null.c
)
add_fixture_test(${_napi_test_prefix}_test_19_properties
  runners/test_19_properties.cc
  ${NAPI_TESTS_ROOT}/js-native-api/test_properties/test_properties.c
)
add_fixture_test(${_napi_test_prefix}_test_21_general
  runners/test_21_general.cc
  runners/addon_21_general.cc
)
target_compile_definitions(${_napi_test_prefix}_test_21_general PRIVATE NAPI_EXPERIMENTAL)
add_fixture_test(${_napi_test_prefix}_test_25_object_exceptions
  runners/test_25_object_exceptions.cc
  ${NAPI_TESTS_ROOT}/js-native-api/test_object/test_exceptions.c
)
add_fixture_test(${_napi_test_prefix}_test_26_object_null
  runners/test_26_object_null.cc
  ${NAPI_TESTS_ROOT}/js-native-api/test_object/test_null.c
)
add_fixture_test(${_napi_test_prefix}_test_27_object
  runners/test_27_object.cc
  ${NAPI_TESTS_ROOT}/js-native-api/test_object/test_object.c
  ${NAPI_TESTS_ROOT}/js-native-api/test_object/test_null.c
)
target_compile_definitions(${_napi_test_prefix}_test_27_object PRIVATE NAPI_EXPERIMENTAL)
add_fixture_test(${_napi_test_prefix}_test_29_bigint
  runners/test_29_bigint.cc
  ${NAPI_TESTS_ROOT}/js-native-api/test_bigint/test_bigint.c
)
add_fixture_test(${_napi_test_prefix}_test_30_date
  runners/test_30_date.cc
  ${NAPI_TESTS_ROOT}/js-native-api/test_date/test_date.c
)
add_fixture_test(${_napi_test_prefix}_test_31_dataview
  runners/test_31_dataview.cc
  ${NAPI_TESTS_ROOT}/js-native-api/test_dataview/test_dataview.c
)
target_compile_definitions(${_napi_test_prefix}_test_31_dataview PRIVATE NAPI_EXPERIMENTAL NODE_API_EXPERIMENTAL_NO_WARNING)
add_fixture_test(${_napi_test_prefix}_test_32_sharedarraybuffer
  runners/test_32_sharedarraybuffer.cc
  ${NAPI_TESTS_ROOT}/js-native-api/test_sharedarraybuffer/test_sharedarraybuffer.c
)
target_compile_definitions(${_napi_test_prefix}_test_32_sharedarraybuffer PRIVATE NAPI_EXPERIMENTAL NODE_API_EXPERIMENTAL_NO_WARNING)
add_fixture_test(${_napi_test_prefix}_test_33_typedarray
  runners/test_33_typedarray.cc
  ${NAPI_TESTS_ROOT}/js-native-api/test_typedarray/test_typedarray.c
)
add_fixture_test(${_napi_test_prefix}_test_34_new_target
  runners/test_34_new_target.cc
  ${NAPI_TESTS_ROOT}/js-native-api/test_new_target/test_new_target.c
)
add_fixture_test(${_napi_test_prefix}_test_35_promise
  runners/test_35_promise.cc
  ${NAPI_TESTS_ROOT}/js-native-api/test_promise/test_promise.c
)
add_fixture_test(${_napi_test_prefix}_test_36_handle_scope
  runners/test_36_handle_scope.cc
  ${NAPI_TESTS_ROOT}/js-native-api/test_handle_scope/test_handle_scope.c
)
add_fixture_test(${_napi_test_prefix}_test_37_reference_double_free
  runners/test_37_reference_double_free.cc
  ${NAPI_TESTS_ROOT}/js-native-api/test_reference_double_free/test_reference_double_free.c
)
add_fixture_test(${_napi_test_prefix}_test_38_finalizer
  runners/test_38_finalizer.cc
  runners/addon_38_finalizer.cc
)
target_compile_definitions(${_napi_test_prefix}_test_38_finalizer PRIVATE NAPI_EXPERIMENTAL NODE_API_EXPERIMENTAL_NO_WARNING)
add_fixture_test(${_napi_test_prefix}_test_39_cannot_run_js
  runners/test_39_cannot_run_js.cc
  ${NAPI_TESTS_ROOT}/js-native-api/test_cannot_run_js/test_cannot_run_js.c
)
target_compile_definitions(${_napi_test_prefix}_test_39_cannot_run_js PRIVATE NAPI_VERSION=10)
add_fixture_test(${_napi_test_prefix}_test_40_pending_exception
  runners/test_40_pending_exception.cc
  ${NAPI_TESTS_ROOT}/js-native-api/test_cannot_run_js/test_cannot_run_js.c
)
target_compile_definitions(${_napi_test_prefix}_test_40_pending_exception PRIVATE NAPI_VERSION=9)
add_fixture_test(${_napi_test_prefix}_test_41_instance_data
  runners/test_41_instance_data.cc
  ${NAPI_TESTS_ROOT}/js-native-api/test_instance_data/test_instance_data.c
)
add_fixture_test(${_napi_test_prefix}_test_65_unofficial_contextify
  runners/test_65_unofficial_contextify.cc
)

add_fixture_test(${_napi_test_prefix}_test_allocator
  src/napi_allocator_test.cc
)
