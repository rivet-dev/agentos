/* Test of nl_langinfo replacement.
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

#include <config.h>

#include <langinfo.h>

#include <locale.h>
#include <stdlib.h>
#include <string.h>

#include "c-strcase.h"
#include "c-strcasestr.h"
#include "macros.h"

int
main (int argc, char *argv[])
{
#if HAVE_WORKING_USELOCALE
  /* Check that nl_langinfo() uses the per-thread locale.  */
  int pass;
  bool skipped_all = true;

  /* Extract a few items from the C locale.  */
  /* nl_langinfo items of the LC_CTYPE category */
  const char *c_CODESET = nl_langinfo (CODESET);
  /* nl_langinfo items of the LC_NUMERIC category */
  const char *c_RADIXCHAR = nl_langinfo (RADIXCHAR);
  /* nl_langinfo items of the LC_TIME category */
  const char *c_T_FMT_AMPM = nl_langinfo (T_FMT_AMPM);
  const char *c_AM_STR = nl_langinfo (AM_STR);
  const char *c_PM_STR = nl_langinfo (PM_STR);
  /* nl_langinfo items of the LC_MONETARY category */
  const char *c_CRNCYSTR = nl_langinfo (CRNCYSTR);
  /* nl_langinfo items of the LC_MESSAGES category */
  const char *c_YESEXPR = nl_langinfo (YESEXPR);

  /* Sanity checks.  */
  (void) c_CODESET;
  ASSERT (strcmp (c_RADIXCHAR, ".") == 0);
  ASSERT (strlen (c_T_FMT_AMPM) > 0);
  ASSERT (strlen (c_AM_STR) > 0);
  ASSERT (strlen (c_PM_STR) > 0);
  ASSERT (strlen (c_CRNCYSTR) <= 1); /* "-", "+", ".", or "" */
  ASSERT (c_strcasestr (c_YESEXPR, "y" /* from "yes" */) != NULL);

  for (pass = 1; pass <= 2; pass++)
    {
      /* pass    locale
          1        traditional French locale
          2        French UTF-8 locale
       */
      const char *fr_locale_name =
        getenv (pass == 1 ? "LOCALE_FR" : "LOCALE_FR_UTF8");
      if (strcmp (fr_locale_name, "none") != 0)
        {
          /* Use a per-thread locale.  */
          locale_t fr_locale = newlocale (LC_ALL_MASK, fr_locale_name, NULL);
          if (fr_locale != NULL)
            {
              uselocale (fr_locale);

              /* Extract a few items from the current locale, and check the
                 values.  */

              /* nl_langinfo items of the LC_CTYPE category */
              const char *fr_CODESET = nl_langinfo (CODESET);
              if (pass == 1)
                ASSERT (strstr (fr_CODESET, "8859") != NULL);
              else if (pass == 2)
                ASSERT (c_strcasecmp (fr_CODESET, "UTF-8") == 0
                        || c_strcasecmp (fr_CODESET, "UTF8") == 0);

              /* In musl libc, locales differ at most in the LC_MESSAGES
                 category.  */
              #if !defined MUSL_LIBC

              /* nl_langinfo items of the LC_NUMERIC category */
              const char *fr_RADIXCHAR = nl_langinfo (RADIXCHAR);
              ASSERT (strcmp (fr_RADIXCHAR, ",") == 0);

              /* nl_langinfo items of the LC_TIME category */
              /* macOS and Solaris 11 don't get the LC_TIME values right.
                 Poor.  */
              #if !((defined __APPLE__ && defined __MACH__) || defined __sun)
              const char *fr_T_FMT_AMPM = nl_langinfo (T_FMT_AMPM);
              const char *fr_AM_STR = nl_langinfo (AM_STR);
              const char *fr_PM_STR = nl_langinfo (PM_STR);
              ASSERT (strlen (fr_T_FMT_AMPM) == 0
                      || strcmp (fr_T_FMT_AMPM, "%I:%M:%S") == 0);
              ASSERT (strlen (fr_AM_STR) == 0);
              ASSERT (strlen (fr_PM_STR) == 0);
              #endif

              /* nl_langinfo items of the LC_MONETARY category */
              /* macOS doesn't get the EUR currency symbol or abbreviation
                 right.  Very poor.  */
              #if !(defined __APPLE__ && defined __MACH__)
              const char *fr_CRNCYSTR = nl_langinfo (CRNCYSTR);
              if (pass == 2)
                ASSERT (strlen (fr_CRNCYSTR) >= 1
                        && strcmp (fr_CRNCYSTR + 1, "€") == 0);
              #endif

              #endif

              /* nl_langinfo items of the LC_MESSAGES category */
              /* In musl libc, this works only if the package 'musl-locales' is
                 installed.  */
              #if !defined MUSL_LIBC
              const char *fr_YESEXPR = nl_langinfo (YESEXPR);
              ASSERT (c_strcasestr (fr_YESEXPR, "o" /* from "oui" */) != NULL);
              #endif

              skipped_all = false;
            }
        }
    }

  if (skipped_all)
    {
      if (test_exit_status != EXIT_SUCCESS)
        return test_exit_status;
      fputs ("Skipping test: French locale is not installed\n", stderr);
      return 77;
    }

  return test_exit_status;
#else
  fputs ("Skipping test: uselocale() not available\n", stderr);
  return 77;
#endif
}
