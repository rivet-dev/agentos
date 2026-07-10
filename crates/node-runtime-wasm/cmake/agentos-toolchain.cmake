set(CMAKE_SYSTEM_NAME WASI)
set(CMAKE_SYSTEM_PROCESSOR wasm32)

if("$ENV{AGENTOS_WASI_SDK_ROOT}" STREQUAL "")
  message(FATAL_ERROR "AGENTOS_WASI_SDK_ROOT must name the pinned wasi-sdk directory")
endif()
if("$ENV{AGENTOS_WASI_SYSROOT}" STREQUAL "")
  message(FATAL_ERROR "AGENTOS_WASI_SYSROOT must name the generated AgentOS sysroot")
endif()

set(_agentos_wasi_sdk "$ENV{AGENTOS_WASI_SDK_ROOT}")
set(_agentos_sysroot "$ENV{AGENTOS_WASI_SYSROOT}")
set(CMAKE_C_COMPILER "${_agentos_wasi_sdk}/bin/clang" CACHE FILEPATH "" FORCE)
set(CMAKE_CXX_COMPILER "${_agentos_wasi_sdk}/bin/clang++" CACHE FILEPATH "" FORCE)
set(CMAKE_AR "${_agentos_wasi_sdk}/bin/llvm-ar" CACHE FILEPATH "" FORCE)
set(CMAKE_RANLIB "${_agentos_wasi_sdk}/bin/llvm-ranlib" CACHE FILEPATH "" FORCE)
set(CMAKE_LINKER "${_agentos_wasi_sdk}/bin/wasm-ld" CACHE FILEPATH "" FORCE)
set(CMAKE_SYSROOT "${_agentos_sysroot}" CACHE PATH "" FORCE)
set(CMAKE_FIND_ROOT_PATH "${_agentos_sysroot}" CACHE PATH "" FORCE)

# The Node runtime is a pthread-enabled wasm32-wasip1 module. Its libc calls
# resolve only to the AgentOS sysroot declarations; no WASIX SDK or runtime is
# part of this toolchain. V8 supplies the WebAssembly threads, atomics, SIMD,
# bulk-memory, and exception features inside the existing isolate.
set(_agentos_common_flags
  "--target=wasm32-wasi-threads -matomics -mbulk-memory -msimd128 -pthread -fwasm-exceptions -D_WASI_EMULATED_MMAN -D_WASI_EMULATED_PROCESS_CLOCKS -fno-omit-frame-pointer -ffile-prefix-map=${CMAKE_SOURCE_DIR}=."
)
set(CMAKE_C_FLAGS_INIT "${_agentos_common_flags}")
set(CMAKE_CXX_FLAGS_INIT "${_agentos_common_flags}")
set(CMAKE_EXE_LINKER_FLAGS_INIT
  "-Wl,--import-memory -Wl,--shared-memory -Wl,--initial-memory=67108864 -Wl,--max-memory=268435456 -lwasi-emulated-mman -lwasi-emulated-process-clocks -lsetjmp"
)

set(CMAKE_FIND_ROOT_PATH_MODE_PROGRAM NEVER)
set(CMAKE_FIND_ROOT_PATH_MODE_LIBRARY ONLY)
set(CMAKE_FIND_ROOT_PATH_MODE_INCLUDE ONLY)
set(CMAKE_FIND_ROOT_PATH_MODE_PACKAGE ONLY)
