## Process this file with automake to create Makefile.in
# Copyright 1997-1998, 2005-2025 Free Software Foundation, Inc.
#
# This program is free software; you can redistribute it and/or modify
# it under the terms of the GNU General Public License as published by
# the Free Software Foundation; either version 3, or (at your option)
# any later version.
#
# This program is distributed in the hope that it will be useful,
# but WITHOUT ANY WARRANTY; without even the implied warranty of
# MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE.  See the
# GNU General Public License for more details.
#
# You should have received a copy of the GNU General Public License
# along with this program.  If not, see <https://www.gnu.org/licenses/>.

TEST_EXTENSIONS = .sh .pl

if HAVE_PERL
TESTSUITE_PERL = $(PERL)
else
TESTSUITE_PERL = $(SHELL) $(srcdir)/no-perl
endif

# Options passed to the perl invocations running the perl test scripts.
TESTSUITE_PERL_OPTIONS = -w -I$(srcdir) -MCoreutils -MCuSkip
# '$f' is set by the Automake-generated test harness to the path of the
# current test script stripped of VPATH components, and is used by the
# CuTmpdir module to determine the name of the temporary files to be
# used.  Note that $f is a shell variable, not a make macro, so the use
# of '$$f' below is correct, and not a typo.
TESTSUITE_PERL_OPTIONS += -M"CuTmpdir qw($$f)"

SH_LOG_COMPILER = $(SHELL)
PL_LOG_COMPILER = $(TESTSUITE_PERL) $(TESTSUITE_PERL_OPTIONS)

check_PROGRAMS = get-mb-cur-max
AM_CPPFLAGS = -I$(top_builddir)/lib -I$(top_srcdir)/lib \
  -I$(top_srcdir)/src
AM_CFLAGS = $(WARN_CFLAGS) $(WERROR_CFLAGS)

# Tell the linker to omit references to unused shared libraries.
AM_LDFLAGS = $(IGNORE_UNUSED_LIBRARIES_CFLAGS)
LDADD = ../lib/libgreputils.a $(LIBINTL) ../lib/libgreputils.a \
  $(HARD_LOCALE_LIB) $(LIBC32CONV) $(LIBCSTACK) \
  $(LIBSIGSEGV) $(LIBUNISTRING) $(MBRTOWC_LIB) $(SETLOCALE_NULL_LIB) \
  $(LIBTHREAD)

# The triple-backref test is expected to fail with both the system
# matcher (i.e., with glibc) and with the included matcher.
# Both matchers need to be fixed.
# FIXME-2025: Remove this once the glibc and gnulib bugs are fixed.
XFAIL_TESTS = triple-backref

# The glibc-infloop test is expected to fail with both the system
# matcher (i.e., with glibc) and with the included matcher.
# Both matchers need to be fixed.
# FIXME-2025: Remove this once the glibc and gnulib bugs are fixed.
XFAIL_TESTS += glibc-infloop

# Equivalence classes are only supported when using the system
# matcher (which means only with glibc).
# The included matcher needs to be fixed.
# FIXME-2025: Remove this once the gnulib bug is fixed.
if USE_INCLUDED_REGEX
XFAIL_TESTS += equiv-classes
else
# The backref-alt test fails for glibc 2.27 and earlier.
# If you're using older glibc you can upgrade to glibc 2.28 or later,
# configure --with-included-regex, or ignore the test failure.
endif

TESTS =						\
  100k-entries					\
  backref					\
  backref-alt					\
  backref-multibyte-slow			\
  backref-word					\
  backslash-dot					\
  backslash-s-and-repetition-operators		\
  backslash-s-vs-invalid-multibyte		\
  big-hole					\
  big-match					\
  binary-file-matches				\
  bogus-wctob					\
  bre						\
  c-locale					\
  case-fold-backref				\
  case-fold-backslash-w				\
  case-fold-char-class				\
  case-fold-char-range				\
  case-fold-char-type				\
  case-fold-titlecase				\
  char-class-multibyte				\
  char-class-multibyte2				\
  color-colors					\
  context-0					\
  count-newline					\
  dfa-coverage					\
  dfa-heap-overrun				\
  dfa-infloop					\
  dfa-invalid-utf8				\
  dfaexec-multibyte				\
  empty						\
  empty-line					\
  empty-line-mb					\
  encoding-error				\
  epipe						\
  equiv-classes					\
  ere						\
  euc-mb					\
  false-match-mb-non-utf8			\
  fedora					\
  fgrep-infloop					\
  fgrep-longest					\
  file						\
  filename-lineno.pl				\
  fillbuf-long-line				\
  fmbtest					\
  foad1						\
  glibc-infloop					\
  grep-dev-null					\
  grep-dev-null-out				\
  grep-dir					\
  hangul-syllable				\
  hash-collision-perf				\
  help-version					\
  high-bit-range				\
  in-eq-out-infloop				\
  include-exclude				\
  inconsistent-range				\
  initial-tab					\
  invalid-multibyte-infloop			\
  khadafy					\
  kwset-abuse					\
  long-line-vs-2GiB-read			\
  long-pattern-perf				\
  many-regex-performance			\
  match-lines					\
  max-count-overread				\
  max-count-vs-context				\
  mb-dot-newline				\
  mb-non-UTF8-overrun				\
  mb-non-UTF8-perf-Fw				\
  mb-non-UTF8-performance			\
  mb-non-UTF8-word-boundary			\
  multibyte-white-space				\
  multiple-begin-or-end-line			\
  null-byte					\
  options					\
  pcre						\
  pcre-abort					\
  pcre-ascii-digits				\
  pcre-context					\
  pcre-count					\
  pcre-infloop					\
  pcre-invalid-utf8-infloop			\
  pcre-invalid-utf8-input			\
  pcre-jitstack					\
  pcre-o					\
  pcre-utf8					\
  pcre-utf8-bug224				\
  pcre-utf8-w					\
  pcre-w					\
  pcre-wx-backref				\
  pcre-z					\
  posix-bracket					\
  prefix-of-multibyte				\
  proc						\
  r-dot						\
  repetition-overflow				\
  reversed-range-endpoints			\
  sjis-mb					\
  skip-device					\
  skip-read					\
  spencer1					\
  spencer1-locale				\
  stack-overflow				\
  status					\
  surrogate-pair				\
  surrogate-search				\
  symlink					\
  triple-backref				\
  turkish-I					\
  turkish-I-without-dot				\
  turkish-eyes					\
  two-chars					\
  two-files					\
  unibyte-binary				\
  unibyte-bracket-expr				\
  unibyte-negated-circumflex			\
  utf8-bracket					\
  version-pcre					\
  warn-char-classes				\
  word-delim-multibyte				\
  word-multi-file				\
  word-multibyte				\
  write-error-msg				\
  y2038-vs-32-bit				\
  yesno						\
  z-anchor-newline

EXTRA_DIST =					\
  $(TESTS)					\
  bre.awk					\
  bre.tests					\
  Coreutils.pm					\
  CuSkip.pm					\
  CuTmpdir.pm					\
  envvar-check					\
  ere.awk					\
  ere.tests					\
  init.cfg					\
  init.sh					\
  khadafy.lines					\
  khadafy.regexp				\
  no-perl					\
  spencer1.awk					\
  spencer1.tests				\
  spencer1-locale.awk

# Default to a nonzero value.  Environment overrides.
# https://udrepper.livejournal.com/11429.html
MALLOC_PERTURB_ = 1

TESTS_ENVIRONMENT =				\
  tmp__=$${TMPDIR-/tmp};			\
  test -d "$$tmp__" && test -w "$$tmp__" || tmp__=.;	\
  . $(srcdir)/envvar-check;			\
  TMPDIR=$$tmp__; export TMPDIR;		\
						\
  if test -n "$$BASH_VERSION" || (eval "export v=x") 2>/dev/null; then \
    export_with_values () { export "$$@"; };		\
  else							\
    export_with_values ()				\
    {							\
      sed_extract_var='s/=.*//';			\
      sed_quote_value="s/'/'\\\\''/g;s/=\\(.*\\)/='\\1'/";\
      for arg in "$$@"; do				\
        var=`echo "$$arg" | sed "$$sed_extract_var"`;	\
        arg=`echo "$$arg" | sed "$$sed_quote_value"`;	\
        eval "$$arg";					\
        export "$$var";					\
      done;						\
    };							\
  fi;							\
							\
  : 'Test egrep/fgrep help if they use our grep.';	\
  grep=`echo grep | sed -e '$(transform)'` || exit;	\
  if test "$$grep" = grep; then				\
    built_programs='grep egrep fgrep';			\
  else							\
    built_programs='grep';				\
  fi;							\
						\
  export_with_values				\
  VERSION='$(VERSION)'				\
  LOCALE_FR='$(LOCALE_FR)'			\
  LOCALE_FR_UTF8='$(LOCALE_FR_UTF8)'		\
  AWK=$(AWK)					\
  LC_ALL=C					\
  abs_top_builddir='$(abs_top_builddir)'	\
  abs_top_srcdir='$(abs_top_srcdir)'		\
  abs_srcdir='$(abs_srcdir)'			\
  built_programs="$$built_programs"		\
  host_triplet='$(host_triplet)'		\
  srcdir='$(srcdir)'				\
  top_srcdir='$(top_srcdir)'			\
  CC='$(CC)'					\
  CONFIG_HEADER='$(abs_top_builddir)/$(CONFIG_INCLUDE)' \
  GREP_TEST_NAME=`echo $$tst|sed 's,^\./,,;s,/,-,g'` \
  MAKE=$(MAKE)					\
  MALLOC_PERTURB_=$(MALLOC_PERTURB_)		\
  PACKAGE_BUGREPORT='$(PACKAGE_BUGREPORT)'	\
  PACKAGE_VERSION=$(PACKAGE_VERSION)		\
  PERL='$(PERL)'				\
  SHELL='$(SHELL)'				\
  PATH='$(abs_top_builddir)/src$(PATH_SEPARATOR)'"$$PATH" \
  ;						\
						\
  : 'set this envvar to indicate whether -P works';	\
  m=0; if err=`echo .|grep -Pq . 2>&1`; then		\
    test -z "$$err" && m=1; fi;				\
  export PCRE_WORKS=$$m;				\
  9>&2

LOG_COMPILER = $(SHELL)

VERBOSE = yes

check: check_executable_TESTS
.PHONY: check_executable_TESTS
check_executable_TESTS: Makefile
	$(AM_V_GEN)fail=0;						\
        cd $(srcdir) && for i in $(TESTS); do				\
	  test -x $$i || { fail=1; echo $$i >&2; };			\
	done;								\
	test $$fail = 1							\
          && { echo the above test scripts are not executable >&2; exit 1; } \
          || :
