/* Test of calloc function.
   Copyright (C) 2010-2025 Free Software Foundation, Inc.

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
  /* Check that calloc (0, 0) is not a NULL pointer.  */
  {
    void *p = calloc (0, 0);
    ASSERT (p != NULL);
    free (p);
  }

  /* Check that calloc fails when requested to allocate a block of memory
     larger than PTRDIFF_MAX or SIZE_MAX bytes.  */
  {
    for (size_t n = 2; n != 0; n <<= 1)
      {
        void *p = calloc (PTRDIFF_MAX / n + 1, n);
        ASSERT (p == NULL);
        ASSERT (errno == ENOMEM);

        p = calloc (SIZE_MAX / n + 1, n);
        ASSERT (p == NULL);
        ASSERT (errno == ENOMEM);
      }
  }

  return test_exit_status;
}
