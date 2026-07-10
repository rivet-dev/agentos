# Customize maint.mk                           -*- makefile -*-
# Copyright (C) 2009-2025 Free Software Foundation, Inc.

# This program is free software: you can redistribute it and/or modify
# it under the terms of the GNU General Public License as published by
# the Free Software Foundation, either version 3 of the License, or
# (at your option) any later version.

# This program is distributed in the hope that it will be useful,
# but WITHOUT ANY WARRANTY; without even the implied warranty of
# MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE.  See the
# GNU General Public License for more details.

# You should have received a copy of the GNU General Public License
# along with this program.  If not, see <https://www.gnu.org/licenses/>.

# Cause the tool(s) built by this package to be used also when running
# commands via e.g., "make syntax-check".  Doing this a little sooner
# would have avoided a grep infloop bug.
ifeq ($(build_triplet), $(host_triplet))
export PATH := $(builddir)/src$(PATH_SEPARATOR)$(PATH)
endif

# Used in maint.mk's web-manual rule
manual_title = GNU Grep: Print lines matching a pattern

# Use the direct link.  This is guaranteed to work immediately, while
# it can take a while for the faster mirror links to become usable.
url_dir_list = https://ftp.gnu.org/gnu/$(PACKAGE)

# Tests not to run as part of "make distcheck".
local-checks-to-skip =			\
  sc_indent				\
  sc_texinfo_acronym			\
  sc_unportable_grep_q

# Tools used to bootstrap this package, used for "announcement".
bootstrap-tools = autoconf,automake,gnulib

# Override the default Cc: used in generating an announcement.
announcement_Cc_ = $(translation_project_), $(PACKAGE)-devel@gnu.org

# The tight_scope test gets confused about inline functions.
# like 'to_uchar'.
_gl_TS_unmarked_extern_functions = \
  main usage mb_clen to_uchar dfaerror dfawarn imbrlen

# Write base64-encoded (not hex) checksums into the announcement.
announce_gen_args = --cksum-checksums

# Add an exemption for sc_makefile_at_at_check.
_makefile_at_at_check_exceptions = ' && !/MAKEINFO/'

# Now that we have better tests, make this the default.
export VERBOSE = yes

# Comparing tarball sizes compressed using different xz presets, we see
# that -6e adds only 60 bytes to the size of the tarball, yet reduces
# (from -9) the decompression memory requirement from 64 MiB to 9 MiB.
# Don't be tempted by -5e, since -6 and -5 use the same dictionary size.
# $ for i in {4,5,6,7,8,9}{e,}; do \
#     (n=$(xz -$i < grep-2.11.tar|wc -c);echo $n $i) & done |sort -nr
# 1236632 4
# 1162564 5
# 1140988 4e
# 1139620 6
# 1139480 7
# 1139480 8
# 1139480 9
# 1129552 5e
# 1127616 6e
# 1127556 7e
# 1127556 8e
# 1127556 9e
export XZ_OPT = -6e

old_NEWS_hash = 2067ccba9ea30b38ca930e1098ef9364

# We prefer to spell it back-reference, as POSIX does.
sc_prohibit_backref:
	@prohibit=back''reference					\
	halt='spell it "back-reference"'				\
	  $(_sc_search_regexp)

# Many m4 macros names once began with 'jm_'.
# Make sure that none are inadvertently reintroduced.
sc_prohibit_jm_in_m4:
	@grep -nE 'jm_[A-Z]'						\
		$$($(VC_LIST) m4 |grep '\.m4$$'; echo /dev/null) &&	\
	    { echo '$(ME): do not use jm_ in m4 macro names'		\
	      1>&2; exit 1; } || :

sc_prohibit_echo_minus_en:
	@prohibit='\<echo -[en]'					\
	halt='do not use echo ''-e or echo ''-n; use printf instead'	\
	  $(_sc_search_regexp)

# Look for lines longer than 80 characters, except omit:
# - program-generated long lines in diff headers,
# - the help2man script copied from upstream,
# - tests involving long checksum lines, and
# - the 'pr' test cases.
LINE_LEN_MAX = 80
FILTER_LONG_LINES =							\
  /^[^:]*\.diff:[^:]*:@@ / d;						\
  \|^[^:]*TODO:| d;							\
  \|^[^:]*doc/fdl.texi:| d;						\
  \|^[^:]*man/help2man:| d;						\
  \|^[^:]*tests/misc/sha[0-9]*sum.*\.pl[-:]| d;				\
  \|^[^:]*tests/pr/|{ \|^[^:]*tests/pr/pr-tests:| !d; };
sc_long_lines:
	@files=$$($(VC_LIST_EXCEPT))					\
	halt='line(s) with more than $(LINE_LEN_MAX) characters; reindent'; \
	for file in $$files; do						\
	  expand $$file | grep -nE '^.{$(LINE_LEN_MAX)}.' |		\
	  sed -e "s|^|$$file:|" -e '$(FILTER_LONG_LINES)';		\
	done | grep . && { msg="$$halt" $(_sc_say_and_exit) } || :

# Indent only with spaces.
sc_prohibit_tab_based_indentation:
	@prohibit='^ *	'						\
	halt='TAB in indentation; use only spaces'			\
	  $(_sc_search_regexp)

# Don't use "indent-tabs-mode: nil" anymore.  No longer needed.
sc_prohibit_emacs__indent_tabs_mode__setting:
	@prohibit='^( *[*#] *)?indent-tabs-mode:'			\
	halt='use of emacs indent-tabs-mode: setting'			\
	  $(_sc_search_regexp)

# Ensure that the list of test file names in tests/Makefile.am is sorted.
sc_sorted_tests:
	@perl -0777 -ne \
	    '/^TESTS =(.*?)^$$/ms; ($$t = $$1) =~ s/[\\\s\n]+/\n/g;print $$t' \
	  tests/Makefile.am | sort -c

# THANKS.in is a list of name/email pairs for people who are mentioned in
# commit logs (and generated ChangeLog), but who are not also listed as an
# author of a commit.  Name/email pairs of commit authors are automatically
# extracted from the repository.  As a very minor factorization, when
# someone who was initially listed only in THANKS.in later authors a commit,
# this rule detects that their pair may now be removed from THANKS.in.
sc_THANKS_in_duplicates:
	@{ git log --pretty=format:%aN | sort -u;			\
	    cut -b-36 THANKS.in | sed '/^$$/d;s/  *$$//'; }		\
	  | sort | uniq -d | grep .					\
	    && { echo '$(ME): remove the above names from THANKS.in'	\
		  1>&2; exit 1; } || :

# Ensure that tests don't use `cmd ... && fail=1` as that hides crashes.
# The "exclude" expression allows common idioms like `test ... && fail=1`
# and the 2>... portion allows commands that redirect stderr and so probably
# independently check its contents and thus detect any crash messages.
sc_prohibit_and_fail_1:
	@prohibit='&& fail=1'						\
	exclude='(stat|kill|test |EGREP|grep|compare|2> *[^/])'		\
	halt='&& fail=1 detected. Please use: returns_ 1 ... || fail=1'	\
	in_vc_files='^tests/'						\
	  $(_sc_search_regexp)

update-copyright-env = \
  UPDATE_COPYRIGHT_USE_INTERVALS=1 \
  UPDATE_COPYRIGHT_MAX_LINE_LENGTH=79

include $(abs_top_srcdir)/dist-check.mk

exclude_file_name_regexp--sc_bindtextdomain = \
  ^tests/get-mb-cur-max\.c$$

exclude_file_name_regexp--sc_prohibit_strcmp = /colorize-.*\.c$$
exclude_file_name_regexp--sc_prohibit_xalloc_without_use = ^src/kwset\.c$$
exclude_file_name_regexp--sc_prohibit_tab_based_indentation = \
  (Makefile|\.(am|mk)$$)

exclude_file_name_regexp--sc_prohibit_doubled_word = ^tests/count-newline$$

exclude_file_name_regexp--sc_long_lines = ^tests/.*$$

# If a test uses timeout, it must also use require_timeout_.
# Grandfather-exempt the fedora test, since it ensures timeout works
# as expected before using it.
sc_timeout_prereq:
	@$(VC_LIST_EXCEPT)						\
	  | grep '^tests/'						\
	  | grep -v '^tests/fedora$$'					\
	  | xargs grep -lw timeout					\
	  | xargs grep -FLw require_timeout_				\
	  | $(GREP) .							\
	  && { echo '$(ME): timeout without use of require_timeout_'	\
	    1>&2; exit 1; } || :

codespell_ignore_words_list = clen,allo,Nd,abd,alph,debbugs,wee,UE,ois,creche
