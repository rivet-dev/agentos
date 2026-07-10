cmake_minimum_required(VERSION 3.20)
project(napi_engines LANGUAGES C CXX)

option(NAPI_BUILD_QUICKJS "Build the QuickJS-backed Node-API engine" OFF)
option(NAPI_BUILD_V8 "Build the V8-backed Node-API engine" ON)
option(NAPI_BUILD_BUNDLED_QUICKJS "Build the vendored QuickJS runtime for napi_quickjs" ON)

if(NAPI_BUILD_QUICKJS OR NAPI_BUILD_V8)
  enable_testing()
endif()

set(NAPI_ROOT "${CMAKE_CURRENT_SOURCE_DIR}" CACHE PATH "Path to the NAPI repository root")
if(CMAKE_SOURCE_DIR STREQUAL CMAKE_CURRENT_SOURCE_DIR)
  set(_napi_project_root_default "${CMAKE_CURRENT_SOURCE_DIR}")
else()
  set(_napi_project_root_default "${CMAKE_SOURCE_DIR}")
endif()
set(NAPI_PROJECT_ROOT "${_napi_project_root_default}" CACHE PATH
  "Project root for standalone or embedded NAPI builds")


if(NAPI_BUILD_QUICKJS)
  if(NAPI_BUILD_BUNDLED_QUICKJS AND NOT TARGET qjs)
    set(QJS_ENABLE_INSTALL OFF CACHE BOOL "" FORCE)
    set(QJS_BUILD_EXAMPLES OFF CACHE BOOL "" FORCE)
    set(QJS_BUILD_CLI_STATIC OFF CACHE BOOL "" FORCE)
    add_subdirectory(quickjs/deps/quickjs "${CMAKE_CURRENT_BINARY_DIR}/quickjs-engine" EXCLUDE_FROM_ALL)
    set(NAPI_QUICKJS_INCLUDE_DIR "${CMAKE_CURRENT_SOURCE_DIR}/quickjs/deps/quickjs" CACHE PATH
      "Path to QuickJS headers" FORCE)
  endif()
  add_subdirectory(quickjs)
endif()

if(NAPI_BUILD_V8)
  add_subdirectory(v8)
endif()
