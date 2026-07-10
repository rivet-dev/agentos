/* Test the putenv function.
   Copyright (C) 2024-2025 Free Software Foundation, Inc.

   This file is free software: you can redistribute it and/or modify
   it under the terms of the GNU General Public License as published
   by the Free Software Foundation, either version 3 of the License,
   or (at your option) any later version.

   This file is distributed in the hope that it will be useful,
   but WITHOUT ANY WARRANTY; without even the implied warranty of
   MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE.  See the
   GNU General Public License for more details.

   You should have received a copy of the GNU General Public License
   along with this program.  If not, see <https://www.gnu.org/licenses/>.  */

/* Written by Collin Funk <collin.funk1@gmail.com>, 2024.  */

#include <config.h>

/* Specification.  */
#include <stdlib.h>

#include "signature.h"
SIGNATURE_CHECK (putenv, int, (char *));

#include <string.h>

#include "macros.h"

int
main (void)
{
  char *ptr;

  /* Verify the environment is clean.  */
  unsetenv ("TEST_VAR");
  ASSERT (getenv ("TEST_VAR") == NULL);

  /* Use static on variables passed to the environment to pacify
     -Wanalyzer-putenv-of-auto-var.  */

  /* Verify adding an environment variable.  */
  {
    static char var[] = "TEST_VAR=abc";
    ASSERT (putenv (var) == 0);
    ptr = getenv ("TEST_VAR");
    ASSERT (ptr != NULL);
    ASSERT (STREQ (ptr, "abc"));
  }

  /* Verify removing an environment variable.  */
  {
    static char var[] = "TEST_VAR";
    ASSERT (putenv (var) == 0);
    ASSERT (getenv ("TEST_VAR") == NULL);
  }

  /* Verify the behavior when removing a variable not in the environment.  */
  {
    static char var[] = "TEST_VAR";
    ASSERT (putenv (var) == 0);
    ASSERT (getenv ("TEST_VAR") == NULL);
  }

  return test_exit_status;
}
