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

/* Specification.  */
#include <pthread.h>

#include <errno.h>
#include <stdio.h>
#include <string.h>

#include "macros.h"

/* Returns the effective type of a lock.  */
static const char *
get_effective_type (pthread_mutex_t *lock)
{
  /* Lock once.  */
  ASSERT (pthread_mutex_lock (lock) == 0);

  /* Try to lock a second time.  */
  int err = pthread_mutex_trylock (lock);
  if (err == 0)
    return "RECURSIVE";
  if (err == EBUSY)
    return "NORMAL";

  /* We can't really check whether the lock is effectively ERRORCHECK, without
     risking a deadlock.  */

  return "unknown!";
}

int
main ()
{
  /* Find the effective type of a NORMAL lock.  */
  const char *type_normal;
  {
    pthread_mutex_t lock;
    pthread_mutexattr_t attr;
    ASSERT (pthread_mutexattr_init (&attr) == 0);
    ASSERT (pthread_mutexattr_settype (&attr, PTHREAD_MUTEX_NORMAL) == 0);
    ASSERT (pthread_mutex_init (&lock, &attr) == 0);
    ASSERT (pthread_mutexattr_destroy (&attr) == 0);
    type_normal = get_effective_type (&lock);
  }

  /* Find the effective type of an ERRORCHECK lock.  */
  const char *type_errorcheck;
  {
    pthread_mutex_t lock;
    pthread_mutexattr_t attr;
    ASSERT (pthread_mutexattr_init (&attr) == 0);
    ASSERT (pthread_mutexattr_settype (&attr, PTHREAD_MUTEX_ERRORCHECK) == 0);
    ASSERT (pthread_mutex_init (&lock, &attr) == 0);
    ASSERT (pthread_mutexattr_destroy (&attr) == 0);
    type_errorcheck = get_effective_type (&lock);
  }

  /* Find the effective type of a RECURSIVE lock.  */
  const char *type_recursive;
  {
    pthread_mutex_t lock;
    pthread_mutexattr_t attr;
    ASSERT (pthread_mutexattr_init (&attr) == 0);
    ASSERT (pthread_mutexattr_settype (&attr, PTHREAD_MUTEX_RECURSIVE) == 0);
    ASSERT (pthread_mutex_init (&lock, &attr) == 0);
    ASSERT (pthread_mutexattr_destroy (&attr) == 0);
    type_recursive = get_effective_type (&lock);
  }

  /* Find the effective type of a DEFAULT lock.  */
  const char *type_default;
  {
    pthread_mutex_t lock;
    pthread_mutexattr_t attr;
    ASSERT (pthread_mutexattr_init (&attr) == 0);
    ASSERT (pthread_mutexattr_settype (&attr, PTHREAD_MUTEX_DEFAULT) == 0);
    ASSERT (pthread_mutex_init (&lock, &attr) == 0);
    ASSERT (pthread_mutexattr_destroy (&attr) == 0);
    type_default = get_effective_type (&lock);
  }

  /* Find the effective type of a default-initialized lock.  */
  const char *type_def;
  {
    pthread_mutex_t lock;
    ASSERT (pthread_mutex_init (&lock, NULL) == 0);
    type_def = get_effective_type (&lock);
  }

  printf ("PTHREAD_MUTEX_NORMAL     -> type = %s\n", type_normal);
  printf ("PTHREAD_MUTEX_ERRORCHECK -> type = %s\n", type_errorcheck);
  printf ("PTHREAD_MUTEX_RECURSIVE  -> type = %s\n", type_recursive);
  printf ("PTHREAD_MUTEX_DEFAULT    -> type = %s\n", type_default);
  printf ("Default                  -> type = %s\n", type_def);

  ASSERT (strcmp (type_normal,        "NORMAL") == 0);
  ASSERT (strcmp (type_errorcheck,    "NORMAL") == 0);
  ASSERT (strcmp (type_recursive,     "RECURSIVE") == 0);

  ASSERT (strcmp (type_default, type_def) == 0);

  /* This is not required by POSIX, but happens to be the case on all
     platforms.  */
  ASSERT (strcmp (type_default,       "NORMAL") == 0);
  ASSERT (strcmp (type_def,           "NORMAL") == 0);

  return test_exit_status;
}
