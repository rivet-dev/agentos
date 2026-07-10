function(edge_quickjs_wasmer_asset_name out_var)
  # The Wasmer C API distribution is a build-host artifact: WASIX targets only
  # consume its headers, so pick the asset by host, not by CMAKE_SYSTEM_NAME
  # (which is WASI when cross-compiling with the WASIX toolchain).
  string(TOLOWER "${CMAKE_HOST_SYSTEM_NAME}" _edge_wasmer_system)
  string(TOLOWER "${CMAKE_HOST_SYSTEM_PROCESSOR}" _edge_wasmer_processor)

  if(_edge_wasmer_system MATCHES "darwin")
    set(_edge_wasmer_os "darwin")
  elseif(_edge_wasmer_system MATCHES "linux")
    set(_edge_wasmer_os "linux")
  elseif(_edge_wasmer_system MATCHES "windows")
    set(_edge_wasmer_os "windows")
  else()
    message(FATAL_ERROR "Unsupported Wasmer C API host OS: ${CMAKE_HOST_SYSTEM_NAME}")
  endif()

  if(_edge_wasmer_processor MATCHES "^(arm64|aarch64)$")
    set(_edge_wasmer_arch "arm64")
    if(_edge_wasmer_os STREQUAL "linux")
      set(_edge_wasmer_arch "aarch64")
    endif()
  elseif(_edge_wasmer_processor MATCHES "^(x86_64|amd64)$")
    set(_edge_wasmer_arch "amd64")
  elseif(_edge_wasmer_processor MATCHES "^riscv64$")
    set(_edge_wasmer_arch "riscv64")
  else()
    message(FATAL_ERROR "Unsupported Wasmer C API host architecture: ${CMAKE_HOST_SYSTEM_PROCESSOR}")
  endif()

  set(${out_var} "wasmer-${_edge_wasmer_os}-${_edge_wasmer_arch}.tar.gz" PARENT_SCOPE)
endfunction()

function(edge_quickjs_wasmer_find_dist_root candidate out_var)
  set(_edge_wasmer_found "")
  if(candidate AND
     EXISTS "${candidate}/include/wasm.h" AND
     EXISTS "${candidate}/include/wasmer.h" AND
     EXISTS "${candidate}/lib/libwasmer.a")
    set(_edge_wasmer_found "${candidate}")
  endif()
  set(${out_var} "${_edge_wasmer_found}" PARENT_SCOPE)
endfunction()

function(edge_quickjs_wasmer_download version asset_name out_var)
  set(_edge_wasmer_url
    "https://github.com/wasmerio/wasmer/releases/download/${version}/${asset_name}")
  set(_edge_wasmer_archive
    "${CMAKE_BINARY_DIR}/_deps/quickjs-wasmer/${version}/${asset_name}")
  set(_edge_wasmer_extract_dir
    "${CMAKE_BINARY_DIR}/_deps/quickjs-wasmer/${version}/extract")

  if(NOT EXISTS "${_edge_wasmer_archive}")
    file(MAKE_DIRECTORY "${CMAKE_BINARY_DIR}/_deps/quickjs-wasmer/${version}")
    message(STATUS "EDGE: downloading Wasmer C API ${version} from ${_edge_wasmer_url}")
    file(DOWNLOAD
      "${_edge_wasmer_url}"
      "${_edge_wasmer_archive}"
      SHOW_PROGRESS
      STATUS _edge_wasmer_download_status
    )
    list(GET _edge_wasmer_download_status 0 _edge_wasmer_download_code)
    list(GET _edge_wasmer_download_status 1 _edge_wasmer_download_message)
    if(NOT _edge_wasmer_download_code EQUAL 0)
      message(FATAL_ERROR
        "Failed to download Wasmer C API asset: ${_edge_wasmer_download_message}")
    endif()
  endif()

  if(NOT EXISTS "${_edge_wasmer_extract_dir}")
    file(MAKE_DIRECTORY "${_edge_wasmer_extract_dir}")
    execute_process(
      COMMAND "${CMAKE_COMMAND}" -E tar xzf "${_edge_wasmer_archive}"
      WORKING_DIRECTORY "${_edge_wasmer_extract_dir}"
      RESULT_VARIABLE _edge_wasmer_extract_result
    )
    if(NOT _edge_wasmer_extract_result EQUAL 0)
      message(FATAL_ERROR "Failed to extract ${_edge_wasmer_archive}")
    endif()
  endif()

  edge_quickjs_wasmer_find_dist_root("${_edge_wasmer_extract_dir}" _edge_wasmer_root)
  if(NOT _edge_wasmer_root)
    file(GLOB _edge_wasmer_extract_children
      LIST_DIRECTORIES true
      "${_edge_wasmer_extract_dir}/*"
    )
    foreach(_edge_wasmer_child IN LISTS _edge_wasmer_extract_children)
      edge_quickjs_wasmer_find_dist_root("${_edge_wasmer_child}" _edge_wasmer_child_root)
      if(_edge_wasmer_child_root)
        set(_edge_wasmer_root "${_edge_wasmer_child_root}")
        break()
      endif()
    endforeach()
  endif()

  if(NOT _edge_wasmer_root)
    message(FATAL_ERROR
      "Downloaded Wasmer C API asset did not contain include/wasm.h, "
      "include/wasmer.h, and lib/libwasmer.a")
  endif()

  set(${out_var} "${_edge_wasmer_root}" PARENT_SCOPE)
endfunction()

function(edge_configure_quickjs_webassembly)
  set(EDGE_QUICKJS_WEBASSEMBLY_ENABLED OFF PARENT_SCOPE)
  if(NOT EDGE_QUICKJS_WEBASSEMBLY)
    return()
  endif()
  if(NOT EDGE_NAPI_PROVIDER STREQUAL "quickjs")
    return()
  endif()

  set(_edge_wasmer_version "${EDGE_QUICKJS_WASMER_VERSION}")
  if(_edge_wasmer_version STREQUAL "" OR _edge_wasmer_version STREQUAL "latest-stable")
    if(EDGE_QUICKJS_WASMER_ALLOW_PRERELEASE)
      set(_edge_wasmer_version "v7.2.0-alpha.3")
    else()
      set(_edge_wasmer_version "v7.1.0")
    endif()
  endif()

  set(_edge_wasmer_root "")
  edge_quickjs_wasmer_find_dist_root("${EDGE_QUICKJS_WASMER_DIST_ROOT}" _edge_wasmer_root)
  if(NOT _edge_wasmer_root AND DEFINED ENV{HOME})
    edge_quickjs_wasmer_find_dist_root("$ENV{HOME}/.wasmer" _edge_wasmer_root)
  endif()
  if(NOT _edge_wasmer_root)
    edge_quickjs_wasmer_asset_name(_edge_wasmer_asset_name)
    edge_quickjs_wasmer_download("${_edge_wasmer_version}" "${_edge_wasmer_asset_name}" _edge_wasmer_root)
  endif()

  add_library(edge_quickjs_wasmer_c_api INTERFACE)
  target_include_directories(edge_quickjs_wasmer_c_api INTERFACE
    "${_edge_wasmer_root}/include"
  )
  if(NOT EDGE_IS_WASIX_TARGET)
    target_link_libraries(edge_quickjs_wasmer_c_api INTERFACE
      "${_edge_wasmer_root}/lib/libwasmer.a"
    )
    if(APPLE)
      target_link_libraries(edge_quickjs_wasmer_c_api INTERFACE
        "-framework CoreFoundation"
        "-framework Foundation"
        "-framework Security"
        "-framework SystemConfiguration"
      )
    elseif(UNIX)
      target_link_libraries(edge_quickjs_wasmer_c_api INTERFACE dl m pthread)
    endif()
  endif()

  message(STATUS "EDGE: QuickJS WebAssembly enabled using Wasmer C API at ${_edge_wasmer_root}")
  set(EDGE_QUICKJS_WEBASSEMBLY_ENABLED ON PARENT_SCOPE)
  set(EDGE_QUICKJS_WASMER_DIST_ROOT_RESOLVED "${_edge_wasmer_root}" PARENT_SCOPE)
endfunction()
