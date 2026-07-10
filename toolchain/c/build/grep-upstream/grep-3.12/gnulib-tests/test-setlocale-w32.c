/* Test of setting the current locale.
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

#include <config.h>

#include <locale.h>

#include <stdio.h>
#include <string.h>

#include "macros.h"

#if defined _WIN32 && !defined __CYGWIN__

# define ENGLISH   "English_United States"
# define FRENCH    "French_France"
# define GERMAN    "German_Germany"
# define BRAZILIAN "Portuguese_Brazil"
# define CATALAN   "Catalan_Spain"
# define ENCODING ".1252"

# define LOCALE1 GERMAN ENCODING
# define LOCALE2 FRENCH ENCODING
# define LOCALE3 ENGLISH ENCODING
# define LOCALE4 BRAZILIAN ENCODING
# define LOCALE5 CATALAN ENCODING

int
main ()
{
  const char *ret;
  const char *ret_all;

  /* Set a mixed locale.  */
  if (setlocale (LC_ALL, LOCALE1) == NULL)
    {
      fprintf (stderr, "Setting locale %s failed.\n", LOCALE1);
      return 1;
    }
  if (setlocale (LC_NUMERIC, LOCALE2) == NULL)
    {
      fprintf (stderr, "Setting locale %s failed.\n", LOCALE2);
      return 1;
    }
  if (setlocale (LC_TIME, LOCALE3) == NULL)
    {
      fprintf (stderr, "Setting locale %s failed.\n", LOCALE3);
      return 1;
    }
  if (setlocale (LC_MESSAGES, LOCALE4) == NULL)
    {
      fprintf (stderr, "Setting locale %s failed.\n", LOCALE4);
      return 1;
    }
  if (setlocale (LC_MONETARY, LOCALE5) == NULL)
    {
      fprintf (stderr, "Setting locale %s failed.\n", LOCALE5);
      return 1;
    }

  /* Check the individual categories.  */
  ret = setlocale (LC_COLLATE, NULL);
  ASSERT (ret != NULL && strcmp (ret, LOCALE1) == 0);
  ret = setlocale (LC_CTYPE, NULL);
  ASSERT (ret != NULL && strcmp (ret, LOCALE1) == 0);
  ret = setlocale (LC_MONETARY, NULL);
  ASSERT (ret != NULL && strcmp (ret, LOCALE5) == 0);
  ret = setlocale (LC_NUMERIC, NULL);
  ASSERT (ret != NULL && strcmp (ret, LOCALE2) == 0);
  ret = setlocale (LC_TIME, NULL);
  ASSERT (ret != NULL && strcmp (ret, LOCALE3) == 0);
  ret = setlocale (LC_MESSAGES, NULL);
  ASSERT (ret != NULL && strcmp (ret, LOCALE4) == 0);
  ret = setlocale (LC_ALL, NULL);
  ASSERT (ret != NULL
          && strcmp (ret, "LC_COLLATE=" LOCALE1 ";"
                          "LC_CTYPE=" LOCALE1 ";"
                          "LC_MONETARY=" LOCALE5 ";"
                          "LC_NUMERIC=" LOCALE2 ";"
                          "LC_TIME=" LOCALE3 ";"
                          "LC_MESSAGES=" LOCALE4)
             == 0);
  ret_all = _strdup (ret);

  /* Reset the locale.  */
  ASSERT (setlocale (LC_ALL, "C") != NULL);

  ret = setlocale (LC_ALL, NULL);
  ASSERT (ret != NULL && strcmp (ret, "C") == 0);
  ret = setlocale (LC_MESSAGES, NULL);
  ASSERT (ret != NULL && strcmp (ret, "C") == 0);

  /* Restore the locale.  */
  ret = setlocale (LC_ALL, ret_all);
  ASSERT (ret != NULL && strcmp (ret, ret_all) == 0);

  /* Check the individual categories again.  */
  ret = setlocale (LC_COLLATE, NULL);
  ASSERT (ret != NULL && strcmp (ret, LOCALE1) == 0);
  ret = setlocale (LC_CTYPE, NULL);
  ASSERT (ret != NULL && strcmp (ret, LOCALE1) == 0);
  ret = setlocale (LC_MONETARY, NULL);
  ASSERT (ret != NULL && strcmp (ret, LOCALE5) == 0);
  ret = setlocale (LC_NUMERIC, NULL);
  ASSERT (ret != NULL && strcmp (ret, LOCALE2) == 0);
  ret = setlocale (LC_TIME, NULL);
  ASSERT (ret != NULL && strcmp (ret, LOCALE3) == 0);
  ret = setlocale (LC_MESSAGES, NULL);
  ASSERT (ret != NULL && strcmp (ret, LOCALE4) == 0);
  ret = setlocale (LC_ALL, NULL);
  ASSERT (ret != NULL
          && strcmp (ret, "LC_COLLATE=" LOCALE1 ";"
                          "LC_CTYPE=" LOCALE1 ";"
                          "LC_MONETARY=" LOCALE5 ";"
                          "LC_NUMERIC=" LOCALE2 ";"
                          "LC_TIME=" LOCALE3 ";"
                          "LC_MESSAGES=" LOCALE4)
             == 0);

  return test_exit_status;
}

#else

int
main ()
{
  fputs ("Skipping test: not a native Windows system\n", stderr);
  return 77;
}

#endif
