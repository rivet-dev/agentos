function(edge_detect_native_openssl_target_os out_var)
  if(CMAKE_SYSTEM_NAME STREQUAL "Linux")
    set(_value "linux")
  elseif(CMAKE_SYSTEM_NAME STREQUAL "Darwin")
    set(_value "mac")
  elseif(CMAKE_SYSTEM_NAME STREQUAL "Windows")
    set(_value "win")
  else()
    message(FATAL_ERROR
      "Vendored OpenSSL is not yet wired for native target system '${CMAKE_SYSTEM_NAME}'. "
      "Use -DEDGE_SHARED_OPENSSL=ON for this platform.")
  endif()
  set(${out_var} "${_value}" PARENT_SCOPE)
endfunction()

function(edge_detect_native_openssl_target_arch out_var)
  if(CMAKE_SYSTEM_PROCESSOR MATCHES "^(x86_64|amd64|AMD64)$")
    set(_value "x64")
  elseif(CMAKE_SYSTEM_PROCESSOR MATCHES "^(x86|i[3-6]86|X86)$")
    set(_value "x86")
  elseif(CMAKE_SYSTEM_PROCESSOR MATCHES "^(arm64|aarch64|ARM64)$")
    set(_value "arm64")
  else()
    message(FATAL_ERROR
      "Vendored OpenSSL is not yet wired for native target architecture '${CMAKE_SYSTEM_PROCESSOR}'. "
      "Use -DEDGE_SHARED_OPENSSL=ON for this target.")
  endif()
  set(${out_var} "${_value}" PARENT_SCOPE)
endfunction()

function(edge_require_shared_openssl_target target_name display_name)
  set(_shared FALSE)
  foreach(_prop
      IMPORTED_IMPLIB
      IMPORTED_IMPLIB_RELEASE
      IMPORTED_IMPLIB_DEBUG
      IMPORTED_IMPLIB_RELWITHDEBINFO
      IMPORTED_IMPLIB_MINSIZEREL
      IMPORTED_IMPLIB_NOCONFIG)
    get_target_property(_value "${target_name}" "${_prop}")
    if(_value AND NOT _value MATCHES "-NOTFOUND$")
      set(_shared TRUE)
    endif()
  endforeach()
  if(NOT _shared)
    foreach(_prop
        IMPORTED_LOCATION
        IMPORTED_LOCATION_RELEASE
        IMPORTED_LOCATION_DEBUG
        IMPORTED_LOCATION_RELWITHDEBINFO
        IMPORTED_LOCATION_MINSIZEREL
        IMPORTED_LOCATION_NOCONFIG)
      get_target_property(_value "${target_name}" "${_prop}")
      if(NOT _value OR _value MATCHES "-NOTFOUND$")
        continue()
      endif()
      if(_value MATCHES "\\.(so(\\..*)?|dylib|dll)$")
        set(_shared TRUE)
      endif()
    endforeach()
  endif()
  if(NOT _shared)
    message(FATAL_ERROR
      "${display_name} resolved to a non-shared OpenSSL library. "
      "Shared mode requires shared/system OpenSSL libraries.")
  endif()
endfunction()

macro(edge_configure_openssl)
  if(EDGE_IS_WASIX_TARGET)
    set(EDGE_OPENSSL_WASIX_ROOT
      "${PROJECT_ROOT}/deps/openssl-wasix"
      CACHE PATH "Path to wasix-org OpenSSL source/build tree"
    )
    set(_edge_openssl_include_dir "${EDGE_OPENSSL_WASIX_ROOT}/include")
    if(NOT EXISTS "${_edge_openssl_include_dir}")
      message(FATAL_ERROR
        "OpenSSL include directory not found: ${_edge_openssl_include_dir}. "
        "For WASIX, clone deps with wasix/setup-wasix-deps.sh.")
    endif()

    set(_edge_openssl_crypto "")
    foreach(_candidate
        "${EDGE_OPENSSL_WASIX_ROOT}/lib/libcrypto.a"
        "${EDGE_OPENSSL_WASIX_ROOT}/libcrypto.a")
      if(EXISTS "${_candidate}")
        set(_edge_openssl_crypto "${_candidate}")
        break()
      endif()
    endforeach()

    set(_edge_openssl_ssl "")
    foreach(_candidate
        "${EDGE_OPENSSL_WASIX_ROOT}/lib/libssl.a"
        "${EDGE_OPENSSL_WASIX_ROOT}/libssl.a")
      if(EXISTS "${_candidate}")
        set(_edge_openssl_ssl "${_candidate}")
        break()
      endif()
    endforeach()

    if(_edge_openssl_crypto STREQUAL "" OR _edge_openssl_ssl STREQUAL "")
      message(FATAL_ERROR
        "WASIX OpenSSL static libraries not found under ${EDGE_OPENSSL_WASIX_ROOT}. "
        "Expected libcrypto.a and libssl.a (either root or lib/).")
    endif()

    add_library(OpenSSL::Crypto STATIC IMPORTED GLOBAL)
    set_target_properties(OpenSSL::Crypto PROPERTIES
      IMPORTED_LOCATION "${_edge_openssl_crypto}"
      INTERFACE_INCLUDE_DIRECTORIES "${_edge_openssl_include_dir}"
    )
    add_library(OpenSSL::SSL STATIC IMPORTED GLOBAL)
    set_target_properties(OpenSSL::SSL PROPERTIES
      IMPORTED_LOCATION "${_edge_openssl_ssl}"
      INTERFACE_INCLUDE_DIRECTORIES "${_edge_openssl_include_dir}"
    )
    target_link_libraries(OpenSSL::SSL INTERFACE OpenSSL::Crypto)
  elseif(NOT EDGE_SHARED_OPENSSL)
    if(NOT EDGE_SHARED_OPENSSL_INCLUDES STREQUAL "" OR
       NOT EDGE_SHARED_OPENSSL_LIBPATH STREQUAL "" OR
       NOT EDGE_SHARED_OPENSSL_LIBNAME STREQUAL "crypto;ssl")
      message(FATAL_ERROR
        "EDGE_SHARED_OPENSSL_* overrides require -DEDGE_SHARED_OPENSSL=ON.")
    endif()

    edge_detect_native_openssl_target_os(EDGE_OPENSSL_TARGET_OS)
    edge_detect_native_openssl_target_arch(EDGE_OPENSSL_TARGET_ARCH)
    if(CMAKE_C_COMPILER_ID MATCHES "Clang")
      set(_edge_openssl_compiler_family "clang")
    elseif(MSVC)
      set(_edge_openssl_compiler_family "msvc")
    else()
      set(_edge_openssl_compiler_family "gcc")
    endif()

    set(EDGE_OPENSSL_MODULES_DIR
      "${CMAKE_CURRENT_BINARY_DIR}/deps/openssl/lib/openssl-modules"
    )
    file(MAKE_DIRECTORY "${EDGE_OPENSSL_MODULES_DIR}")

    set(_edge_vendored_openssl_manifest
      "${CMAKE_CURRENT_BINARY_DIR}/edge_vendored_openssl.cmake"
    )
    execute_process(
      COMMAND
        "${Python3_EXECUTABLE}"
        "${PROJECT_ROOT}/scripts/generate_vendored_openssl_cmake.py"
        --project-root "${PROJECT_ROOT}"
        --target-os "${EDGE_OPENSSL_TARGET_OS}"
        --target-arch "${EDGE_OPENSSL_TARGET_ARCH}"
        --compiler-family "${_edge_openssl_compiler_family}"
        --modules-dir "${EDGE_OPENSSL_MODULES_DIR}"
        --output "${_edge_vendored_openssl_manifest}"
      RESULT_VARIABLE _edge_vendored_openssl_status
      OUTPUT_VARIABLE _edge_vendored_openssl_stdout
      ERROR_VARIABLE _edge_vendored_openssl_stderr
    )
    if(NOT _edge_vendored_openssl_status EQUAL 0)
      message(FATAL_ERROR
        "Failed to generate vendored OpenSSL build manifest:\n${_edge_vendored_openssl_stderr}")
    endif()
    include("${_edge_vendored_openssl_manifest}")

    set(_edge_vendored_openssl_asm_sources
      ${EDGE_VENDORED_OPENSSL_SOURCES}
      ${EDGE_VENDORED_OPENSSL_CLI_SOURCES}
    )
    list(FILTER _edge_vendored_openssl_asm_sources INCLUDE REGEX "\\.(S|s|asm)$")
    if(_edge_vendored_openssl_asm_sources)
      if(WIN32)
        enable_language(ASM_MASM)
      else()
        enable_language(ASM)
      endif()
    endif()

    add_library(edge_vendored_openssl STATIC
      ${EDGE_VENDORED_OPENSSL_SOURCES}
    )
    target_include_directories(edge_vendored_openssl
      PUBLIC
        ${EDGE_VENDORED_OPENSSL_INCLUDE_DIRS}
    )
    target_compile_definitions(edge_vendored_openssl
      PRIVATE
        ${EDGE_VENDORED_OPENSSL_DEFINES}
    )
    target_compile_options(edge_vendored_openssl
      PRIVATE
        ${EDGE_VENDORED_OPENSSL_COMPILE_OPTIONS}
    )
    target_link_libraries(edge_vendored_openssl
      PUBLIC
        ${EDGE_VENDORED_OPENSSL_LINK_LIBRARIES}
    )

    add_library(edge_vendored_openssl_ssl INTERFACE)
    target_link_libraries(edge_vendored_openssl_ssl
      INTERFACE
        edge_vendored_openssl
    )

    add_library(OpenSSL::Crypto ALIAS edge_vendored_openssl)
    add_library(OpenSSL::SSL ALIAS edge_vendored_openssl_ssl)

    add_executable(edge_openssl_cli
      ${EDGE_VENDORED_OPENSSL_CLI_SOURCES}
    )
    set_target_properties(edge_openssl_cli PROPERTIES OUTPUT_NAME "openssl-cli")
    target_include_directories(edge_openssl_cli
      PRIVATE
        ${EDGE_VENDORED_OPENSSL_CLI_INCLUDE_DIRS}
    )
    target_compile_definitions(edge_openssl_cli
      PRIVATE
        ${EDGE_VENDORED_OPENSSL_CLI_DEFINES}
    )
    target_compile_options(edge_openssl_cli
      PRIVATE
        ${EDGE_VENDORED_OPENSSL_CLI_COMPILE_OPTIONS}
    )
    target_link_libraries(edge_openssl_cli
      PRIVATE
        OpenSSL::SSL
        ${EDGE_VENDORED_OPENSSL_CLI_LINK_LIBRARIES}
    )
  else()
    if(DEFINED OPENSSL_USE_STATIC_LIBS AND OPENSSL_USE_STATIC_LIBS)
      message(FATAL_ERROR
        "EDGE_SHARED_OPENSSL=ON is incompatible with OPENSSL_USE_STATIC_LIBS=TRUE.")
    endif()

    if(EDGE_SHARED_OPENSSL_INCLUDES)
      set(OPENSSL_INCLUDE_DIR "${EDGE_SHARED_OPENSSL_INCLUDES}" CACHE PATH "" FORCE)
    endif()

    list(LENGTH EDGE_SHARED_OPENSSL_LIBNAME _edge_shared_openssl_libname_count)
    if(NOT _edge_shared_openssl_libname_count EQUAL 2)
      message(FATAL_ERROR
        "EDGE_SHARED_OPENSSL_LIBNAME must contain exactly two entries: crypto;ssl.")
    endif()
    list(GET EDGE_SHARED_OPENSSL_LIBNAME 0 _edge_shared_openssl_crypto_name)
    list(GET EDGE_SHARED_OPENSSL_LIBNAME 1 _edge_shared_openssl_ssl_name)

    if(EDGE_SHARED_OPENSSL_LIBPATH)
      find_library(_edge_shared_openssl_crypto_lib
        NAMES "${_edge_shared_openssl_crypto_name}"
        PATHS "${EDGE_SHARED_OPENSSL_LIBPATH}"
        NO_DEFAULT_PATH
      )
      find_library(_edge_shared_openssl_ssl_lib
        NAMES "${_edge_shared_openssl_ssl_name}"
        PATHS "${EDGE_SHARED_OPENSSL_LIBPATH}"
        NO_DEFAULT_PATH
      )
      if(NOT _edge_shared_openssl_crypto_lib OR NOT _edge_shared_openssl_ssl_lib)
        message(FATAL_ERROR
          "Could not locate shared OpenSSL libraries '${EDGE_SHARED_OPENSSL_LIBNAME}' under "
          "${EDGE_SHARED_OPENSSL_LIBPATH}.")
      endif()
      set(OPENSSL_CRYPTO_LIBRARY "${_edge_shared_openssl_crypto_lib}" CACHE FILEPATH "" FORCE)
      set(OPENSSL_SSL_LIBRARY "${_edge_shared_openssl_ssl_lib}" CACHE FILEPATH "" FORCE)
    endif()

    set(OPENSSL_USE_STATIC_LIBS FALSE CACHE BOOL "" FORCE)
    find_package(OpenSSL REQUIRED)
    edge_require_shared_openssl_target(OpenSSL::Crypto "OpenSSL::Crypto")
    edge_require_shared_openssl_target(OpenSSL::SSL "OpenSSL::SSL")
  endif()
endmacro()
