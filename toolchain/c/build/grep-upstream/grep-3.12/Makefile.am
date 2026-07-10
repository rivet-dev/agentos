# Process this file with automake to create Makefile.in
#
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
#
AUTOMAKE_OPTIONS = gnu 1.12

SUBDIRS = po lib doc src tests gnulib-tests

EXTRA_DIST =		\
  .mailmap		\
  ChangeLog-2009	\
  dist-check.mk		\
  README		\
  README-alpha		\
  THANKS.in		\
  TODO			\
  cfg.mk		\
  thanks-gen

# Shortcut targets to make it easier to run expensive tests.
.PHONY: check-expensive
check-expensive:
	$(MAKE) check RUN_EXPENSIVE_TESTS=yes
.PHONY: check-very-expensive
check-very-expensive: check-expensive

# Run syntax-check rules before creating a distribution tarball.
.PHONY: run-syntax-check
run-syntax-check: all
	$(AM_V_GEN)test ! -d .git || $(MAKE) syntax-check

# Arrange so that .tarball-version appears only in the distribution
# tarball, and never in a checked-out repository.
dist-hook: gen-ChangeLog run-syntax-check
	$(AM_V_GEN)echo $(VERSION) > $(distdir)/.tarball-version

gen_start_date = 2009-11-27
.PHONY: gen-ChangeLog
gen-ChangeLog:
	$(AM_V_GEN)if test -d .git; then				\
	  log_fix="$(srcdir)/build-aux/git-log-fix";			\
	  test -e "$$log_fix"						\
	    && amend_git_log="--amend=$$log_fix"			\
	    || amend_git_log=;						\
	  $(top_srcdir)/build-aux/gitlog-to-changelog			\
	    $$amend_git_log --since=$(gen_start_date) > $(distdir)/cl-t && \
	    { rm -f $(distdir)/ChangeLog &&				\
	      mv $(distdir)/cl-t $(distdir)/ChangeLog; }		\
	fi

# Sort in traditional ASCII order, regardless of the current locale;
# otherwise we may get into trouble with distinct strings that the
# current locale considers to be equal.
ASSORT = LC_ALL=C sort

# Extract all lines up to the first one starting with "##".
prologue = perl -ne '/^\#\#/ and exit; print' $(srcdir)/THANKS.in

THANKS: THANKS.in Makefile.am .mailmap thanks-gen
	$(AM_V_GEN)rm -f $@-t $@;					\
	{								\
	  $(prologue); echo;						\
	  { perl -ne '/^$$/.../^$$/ and !/^$$/ and s/  +/\0/ and print'	\
	      $(srcdir)/THANKS.in;					\
	    git log --pretty=format:'%aN%x00%aE'			\
	      | $(ASSORT) -u;						\
	  } | $(srcdir)/thanks-gen					\
	    | LC_ALL=en_US.UTF-8 sort -k1,1;				\
	  echo;								\
	  printf ';; %s\n' 'Local Variables:' 'coding: utf-8' End:;	\
	} > $@-t && chmod a-w $@-t && mv $@-t $@
