include(GoogleTest)

set(NAPI_V8_GTEST_ROOT "${PROJECT_ROOT}/deps/googletest")
set(EDGE_GTEST_PER_TEST_TIMEOUT_SECONDS 60)
set(EDGE_CTEST_RESOURCE_LOCK_BUCKETS "24" CACHE STRING
    "Max concurrent edge gtest cases when ctest parallelism is unbounded (0 disables).")
set(EDGE_CTEST_RESOURCE_LOCK_PREFIX "edge_gtest_slot" CACHE STRING
    "Prefix used for generated CTest RESOURCE_LOCK buckets.")

if(TARGET napi_v8 AND NAPI_V8_LIBRARY)
  add_library(edge_gtest STATIC
    "${NAPI_V8_GTEST_ROOT}/src/gtest-all.cc"
    "${NAPI_V8_GTEST_ROOT}/src/gtest_main.cc"
  )

  target_include_directories(edge_gtest
    PRIVATE
      "${NAPI_V8_GTEST_ROOT}"
      "${NAPI_V8_GTEST_ROOT}/include"
      "${EDGE_ROOT}/tests/runners"
      "${EDGE_ROOT}/src"
      "${NAPI_INCLUDE_ROOT}"
  )

  function(add_edge_node_api_test target_name)
    add_executable(${target_name} ${ARGN})
    target_include_directories(${target_name}
      PRIVATE
        "${NAPI_V8_GTEST_ROOT}"
        "${NAPI_V8_GTEST_ROOT}/include"
        "${EDGE_ROOT}/tests/runners"
        "${EDGE_ROOT}/src"
        "${NAPI_INCLUDE_ROOT}"
    )
    target_link_libraries(${target_name}
      PRIVATE
        edge_gtest
        napi_v8
        edge_node_api
        edge_runtime
        edge_cli
    )
    target_compile_definitions(${target_name}
      PRIVATE
        PROJECT_ROOT_PATH="${PROJECT_ROOT}"
        NAPI_V8_ROOT_PATH="${EDGE_ROOT}"
        EDGE_VERSION_COMMIT="${EDGE_VERSION_COMMIT}"
        EDGE_VERSION_SUFFIX="${EDGE_VERSION_SUFFIX}"
    )
    if(NAPI_V8_DEFINES)
      target_compile_definitions(${target_name} PRIVATE ${NAPI_V8_DEFINES})
    endif()
    if(ENABLE_TRACING)
      target_compile_definitions(${target_name} PRIVATE ENABLE_TRACING=1)
    endif()
    # Keep a stable path to the generated CTest manifest for this target.
    set(_gtest_tests_manifest "${CMAKE_CURRENT_BINARY_DIR}/${target_name}[1]_tests.cmake")
    gtest_discover_tests(
      ${target_name}
      DISCOVERY_TIMEOUT 30
      TEST_PREFIX "edge.${target_name}."
      PROPERTIES
        TIMEOUT ${EDGE_GTEST_PER_TEST_TIMEOUT_SECONDS}
    )
    # CMake 3.31 can emit duplicated add_test entries for very large gtest lists.
    # Deduplicate generated manifest entries so each gtest runs once in ctest.
    add_custom_command(
      TARGET ${target_name}
      POST_BUILD
      COMMAND /usr/bin/env python3
              ${EDGE_ROOT}/scripts/dedupe_gtest_manifest.py
              ${_gtest_tests_manifest}
              --resource-lock-buckets ${EDGE_CTEST_RESOURCE_LOCK_BUCKETS}
              --resource-lock-prefix ${EDGE_CTEST_RESOURCE_LOCK_PREFIX}
      VERBATIM
    )
  endfunction()

  add_edge_node_api_test(edge_test_0_runtime_phase01
    "${EDGE_ROOT}/tests/runners/test_0_runtime_phase01.cc"
  )
  add_edge_node_api_test(edge_test_1_cli_phase01
    "${EDGE_ROOT}/tests/runners/test_1_cli_phase01.cc"
  )
  add_edge_node_api_test(edge_test_2_module_loader_phase02
    "${EDGE_ROOT}/tests/runners/test_2_module_loader_phase02.cc"
  )
  add_edge_node_api_test(edge_test_3_node_dropin_subset_phase02
    "${EDGE_ROOT}/tests/runners/test_3_node_dropin_subset_phase02.cc"
  )
  add_edge_node_api_test(edge_test_4_internal_binding_dispatch_phase02
    "${EDGE_ROOT}/tests/runners/test_4_internal_binding_dispatch_phase02.cc"
  )
  add_edge_node_api_test(edge_test_5_internal_binding_parity_phase03
    "${EDGE_ROOT}/tests/runners/test_5_internal_binding_parity_phase03.cc"
  )
  add_edge_node_api_test(edge_test_6_binding_registry_phase04
    "${EDGE_ROOT}/tests/runners/test_6_binding_registry_phase04.cc"
  )
  add_edge_node_api_test(edge_test_7_bytecode_cache_phase05
    "${EDGE_ROOT}/tests/runners/test_7_bytecode_cache_phase05.cc"
  )
  add_edge_node_api_test(edge_test_8_builtin_bytecode
    "${EDGE_ROOT}/tests/runners/test_8_builtin_bytecode.cc"
  )
else()
  message(STATUS "Skipping edge node-api tests: napi_v8 runtime target/linkage is unavailable")
endif()

add_test(
  NAME edge.guard_internal_binding_dispatch
  COMMAND /usr/bin/env python3
          "${EDGE_ROOT}/scripts/check_internal_binding_dispatch.py"
          "${EDGE_ROOT}/src/edge_module_loader.cc"
)
