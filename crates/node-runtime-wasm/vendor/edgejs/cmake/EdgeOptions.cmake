function(edge_configure_options)
  set(EDGE_DEFAULT_WASMER_PACKAGE
    "$ENV{EDGE_WASMER_PACKAGE}"
    CACHE STRING "Default Wasmer package used by edge --safe"
  )

  option(EDGE_EXTERNAL_NAPI_V8
    "Deprecated alias. Use EDGE_NAPI_PROVIDER=imports."
    OFF
  )
  option(EDGE_ALLOW_UNDEFINED_IMPORTS
    "Allow undefined imports at link time (useful for external runtime deps in wasm/wasix)"
    OFF
  )
  option(EDGE_PREFER_REPO_LOCAL_V8
    "Prefer the repo-local V8 dist bundle when it exists"
    ON
  )
  option(EDGE_BUILD_NAPI_TESTS
    "Build the selected embedded N-API provider tests as part of the Edge build"
    OFF
  )
  option(EDGE_SHARED_OPENSSL
    "Link against a shared/system OpenSSL instead of the vendored OpenSSL"
    OFF
  )
  option(EDGE_QUICKJS_WEBASSEMBLY
    "Install a QuickJS-only WebAssembly global backed by Wasmer's wasm_c_api"
    ON
  )
  set(EDGE_QUICKJS_WASMER_VERSION
    "latest-stable"
    CACHE STRING "Wasmer C API version for QuickJS WebAssembly, or latest-stable"
  )
  set(EDGE_QUICKJS_WASMER_DIST_ROOT
    "$ENV{EDGE_QUICKJS_WASMER_DIST_ROOT}"
    CACHE PATH "Path to a Wasmer C API distribution with include/ and lib/libwasmer.a"
  )
  option(EDGE_QUICKJS_WASMER_ALLOW_PRERELEASE
    "Allow prerelease Wasmer C API versions when resolving latest"
    OFF
  )
  option(ENABLE_TRACING
    "Compile startup tracing support"
    OFF
  )
  set(EDGE_SHARED_OPENSSL_INCLUDES
    ""
    CACHE PATH "Directory containing OpenSSL headers when EDGE_SHARED_OPENSSL is ON"
  )
  set(EDGE_SHARED_OPENSSL_LIBPATH
    ""
    CACHE PATH "Directory containing shared OpenSSL libraries when EDGE_SHARED_OPENSSL is ON"
  )
  set(EDGE_SHARED_OPENSSL_LIBNAME
    "crypto;ssl"
    CACHE STRING "Semicolon-separated shared OpenSSL library names (crypto;ssl)"
  )
  set(EDGE_REPO_LOCAL_V8_DIST_ROOT
    "${PROJECT_ROOT}/v8-custom-builds/v8/out/dist"
    CACHE PATH "Path to the repo-local V8 dist bundle used when EDGE_PREFER_REPO_LOCAL_V8 is ON"
  )

  set(EDGE_IS_WASIX_TARGET OFF)
  if(CMAKE_SYSTEM_NAME MATCHES "WASI|WASIX" OR
     CMAKE_C_COMPILER MATCHES "wasixcc" OR
     CMAKE_CXX_COMPILER MATCHES "wasixcc\\+\\+")
    set(EDGE_IS_WASIX_TARGET ON)
  endif()

  set(EDGE_DEFAULT_WASMER_PACKAGE "${EDGE_DEFAULT_WASMER_PACKAGE}" PARENT_SCOPE)
  set(EDGE_EXTERNAL_NAPI_V8 "${EDGE_EXTERNAL_NAPI_V8}" PARENT_SCOPE)
  set(EDGE_ALLOW_UNDEFINED_IMPORTS "${EDGE_ALLOW_UNDEFINED_IMPORTS}" PARENT_SCOPE)
  set(EDGE_PREFER_REPO_LOCAL_V8 "${EDGE_PREFER_REPO_LOCAL_V8}" PARENT_SCOPE)
  set(EDGE_BUILD_NAPI_TESTS "${EDGE_BUILD_NAPI_TESTS}" PARENT_SCOPE)
  set(EDGE_SHARED_OPENSSL "${EDGE_SHARED_OPENSSL}" PARENT_SCOPE)
  set(EDGE_QUICKJS_WEBASSEMBLY "${EDGE_QUICKJS_WEBASSEMBLY}" PARENT_SCOPE)
  set(EDGE_QUICKJS_WASMER_VERSION "${EDGE_QUICKJS_WASMER_VERSION}" PARENT_SCOPE)
  set(EDGE_QUICKJS_WASMER_DIST_ROOT "${EDGE_QUICKJS_WASMER_DIST_ROOT}" PARENT_SCOPE)
  set(EDGE_QUICKJS_WASMER_ALLOW_PRERELEASE "${EDGE_QUICKJS_WASMER_ALLOW_PRERELEASE}" PARENT_SCOPE)
  set(ENABLE_TRACING "${ENABLE_TRACING}" PARENT_SCOPE)
  set(EDGE_SHARED_OPENSSL_INCLUDES "${EDGE_SHARED_OPENSSL_INCLUDES}" PARENT_SCOPE)
  set(EDGE_SHARED_OPENSSL_LIBPATH "${EDGE_SHARED_OPENSSL_LIBPATH}" PARENT_SCOPE)
  set(EDGE_SHARED_OPENSSL_LIBNAME "${EDGE_SHARED_OPENSSL_LIBNAME}" PARENT_SCOPE)
  set(EDGE_REPO_LOCAL_V8_DIST_ROOT "${EDGE_REPO_LOCAL_V8_DIST_ROOT}" PARENT_SCOPE)
  set(EDGE_IS_WASIX_TARGET "${EDGE_IS_WASIX_TARGET}" PARENT_SCOPE)
endfunction()
