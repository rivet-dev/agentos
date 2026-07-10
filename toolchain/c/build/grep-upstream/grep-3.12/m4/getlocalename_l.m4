# getlocalename_l.m4
# serial 3
dnl Copyright (C) 2025 Free Software Foundation, Inc.
dnl This file is free software; the Free Software Foundation
dnl gives unlimited permission to copy and/or distribute it,
dnl with or without modifications, as long as this notice is preserved.
dnl This file is offered as-is, without any warranty.

AC_DEFUN([gl_FUNC_GETLOCALENAME_L_SIMPLE],
[
  AC_REQUIRE([gl_LOCALE_H_DEFAULTS])

  dnl Persuade glibc <locale.h> to declare getlocalename_l().
  AC_REQUIRE([AC_USE_SYSTEM_EXTENSIONS])

  AC_REQUIRE([gl_FUNC_GETLOCALENAME_L_UNSAFE])
])

# Prerequisites of lib/getlocalename_l.c.
AC_DEFUN([gl_PREREQ_GETLOCALENAME_L_SIMPLE],
[
  :
])

AC_DEFUN_ONCE([gl_FUNC_GETLOCALENAME_L_UNSAFE],
[
  AC_REQUIRE([gl_FUNC_SETLOCALE_NULL])
  AC_CHECK_FUNCS_ONCE([getlocalename_l])
  if test $ac_cv_func_getlocalename_l = yes; then
    dnl Check against the Cygwin 3.6.0 bug: It returns an invalid pointer when
    dnl the second argument is LC_GLOBAL_LOCALE.
    AC_REQUIRE([AC_CANONICAL_HOST])
    AC_CACHE_CHECK([whether getlocalename_l works],
      [gl_cv_func_getlocalename_l_works],
      [AC_RUN_IFELSE(
         [AC_LANG_SOURCE([[
#include <locale.h>
#include <string.h>
int main ()
{
  const char *ret = getlocalename_l (LC_COLLATE, LC_GLOBAL_LOCALE);
  return strlen (ret) == 0;
}]])],
         [gl_cv_func_getlocalename_l_works=yes],
         [gl_cv_func_getlocalename_l_works=no],
         [case "$host_os" in
            cygwin*) # Guess no on Cygwin.
              gl_cv_func_getlocalename_l_works="guessing no" ;;
            *)       # Guess yes otherwise.
              gl_cv_func_getlocalename_l_works="guessing yes" ;;
          esac
         ])
      ])
    case "$gl_cv_func_getlocalename_l_works" in
      *yes) ;;
      *) REPLACE_GETLOCALENAME_L=1 ;;
    esac
  else
    HAVE_GETLOCALENAME_L=0
  fi
  if test $HAVE_GETLOCALENAME_L = 0 || test $REPLACE_GETLOCALENAME_L = 1; then
    GETLOCALENAME_L_LIB="$SETLOCALE_NULL_LIB"
  else
    GETLOCALENAME_L_LIB=
  fi
  dnl GETLOCALENAME_L_LIB is expected to be '-pthread' or '-lpthread' on AIX
  dnl with gcc or xlc, and empty otherwise.
  AC_SUBST([GETLOCALENAME_L_LIB])
])

# Prerequisites of lib/getlocalename_l-unsafe.c.
AC_DEFUN([gl_PREREQ_GETLOCALENAME_L_UNSAFE],
[
  AC_REQUIRE([gl_LOCALE_H_DEFAULTS])
  AC_REQUIRE([gl_LOCALE_T])
  AC_REQUIRE([gt_INTL_THREAD_LOCALE_NAME])
  AC_CHECK_HEADERS_ONCE([langinfo.h])
  if test $HAVE_LOCALE_T = 1; then
    gl_CHECK_FUNCS_ANDROID([newlocale], [[#include <locale.h>]])
    gl_CHECK_FUNCS_ANDROID([duplocale], [[#include <locale.h>]])
    gl_CHECK_FUNCS_ANDROID([freelocale], [[#include <locale.h>]])
    gl_func_newlocale="$ac_cv_func_newlocale"
    gl_func_duplocale="$ac_cv_func_duplocale"
    gl_func_freelocale="$ac_cv_func_freelocale"
  else
    dnl In 2019, some versions of z/OS lack the locale_t type and have broken
    dnl newlocale, duplocale, freelocale functions.
    gl_cv_onwards_func_newlocale='future OS version'
    gl_cv_onwards_func_duplocale='future OS version'
    gl_cv_onwards_func_freelocale='future OS version'
    gl_func_newlocale=no
    gl_func_duplocale=no
    gl_func_freelocale=no
  fi
  if test $gl_func_newlocale != yes; then
    HAVE_NEWLOCALE=0
    case "$gl_cv_onwards_func_newlocale" in
      future*) REPLACE_NEWLOCALE=1 ;;
    esac
  fi
  if test $gl_func_duplocale != yes; then
    HAVE_DUPLOCALE=0
    case "$gl_cv_onwards_func_duplocale" in
      future*) REPLACE_DUPLOCALE=1 ;;
    esac
  fi
  if test $gl_func_freelocale != yes; then
    HAVE_FREELOCALE=0
    case "$gl_cv_onwards_func_freelocale" in
      future*) REPLACE_FREELOCALE=1 ;;
    esac
  fi
  if test $gt_localename_enhances_locale_funcs = yes; then
    REPLACE_NEWLOCALE=1
    REPLACE_DUPLOCALE=1
    REPLACE_FREELOCALE=1
  fi
])
