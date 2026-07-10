/* Test of localcharset() function
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

#include "localcharset.h"

#include <locale.h>
#include <stdio.h>
#include <string.h>
#include <stdlib.h>

#define WIN32_LEAN_AND_MEAN
#include <windows.h>

int
main (void)
{
#ifdef _UCRT
  unsigned int active_codepage = GetACP ();
  if (!(active_codepage == 65001))
    {
      fprintf (stderr,
               "The active codepage is %u, not 65001 as expected.\n"
               "(This is normal on Windows older than Windows 10.)\n",
               active_codepage);
      exit (1);
    }

  setlocale (LC_ALL, "");
  const char *lc = locale_charset ();
  if (!(strcmp (lc, "UTF-8") == 0))
    {
      fprintf (stderr,
               "locale_charset () is \"%s\", not \"UTF-8\" as expected.\n",
               lc);
      exit (1);
    }

  return 0;
#else
  fputs ("Skipping test: not using the UCRT runtime\n", stderr);
  return 77;
#endif
}
