# Install script for directory: /home/nathan/.herdr/workspaces/agent-os/reg-tests/toolchain/c/libs/duckdb

# Set the install prefix
if(NOT DEFINED CMAKE_INSTALL_PREFIX)
  set(CMAKE_INSTALL_PREFIX "/usr/local")
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

if(NOT CMAKE_INSTALL_LOCAL_ONLY)
  # Include the install script for the subdirectory.
  include("/home/nathan/.herdr/workspaces/agent-os/reg-tests/toolchain/c/build/duckdb-cmake/extension/core_functions/cmake_install.cmake")
endif()

if(NOT CMAKE_INSTALL_LOCAL_ONLY)
  # Include the install script for the subdirectory.
  include("/home/nathan/.herdr/workspaces/agent-os/reg-tests/toolchain/c/build/duckdb-cmake/src/cmake_install.cmake")
endif()

if(NOT CMAKE_INSTALL_LOCAL_ONLY)
  # Include the install script for the subdirectory.
  include("/home/nathan/.herdr/workspaces/agent-os/reg-tests/toolchain/c/build/duckdb-cmake/tools/cmake_install.cmake")
endif()

if(NOT CMAKE_INSTALL_LOCAL_ONLY)
  # Include the install script for the subdirectory.
  include("/home/nathan/.herdr/workspaces/agent-os/reg-tests/toolchain/c/build/duckdb-cmake/extension/cmake_install.cmake")
endif()

if(NOT CMAKE_INSTALL_LOCAL_ONLY)
  # Include the install script for the subdirectory.
  include("/home/nathan/.herdr/workspaces/agent-os/reg-tests/toolchain/c/build/duckdb-cmake/third_party/fastpforlib/cmake_install.cmake")
endif()

if(NOT CMAKE_INSTALL_LOCAL_ONLY)
  # Include the install script for the subdirectory.
  include("/home/nathan/.herdr/workspaces/agent-os/reg-tests/toolchain/c/build/duckdb-cmake/third_party/fmt/cmake_install.cmake")
endif()

if(NOT CMAKE_INSTALL_LOCAL_ONLY)
  # Include the install script for the subdirectory.
  include("/home/nathan/.herdr/workspaces/agent-os/reg-tests/toolchain/c/build/duckdb-cmake/third_party/fsst/cmake_install.cmake")
endif()

if(NOT CMAKE_INSTALL_LOCAL_ONLY)
  # Include the install script for the subdirectory.
  include("/home/nathan/.herdr/workspaces/agent-os/reg-tests/toolchain/c/build/duckdb-cmake/third_party/hyperloglog/cmake_install.cmake")
endif()

if(NOT CMAKE_INSTALL_LOCAL_ONLY)
  # Include the install script for the subdirectory.
  include("/home/nathan/.herdr/workspaces/agent-os/reg-tests/toolchain/c/build/duckdb-cmake/third_party/libpg_query/cmake_install.cmake")
endif()

if(NOT CMAKE_INSTALL_LOCAL_ONLY)
  # Include the install script for the subdirectory.
  include("/home/nathan/.herdr/workspaces/agent-os/reg-tests/toolchain/c/build/duckdb-cmake/third_party/mbedtls/cmake_install.cmake")
endif()

if(NOT CMAKE_INSTALL_LOCAL_ONLY)
  # Include the install script for the subdirectory.
  include("/home/nathan/.herdr/workspaces/agent-os/reg-tests/toolchain/c/build/duckdb-cmake/third_party/miniz/cmake_install.cmake")
endif()

if(NOT CMAKE_INSTALL_LOCAL_ONLY)
  # Include the install script for the subdirectory.
  include("/home/nathan/.herdr/workspaces/agent-os/reg-tests/toolchain/c/build/duckdb-cmake/third_party/re2/cmake_install.cmake")
endif()

if(NOT CMAKE_INSTALL_LOCAL_ONLY)
  # Include the install script for the subdirectory.
  include("/home/nathan/.herdr/workspaces/agent-os/reg-tests/toolchain/c/build/duckdb-cmake/third_party/skiplist/cmake_install.cmake")
endif()

if(NOT CMAKE_INSTALL_LOCAL_ONLY)
  # Include the install script for the subdirectory.
  include("/home/nathan/.herdr/workspaces/agent-os/reg-tests/toolchain/c/build/duckdb-cmake/third_party/utf8proc/cmake_install.cmake")
endif()

if(NOT CMAKE_INSTALL_LOCAL_ONLY)
  # Include the install script for the subdirectory.
  include("/home/nathan/.herdr/workspaces/agent-os/reg-tests/toolchain/c/build/duckdb-cmake/third_party/yyjson/cmake_install.cmake")
endif()

if(NOT CMAKE_INSTALL_LOCAL_ONLY)
  # Include the install script for the subdirectory.
  include("/home/nathan/.herdr/workspaces/agent-os/reg-tests/toolchain/c/build/duckdb-cmake/third_party/zstd/cmake_install.cmake")
endif()

if(CMAKE_INSTALL_COMPONENT STREQUAL "Unspecified" OR NOT CMAKE_INSTALL_COMPONENT)
  if(EXISTS "$ENV{DESTDIR}${CMAKE_INSTALL_PREFIX}/lib/cmake/DuckDB/DuckDBExports.cmake")
    file(DIFFERENT _cmake_export_file_changed FILES
         "$ENV{DESTDIR}${CMAKE_INSTALL_PREFIX}/lib/cmake/DuckDB/DuckDBExports.cmake"
         "/home/nathan/.herdr/workspaces/agent-os/reg-tests/toolchain/c/build/duckdb-cmake/CMakeFiles/Export/03dadc87f7614de3308f1cfb212d2bab/DuckDBExports.cmake")
    if(_cmake_export_file_changed)
      file(GLOB _cmake_old_config_files "$ENV{DESTDIR}${CMAKE_INSTALL_PREFIX}/lib/cmake/DuckDB/DuckDBExports-*.cmake")
      if(_cmake_old_config_files)
        string(REPLACE ";" ", " _cmake_old_config_files_text "${_cmake_old_config_files}")
        message(STATUS "Old export file \"$ENV{DESTDIR}${CMAKE_INSTALL_PREFIX}/lib/cmake/DuckDB/DuckDBExports.cmake\" will be replaced.  Removing files [${_cmake_old_config_files_text}].")
        unset(_cmake_old_config_files_text)
        file(REMOVE ${_cmake_old_config_files})
      endif()
      unset(_cmake_old_config_files)
    endif()
    unset(_cmake_export_file_changed)
  endif()
  file(INSTALL DESTINATION "${CMAKE_INSTALL_PREFIX}/lib/cmake/DuckDB" TYPE FILE FILES "/home/nathan/.herdr/workspaces/agent-os/reg-tests/toolchain/c/build/duckdb-cmake/CMakeFiles/Export/03dadc87f7614de3308f1cfb212d2bab/DuckDBExports.cmake")
  if(CMAKE_INSTALL_CONFIG_NAME MATCHES "^([Rr][Ee][Ll][Ee][Aa][Ss][Ee])$")
    file(INSTALL DESTINATION "${CMAKE_INSTALL_PREFIX}/lib/cmake/DuckDB" TYPE FILE FILES "/home/nathan/.herdr/workspaces/agent-os/reg-tests/toolchain/c/build/duckdb-cmake/CMakeFiles/Export/03dadc87f7614de3308f1cfb212d2bab/DuckDBExports-release.cmake")
  endif()
endif()

if(CMAKE_INSTALL_COMPONENT STREQUAL "Unspecified" OR NOT CMAKE_INSTALL_COMPONENT)
  file(INSTALL DESTINATION "${CMAKE_INSTALL_PREFIX}/lib/cmake/DuckDB" TYPE FILE FILES
    "/home/nathan/.herdr/workspaces/agent-os/reg-tests/toolchain/c/build/duckdb-cmake/CMakeFiles/DuckDBConfig.cmake"
    "/home/nathan/.herdr/workspaces/agent-os/reg-tests/toolchain/c/build/duckdb-cmake/DuckDBConfigVersion.cmake"
    )
endif()

if(CMAKE_INSTALL_COMPONENT)
  set(CMAKE_INSTALL_MANIFEST "install_manifest_${CMAKE_INSTALL_COMPONENT}.txt")
else()
  set(CMAKE_INSTALL_MANIFEST "install_manifest.txt")
endif()

string(REPLACE ";" "\n" CMAKE_INSTALL_MANIFEST_CONTENT
       "${CMAKE_INSTALL_MANIFEST_FILES}")
file(WRITE "/home/nathan/.herdr/workspaces/agent-os/reg-tests/toolchain/c/build/duckdb-cmake/${CMAKE_INSTALL_MANIFEST}"
     "${CMAKE_INSTALL_MANIFEST_CONTENT}")
