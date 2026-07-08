dnl
dnl autoconf input file for GNU grep
dnl
dnl Copyright (C) 1997-2006, 2009-2025 Free Software Foundation, Inc.
dnl
dnl This file is part of GNU grep.
dnl
dnl This program is free software; you can redistribute it and/or modify
dnl it under the terms of the GNU General Public License as published by
dnl the Free Software Foundation; either version 3, or (at your option)
dnl any later version.
dnl
dnl This program is distributed in the hope that it will be useful,
dnl but WITHOUT ANY WARRANTY; without even the implied warranty of
dnl MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE.  See the
dnl GNU General Public License for more details.
dnl
dnl You should have received a copy of the GNU General Public License
dnl along with this program.  If not, see <https://www.gnu.org/licenses/>.

AC_INIT([GNU grep],
        m4_esyscmd([build-aux/git-version-gen .tarball-version]),
        [bug-grep@gnu.org])

if test -n "$GREP" || test -n "$EGREP"; then
  AC_MSG_ERROR(
    [no working 'grep' found
  A working 'grep' command is needed to build GNU Grep.
  This 'grep' should support -e and long lines.
  On Solaris 10, install the package SUNWggrp or SUNWxcu4.
  On Solaris 11, install the package text/gnu-grep or system/xopen/xcu4.])
fi

AC_CONFIG_AUX_DIR([build-aux])
AC_CONFIG_SRCDIR([src/grep.c])
AC_DEFINE([GREP], 1, [We are building grep])
AC_PREREQ([2.64])
AC_CONFIG_MACRO_DIRS([m4])

dnl Automake stuff.
AM_INIT_AUTOMAKE([1.11 dist-xz color-tests parallel-tests
                  subdir-objects])
AM_SILENT_RULES([yes]) # make --enable-silent-rules the default.

AC_CONFIG_HEADERS([config.h:config.hin])

dnl Checks for programs.
AC_CANONICAL_HOST
AC_PROG_AWK
AC_PROG_INSTALL
AC_PROG_CC
gl_EARLY
AC_PROG_RANLIB
PKG_PROG_PKG_CONFIG([0.9.0])

# grep never invokes mbrtowc or mbrlen on empty input,
# so don't worry about this common bug,
# as working around it would merely slow grep down.
gl_cv_func_mbrtowc_empty_input='assume yes'
gl_cv_func_mbrlen_empty_input='assume yes'

dnl Checks for typedefs, structures, and compiler characteristics.
gl_INIT

# Ensure VLAs are not used.
# Note -Wvla is implicitly added by gl_MANYWARN_ALL_GCC
AC_DEFINE([GNULIB_NO_VLA], [1], [Define to 1 to disable use of VLAs])

# The test suite needs to know if we have a working perl.
AM_CONDITIONAL([HAVE_PERL], [test "$gl_cv_prog_perl" != no])

# gl_GCC_VERSION_IFELSE([major], [minor], [run-if-found], [run-if-not-found])
# ------------------------------------------------
# If $CPP is gcc-MAJOR.MINOR or newer, then run RUN-IF-FOUND.
# Otherwise, run RUN-IF-NOT-FOUND.
AC_DEFUN([gl_GCC_VERSION_IFELSE],
  [AC_PREPROC_IFELSE(
    [AC_LANG_PROGRAM(
      [[
#if ($1) < __GNUC__ || (($1) == __GNUC__ && ($2) <= __GNUC_MINOR__)
/* ok */
#else
# error "your version of gcc is older than $1.$2"
#endif
      ]]),
    ], [$3], [$4])
  ]
)

AC_ARG_ENABLE([gcc-warnings],
  [AS_HELP_STRING([--enable-gcc-warnings@<:@=TYPE@:>@],
    [control generation of GCC warnings.  The TYPE 'no' disables
     warnings (default for non-developer builds); 'yes' generates
     cheap warnings if available (default for developer builds);
     'expensive' in addition generates expensive-to-compute warnings
     if available.])],
  [case $enableval in
     no|yes|expensive) ;;
     *)      AC_MSG_ERROR([bad value $enableval for gcc-warnings option]) ;;
   esac
   gl_gcc_warnings=$enableval],
  [
   # GCC provides fine-grained control over diagnostics which
   # is used in gnulib for example to suppress warnings from
   # certain sections of code.  So if this is available and
   # we're running from a git repo, then auto enable the warnings.
   gl_gcc_warnings=no
   gl_GCC_VERSION_IFELSE([4], [6],
                         [test -d "$srcdir"/.git \
                          && ! test -f "$srcdir"/.tarball-version \
                          && gl_gcc_warnings=yes])]
)

if test $gl_gcc_warnings != no; then
  gl_WARN_ADD([-Werror], [WERROR_CFLAGS])
  AC_SUBST([WERROR_CFLAGS])

  ew=
  AS_IF([test $gl_gcc_warnings != expensive],
    [# -fanalyzer and related options slow GCC considerably.
     ew="$ew -fanalyzer -Wno-analyzer-double-free -Wno-analyzer-malloc-leak"
     ew="$ew -Wno-analyzer-null-dereference -Wno-analyzer-use-after-free"])

  nw=$ew
  # This, $nw, is the list of warnings we disable.
  nw="$nw -Wvla"                    # suppress a warning in regexec.h
  nw="$nw -Winline"                 # suppress warnings from streq.h's streq5
  nw="$nw -Wsystem-headers"         # Don't let system headers trigger warnings
  nw="$nw -Wstack-protector"        # generates false alarms for useful code

  gl_MANYWARN_ALL_GCC([ws])
  gl_MANYWARN_COMPLEMENT([ws], [$ws], [$nw])
  for w in $ws; do
    gl_WARN_ADD([$w])
  done
  gl_WARN_ADD([-Wno-missing-field-initializers]) # We need this one
  gl_WARN_ADD([-Wno-sign-compare])     # Too many warnings for now
  gl_WARN_ADD([-Wno-unused-parameter]) # Too many warnings for now
  gl_WARN_ADD([-Wno-cast-function-type]) # sig-handler.h's sa_handler_t cast
  gl_WARN_ADD([-Wno-deprecated-declarations]) # clang complains about sprintf

  # In spite of excluding -Wlogical-op above, it is enabled, as of
  # gcc 4.5.0 20090517, and it provokes warnings in cat.c, dd.c, truncate.c
  gl_WARN_ADD([-Wno-logical-op])

  AC_SUBST([WARN_CFLAGS])

  AC_DEFINE([lint], [1], [Define to 1 if the compiler is checking for lint.])
  AC_DEFINE([GNULIB_PORTCHECK], [1], [enable some gnulib portability checks])
  AH_VERBATIM([GNULIB_PORTCHECK_FORTIFY_SOURCE],
  [/* Enable compile-time and run-time bounds-checking, and some warnings,
      without upsetting glibc 2.15+. */
   #if (defined GNULIB_PORTCHECK && !defined _FORTIFY_SOURCE \
        && defined __OPTIMIZE__ && __OPTIMIZE__)
   # define _FORTIFY_SOURCE 2
   #endif
  ])

  # We use a slightly smaller set of warning options for lib/.
  # Remove the following and save the result in GNULIB_WARN_CFLAGS.
  nw=
  nw="$nw -Wunused-macros"
  gl_WARN_ADD([-Wno-format-nonliteral])
  gl_MANYWARN_COMPLEMENT([GNULIB_WARN_CFLAGS], [$WARN_CFLAGS], [$nw])
  AC_SUBST([GNULIB_WARN_CFLAGS])

  # For gnulib-tests, the set is slightly smaller still.
  # It's not worth being this picky about test programs.
  nw=
  nw="$nw -Wformat-truncation=2"    # False alarm in strerror_r.c
  nw="$nw -Wmissing-declarations"
  nw="$nw -Wmissing-prototypes"
  nw="$nw -Wmissing-variable-declarations"
  nw="$nw -Wnull-dereference"
  nw="$nw -Wold-style-definition"
  nw="$nw -Wstrict-prototypes"
  nw="$nw -Wsuggest-attribute=cold"
  nw="$nw -Wsuggest-attribute=const"
  nw="$nw -Wsuggest-attribute=format"
  nw="$nw -Wsuggest-attribute=pure"

  # Disable to avoid warnings in e.g., test-intprops.c and test-limits-h.c
  # due to overlong expansions like this:
  # test-intprops.c:147:5: error: string literal of length 9531 exceeds \
  # maximum length 4095 that ISO C99 compilers are required to support
  nw="$nw -Woverlength-strings"

  gl_MANYWARN_COMPLEMENT([GNULIB_TEST_WARN_CFLAGS],
                         [$GNULIB_WARN_CFLAGS], [$nw])
  gl_WARN_ADD([-Wno-return-type], [GNULIB_TEST_WARN_CFLAGS])
  AC_SUBST([GNULIB_TEST_WARN_CFLAGS])
fi

# By default, argmatch should fail calling usage (EXIT_FAILURE).
AC_DEFINE([ARGMATCH_DIE], [usage (EXIT_FAILURE)],
          [Define to the function xargmatch calls on failures.])
AC_DEFINE([ARGMATCH_DIE_DECL], [void usage (int _e)],
          [Define to the declaration of the xargmatch failure function.])

AC_CHECK_FUNCS_ONCE([setlocale])

dnl I18N feature
AM_GNU_GETTEXT_VERSION([0.18.2])
AM_GNU_GETTEXT([external])

dnl Some installers want to be informed if we do not use our regex.
dnl For example, if the host platform uses dynamic linking and the installer
dnl knows that the grep may be invoked on other hosts with buggy libraries,
dnl then the installer should configure --with-included-regex.
AM_CONDITIONAL([USE_INCLUDED_REGEX], [test "$ac_use_included_regex" = yes])
if test "$ac_use_included_regex" = no; then
  AC_MSG_WARN([Included lib/regex.c not used])
else
  AC_DEFINE([USE_INCLUDED_REGEX], 1, [building with included regex code])
fi

gl_FUNC_PCRE
AM_CONDITIONAL([USE_PCRE], [test $use_pcre = yes])

case $host_os in
  mingw*) suffix=w32 ;;
  *) suffix=posix ;;
esac
COLORIZE_SOURCE=colorize-$suffix.c
AC_SUBST([COLORIZE_SOURCE])

AC_CONFIG_FILES([
  Makefile
  lib/Makefile
  src/Makefile
  tests/Makefile
  po/Makefile.in
  doc/Makefile
  gnulib-tests/Makefile
])
AC_OUTPUT
