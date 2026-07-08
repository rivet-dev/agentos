# nullptr.m4
# serial 2
dnl Copyright 2023-2025 Free Software Foundation, Inc.
dnl This file is free software; the Free Software Foundation
dnl gives unlimited permission to copy and/or distribute it,
dnl with or without modifications, as long as this notice is preserved.
dnl This file is offered as-is, without any warranty.

# Check for nullptr that conforms to C23 and C++11.

AC_DEFUN([gl_NULLPTR],
[
  m4_provide_if([AC_PROG_CC],
    [AC_LANG_PUSH([C])
     AC_CACHE_CHECK([for C nullptr], [gl_cv_c_nullptr],
       [AC_COMPILE_IFELSE(
          [AC_LANG_SOURCE([[int *p = nullptr;]])],
          [gl_cv_c_nullptr=yes
           # Work around <https://gcc.gnu.org/bugzilla/show_bug.cgi?id=114780>.
           gl_saved_CFLAGS=$CFLAGS
           CFLAGS="$CFLAGS -Wall -Werror"
           AC_COMPILE_IFELSE(
             [AC_LANG_PROGRAM(
                [[void f (char const *, ...) __attribute__ ((sentinel));]],
                [[f ("", nullptr);]])],
             [],
             [AC_COMPILE_IFELSE(
                [AC_LANG_PROGRAM(
                   [[void f (char const *, ...) __attribute__ ((sentinel));]],
                   [[f ("", (void *) 0);]])],
                [gl_cv_c_nullptr='not as a sentinel'])])
           CFLAGS=$gl_saved_CFLAGS],
          [gl_cv_c_nullptr=no])])
      gl_c_nullptr=$gl_cv_c_nullptr
      AC_LANG_POP([C])],
     [gl_c_nullptr=no])
  if test "$gl_c_nullptr" = yes; then
    AC_DEFINE([HAVE_C_NULLPTR], [1],
      [Define to 1 if C nullptr is known to work.])
  fi

  m4_provide_if([AC_PROG_CXX],
    [AC_LANG_PUSH([C++])
     AC_CACHE_CHECK([for C++ nullptr], [gl_cv_cxx_nullptr],
       [AC_COMPILE_IFELSE(
          [AC_LANG_SOURCE([[int *p = nullptr;]])],
          [gl_cv_cxx_nullptr=yes],
          [AC_COMPILE_IFELSE(
             [AC_LANG_SOURCE([[#include <stddef.h>
                               int *p = nullptr;]])],
             [gl_cv_cxx_nullptr="yes, but it is a <stddef.h> macro"],
             [gl_cv_cxx_nullptr=no])])])
     AS_CASE([$gl_cv_cxx_nullptr],
       [yes],  [gl_have_cxx_nullptr=1],
       [yes*], [gl_have_cxx_nullptr="(-1)"],
               [gl_have_cxx_nullptr=0])
     AC_DEFINE_UNQUOTED([HAVE_CXX_NULLPTR], [$gl_have_cxx_nullptr],
                        [Define to 1 if C++ nullptr works, 0 if not,
                         (-1) if it is a <stddef.h> macro.])
     AC_LANG_POP([C++])])
])

  AH_VERBATIM([zznullptr],
[#if defined __cplusplus && HAVE_CXX_NULLPTR < 0
# include <stddef.h>
# undef/**/nullptr
#endif
#ifndef nullptr
# if !defined __cplusplus && !defined HAVE_C_NULLPTR
#  define nullptr ((void *) 0)
# elif defined __cplusplus && HAVE_CXX_NULLPTR <= 0
#  if 3 <= __GNUG__
#   define nullptr __null
#  else
#   define nullptr 0L
#  endif
# endif
#endif])
])
