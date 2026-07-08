/* Timed read-write locks (native Windows implementation).
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

/* Written by Bruno Haible <bruno@clisp.org>, 2019.  */

#include <config.h>

/* Specification.  */
#include "windows-timedrwlock.h"

#include <errno.h>
#include <stdlib.h>
#include <sys/time.h>

/* Don't assume that UNICODE is not defined.  */
#undef CreateEvent
#define CreateEvent CreateEventA

/* In this file, the waitqueues are implemented as linked lists.  */
#define glwthread_waitqueue_t glwthread_clinked_waitqueue_t

/* All links of a circular list, except the anchor, are of this type, carrying
   a payload.  */
struct glwthread_waitqueue_element
{
  struct glwthread_waitqueue_link link; /* must be the first field! */
  HANDLE event; /* Waiting thread, represented by an event.
                   This field is immutable once initialized. */
};

static void
glwthread_waitqueue_init (glwthread_waitqueue_t *wq)
{
  wq->wq_list.wql_next = &wq->wq_list;
  wq->wq_list.wql_prev = &wq->wq_list;
  wq->count = 0;
}

/* Enqueues the current thread, represented by an event, in a wait queue.
   Returns NULL if an allocation failure occurs.  */
static struct glwthread_waitqueue_element *
glwthread_waitqueue_add (glwthread_waitqueue_t *wq)
{
  struct glwthread_waitqueue_element *elt;
  HANDLE event;

  /* Allocate the memory for the waitqueue element on the heap, not on the
     thread's stack.  If the thread exits unexpectedly, we prefer to leak
     some memory rather than to access unavailable memory and crash.  */
  elt =
    (struct glwthread_waitqueue_element *)
    malloc (sizeof (struct glwthread_waitqueue_element));
  if (elt == NULL)
    /* No more memory.  */
    return NULL;

  /* Whether the created event is a manual-reset one or an auto-reset one,
     does not matter, since we will wait on it only once.  */
  event = CreateEvent (NULL, TRUE, FALSE, NULL);
  if (event == INVALID_HANDLE_VALUE)
    {
      /* No way to allocate an event.  */
      free (elt);
      return NULL;
    }
  elt->event = event;
  /* Insert elt at the end of the circular list.  */
  (elt->link.wql_prev = wq->wq_list.wql_prev)->wql_next = &elt->link;
  (elt->link.wql_next = &wq->wq_list)->wql_prev = &elt->link;
  wq->count++;
  return elt;
}

/* Removes the current thread, represented by a
   'struct glwthread_waitqueue_element *', from a wait queue.
   Returns true if is was found and removed, false if it was not present.  */
static bool
glwthread_waitqueue_remove (glwthread_waitqueue_t *wq,
                            struct glwthread_waitqueue_element *elt)
{
  if (elt->link.wql_next != NULL && elt->link.wql_prev != NULL)
    {
      /* Remove elt from the circular list.  */
      struct glwthread_waitqueue_link *prev = elt->link.wql_prev;
      struct glwthread_waitqueue_link *next = elt->link.wql_next;
      prev->wql_next = next;
      next->wql_prev = prev;
      elt->link.wql_next = NULL;
      elt->link.wql_prev = NULL;
      wq->count--;
      return true;
    }
  else
    return false;
}

/* Notifies the first thread from a wait queue and dequeues it.  */
static void
glwthread_waitqueue_notify_first (glwthread_waitqueue_t *wq)
{
  if (wq->wq_list.wql_next != &wq->wq_list)
    {
      struct glwthread_waitqueue_element *elt =
        (struct glwthread_waitqueue_element *) wq->wq_list.wql_next;
      struct glwthread_waitqueue_link *prev;
      struct glwthread_waitqueue_link *next;

      /* Remove elt from the circular list.  */
      prev = &wq->wq_list; /* = elt->link.wql_prev; */
      next = elt->link.wql_next;
      prev->wql_next = next;
      next->wql_prev = prev;
      elt->link.wql_next = NULL;
      elt->link.wql_prev = NULL;
      wq->count--;

      SetEvent (elt->event);
      /* After the SetEvent, this thread cannot access *elt any more, because
         the woken-up thread will quickly call  free (elt).  */
    }
}

/* Notifies all threads from a wait queue and dequeues them all.  */
static void
glwthread_waitqueue_notify_all (glwthread_waitqueue_t *wq)
{
  struct glwthread_waitqueue_link *l;

  for (l = wq->wq_list.wql_next; l != &wq->wq_list; )
    {
      struct glwthread_waitqueue_element *elt =
        (struct glwthread_waitqueue_element *) l;
      struct glwthread_waitqueue_link *prev;
      struct glwthread_waitqueue_link *next;

      /* Remove elt from the circular list.  */
      prev = &wq->wq_list; /* = elt->link.wql_prev; */
      next = elt->link.wql_next;
      prev->wql_next = next;
      next->wql_prev = prev;
      elt->link.wql_next = NULL;
      elt->link.wql_prev = NULL;
      wq->count--;

      SetEvent (elt->event);
      /* After the SetEvent, this thread cannot access *elt any more, because
         the woken-up thread will quickly call  free (elt).  */

      l = next;
    }
  if (!(wq->wq_list.wql_next == &wq->wq_list
        && wq->wq_list.wql_prev == &wq->wq_list
        && wq->count == 0))
    abort ();
}

void
glwthread_timedrwlock_init (glwthread_timedrwlock_t *lock)
{
  InitializeCriticalSection (&lock->lock);
  glwthread_waitqueue_init (&lock->waiting_readers);
  glwthread_waitqueue_init (&lock->waiting_writers);
  lock->runcount = 0;
  lock->guard.done = 1;
}

int
glwthread_timedrwlock_rdlock (glwthread_timedrwlock_t *lock)
{
  if (!lock->guard.done)
    {
      if (InterlockedIncrement (&lock->guard.started) == 0)
        /* This thread is the first one to need this lock.  Initialize it.  */
        glwthread_timedrwlock_init (lock);
      else
        {
          /* Don't let lock->guard.started grow and wrap around.  */
          InterlockedDecrement (&lock->guard.started);
          /* Yield the CPU while waiting for another thread to finish
             initializing this lock.  */
          while (!lock->guard.done)
            Sleep (0);
        }
    }
  EnterCriticalSection (&lock->lock);
  /* Test whether only readers are currently running, and whether the runcount
     field will not overflow, and whether no writer is waiting.  The latter
     condition is because POSIX recommends that "write locks shall take
     precedence over read locks", to avoid "writer starvation".  */
  if (!(lock->runcount + 1 > 0 && lock->waiting_writers.count == 0))
    {
      /* This thread has to wait for a while.  Enqueue it among the
         waiting_readers.  */
      struct glwthread_waitqueue_element *elt =
        glwthread_waitqueue_add (&lock->waiting_readers);
      if (elt != NULL)
        {
          HANDLE event = elt->event;
          DWORD result;
          LeaveCriticalSection (&lock->lock);
          /* Wait until another thread signals this event.  */
          result = WaitForSingleObject (event, INFINITE);
          if (result == WAIT_FAILED || result == WAIT_TIMEOUT)
            abort ();
          CloseHandle (event);
          free (elt);
          /* The thread which signalled the event already did the bookkeeping:
             removed us from the waiting_readers, incremented lock->runcount.  */
          if (!(lock->runcount > 0))
            abort ();
          return 0;
        }
      else
        {
          /* Allocation failure.  Weird.  */
          do
            {
              LeaveCriticalSection (&lock->lock);
              Sleep (1);
              EnterCriticalSection (&lock->lock);
            }
          while (!(lock->runcount + 1 > 0));
        }
    }
  lock->runcount++;
  LeaveCriticalSection (&lock->lock);
  return 0;
}

int
glwthread_timedrwlock_wrlock (glwthread_timedrwlock_t *lock)
{
  if (!lock->guard.done)
    {
      if (InterlockedIncrement (&lock->guard.started) == 0)
        /* This thread is the first one to need this lock.  Initialize it.  */
        glwthread_timedrwlock_init (lock);
      else
        {
          /* Don't let lock->guard.started grow and wrap around.  */
          InterlockedDecrement (&lock->guard.started);
          /* Yield the CPU while waiting for another thread to finish
             initializing this lock.  */
          while (!lock->guard.done)
            Sleep (0);
        }
    }
  EnterCriticalSection (&lock->lock);
  /* Test whether no readers or writers are currently running.  */
  if (!(lock->runcount == 0))
    {
      /* This thread has to wait for a while.  Enqueue it among the
         waiting_writers.  */
      struct glwthread_waitqueue_element *elt =
        glwthread_waitqueue_add (&lock->waiting_writers);
      if (elt != NULL)
        {
          HANDLE event = elt->event;
          DWORD result;
          LeaveCriticalSection (&lock->lock);
          /* Wait until another thread signals this event.  */
          result = WaitForSingleObject (event, INFINITE);
          if (result == WAIT_FAILED || result == WAIT_TIMEOUT)
            abort ();
          CloseHandle (event);
          free (elt);
          /* The thread which signalled the event already did the bookkeeping:
             removed us from the waiting_writers, set lock->runcount = -1.  */
          if (!(lock->runcount == -1))
            abort ();
          return 0;
        }
      else
        {
          /* Allocation failure.  Weird.  */
          do
            {
              LeaveCriticalSection (&lock->lock);
              Sleep (1);
              EnterCriticalSection (&lock->lock);
            }
          while (!(lock->runcount == 0));
        }
    }
  lock->runcount--; /* runcount becomes -1 */
  LeaveCriticalSection (&lock->lock);
  return 0;
}

int
glwthread_timedrwlock_tryrdlock (glwthread_timedrwlock_t *lock)
{
  if (!lock->guard.done)
    {
      if (InterlockedIncrement (&lock->guard.started) == 0)
        /* This thread is the first one to need this lock.  Initialize it.  */
        glwthread_timedrwlock_init (lock);
      else
        {
          /* Don't let lock->guard.started grow and wrap around.  */
          InterlockedDecrement (&lock->guard.started);
          /* Yield the CPU while waiting for another thread to finish
             initializing this lock.  */
          while (!lock->guard.done)
            Sleep (0);
        }
    }
  /* It's OK to wait for this critical section, because it is never taken for a
     long time.  */
  EnterCriticalSection (&lock->lock);
  /* Test whether only readers are currently running, and whether the runcount
     field will not overflow, and whether no writer is waiting.  The latter
     condition is because POSIX recommends that "write locks shall take
     precedence over read locks", to avoid "writer starvation".  */
  if (!(lock->runcount + 1 > 0 && lock->waiting_writers.count == 0))
    {
      /* This thread would have to wait for a while.  Return instead.  */
      LeaveCriticalSection (&lock->lock);
      return EBUSY;
    }
  lock->runcount++;
  LeaveCriticalSection (&lock->lock);
  return 0;
}

int
glwthread_timedrwlock_trywrlock (glwthread_timedrwlock_t *lock)
{
  if (!lock->guard.done)
    {
      if (InterlockedIncrement (&lock->guard.started) == 0)
        /* This thread is the first one to need this lock.  Initialize it.  */
        glwthread_timedrwlock_init (lock);
      else
        {
          /* Don't let lock->guard.started grow and wrap around.  */
          InterlockedDecrement (&lock->guard.started);
          /* Yield the CPU while waiting for another thread to finish
             initializing this lock.  */
          while (!lock->guard.done)
            Sleep (0);
        }
    }
  /* It's OK to wait for this critical section, because it is never taken for a
     long time.  */
  EnterCriticalSection (&lock->lock);
  /* Test whether no readers or writers are currently running.  */
  if (!(lock->runcount == 0))
    {
      /* This thread would have to wait for a while.  Return instead.  */
      LeaveCriticalSection (&lock->lock);
      return EBUSY;
    }
  lock->runcount--; /* runcount becomes -1 */
  LeaveCriticalSection (&lock->lock);
  return 0;
}

int
glwthread_timedrwlock_timedrdlock (glwthread_timedrwlock_t *lock,
                                   const struct timespec *abstime)
{
  if (!lock->guard.done)
    {
      if (InterlockedIncrement (&lock->guard.started) == 0)
        /* This thread is the first one to need this lock.  Initialize it.  */
        glwthread_timedrwlock_init (lock);
      else
        {
          /* Don't let lock->guard.started grow and wrap around.  */
          InterlockedDecrement (&lock->guard.started);
          /* Yield the CPU while waiting for another thread to finish
             initializing this lock.  */
          while (!lock->guard.done)
            Sleep (0);
        }
    }
  EnterCriticalSection (&lock->lock);
  /* Test whether only readers are currently running, and whether the runcount
     field will not overflow, and whether no writer is waiting.  The latter
     condition is because POSIX recommends that "write locks shall take
     precedence over read locks", to avoid "writer starvation".  */
  if (!(lock->runcount + 1 > 0 && lock->waiting_writers.count == 0))
    {
      /* This thread has to wait for a while.  Enqueue it among the
         waiting_readers.  */
      struct glwthread_waitqueue_element *elt =
        glwthread_waitqueue_add (&lock->waiting_readers);
      if (elt != NULL)
        {
          HANDLE event = elt->event;
          struct timeval currtime;
          DWORD timeout;
          DWORD result;
          int retval;

          LeaveCriticalSection (&lock->lock);

          gettimeofday (&currtime, NULL);

          /* Wait until another thread signals this event or until the
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
          if (timeout != 0)
            {
              /* WaitForSingleObject
                 <https://docs.microsoft.com/en-us/windows/desktop/api/synchapi/nf-synchapi-waitforsingleobject> */
              result = WaitForSingleObject (event, timeout);
              if (result == WAIT_FAILED)
                abort ();
              if (result != WAIT_TIMEOUT)
                {
                  CloseHandle (event);
                  free (elt);
                  /* The thread which signalled the event already did the
                     bookkeeping: removed us from the waiting_readers,
                     incremented lock->runcount.  */
                  if (!(lock->runcount > 0))
                    abort ();
                  return 0;
                }
            }
          EnterCriticalSection (&lock->lock);
          /* Remove ourselves from the waiting_readers.  */
          if (glwthread_waitqueue_remove (&lock->waiting_readers, elt))
            retval = ETIMEDOUT;
          else
            /* The event was signalled just now.  */
            retval = 0;
          LeaveCriticalSection (&lock->lock);
          CloseHandle (event);
          free (elt);
          if (retval == 0)
            /* Same assertion as above.  */
            if (!(lock->runcount > 0))
              abort ();
          return retval;
        }
      else
        {
          /* Allocation failure.  Weird.  */
          do
            {
              LeaveCriticalSection (&lock->lock);
              Sleep (1);
              EnterCriticalSection (&lock->lock);
            }
          while (!(lock->runcount + 1 > 0));
        }
    }
  lock->runcount++;
  LeaveCriticalSection (&lock->lock);
  return 0;
}

int
glwthread_timedrwlock_timedwrlock (glwthread_timedrwlock_t *lock,
                                   const struct timespec *abstime)
{
  if (!lock->guard.done)
    {
      if (InterlockedIncrement (&lock->guard.started) == 0)
        /* This thread is the first one to need this lock.  Initialize it.  */
        glwthread_timedrwlock_init (lock);
      else
        {
          /* Don't let lock->guard.started grow and wrap around.  */
          InterlockedDecrement (&lock->guard.started);
          /* Yield the CPU while waiting for another thread to finish
             initializing this lock.  */
          while (!lock->guard.done)
            Sleep (0);
        }
    }
  EnterCriticalSection (&lock->lock);
  /* Test whether no readers or writers are currently running.  */
  if (!(lock->runcount == 0))
    {
      /* This thread has to wait for a while.  Enqueue it among the
         waiting_writers.  */
      struct glwthread_waitqueue_element *elt =
        glwthread_waitqueue_add (&lock->waiting_writers);
      if (elt != NULL)
        {
          HANDLE event = elt->event;
          struct timeval currtime;
          DWORD timeout;
          DWORD result;
          int retval;

          LeaveCriticalSection (&lock->lock);

          gettimeofday (&currtime, NULL);

          /* Wait until another thread signals this event or until the
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
          if (timeout != 0)
            {
              /* WaitForSingleObject
                 <https://docs.microsoft.com/en-us/windows/desktop/api/synchapi/nf-synchapi-waitforsingleobject> */
              result = WaitForSingleObject (event, timeout);
              if (result == WAIT_FAILED)
                abort ();
              if (result != WAIT_TIMEOUT)
                {
                  CloseHandle (event);
                  free (elt);
                  /* The thread which signalled the event already did the
                     bookkeeping: removed us from the waiting_writers, set
                     lock->runcount = -1.  */
                  if (!(lock->runcount == -1))
                    abort ();
                  return 0;
                }
            }
          EnterCriticalSection (&lock->lock);
          /* Remove ourselves from the waiting_writers.  */
          if (glwthread_waitqueue_remove (&lock->waiting_writers, elt))
            retval = ETIMEDOUT;
          else
            /* The event was signalled just now.  */
            retval = 0;
          LeaveCriticalSection (&lock->lock);
          CloseHandle (event);
          free (elt);
          if (retval == 0)
            /* Same assertion as above.  */
            if (!(lock->runcount == -1))
              abort ();
          return retval;
        }
      else
        {
          /* Allocation failure.  Weird.  */
          do
            {
              LeaveCriticalSection (&lock->lock);
              Sleep (1);
              EnterCriticalSection (&lock->lock);
            }
          while (!(lock->runcount == 0));
        }
    }
  lock->runcount--; /* runcount becomes -1 */
  LeaveCriticalSection (&lock->lock);
  return 0;
}

int
glwthread_timedrwlock_unlock (glwthread_timedrwlock_t *lock)
{
  if (!lock->guard.done)
    return EINVAL;
  EnterCriticalSection (&lock->lock);
  if (lock->runcount < 0)
    {
      /* Drop a writer lock.  */
      if (!(lock->runcount == -1))
        abort ();
      lock->runcount = 0;
    }
  else
    {
      /* Drop a reader lock.  */
      if (!(lock->runcount > 0))
        {
          LeaveCriticalSection (&lock->lock);
          return EPERM;
        }
      lock->runcount--;
    }
  if (lock->runcount == 0)
    {
      /* POSIX recommends that "write locks shall take precedence over read
         locks", to avoid "writer starvation".  */
      if (lock->waiting_writers.count > 0)
        {
          /* Wake up one of the waiting writers.  */
          lock->runcount--;
          glwthread_waitqueue_notify_first (&lock->waiting_writers);
        }
      else
        {
          /* Wake up all waiting readers.  */
          lock->runcount += lock->waiting_readers.count;
          glwthread_waitqueue_notify_all (&lock->waiting_readers);
        }
    }
  LeaveCriticalSection (&lock->lock);
  return 0;
}

int
glwthread_timedrwlock_destroy (glwthread_timedrwlock_t *lock)
{
  if (!lock->guard.done)
    return EINVAL;
  if (lock->runcount != 0)
    return EBUSY;
  DeleteCriticalSection (&lock->lock);
  lock->guard.done = 0;
  return 0;
}
