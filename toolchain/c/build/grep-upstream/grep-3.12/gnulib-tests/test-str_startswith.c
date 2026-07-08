/* Test of str_startswith() function.
   Copyright (C) 2025 Free Software Foundation, Inc.

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

#include <config.h>

/* Specification.  */
#include <string.h>

#include "macros.h"

int
main ()
{
  ASSERT (str_startswith ("", ""));
  ASSERT (str_startswith ("abc", ""));

  ASSERT (!str_startswith ("", "a"));
  ASSERT (!str_startswith ("x", "a"));
  ASSERT (str_startswith ("a", "a"));
  ASSERT (str_startswith ("abc", "a"));

  ASSERT (!str_startswith ("", "xyz"));
  ASSERT (!str_startswith ("x", "xyz"));
  ASSERT (!str_startswith ("a", "xyz"));
  ASSERT (!str_startswith ("abc", "xyz"));
  ASSERT (str_startswith ("xyz", "xyz"));
  ASSERT (str_startswith ("xyzzy", "xyz"));

  return test_exit_status;
}
