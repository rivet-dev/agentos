# windows-rc.m4
# serial 1
dnl Copyright (C) 2024-2025 Free Software Foundation, Inc.
dnl This file is free software; the Free Software Foundation
dnl gives unlimited permission to copy and/or distribute it,
dnl with or without modifications, as long as this notice is preserved.
dnl This file is offered as-is, without any warranty.

dnl Find the tool that "compiles" a Windows resource file (.rc) to an
dnl object file.

AC_DEFUN_ONCE([gl_WINDOWS_RC],
[
  AC_REQUIRE([AC_CANONICAL_HOST])
  case "$host_os" in
    mingw* | windows*)
      dnl Check for a program that compiles Windows resource files.
      AC_CHECK_TOOL([WINDRES], [windres])
      ;;
  esac
])
