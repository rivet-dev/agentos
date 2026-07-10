/*
 * Copyright (C) 2008-2025 Free Software Foundation, Inc.
 * Written by Eric Blake
 *
 * This program is free software: you can redistribute it and/or modify
 * it under the terms of the GNU General Public License as published by
 * the Free Software Foundation, either version 3 of the License, or
 * (at your option) any later version.
 *
 * This program is distributed in the hope that it will be useful,
 * but WITHOUT ANY WARRANTY; without even the implied warranty of
 * MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE.  See the
 * GNU General Public License for more details.
 *
 * You should have received a copy of the GNU General Public License
 * along with this program.  If not, see <https://www.gnu.org/licenses/>.  */

#include <config.h>

#include "memchr2.h"

#include <stdlib.h>
#include <string.h>

#include "zerosize-ptr.h"
#include "macros.h"

int
main (void)
{
  size_t n = 0x100000;
  char *input = malloc (n);
  ASSERT (input);

  input[0] = 'a';
  input[1] = 'b';
  memset (input + 2, 'c', 1024);
  memset (input + 1026, 'd', n - 1028);
  input[n - 2] = 'e';
  input[n - 1] = 'a';

  /* Basic behavior tests.  */
  ASSERT (memchr2 (input, 'a', 'b', n) == input);
  ASSERT (memchr2 (input, 'b', 'a', n) == input);

  ASSERT (memchr2 (input, 'a', 'b', 0) == NULL);
  void *page_boundary = zerosize_ptr ();
  if (page_boundary)
    ASSERT (memchr2 (page_boundary, 'a', 'b', 0) == NULL);

  ASSERT (memchr2 (input, 'b', 'd', n) == input + 1);
  ASSERT (memchr2 (input + 2, 'b', 'd', n - 2) == input + 1026);

  ASSERT (memchr2 (input, 'd', 'e', n) == input + 1026);
  ASSERT (memchr2 (input, 'e', 'd', n) == input + 1026);

  ASSERT (memchr2 (input + 1, 'a', 'e', n - 1) == input + n - 2);
  ASSERT (memchr2 (input + 1, 'e', 'a', n - 1) == input + n - 2);

  ASSERT (memchr2 (input, 'f', 'g', n) == NULL);
  ASSERT (memchr2 (input, 'f', '\0', n) == NULL);

  ASSERT (memchr2 (input, 'a', 'a', n) == input);
  ASSERT (memchr2 (input + 1, 'a', 'a', n - 1) == input + n - 1);
  ASSERT (memchr2 (input, 'f', 'f', n) == NULL);

  /* Check that a very long haystack is handled quickly if one of the
     two bytes is found near the beginning.  */
  {
    size_t repeat = 10000;
    for (; repeat > 0; repeat--)
      {
        ASSERT (memchr2 (input, 'c', 'e', n) == input + 2);
        ASSERT (memchr2 (input, 'e', 'c', n) == input + 2);
        ASSERT (memchr2 (input, 'c', '\0', n) == input + 2);
        ASSERT (memchr2 (input, '\0', 'c', n) == input + 2);
      }
  }

  /* Alignment tests.  */
  {
    int i, j;
    for (i = 0; i < 32; i++)
      {
        for (j = 0; j < 256; j++)
          input[i + j] = j;
        for (j = 0; j < 256; j++)
          {
            ASSERT (memchr2 (input + i, j, 0xff, 256) == input + i + j);
            ASSERT (memchr2 (input + i, 0xff, j, 256) == input + i + j);
          }
      }
  }

  free (input);

  return test_exit_status;
}
