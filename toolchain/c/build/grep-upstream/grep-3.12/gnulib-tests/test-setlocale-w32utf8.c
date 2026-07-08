/* Test of setting the current locale
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

#include <locale.h>

#include <stdio.h>
#include <stdlib.h>
#include <string.h>

int
main (void)
{
#ifdef _UCRT
  /* Test that setlocale() works as expected in a UTF-8 locale.  */
  char *name;

  /* This looks at all LC_*, LANG environment variables, which are all unset
     at this point.  */
  if (setlocale (LC_ALL, "") == NULL)
    return 1;

  name = setlocale (LC_ALL, NULL);
  /* With the legacy system settings, expect some mixed locale, due to the
     limitations of the native setlocale().
     With the modern system settings, expect some "ll_CC.UTF-8" name.  */
  if (!((strlen (name) > 6 && str_endswith (name, ".UTF-8"))
        || strcmp (name, "LC_COLLATE=English_United States.65001;"
                         "LC_CTYPE=English_United States.65001;"
                         "LC_MONETARY=English_United States.65001;"
                         "LC_NUMERIC=English_United States.65001;"
                         "LC_TIME=English_United States.65001;"
                         "LC_MESSAGES=C.UTF-8")
           == 0
        || strcmp (name, "LC_COLLATE=English_United States.utf8;"
                         "LC_CTYPE=English_United States.utf8;"
                         "LC_MONETARY=English_United States.utf8;"
                         "LC_NUMERIC=English_United States.utf8;"
                         "LC_TIME=English_United States.utf8;"
                         "LC_MESSAGES=C.UTF-8")
           == 0))
    {
      fprintf (stderr, "setlocale() returned \"%s\".\n", name);
      exit (1);
    }

  return 0;
#else
  fputs ("Skipping test: not using the UCRT runtime\n", stderr);
  return 77;
#endif
}
