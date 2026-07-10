# perl.m4
# serial 11
dnl Copyright (C) 1998-2001, 2003-2004, 2007, 2009-2025 Free Software
dnl Foundation, Inc.
dnl This file is free software; the Free Software Foundation
dnl gives unlimited permission to copy and/or distribute it,
dnl with or without modifications, as long as this notice is preserved.
dnl This file is offered as-is, without any warranty.

dnl From Jim Meyering.
dnl Find a new-enough version of Perl.

AC_DEFUN([gl_PERL],
[
  dnl FIXME: don't hard-code 5.005
AC_CACHE_CHECK([for Perl 5.005 or newer],
 [gl_cv_prog_perl],
 [
  if test "${PERL+set}" = set; then
    # 'PERL' is set in the user's environment.
    candidate_perl_names="$PERL"
    perl_specified=yes
  else
    candidate_perl_names='perl perl5'
    perl_specified=no
  fi

  gl_cv_prog_perl=no
  for perl in $candidate_perl_names; do
    # Run test in a subshell; some versions of sh will print an error if
    # an executable is not found, even if stderr is redirected.
    if ( $perl -e 'require 5.005; use File::Compare; use warnings;' ) > /dev/null 2>&1; then
      gl_cv_prog_perl=$perl
      break
    fi
  done
 ])

if test "$gl_cv_prog_perl" != no; then
  PERL=$gl_cv_prog_perl
else
  PERL="$am_missing_run perl"
  AC_MSG_WARN([
WARNING: You don't seem to have perl5.005 or newer installed, or you lack
         a usable version of the Perl File::Compare module.  As a result,
         you may be unable to run a few tests or to regenerate certain
         files if you modify the sources from which they are derived.
] )
fi

AC_SUBST([PERL])

])
