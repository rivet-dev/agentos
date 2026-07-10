/* Test of locking in multithreaded situations.
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

#if defined _WIN32 && !defined __CYGWIN__

/* Specification.  */
# include "windows-timedrecmutex.h"

# include <errno.h>
# include <stdio.h>
# include <string.h>

# include "macros.h"

/* Returns the effective type of a lock.  */
static const char *
get_effective_type (glwthread_timedrecmutex_t *lock)
{
  /* Lock once.  */
  ASSERT (glwthread_timedrecmutex_lock (lock) == 0);

  /* Try to lock a second time.  */
  int err = glwthread_timedrecmutex_trylock (lock);
  if (err == 0)
    return "RECURSIVE";
  if (err == EBUSY)
    return "NORMAL";

  return "impossible!";
}

int
main ()
{
  /* Find the effective type of a lock.  */
  const char *type;
  {
    glwthread_timedrecmutex_t lock;
    ASSERT (glwthread_timedrecmutex_init (&lock) == 0);
    type = get_effective_type (&lock);
  }

  printf ("type = %s\n", type);

  ASSERT (strcmp (type, "RECURSIVE") == 0);

  return test_exit_status;
}

#else

# include <stdio.h>

int
main ()
{
  fputs ("Skipping test: not a native Windows system\n", stderr);
  return 77;
}

#endif
