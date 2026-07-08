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

AM_CFLAGS =
AM_CPPFLAGS =
BUILT_SOURCES =
CLEANFILES =
EXTRA_DIST =
MAINTAINERCLEANFILES =
MOSTLYCLEANDIRS =
MOSTLYCLEANFILES =
SUFFIXES =
noinst_LIBRARIES =

include gnulib.mk

AM_CFLAGS += $(GNULIB_WARN_CFLAGS) $(WERROR_CFLAGS)

nodist_libgreputils_a_SOURCES = colorize.c
libgreputils_a_SOURCES += colorize.h
EXTRA_DIST += colorize-posix.c colorize-w32.c
CLEANFILES += colorize.c
colorize.c:
	$(AM_V_at)echo '#include <$(COLORIZE_SOURCE)>' >$@-t
	$(AM_V_at)mv $@-t $@

libgreputils_a_LIBADD += $(LIBOBJS) $(ALLOCA)
libgreputils_a_DEPENDENCIES += $(LIBOBJS) $(ALLOCA)
