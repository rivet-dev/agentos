/* Test of year2038 module.
   Copyright (C) 2023-2025 Free Software Foundation, Inc.

   This program is free software: you can redistribute it and/or modify
   it under the terms of the GNU General Public License as published by
   the Free Software Foundation, either version 3 of the License, or
   (at your option) any later version.

   This program is distributed in the hope that it will be useful,
   but WITHOUT ANY WARRANTY; without even the implied warranty of
   MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE.  See the
   GNU General Public License for more details.

   You should have received a copy of the GNU General Public License
   along with this program.  If not, see <https://www.gnu.org/licenses/>.  */

/* Written by Bruno Haible <bruno@clisp.org>, 2023.  */

/* This test fails if AC_SYS_YEAR2038 could not arrange for a time_t
   that supports values > 2147483647.
   This can happen
     - on systems that are not year 2038 safe, or
     - if the user specified --disable-year2038 at configure time.
   It is intended that this test fails in these circumstances.  */

#include <config.h>

#include <sys/types.h>
#include "intprops.h"

/* Although this test could be done with static_assert, the test
   harness prefers dynamic checking.  */

int
main (void)
{
  /* Check the range of time_t.  */
  return TYPE_MAXIMUM (time_t) >> 31 == 0;
}
