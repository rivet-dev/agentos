# pthread-mutex.m4
# serial 4
dnl Copyright (C) 2019-2025 Free Software Foundation, Inc.
dnl This file is free software; the Free Software Foundation
dnl gives unlimited permission to copy and/or distribute it,
dnl with or without modifications, as long as this notice is preserved.
dnl This file is offered as-is, without any warranty.

AC_DEFUN([gl_PTHREAD_MUTEX],
[
  AC_REQUIRE([gl_PTHREAD_H])
  AC_REQUIRE([AC_CANONICAL_HOST])

  if { case "$host_os" in mingw* | windows*) true;; *) false;; esac; } \
     && test $gl_threads_api = windows; then
    dnl Choose function names that don't conflict with the mingw-w64 winpthreads
    dnl library.
    REPLACE_PTHREAD_MUTEX_INIT=1
    REPLACE_PTHREAD_MUTEXATTR_INIT=1
    REPLACE_PTHREAD_MUTEXATTR_GETTYPE=1
    REPLACE_PTHREAD_MUTEXATTR_SETTYPE=1
    REPLACE_PTHREAD_MUTEXATTR_GETROBUST=1
    REPLACE_PTHREAD_MUTEXATTR_SETROBUST=1
    REPLACE_PTHREAD_MUTEXATTR_DESTROY=1
    REPLACE_PTHREAD_MUTEX_LOCK=1
    REPLACE_PTHREAD_MUTEX_TRYLOCK=1
    REPLACE_PTHREAD_MUTEX_TIMEDLOCK=1
    REPLACE_PTHREAD_MUTEX_UNLOCK=1
    REPLACE_PTHREAD_MUTEX_DESTROY=1
  else
    if test $HAVE_PTHREAD_H = 0; then
      HAVE_PTHREAD_MUTEX_INIT=0
      HAVE_PTHREAD_MUTEXATTR_INIT=0
      HAVE_PTHREAD_MUTEXATTR_GETTYPE=0
      HAVE_PTHREAD_MUTEXATTR_SETTYPE=0
      HAVE_PTHREAD_MUTEXATTR_GETROBUST=0
      HAVE_PTHREAD_MUTEXATTR_SETROBUST=0
      HAVE_PTHREAD_MUTEXATTR_DESTROY=0
      HAVE_PTHREAD_MUTEX_LOCK=0
      HAVE_PTHREAD_MUTEX_TRYLOCK=0
      dnl HAVE_PTHREAD_MUTEX_TIMEDLOCK is set in pthread_mutex_timedlock.m4.
      HAVE_PTHREAD_MUTEX_UNLOCK=0
      HAVE_PTHREAD_MUTEX_DESTROY=0
    else
      AC_CACHE_CHECK([for pthread_mutexattr_getrobust],
        [gl_cv_func_pthread_mutexattr_getrobust],
        [saved_LIBS="$LIBS"
         LIBS="$LIBS $LIBPMULTITHREAD"
         AC_LINK_IFELSE(
           [AC_LANG_SOURCE(
              [[extern
                #ifdef __cplusplus
                "C"
                #endif
                int pthread_mutexattr_getrobust (void);
                int main ()
                {
                  return pthread_mutexattr_getrobust ();
                }
              ]])],
           [gl_cv_func_pthread_mutexattr_getrobust=yes],
           [gl_cv_func_pthread_mutexattr_getrobust=no])
         LIBS="$saved_LIBS"
        ])
      if test $gl_cv_func_pthread_mutexattr_getrobust = no; then
        HAVE_PTHREAD_MUTEXATTR_GETROBUST=0
        HAVE_PTHREAD_MUTEXATTR_SETROBUST=0
        AC_DEFINE([PTHREAD_MUTEXATTR_ROBUST_UNIMPLEMENTED], [1],
          [Define if the 'robust' attribute of pthread_mutex* doesn't exist.])
      fi
    fi
  fi
])
