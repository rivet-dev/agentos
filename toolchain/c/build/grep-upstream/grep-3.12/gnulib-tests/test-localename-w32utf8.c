/* Test of gl_locale_name function and its variants
   on native Windows in the UTF-8 environment.
   Copyright (C) 2024-2025 Free Software Foundation, Inc.

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

/* Written by Bruno Haible <bruno@clisp.org>, 2024.  */

#include <config.h>

#include "localename.h"

#include <stdio.h>
#include <string.h>

#include "macros.h"

int
main (void)
{
#ifdef _UCRT
  const char *name = gl_locale_name_default ();

  ASSERT (name != NULL);

  /* With the legacy system settings, expect "C.UTF-8", not "C", because "C" is
     a single-byte locale.
     With the modern system settings, expect some "ll_CC.UTF-8" name.  */
  ASSERT (strlen (name) > 6 && strcmp (name + strlen (name)- 6, ".UTF-8") == 0);

  return test_exit_status;
#else
  fputs ("Skipping test: not using the UCRT runtime\n", stderr);
  return 77;
#endif
}
