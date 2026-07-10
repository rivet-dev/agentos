set(CMAKE_HOST_SYSTEM "Linux-6.1.0-41-amd64")
set(CMAKE_HOST_SYSTEM_NAME "Linux")
set(CMAKE_HOST_SYSTEM_VERSION "6.1.0-41-amd64")
set(CMAKE_HOST_SYSTEM_PROCESSOR "x86_64")

include("/home/nathan/.herdr/workspaces/agent-os/reg-tests/toolchain/c/vendor/wasi-sdk/share/cmake/wasi-sdk.cmake")

set(CMAKE_SYSTEM "WASI-1")
set(CMAKE_SYSTEM_NAME "WASI")
set(CMAKE_SYSTEM_VERSION "1")
set(CMAKE_SYSTEM_PROCESSOR "wasm32")

set(CMAKE_CROSSCOMPILING "TRUE")

set(CMAKE_SYSTEM_LOADED 1)
