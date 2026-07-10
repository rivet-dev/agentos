# pthread-cond.m4
# serial 3
dnl Copyright (C) 2019-2025 Free Software Foundation, Inc.
dnl This file is free software; the Free Software Foundation
dnl gives unlimited permission to copy and/or distribute it,
dnl with or without modifications, as long as this notice is preserved.
dnl This file is offered as-is, without any warranty.

AC_DEFUN([gl_PTHREAD_COND],
[
  AC_REQUIRE([gl_PTHREAD_H])
  AC_REQUIRE([AC_CANONICAL_HOST])

  if { case "$host_os" in mingw* | windows*) true;; *) false;; esac; } \
     && test $gl_threads_api = windows; then
    dnl Choose function names that don't conflict with the mingw-w64 winpthreads
    dnl library.
    REPLACE_PTHREAD_COND_INIT=1
    REPLACE_PTHREAD_CONDATTR_INIT=1
    REPLACE_PTHREAD_CONDATTR_DESTROY=1
    REPLACE_PTHREAD_COND_WAIT=1
    REPLACE_PTHREAD_COND_TIMEDWAIT=1
    REPLACE_PTHREAD_COND_SIGNAL=1
    REPLACE_PTHREAD_COND_BROADCAST=1
    REPLACE_PTHREAD_COND_DESTROY=1
  else
    if test $HAVE_PTHREAD_H = 0; then
      HAVE_PTHREAD_COND_INIT=0
      HAVE_PTHREAD_CONDATTR_INIT=0
      HAVE_PTHREAD_CONDATTR_DESTROY=0
      HAVE_PTHREAD_COND_WAIT=0
      HAVE_PTHREAD_COND_TIMEDWAIT=0
      HAVE_PTHREAD_COND_SIGNAL=0
      HAVE_PTHREAD_COND_BROADCAST=0
      HAVE_PTHREAD_COND_DESTROY=0
    fi
  fi
])
