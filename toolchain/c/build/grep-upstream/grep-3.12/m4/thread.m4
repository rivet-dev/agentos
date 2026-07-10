# thread.m4
# serial 5
dnl Copyright (C) 2008-2025 Free Software Foundation, Inc.
dnl This file is free software; the Free Software Foundation
dnl gives unlimited permission to copy and/or distribute it,
dnl with or without modifications, as long as this notice is preserved.
dnl This file is offered as-is, without any warranty.

AC_DEFUN([gl_THREAD],
[
  AC_REQUIRE([gl_THREADLIB])

  if test $gl_threads_api = posix; then
    gl_saved_LIBS="$LIBS"
    LIBS="$LIBS $LIBMULTITHREAD"
    gl_CHECK_FUNCS_ANDROID([pthread_atfork], [[#include <pthread.h>]])
    LIBS="$gl_saved_LIBS"
  fi
])
