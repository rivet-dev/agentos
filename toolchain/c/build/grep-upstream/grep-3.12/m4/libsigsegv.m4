# libsigsegv.m4
# serial 4
dnl Copyright (C) 2002-2003, 2008-2025 Free Software Foundation, Inc.
dnl This file is free software; the Free Software Foundation
dnl gives unlimited permission to copy and/or distribute it,
dnl with or without modifications, as long as this notice is preserved.
dnl This file is offered as-is, without any warranty.

dnl From Bruno Haible, Sam Steingold.

AC_DEFUN([gl_LIBSIGSEGV],
[
  AC_LIB_HAVE_LINKFLAGS([sigsegv], [],
    [#include <sigsegv.h>], [sigsegv_deinstall_handler();],
    [no, consider installing GNU libsigsegv])
  dnl Some other autoconf macros and clisp's configure use this variable.
  gl_cv_lib_sigsegv="$ac_cv_libsigsegv"
])
