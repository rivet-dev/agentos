#!/usr/bin/perl
# Prior to 2.26, an invalid regexp in a -f-specified file would elicit
# a diagnostic like "Unmatched [ or [^", with no indication of the
# file or line number from which the offending regular expression came.
# With 2.26, now, each such diagnostic has a "FILENAME:LINENO: " prefix.

# Copyright (C) 2016-2025 Free Software Foundation, Inc.

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

use strict;

(my $program_name = $0) =~ s|.*/||;

my $prog = 'grep';
my $full_prog_name = `$prog --no-such-option 2>&1`;
$full_prog_name =~ s/:.*//s;
$prog = $full_prog_name if $full_prog_name;

# Turn off localization of executable's output.
@ENV{qw(LANGUAGE LANG LC_ALL)} = ('C') x 3;

# There are at least two variants of one diagnostic:
#   - Unmatched [, [^, [:, [., or [=
#   - Unmatched [ or [^
# Transform each to this: "Unmatched [..."
my $err_subst = {ERR_SUBST => 's/(: Unmatched \[).*/$1.../'};

my $no_pcre = "$prog: Perl matching not supported in a --disable-perl-regexp build\n";

my @Tests =
  (
   # Show that grep now includes filename:lineno in the diagnostic:
   ['invalid-re', '-f g', {AUX=>{g=>"1\n2\n3\n4[[\n"}}, {EXIT=>2},
    $err_subst,
    {ERR => "$prog: g:4: Unmatched [...\n"},
   ],

   # Show that with two or more errors, grep now prints all diagnostics:
   ['invalid-re-2-files', '-f g -f h', {EXIT=>2},
    {AUX=>{g=>"1\n2[[\n3\n4[[\n"}},
    {AUX=>{h=>"5\n6\n7[[\n"}},
    $err_subst,
    {ERR => "$prog: g:2: Unmatched [...\n"
         . "$prog: g:4: Unmatched [...\n"
         . "$prog: h:3: Unmatched [...\n"
    },
   ],

   # Like the above, but on the other lines.
   ['invalid-re-2-files2', '-f g -f h', {EXIT=>2},
    {AUX=>{g=>"1[[\n2\n3[[\n4\n"}},
    {AUX=>{h=>"5[[\n6[[\n7\n"}},
    $err_subst,
    {ERR => "$prog: g:1: Unmatched [...\n"
         . "$prog: g:3: Unmatched [...\n"
         . "$prog: h:1: Unmatched [...\n"
         . "$prog: h:2: Unmatched [...\n"
    },
   ],

   # Make sure the line numbers are right when some regexps are duplicates.
   ['invalid-re-line-numbers', '-f g -f h', {EXIT=>2},
    {AUX=>{g=>"1[[\n\n3[[\n\n5[[\n"}},
    {AUX=>{h=>"1[[\n\n\n4[[\n\n6[[\n"}},
    $err_subst,
    {ERR => "$prog: g:1: Unmatched [...\n"
         . "$prog: g:3: Unmatched [...\n"
         . "$prog: g:5: Unmatched [...\n"
         . "$prog: h:4: Unmatched [...\n"
         . "$prog: h:6: Unmatched [...\n"
    },
   ],

   # Show that with two '-e'-specified erroneous regexps,
   # there is no file name or line number.
   ['invalid-re-2e', '-e "1[[" -e "2[["', {EXIT=>2},
    $err_subst,
    {ERR => "$prog: Unmatched [...\n" x 2},
   ],

   # Test unmatched ) as well.  It is OK with -E and an error with -G and -P.
   ['invalid-re-E-paren', '-E ")"', {IN=>''}, {EXIT=>1}],
   ['invalid-re-E-star-paren', '-E ".*)"', {IN=>''}, {EXIT=>1}],
   ['invalid-re-G-paren', '-G "\\)"', {EXIT=>2},
    {ERR => "$prog: Unmatched ) or \\)\n"},
   ],
   ['invalid-re-G-star-paren', '-G "a.*\\)"', {EXIT=>2},
    {ERR => "$prog: Unmatched ) or \\)\n"},
   ],
   ['invalid-re-P-paren', '-P ")"', {EXIT=>2},
    {ERR => $ENV{PCRE_WORKS} == 1
       ? "$prog: unmatched closing parenthesis\n"
       : $no_pcre
    },
   ],
   ['invalid-re-P-star-paren', '-P "a.*)"', {EXIT=>2},
    {ERR => $ENV{PCRE_WORKS} == 1
       ? "$prog: unmatched closing parenthesis\n"
       : $no_pcre
    },
   ],

   # Prior to grep-3.6, the name of the offending file was not printed.
   ['backtracking-with-file', '-P "((a+)*)+$"', {EXIT=>2},
    {IN=>{f=>"a"x20 ."b"}},
    {ERR => $ENV{PCRE_WORKS} == 1
       ? "$prog: f: exceeded PCRE's backtracking limit\n"
       : $no_pcre
    },
   ],

  );

my $save_temps = $ENV{DEBUG};
my $verbose = $ENV{VERBOSE};

my $fail = run_tests ($program_name, $prog, \@Tests, $save_temps, $verbose);
exit $fail;
