# Install script for directory: /home/nathan/.herdr/workspaces/agent-os/reg-tests/toolchain/c/vendor/llvm-project/libunwind/include

# Set the install prefix
if(NOT DEFINED CMAKE_INSTALL_PREFIX)
  set(CMAKE_INSTALL_PREFIX "/home/nathan/.herdr/workspaces/agent-os/reg-tests/toolchain/c/build/llvm-runtimes-install")
endif()
string(REGEX REPLACE "/$" "" CMAKE_INSTALL_PREFIX "${CMAKE_INSTALL_PREFIX}")

# Set the install configuration name.
if(NOT DEFINED CMAKE_INSTALL_CONFIG_NAME)
  if(BUILD_TYPE)
    string(REGEX REPLACE "^[^A-Za-z0-9_]+" ""
           CMAKE_INSTALL_CONFIG_NAME "${BUILD_TYPE}")
  else()
    set(CMAKE_INSTALL_CONFIG_NAME "Release")
  endif()
  message(STATUS "Install configuration: \"${CMAKE_INSTALL_CONFIG_NAME}\"")
endif()

# Set the component getting installed.
if(NOT CMAKE_INSTALL_COMPONENT)
  if(COMPONENT)
    message(STATUS "Install component: \"${COMPONENT}\"")
    set(CMAKE_INSTALL_COMPONENT "${COMPONENT}")
  else()
    set(CMAKE_INSTALL_COMPONENT)
  endif()
endif()

# Is this installation the result of a crosscompile?
if(NOT DEFINED CMAKE_CROSSCOMPILING)
  set(CMAKE_CROSSCOMPILING "TRUE")
endif()

# Set default install directory permissions.
if(NOT DEFINED CMAKE_OBJDUMP)
  set(CMAKE_OBJDUMP "/home/nathan/.herdr/workspaces/agent-os/reg-tests/toolchain/c/vendor/wasi-sdk/bin/llvm-objdump")
endif()

if(CMAKE_INSTALL_COMPONENT STREQUAL "unwind-headers" OR NOT CMAKE_INSTALL_COMPONENT)
  file(INSTALL DESTINATION "${CMAKE_INSTALL_PREFIX}/include" TYPE FILE PERMISSIONS OWNER_READ OWNER_WRITE GROUP_READ WORLD_READ FILES "/home/nathan/.herdr/workspaces/agent-os/reg-tests/toolchain/c/vendor/llvm-project/libunwind/include/__libunwind_config.h")
endif()

if(CMAKE_INSTALL_COMPONENT STREQUAL "unwind-headers" OR NOT CMAKE_INSTALL_COMPONENT)
  file(INSTALL DESTINATION "${CMAKE_INSTALL_PREFIX}/include" TYPE FILE PERMISSIONS OWNER_READ OWNER_WRITE GROUP_READ WORLD_READ FILES "/home/nathan/.herdr/workspaces/agent-os/reg-tests/toolchain/c/vendor/llvm-project/libunwind/include/libunwind.h")
endif()

if(CMAKE_INSTALL_COMPONENT STREQUAL "unwind-headers" OR NOT CMAKE_INSTALL_COMPONENT)
  file(INSTALL DESTINATION "${CMAKE_INSTALL_PREFIX}/include" TYPE FILE PERMISSIONS OWNER_READ OWNER_WRITE GROUP_READ WORLD_READ FILES "/home/nathan/.herdr/workspaces/agent-os/reg-tests/toolchain/c/vendor/llvm-project/libunwind/include/libunwind.modulemap")
endif()

if(CMAKE_INSTALL_COMPONENT STREQUAL "unwind-headers" OR NOT CMAKE_INSTALL_COMPONENT)
  file(INSTALL DESTINATION "${CMAKE_INSTALL_PREFIX}/include/mach-o" TYPE FILE PERMISSIONS OWNER_READ OWNER_WRITE GROUP_READ WORLD_READ FILES "/home/nathan/.herdr/workspaces/agent-os/reg-tests/toolchain/c/vendor/llvm-project/libunwind/include/mach-o/compact_unwind_encoding.h")
endif()

if(CMAKE_INSTALL_COMPONENT STREQUAL "unwind-headers" OR NOT CMAKE_INSTALL_COMPONENT)
  file(INSTALL DESTINATION "${CMAKE_INSTALL_PREFIX}/include" TYPE FILE PERMISSIONS OWNER_READ OWNER_WRITE GROUP_READ WORLD_READ FILES "/home/nathan/.herdr/workspaces/agent-os/reg-tests/toolchain/c/vendor/llvm-project/libunwind/include/unwind_arm_ehabi.h")
endif()

if(CMAKE_INSTALL_COMPONENT STREQUAL "unwind-headers" OR NOT CMAKE_INSTALL_COMPONENT)
  file(INSTALL DESTINATION "${CMAKE_INSTALL_PREFIX}/include" TYPE FILE PERMISSIONS OWNER_READ OWNER_WRITE GROUP_READ WORLD_READ FILES "/home/nathan/.herdr/workspaces/agent-os/reg-tests/toolchain/c/vendor/llvm-project/libunwind/include/unwind_itanium.h")
endif()

if(CMAKE_INSTALL_COMPONENT STREQUAL "unwind-headers" OR NOT CMAKE_INSTALL_COMPONENT)
  file(INSTALL DESTINATION "${CMAKE_INSTALL_PREFIX}/include" TYPE FILE PERMISSIONS OWNER_READ OWNER_WRITE GROUP_READ WORLD_READ FILES "/home/nathan/.herdr/workspaces/agent-os/reg-tests/toolchain/c/vendor/llvm-project/libunwind/include/unwind.h")
endif()

