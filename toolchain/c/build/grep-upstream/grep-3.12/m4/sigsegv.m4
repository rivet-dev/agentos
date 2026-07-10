# sigsegv.m4
# serial 2
dnl Copyright (C) 2021-2025 Free Software Foundation, Inc.
dnl This file is free software; the Free Software Foundation
dnl gives unlimited permission to copy and/or distribute it,
dnl with or without modifications, as long as this notice is preserved.
dnl This file is offered as-is, without any warranty.

dnl From Bruno Haible.

AC_DEFUN_ONCE([gl_SIGSEGV],
[
  AC_ARG_WITH([libsigsegv],
    [AS_HELP_STRING([[--with-libsigsegv]],
       [use the GNU libsigsegv library, when present, instead of the gnulib module 'sigsegv'])])
  if test "$with_libsigsegv" = yes; then
    gl_LIBSIGSEGV
    gl_sigsegv_uses_libsigsegv="$gl_cv_lib_sigsegv"
  else
    gl_sigsegv_uses_libsigsegv=no
  fi
])
