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

LN = ln

AM_CFLAGS = $(WARN_CFLAGS) $(WERROR_CFLAGS) $(PCRE_CFLAGS)

# Tell the linker to omit references to unused shared libraries.
AM_LDFLAGS = $(IGNORE_UNUSED_LIBRARIES_CFLAGS)

bin_PROGRAMS = grep
bin_SCRIPTS = egrep fgrep
grep_SOURCES =					\
  dfasearch.c					\
  die.h						\
  grep.c					\
  kwsearch.c					\
  kwset.c					\
  searchutils.c
if USE_PCRE
grep_SOURCES += pcresearch.c
endif

noinst_HEADERS = grep.h kwset.h search.h system.h

# Sometimes, the expansion of $(LIBINTL) includes -lc which may
# include modules defining variables like 'optind', so libgreputils.a
# must precede $(LIBINTL) in order to ensure we use GNU getopt.
# But libgreputils.a must also follow $(LIBINTL), since libintl uses
# replacement functions defined in libgreputils.a.
LDADD = \
  ../lib/libgreputils.a $(LIBINTL) ../lib/libgreputils.a \
  $(HARD_LOCALE_LIB) $(LIBC32CONV) \
  $(LIBSIGSEGV) $(LIBUNISTRING) $(MBRTOWC_LIB) $(SETLOCALE_NULL_LIB) \
  $(LIBTHREAD)

grep_LDADD = $(LDADD) $(PCRE_LIBS) $(LIBCSTACK)
localedir = $(datadir)/locale
AM_CPPFLAGS = -I$(top_builddir)/lib -I$(top_srcdir)/lib

EXTRA_DIST = egrep.sh

egrep fgrep: egrep.sh Makefile
	$(AM_V_GEN)grep=`echo grep | sed -e '$(transform)'` &&		\
	case $@ in egrep) option=-E;; fgrep) option=-F;; esac &&	\
	shell_does_substrings='set x/y && d=$${1##*/} && test "$$d" = y' && \
	if $(SHELL) -c "$$shell_does_substrings" 2>/dev/null; then	\
	  edit_substring='s,X,X,';					\
	else								\
	  edit_substring='s,\$${0##\*/},`expr "X$$0" : '\''X\\(.*\\)/'\''`,g'; \
	fi &&								\
	sed -e 's|[@]SHELL@|$(SHELL)|g'					\
	    -e "$$edit_substring"					\
	    -e "s|[@]grep@|$$grep|g"					\
	    -e "s|[@]option@|$$option|g" <$(srcdir)/egrep.sh >$@-t
	$(AM_V_at)chmod +x $@-t
	$(AM_V_at)mv $@-t $@

CLEANFILES = egrep fgrep *-t
