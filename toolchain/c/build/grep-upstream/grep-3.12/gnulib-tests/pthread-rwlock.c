/* POSIX read-write locks.
   Copyright (C) 2019-2025 Free Software Foundation, Inc.

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

/* Written by Bruno Haible <bruno@clisp.org>, 2019.  */

#include <config.h>

/* Specification.  */
#include <pthread.h>

#if (defined _WIN32 && ! defined __CYGWIN__) && USE_WINDOWS_THREADS
# include "windows-timedrwlock.h"
#else
# include <errno.h>
# include <limits.h>
# include <sys/time.h>
# include <time.h>
#endif

#ifndef MIN
# define MIN(a, b) ((a) < (b) ? (a) : (b))
#endif

#if ((defined _WIN32 && ! defined __CYGWIN__) && USE_WINDOWS_THREADS) || !HAVE_PTHREAD_H

int
pthread_rwlockattr_init (pthread_rwlockattr_t *attr)
{
  *attr = 0;
  return 0;
}

int
pthread_rwlockattr_destroy (_GL_UNUSED pthread_rwlockattr_t *attr)
{
  return 0;
}

#elif PTHREAD_RWLOCK_BAD_WAITQUEUE

/* Override pthread_rwlockattr_init, to use the kind PREFER_WRITER_NONRECURSIVE
   (or possibly PREFER_WRITER) instead of the kind DEFAULT.  */
int
pthread_rwlockattr_init (pthread_rwlockattr_t *attr)
# undef pthread_rwlockattr_init
{
  int err;

  err = pthread_rwlockattr_init (attr);
  if (err != 0)
    return err;
  err = pthread_rwlockattr_setkind_np (attr,
                                       PTHREAD_RWLOCK_PREFER_WRITER_NONRECURSIVE_NP);
  if (err != 0)
    {
      pthread_rwlockattr_destroy (attr);
      return err;
    }
  return 0;
}

#endif

#if (defined _WIN32 && ! defined __CYGWIN__) && USE_WINDOWS_THREADS
/* Use Windows threads.  */

int
pthread_rwlock_init (pthread_rwlock_t *lock,
                     _GL_UNUSED const pthread_rwlockattr_t *attr)
{
  glwthread_timedrwlock_init (lock);
  return 0;
}

int
pthread_rwlock_rdlock (pthread_rwlock_t *lock)
{
  return glwthread_timedrwlock_rdlock (lock);
}

int
pthread_rwlock_wrlock (pthread_rwlock_t *lock)
{
  return glwthread_timedrwlock_wrlock (lock);
}

int
pthread_rwlock_tryrdlock (pthread_rwlock_t *lock)
{
  return glwthread_timedrwlock_tryrdlock (lock);
}

int
pthread_rwlock_trywrlock (pthread_rwlock_t *lock)
{
  return glwthread_timedrwlock_trywrlock (lock);
}

int
pthread_rwlock_timedrdlock (pthread_rwlock_t *lock,
                            const struct timespec *abstime)
{
  return glwthread_timedrwlock_timedrdlock (lock, abstime);
}

int
pthread_rwlock_timedwrlock (pthread_rwlock_t *lock,
                            const struct timespec *abstime)
{
  return glwthread_timedrwlock_timedwrlock (lock, abstime);
}

int
pthread_rwlock_unlock (pthread_rwlock_t *lock)
{
  return glwthread_timedrwlock_unlock (lock);
}

int
pthread_rwlock_destroy (pthread_rwlock_t *lock)
{
  return glwthread_timedrwlock_destroy (lock);
}

#elif HAVE_PTHREAD_H
/* Provide workarounds for POSIX threads.  */

# if PTHREAD_RWLOCK_UNIMPLEMENTED

int
pthread_rwlock_init (pthread_rwlock_t *lock,
                     _GL_UNUSED const pthread_rwlockattr_t *attr)
{
  int err;

  err = pthread_mutex_init (&lock->lock, NULL);
  if (err != 0)
    return err;
  err = pthread_cond_init (&lock->waiting_readers, NULL);
  if (err != 0)
    return err;
  err = pthread_cond_init (&lock->waiting_writers, NULL);
  if (err != 0)
    return err;
  lock->waiting_writers_count = 0;
  lock->runcount = 0;
  return 0;
}

int
pthread_rwlock_rdlock (pthread_rwlock_t *lock)
{
  int err;

  err = pthread_mutex_lock (&lock->lock);
  if (err != 0)
    return err;
  /* Test whether only readers are currently running, and whether the runcount
     field will not overflow, and whether no writer is waiting.  The latter
     condition is because POSIX recommends that "write locks shall take
     precedence over read locks", to avoid "writer starvation".  */
  while (!(lock->runcount + 1 > 0 && lock->waiting_writers_count == 0))
    {
      /* This thread has to wait for a while.  Enqueue it among the
         waiting_readers.  */
      err = pthread_cond_wait (&lock->waiting_readers, &lock->lock);
      if (err != 0)
        {
          pthread_mutex_unlock (&lock->lock);
          return err;
        }
    }
  lock->runcount++;
  return pthread_mutex_unlock (&lock->lock);
}

int
pthread_rwlock_wrlock (pthread_rwlock_t *lock)
{
  int err;

  err = pthread_mutex_lock (&lock->lock);
  if (err != 0)
    return err;
  /* Test whether no readers or writers are currently running.  */
  while (!(lock->runcount == 0))
    {
      /* This thread has to wait for a while.  Enqueue it among the
         waiting_writers.  */
      lock->waiting_writers_count++;
      err = pthread_cond_wait (&lock->waiting_writers, &lock->lock);
      if (err != 0)
        {
          lock->waiting_writers_count--;
          pthread_mutex_unlock (&lock->lock);
          return err;
        }
      lock->waiting_writers_count--;
    }
  lock->runcount--; /* runcount becomes -1 */
  return pthread_mutex_unlock (&lock->lock);
}

int
pthread_rwlock_tryrdlock (pthread_rwlock_t *lock)
{
  int err;

  err = pthread_mutex_lock (&lock->lock);
  if (err != 0)
    return err;
  /* Test whether only readers are currently running, and whether the runcount
     field will not overflow, and whether no writer is waiting.  The latter
     condition is because POSIX recommends that "write locks shall take
     precedence over read locks", to avoid "writer starvation".  */
  if (!(lock->runcount + 1 > 0 && lock->waiting_writers_count == 0))
    {
      /* This thread would have to wait for a while.  Return instead.  */
      pthread_mutex_unlock (&lock->lock);
      return EBUSY;
    }
  lock->runcount++;
  return pthread_mutex_unlock (&lock->lock);
}

int
pthread_rwlock_trywrlock (pthread_rwlock_t *lock)
{
  int err;

  err = pthread_mutex_lock (&lock->lock);
  if (err != 0)
    return err;
  /* Test whether no readers or writers are currently running.  */
  if (!(lock->runcount == 0))
    {
      /* This thread would have to wait for a while.  Return instead.  */
      pthread_mutex_unlock (&lock->lock);
      return EBUSY;
    }
  lock->runcount--; /* runcount becomes -1 */
  return pthread_mutex_unlock (&lock->lock);
}

int
pthread_rwlock_timedrdlock (pthread_rwlock_t *lock,
                            const struct timespec *abstime)
{
  int err;

  err = pthread_mutex_lock (&lock->lock);
  if (err != 0)
    return err;
  /* Test whether only readers are currently running, and whether the runcount
     field will not overflow, and whether no writer is waiting.  The latter
     condition is because POSIX recommends that "write locks shall take
     precedence over read locks", to avoid "writer starvation".  */
  while (!(lock->runcount + 1 > 0 && lock->waiting_writers_count == 0))
    {
      /* This thread has to wait for a while.  Enqueue it among the
         waiting_readers.  */
      err = pthread_cond_timedwait (&lock->waiting_readers, &lock->lock,
                                    abstime);
      if (err != 0)
        {
          pthread_mutex_unlock (&lock->lock);
          return err;
        }
    }
  lock->runcount++;
  return pthread_mutex_unlock (&lock->lock);
}

int
pthread_rwlock_timedwrlock (pthread_rwlock_t *lock,
                            const struct timespec *abstime)
{
  int err;

  err = pthread_mutex_lock (&lock->lock);
  if (err != 0)
    return err;
  /* Test whether no readers or writers are currently running.  */
  while (!(lock->runcount == 0))
    {
      /* This thread has to wait for a while.  Enqueue it among the
         waiting_writers.  */
      lock->waiting_writers_count++;
      err = pthread_cond_timedwait (&lock->waiting_writers, &lock->lock,
                                    abstime);
      if (err != 0)
        {
          lock->waiting_writers_count--;
          pthread_mutex_unlock (&lock->lock);
          return err;
        }
      lock->waiting_writers_count--;
    }
  lock->runcount--; /* runcount becomes -1 */
  return pthread_mutex_unlock (&lock->lock);
}

int
pthread_rwlock_unlock (pthread_rwlock_t *lock)
{
  int err;

  err = pthread_mutex_lock (&lock->lock);
  if (err != 0)
    return err;
  if (lock->runcount < 0)
    {
      /* Drop a writer lock.  */
      if (!(lock->runcount == -1))
        {
          pthread_mutex_unlock (&lock->lock);
          return EINVAL;
        }
      lock->runcount = 0;
    }
  else
    {
      /* Drop a reader lock.  */
      if (!(lock->runcount > 0))
        {
          pthread_mutex_unlock (&lock->lock);
          return EINVAL;
        }
      lock->runcount--;
    }
  if (lock->runcount == 0)
    {
      /* POSIX recommends that "write locks shall take precedence over read
         locks", to avoid "writer starvation".  */
      if (lock->waiting_writers_count > 0)
        {
          /* Wake up one of the waiting writers.  */
          err = pthread_cond_signal (&lock->waiting_writers);
          if (err != 0)
            {
              pthread_mutex_unlock (&lock->lock);
              return err;
            }
        }
      else
        {
          /* Wake up all waiting readers.  */
          err = pthread_cond_broadcast (&lock->waiting_readers);
          if (err != 0)
            {
              pthread_mutex_unlock (&lock->lock);
              return err;
            }
        }
    }
  return pthread_mutex_unlock (&lock->lock);
}

int
pthread_rwlock_destroy (pthread_rwlock_t *lock)
{
  int err;

  err = pthread_mutex_destroy (&lock->lock);
  if (err != 0)
    return err;
  err = pthread_cond_destroy (&lock->waiting_readers);
  if (err != 0)
    return err;
  err = pthread_cond_destroy (&lock->waiting_writers);
  if (err != 0)
    return err;
  return 0;
}

# else

#  if PTHREAD_RWLOCK_BAD_WAITQUEUE

/* Override pthread_rwlock_init, to use the kind PREFER_WRITER_NONRECURSIVE
   (or possibly PREFER_WRITER) instead of the default, when no
   pthread_rwlockattr_t object is specified.  */
int
pthread_rwlock_init (pthread_rwlock_t *lock, const pthread_rwlockattr_t *attr)
#   undef pthread_rwlock_init
{
  int err;

  if (attr != NULL)
    err = pthread_rwlock_init (lock, attr);
  else
    {
      pthread_rwlockattr_t replacement_attr;

      err = pthread_rwlockattr_init (&replacement_attr);
      if (err != 0)
        return err;
      err = pthread_rwlockattr_setkind_np (&replacement_attr,
                                           PTHREAD_RWLOCK_PREFER_WRITER_NONRECURSIVE_NP);
      if (err != 0)
        {
          pthread_rwlockattr_destroy (&replacement_attr);
          return err;
        }
      err = pthread_rwlock_init (lock, &replacement_attr);
      pthread_rwlockattr_destroy (&replacement_attr);
    }
  return err;
}

#  endif

#  if PTHREAD_RWLOCK_LACKS_TIMEOUT

int
pthread_rwlock_timedrdlock (pthread_rwlock_t *lock,
                            const struct timespec *abstime)
{
  /* Poll the lock's state in regular intervals.  Ugh.  */
  for (;;)
    {
      int err;
      struct timeval currtime;
      unsigned long remaining;

      err = pthread_rwlock_tryrdlock (lock);
      if (err != EBUSY)
        return err;

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

      /* Sleep 1 ms.  */
      struct timespec duration =
        {
          .tv_sec = 0,
          .tv_nsec = MIN (1000000, remaining)
        };
      nanosleep (&duration, NULL);
    }
}

int
pthread_rwlock_timedwrlock (pthread_rwlock_t *lock,
                            const struct timespec *abstime)
{
  /* Poll the lock's state in regular intervals.  Ugh.  */
  for (;;)
    {
      int err;
      struct timeval currtime;
      unsigned long remaining;

      err = pthread_rwlock_trywrlock (lock);
      if (err != EBUSY)
        return err;

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

      /* Sleep 1 ms.  */
      struct timespec duration =
        {
          .tv_sec = 0,
          .tv_nsec = MIN (1000000, remaining)
        };
      nanosleep (&duration, NULL);
    }
}

#  endif

# endif

#else
/* Provide a dummy implementation for single-threaded applications.  */

/* The pthread_rwlock_t is an 'int', representing the number of readers running,
   or -1 when a writer runs.  */

int
pthread_rwlock_init (pthread_rwlock_t *lock,
                     _GL_UNUSED const pthread_rwlockattr_t *attr)
{
  *lock = 0;
  return 0;
}

int
pthread_rwlock_rdlock (pthread_rwlock_t *lock)
{
  if (*lock < 0)
    return EDEADLK;
  (*lock)++;
  return 0;
}

int
pthread_rwlock_wrlock (pthread_rwlock_t *lock)
{
  if (*lock != 0)
    return EDEADLK;
  *lock = -1;
  return 0;
}

int
pthread_rwlock_tryrdlock (pthread_rwlock_t *lock)
{
  return pthread_rwlock_rdlock (lock);
}

int
pthread_rwlock_trywrlock (pthread_rwlock_t *lock)
{
  return pthread_rwlock_wrlock (lock);
}

int
pthread_rwlock_timedrdlock (pthread_rwlock_t *lock,
                            _GL_UNUSED const struct timespec *abstime)
{
  return pthread_rwlock_rdlock (lock);
}

int
pthread_rwlock_timedwrlock (pthread_rwlock_t *lock,
                            _GL_UNUSED const struct timespec *abstime)
{
  return pthread_rwlock_wrlock (lock);
}

int
pthread_rwlock_unlock (pthread_rwlock_t *lock)
{
  if (*lock == 0)
    return EPERM;
  if (*lock < 0)
    *lock = 0;
  else /* *lock > 0 */
    (*lock)--;
  return 0;
}

int
pthread_rwlock_destroy (pthread_rwlock_t *lock)
{
  if (*lock)
    return EBUSY;
  return 0;
}

#endif
