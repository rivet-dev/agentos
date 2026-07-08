/* Test of once-only execution in multithreaded situations.
   Copyright (C) 2018-2025 Free Software Foundation, Inc.

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

/* Written by Bruno Haible <bruno@clisp.org>, 2018.  */

#include <config.h>

#include <pthread.h>

#include "macros.h"

static pthread_once_t a_once = PTHREAD_ONCE_INIT;

static int a;

static void
a_init (void)
{
  a = 42;
}

int
main ()
{
  ASSERT (pthread_once (&a_once, a_init) == 0);

  ASSERT (a == 42);

  return test_exit_status;
}
