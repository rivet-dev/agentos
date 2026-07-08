# Process this file with automake to create Makefile.in
# Makefile.am for grep/doc.
#
# Copyright 2008-2025 Free Software Foundation, Inc.
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

# The customization variable CHECK_NORMAL_MENU_STRUCTURE is necessary with
# makeinfo versions ≥ 6.8.
MAKEINFO = @MAKEINFO@ -c CHECK_NORMAL_MENU_STRUCTURE=1

info_TEXINFOS = grep.texi
grep_TEXINFOS = fdl.texi

man_MANS = grep.1

EXTRA_DIST = grep.in.1
CLEANFILES = grep.1

grep.1: grep.in.1
	$(AM_V_GEN)rm -f $@-t $@
	$(AM_V_at)sed 's/@''VERSION@/$(VERSION)/' $(srcdir)/grep.in.1 > $@-t
	$(AM_V_at)chmod a=r $@-t
	$(AM_V_at)mv -f $@-t $@
