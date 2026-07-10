/* Test of type-safe arrays that grow dynamically.
   Copyright (C) 2021-2025 Free Software Foundation, Inc.

   This program is free software; you can redistribute it and/or modify
   it under the terms of the GNU General Public License as published by
   the Free Software Foundation; either version 2, or (at your option)
   any later version.

   This program is distributed in the hope that it will be useful,
   but WITHOUT ANY WARRANTY; without even the implied warranty of
   MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE.  See the
   GNU General Public License for more details.

   You should have received a copy of the GNU General Public License
   along with this program; if not, see <https://www.gnu.org/licenses/>.  */

/* Written by Bruno Haible <bruno@clisp.org>, 2021.  */

#include <config.h>

#define DYNARRAY_STRUCT int_sequence
#define DYNARRAY_ELEMENT int
#define DYNARRAY_PREFIX intseq_
#include "dynarray.h"

#include "macros.h"

#define N 100000

static int
value_at (long long int i)
{
  return (i % 13) + ((i * i) % 251);
}

int
main ()
{
  struct int_sequence s;
  int i;

  intseq_init (&s);
  for (i = 0; i < N; i++)
    intseq_add (&s, value_at (i));
  for (i = N - 1; i >= N / 2; i--)
    {
      ASSERT (* intseq_at (&s, i) == value_at (i));
      intseq_remove_last (&s);
    }
  intseq_free (&s);

  return test_exit_status;
}
