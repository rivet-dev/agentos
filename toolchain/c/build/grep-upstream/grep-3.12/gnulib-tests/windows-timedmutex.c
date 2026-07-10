/* Timed mutexes (native Windows implementation).
   Copyright (C) 2005-2025 Free Software Foundation, Inc.

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

/* Written by Bruno Haible <bruno@clisp.org>, 2005, 2019.
   Based on GCC's gthr-win32.h.  */

#include <config.h>

/* Specification.  */
#include "windows-timedmutex.h"

#include <errno.h>
#include <stdlib.h>
#include <sys/time.h>

/* Don't assume that UNICODE is not defined.  */
#undef CreateEvent
#define CreateEvent CreateEventA

int
glwthread_timedmutex_init (glwthread_timedmutex_t *mutex)
{
  /* Attempt to allocate an auto-reset event object.  */
  /* CreateEvent
     <https://docs.microsoft.com/en-us/windows/desktop/api/synchapi/nf-synchapi-createeventa> */
  HANDLE event = CreateEvent (NULL, FALSE, FALSE, NULL);
  if (event == INVALID_HANDLE_VALUE)
    return EAGAIN;
  mutex->event = event;
  mutex->owner = 0;
  InitializeCriticalSection (&mutex->lock);
  mutex->guard.done = 1;
  return 0;
}

int
glwthread_timedmutex_lock (glwthread_timedmutex_t *mutex)
{
  if (!mutex->guard.done)
    {
      if (InterlockedIncrement (&mutex->guard.started) == 0)
        {
          /* This thread is the first one to need this mutex.
             Initialize it.  */
          int err = glwthread_timedmutex_init (mutex);
          if (err != 0)
            {
              /* Undo increment.  */
              InterlockedDecrement (&mutex->guard.started);
              return err;
            }
        }
      else
        {
          /* Don't let mutex->guard.started grow and wrap around.  */
          InterlockedDecrement (&mutex->guard.started);
          /* Yield the CPU while waiting for another thread to finish
             initializing this mutex.  */
          while (!mutex->guard.done)
            Sleep (0);
        }
    }
  /* If this thread already owns the mutex, POSIX pthread_mutex_lock() is
     required to deadlock here.  But let's not do that on purpose.  */
  EnterCriticalSection (&mutex->lock);
  {
    DWORD self = GetCurrentThreadId ();
    mutex->owner = self;
  }
  return 0;
}

int
glwthread_timedmutex_trylock (glwthread_timedmutex_t *mutex)
{
  if (!mutex->guard.done)
    {
      if (InterlockedIncrement (&mutex->guard.started) == 0)
        {
          /* This thread is the first one to need this mutex.
             Initialize it.  */
          int err = glwthread_timedmutex_init (mutex);
          if (err != 0)
            {
              /* Undo increment.  */
              InterlockedDecrement (&mutex->guard.started);
              return err;
            }
        }
      else
        {
          /* Don't let mutex->guard.started grow and wrap around.  */
          InterlockedDecrement (&mutex->guard.started);
          /* Let another thread finish initializing this mutex, and let it also
             lock this mutex.  */
          return EBUSY;
        }
    }
  if (!TryEnterCriticalSection (&mutex->lock))
    return EBUSY;
  {
    DWORD self = GetCurrentThreadId ();
    /* TryEnterCriticalSection succeeded.  This means that the mutex was either
       previously unlocked (and thus mutex->owner == 0) or previously locked by
       this thread (and thus mutex->owner == self).  Since the mutex is meant to
       be plain, we need to fail in the latter case.  */
    if (mutex->owner == self)
      {
        LeaveCriticalSection (&mutex->lock);
        return EBUSY;
      }
    if (mutex->owner != 0)
      abort ();
    mutex->owner = self;
  }
  return 0;
}

int
glwthread_timedmutex_timedlock (glwthread_timedmutex_t *mutex,
                                const struct timespec *abstime)
{
  if (!mutex->guard.done)
    {
      if (InterlockedIncrement (&mutex->guard.started) == 0)
        {
          /* This thread is the first one to need this mutex.
             Initialize it.  */
          int err = glwthread_timedmutex_init (mutex);
          if (err != 0)
            {
              /* Undo increment.  */
              InterlockedDecrement (&mutex->guard.started);
              return err;
            }
        }
      else
        {
          /* Don't let mutex->guard.started grow and wrap around.  */
          InterlockedDecrement (&mutex->guard.started);
          /* Yield the CPU while waiting for another thread to finish
             initializing this mutex.  */
          while (!mutex->guard.done)
            Sleep (0);
        }
    }

  /* POSIX says:
      "Under no circumstance shall the function fail with a timeout if
       the mutex can be locked immediately. The validity of the abstime
       parameter need not be checked if the mutex can be locked
       immediately."
     Therefore start the loop with a TryEnterCriticalSection call.  */
  for (;;)
    {
      if (TryEnterCriticalSection (&mutex->lock))
        break;

      {
        struct timeval currtime;
        DWORD timeout;
        DWORD result;

        gettimeofday (&currtime, NULL);

        /* Wait until another thread signals the event or until the
           abstime passes.  */
        if (currtime.tv_sec > abstime->tv_sec)
          timeout = 0;
        else
          {
            unsigned long seconds = abstime->tv_sec - currtime.tv_sec;
            timeout = seconds * 1000;
            if (timeout / 1000 != seconds) /* overflow? */
              timeout = INFINITE;
            else
              {
                long milliseconds =
                  abstime->tv_nsec / 1000000 - currtime.tv_usec / 1000;
                if (milliseconds >= 0)
                  {
                    timeout += milliseconds;
                    if (timeout < milliseconds) /* overflow? */
                      timeout = INFINITE;
                  }
                else
                  {
                    if (timeout >= - milliseconds)
                      timeout -= (- milliseconds);
                    else
                      timeout = 0;
                  }
              }
          }
        if (timeout == 0)
          return ETIMEDOUT;

        /* WaitForSingleObject
           <https://docs.microsoft.com/en-us/windows/desktop/api/synchapi/nf-synchapi-waitforsingleobject> */
        result = WaitForSingleObject (mutex->event, timeout);
        if (result == WAIT_FAILED)
          abort ();
        if (result == WAIT_TIMEOUT)
          return ETIMEDOUT;
        /* Another thread has just unlocked the mutex.  We have good chances at
           locking it now.  */
      }
    }
  {
    DWORD self = GetCurrentThreadId ();
    /* TryEnterCriticalSection succeeded.  This means that the mutex was either
       previously unlocked (and thus mutex->owner == 0) or previously locked by
       this thread (and thus mutex->owner == self).  Since the mutex is meant to
       be plain, it is useful to fail in the latter case.  */
    if (mutex->owner == self)
      {
        LeaveCriticalSection (&mutex->lock);
        return EDEADLK;
      }
    if (mutex->owner != 0)
      abort ();
    mutex->owner = self;
  }
  return 0;
}

int
glwthread_timedmutex_unlock (glwthread_timedmutex_t *mutex)
{
  if (!mutex->guard.done)
    return EINVAL;
  mutex->owner = 0;
  LeaveCriticalSection (&mutex->lock);
  /* Notify one of the threads that were waiting with a timeout.  */
  /* SetEvent
     <https://docs.microsoft.com/en-us/windows/desktop/api/synchapi/nf-synchapi-setevent> */
  SetEvent (mutex->event);
  return 0;
}

int
glwthread_timedmutex_destroy (glwthread_timedmutex_t *mutex)
{
  if (!mutex->guard.done)
    return EINVAL;
  DeleteCriticalSection (&mutex->lock);
  /* CloseHandle
     <https://docs.microsoft.com/en-us/windows/desktop/api/handleapi/nf-handleapi-closehandle> */
  CloseHandle (mutex->event);
  mutex->guard.done = 0;
  return 0;
}
