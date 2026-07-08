/* POSIX condition variables.
   Copyright (C) 2010-2025 Free Software Foundation, Inc.

   This file is free software: you can redistribute it and/or modify
   it under the terms of the GNU Lesser General Public License as
   published by the Free Software Foundation; either version 2.1 of the
   License, or (at your option) any later version.

   This file is distributed in the hope that it will be useful,
   but WITHOUT ANY WARRANTY; without even the implied warranty of
   MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE.  See the
   GNU Lesser General Public License for more details.

   You should have received a copy of the GNU Lesser General Public License
   along with this program.  If not, see <https://www.gnu.org/licenses/>.  */

/* Written by Paul Eggert, 2010, and Bruno Haible <bruno@clisp.org>, 2019.  */

#include <config.h>

/* Specification.  */
#include <pthread.h>

#if (defined _WIN32 && ! defined __CYGWIN__) && USE_WINDOWS_THREADS
# include "windows-cond.h"
#else
# include <errno.h>
# include <limits.h>
# include <sys/time.h>
# include <time.h>
#endif

#if ((defined _WIN32 && ! defined __CYGWIN__) && USE_WINDOWS_THREADS) || !HAVE_PTHREAD_H

int
pthread_condattr_init (pthread_condattr_t *attr)
{
  *attr = 0;
  return 0;
}

int
pthread_condattr_destroy (_GL_UNUSED pthread_condattr_t *attr)
{
  return 0;
}

#endif

#if (defined _WIN32 && ! defined __CYGWIN__) && USE_WINDOWS_THREADS
/* Use Windows threads.  */

int
pthread_cond_init (pthread_cond_t *cond,
                   _GL_UNUSED const pthread_condattr_t *attr)
{
  return glwthread_cond_init (cond);
}

int
pthread_cond_wait (pthread_cond_t *cond, pthread_mutex_t *mutex)
{
  return glwthread_cond_wait (cond, mutex,
                              (int (*) (void *)) pthread_mutex_lock,
                              (int (*) (void *)) pthread_mutex_unlock);
}

int
pthread_cond_timedwait (pthread_cond_t *cond, pthread_mutex_t *mutex,
                        const struct timespec *abstime)
{
  return glwthread_cond_timedwait (cond, mutex,
                                   (int (*) (void *)) pthread_mutex_lock,
                                   (int (*) (void *)) pthread_mutex_unlock,
                                   abstime);
}

int
pthread_cond_signal (pthread_cond_t *cond)
{
  return glwthread_cond_signal (cond);
}

int
pthread_cond_broadcast (pthread_cond_t *cond)
{
  return glwthread_cond_broadcast (cond);
}

int
pthread_cond_destroy (pthread_cond_t *cond)
{
  return glwthread_cond_destroy (cond);
}

#elif HAVE_PTHREAD_H
/* Provide workarounds for POSIX threads.  */

#else
/* Provide a dummy implementation for single-threaded applications.  */

int
pthread_cond_init (_GL_UNUSED pthread_cond_t *cond,
                   _GL_UNUSED const pthread_condattr_t *attr)
{
  /* COND is never seriously used.  */
  return 0;
}

int
pthread_cond_wait (_GL_UNUSED pthread_cond_t *cond,
                   _GL_UNUSED pthread_mutex_t *mutex)
{
  /* No other thread can signal this condition variable.
     Wait endlessly.  */
  for (;;)
    {
      struct timespec duration =
        {
          .tv_sec = 86400,
          .tv_nsec = 0
        };
      nanosleep (&duration, NULL);
    }
}

int
pthread_cond_timedwait (_GL_UNUSED pthread_cond_t *cond,
                        _GL_UNUSED pthread_mutex_t *mutex,
                        const struct timespec *abstime)
{
  /* No other thread can signal this condition variable.
     Wait until ABSTIME is reached.  */
  for (;;)
    {
      struct timeval currtime;
      unsigned long remaining;

      gettimeofday (&currtime, NULL);

      if (currtime.tv_sec > abstime->tv_sec)
        remaining = 0;
      else
        {
          unsigned long seconds = abstime->tv_sec - currtime.tv_sec;
          remaining = seconds * 1000000000;
          if (remaining / 1000000000 != seconds) /* overflow? */
            remaining = ULONG_MAX;
          else
            {
              long nanoseconds =
                abstime->tv_nsec - currtime.tv_usec * 1000;
              if (nanoseconds >= 0)
                {
                  remaining += nanoseconds;
                  if (remaining < nanoseconds) /* overflow? */
                    remaining = ULONG_MAX;
                }
              else
                {
                  if (remaining >= - nanoseconds)
                    remaining -= (- nanoseconds);
                  else
                    remaining = 0;
                }
            }
        }
      if (remaining == 0)
        return ETIMEDOUT;

      /* Sleep up to REMAINING ns.  */
      struct timespec duration = { .tv_sec  = remaining / 1000000000,
                                   .tv_nsec = remaining % 1000000000 };
      nanosleep (&duration, NULL);
    }
}

int
pthread_cond_signal (_GL_UNUSED pthread_cond_t *cond)
{
  /* No threads can currently be blocked on COND.  */
  return 0;
}

int
pthread_cond_broadcast (_GL_UNUSED pthread_cond_t *cond)
{
  /* No threads can currently be blocked on COND.  */
  return 0;
}

int
pthread_cond_destroy (_GL_UNUSED pthread_cond_t *cond)
{
  /* COND is never seriously used.  */
  return 0;
}

#endif
