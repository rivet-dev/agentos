# pcre.m4 - check for PCRE library support

# Copyright (C) 2010-2025 Free Software Foundation, Inc.
# This file is free software; the Free Software Foundation
# gives unlimited permission to copy and/or distribute it,
# with or without modifications, as long as this notice is preserved.

AC_DEFUN([gl_FUNC_PCRE],
[
  AC_ARG_ENABLE([perl-regexp],
    AS_HELP_STRING([--disable-perl-regexp],
                   [disable perl-regexp (PCRE) support]),
    [case $enableval in
       yes|no) test_pcre=$enableval;;
       *) AC_MSG_ERROR([invalid value $enableval for --disable-perl-regexp]);;
     esac],
    [test_pcre=maybe])

  AC_SUBST([PCRE_CFLAGS])
  AC_SUBST([PCRE_LIBS])
  use_pcre=no

  if test $test_pcre != no; then

    if test -z "${PCRE_CFLAGS+set}" && test -z "${PCRE_LIBS+set}"; then
      PKG_CHECK_MODULES([PCRE], [libpcre2-8], [], [: ${PCRE_LIBS=-lpcre2-8}])
    fi

    AC_CACHE_CHECK([for pcre2_compile], [pcre_cv_have_pcre2_compile],
      [pcre_saved_CFLAGS=$CFLAGS
       pcre_saved_LIBS=$LIBS
       CFLAGS="$CFLAGS $PCRE_CFLAGS"
       LIBS="$PCRE_LIBS $LIBS"
       AC_LINK_IFELSE(
         [AC_LANG_PROGRAM([[#define PCRE2_CODE_UNIT_WIDTH 8
                            #include <pcre2.h>
                          ]],
            [[pcre2_code *p = pcre2_compile (0, 0, 0, 0, 0, 0);
              return !p;]])],
         [pcre_cv_have_pcre2_compile=yes],
         [pcre_cv_have_pcre2_compile=no])
       CFLAGS=$pcre_saved_CFLAGS
       LIBS=$pcre_saved_LIBS])

    if test "$pcre_cv_have_pcre2_compile" = yes; then
      use_pcre=yes
    elif test $test_pcre = maybe; then
      AC_MSG_WARN([AC_PACKAGE_NAME will be built without PCRE support.])
    else
      AC_MSG_ERROR([PCRE support not available])
    fi
  fi

  if test $use_pcre = yes; then
    AC_DEFINE([HAVE_LIBPCRE], [1],
      [Define to 1 if you have the Perl Compatible Regular Expressions
       library.])
  else
    PCRE_CFLAGS=
    PCRE_LIBS=
  fi
])
