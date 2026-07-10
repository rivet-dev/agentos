#!/bin/sh
# This would fail with grep-2.21's dfa.c.

# Copyright 2014-2025 Free Software Foundation, Inc.

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

. "${srcdir=.}/init.sh"; path_prepend_ ../src

# Add "." to PATH for the use of test-dfa-match-aux.
path_prepend_ .

if (type timeout) >/dev/null 2>&1; then
    # Busybox's timeout required -t until its 1.30.0 release on 2018-12-31,
    # after which it became compatible with coreutils' timeout.
  if timeout --help 2>&1 | grep BusyBox && timeout -t 0 true; then
    timeout_10='timeout -t 10'
  else
    timeout_10='timeout 10'
  fi
else
  timeout_10=
fi

fail=0

${CHECKER} test-dfa-match-aux a ba 0 > out || fail=1
compare /dev/null out || fail=1

in=$(printf "bb\nbb")
$timeout_10 ${CHECKER} test-dfa-match-aux a "$in" 1 > out || fail=1
compare /dev/null out || fail=1

# If the platform supports U+00E9 LATIN SMALL LETTER E WITH ACUTE,
# test U+D45C HANGUL SYLLABLE PYO.
U_00E9=$(printf '\303\251\n')
U_D45C=$(printf '\355\221\234\n')
if testout=$(LC_ALL=en_US.UTF-8 $CHECKER test-dfa-match-aux '^.$' "$U_00E9") &&
   test "$testout" = 2
then
  testout=$(LC_ALL=en_US.UTF-8 $CHECKER test-dfa-match-aux '^.$' "$U_D45C") &&
  test "$testout" = 3 || fail=1
fi

Exit $fail
