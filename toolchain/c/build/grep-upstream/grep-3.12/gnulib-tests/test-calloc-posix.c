/* Test of calloc function.
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

/* Specification.  */
#include <stdlib.h>

#include <errno.h>
#include <stdio.h>
#include <stdint.h>

#include "macros.h"

/* Work around clang bug
   <https://github.com/llvm/llvm-project/issues/114772>.  */
void *(*volatile my_calloc) (size_t, size_t) = calloc;
#undef calloc
#define calloc my_calloc

int
main ()
{
  /* Check that calloc sets errno when it fails.
     Do this only in 64-bit processes, because there are many bi-arch systems
     nowadays where a 32-bit process can actually allocate 2 GiB of RAM.  */
  if (sizeof (size_t) >= 8)
    {
      void *p;

      errno = 0;
      p = calloc (SIZE_MAX / 20, 2);
      ASSERT (p == NULL);
      ASSERT (errno == ENOMEM);

      errno = 0;
      p = calloc (SIZE_MAX / 6, 2);
      ASSERT (p == NULL);
      ASSERT (errno == ENOMEM);

      return test_exit_status;
    }
  else
    {
      fputs ("Skipping test: size_t is not 64-bits wide\n", stderr);
      return 77;
    }
}
