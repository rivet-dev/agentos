function(edge_configure_napi_provider)
  set(_edge_napi_provider_default "bundled-v8")
  if(EDGE_IS_WASIX_TARGET)
    set(_edge_napi_provider_default "imports")
  endif()
  set(EDGE_NAPI_PROVIDER
    "${_edge_napi_provider_default}"
    CACHE STRING "N-API provider backend: bundled-v8|imports|quickjs"
  )
  set_property(CACHE EDGE_NAPI_PROVIDER PROPERTY STRINGS bundled-v8 imports quickjs)
  if(EDGE_EXTERNAL_NAPI_V8)
    message(WARNING
      "EDGE_EXTERNAL_NAPI_V8 is deprecated; use EDGE_NAPI_PROVIDER=imports.")
    set(EDGE_NAPI_PROVIDER "imports" CACHE STRING
      "N-API provider backend: bundled-v8|imports|quickjs" FORCE)
  endif()
  string(TOLOWER "${EDGE_NAPI_PROVIDER}" EDGE_NAPI_PROVIDER)
  if(NOT EDGE_NAPI_PROVIDER MATCHES "^(bundled-v8|imports|quickjs)$")
    message(FATAL_ERROR
      "Invalid EDGE_NAPI_PROVIDER='${EDGE_NAPI_PROVIDER}'. "
      "Valid values: bundled-v8, imports, quickjs.")
  endif()
  set(EDGE_NAPI_PROVIDER "${EDGE_NAPI_PROVIDER}" CACHE STRING
    "N-API provider backend: bundled-v8|imports|quickjs" FORCE)
  if(EDGE_IS_WASIX_TARGET AND EDGE_NAPI_PROVIDER STREQUAL "imports")
    set(EDGE_ALLOW_UNDEFINED_IMPORTS ON CACHE BOOL "" FORCE)
  elseif(EDGE_IS_WASIX_TARGET AND
         EDGE_NAPI_PROVIDER STREQUAL "quickjs" AND
         EDGE_QUICKJS_WEBASSEMBLY)
    set(EDGE_ALLOW_UNDEFINED_IMPORTS ON CACHE BOOL "" FORCE)
  elseif(EDGE_NAPI_PROVIDER STREQUAL "quickjs")
    set(EDGE_ALLOW_UNDEFINED_IMPORTS OFF CACHE BOOL "" FORCE)
  endif()

  set(EDGE_NAPI_PROVIDER "${EDGE_NAPI_PROVIDER}" PARENT_SCOPE)
  set(EDGE_ALLOW_UNDEFINED_IMPORTS "${EDGE_ALLOW_UNDEFINED_IMPORTS}" PARENT_SCOPE)
endfunction()
