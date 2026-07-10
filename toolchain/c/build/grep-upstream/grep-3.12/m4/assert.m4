# assert.m4
# serial 8
dnl Copyright (C) 1998-1999, 2001, 2004, 2008-2025 Free Software Foundation,
dnl Inc.
dnl This file is free software; the Free Software Foundation
dnl gives unlimited permission to copy and/or distribute it,
dnl with or without modifications, as long as this notice is preserved.
dnl This file is offered as-is, without any warranty.

dnl based on code from Eleftherios Gkioulekas
dnl Autoconf 2.60 provides AC_HEADER_ASSERT for the same purpose, but
dnl it has broken semantics for --enable-assert until 2.64.
AC_DEFUN([gl_ASSERT],
[
  AC_MSG_CHECKING([whether to enable assertions])
  AC_ARG_ENABLE([assert],
    [AS_HELP_STRING([[--disable-assert]], [turn off assertions])],
    [AS_IF([test "x$enableval" = xno],
      [AC_DEFINE([NDEBUG], [1],
        [Define to 1 if assertions should be disabled.])],
      [test "x$enableval" != xyes],
      [AC_MSG_WARN([invalid argument supplied to --enable-assert])
      enable_assert=yes])],
    [enable_assert=yes])
  AC_MSG_RESULT([$enable_assert])
])
