# localename.m4
# serial 14
dnl Copyright (C) 2007, 2009-2025 Free Software Foundation, Inc.
dnl This file is free software; the Free Software Foundation
dnl gives unlimited permission to copy and/or distribute it,
dnl with or without modifications, as long as this notice is preserved.
dnl This file is offered as-is, without any warranty.

AC_DEFUN([gl_LOCALENAME_UNSAFE],
[
  AC_REQUIRE([gl_LOCALE_H_DEFAULTS])
  AC_REQUIRE([gt_LC_MESSAGES])
  AC_REQUIRE([gt_INTL_MACOSX])
])

AC_DEFUN([gl_LOCALENAME_UNSAFE_LIMITED],
[
  AC_REQUIRE([gt_LC_MESSAGES])
])

AC_DEFUN([gl_LOCALENAME_ENVIRON],
[
  AC_REQUIRE([gt_INTL_MACOSX])
])
