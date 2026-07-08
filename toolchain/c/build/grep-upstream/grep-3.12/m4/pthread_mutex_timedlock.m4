# pthread_mutex_timedlock.m4
# serial 6
dnl Copyright (C) 2019-2025 Free Software Foundation, Inc.
dnl This file is free software; the Free Software Foundation
dnl gives unlimited permission to copy and/or distribute it,
dnl with or without modifications, as long as this notice is preserved.
dnl This file is offered as-is, without any warranty.

AC_DEFUN([gl_FUNC_PTHREAD_MUTEX_TIMEDLOCK],
[
  AC_REQUIRE([gl_PTHREAD_H_DEFAULTS])

  AC_CHECK_DECL([pthread_mutex_timedlock],
    [dnl Test whether the gnulib module 'threadlib' is in use.
     dnl Some packages like Emacs use --avoid=threadlib.
     dnl Write the symbol in such a way that it does not cause 'aclocal' to pick
     dnl the threadlib.m4 file that is installed in $PREFIX/share/aclocal/.
     m4_ifdef([gl_][THREADLIB], [
       AC_REQUIRE([gl_][THREADLIB])
       dnl Test whether the function actually exists.
       dnl FreeBSD 5.2.1 declares it but does not define it.
       AC_CACHE_CHECK([for pthread_mutex_timedlock],
         [gl_cv_func_pthread_mutex_timedlock_in_LIBMULTITHREAD],
         [gl_saved_LIBS="$LIBS"
          LIBS="$LIBS $LIBMULTITHREAD"
          AC_LINK_IFELSE(
            [AC_LANG_PROGRAM(
               [[#include <pthread.h>
                 #include <time.h>
               ]],
               [[pthread_mutex_t lock;
                 struct timespec ts = { 0 };
                 return pthread_mutex_timedlock (&lock, &ts);
               ]])
            ],
            [gl_cv_func_pthread_mutex_timedlock_in_LIBMULTITHREAD=yes],
            [gl_cv_func_pthread_mutex_timedlock_in_LIBMULTITHREAD=no])
          LIBS="$gl_saved_LIBS"
         ])
       if test $gl_cv_func_pthread_mutex_timedlock_in_LIBMULTITHREAD != yes; then
         HAVE_PTHREAD_MUTEX_TIMEDLOCK=0
       fi
     ], [
       :
     ])
    ],
    [HAVE_PTHREAD_MUTEX_TIMEDLOCK=0],
    [[#include <pthread.h>]])
])
