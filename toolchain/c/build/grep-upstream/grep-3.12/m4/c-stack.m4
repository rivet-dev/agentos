# c-stack.m4
# serial 25
dnl Copyright (C) 2002-2004, 2008-2025 Free Software Foundation, Inc.
dnl This file is free software; the Free Software Foundation
dnl gives unlimited permission to copy and/or distribute it,
dnl with or without modifications, as long as this notice is preserved.
dnl This file is offered as-is, without any warranty.

# Check prerequisites for compiling lib/c-stack.c.

# Written by Paul Eggert.

AC_DEFUN([gl_C_STACK],
[
  dnl 'c-stack' needs -lsigsegv if and only if the 'sigsegv' module needs it.
  AC_REQUIRE([gl_SIGSEGV])
  if test $gl_sigsegv_uses_libsigsegv = yes; then
    AC_SUBST([LIBCSTACK], [$LIBSIGSEGV])
    AC_SUBST([LTLIBCSTACK], [$LTLIBSIGSEGV])
  fi
])
