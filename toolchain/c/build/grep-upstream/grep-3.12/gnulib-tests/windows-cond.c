/* Condition variables (native Windows implementation).
   Copyright (C) 2008-2025 Free Software Foundation, Inc.

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

/* Written by Yoann Vandoorselaere <yoann@prelude-ids.org>, 2008,
   and Bruno Haible <bruno@clisp.org>, 2008.  */

#include <config.h>

/* Specification.  */
#include "windows-cond.h"

#include <errno.h>
#include <stdlib.h>
#include <sys/time.h>

/* Don't assume that UNICODE is not defined.  */
#undef CreateEvent
#define CreateEvent CreateEventA

/* In this file, the waitqueues are implemented as linked lists.  */
#define glwthread_waitqueue_t glwthread_linked_waitqueue_t

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

      SetEvent (elt->event);
      /* After the SetEvent, this thread cannot access *elt any more, because
         the woken-up thread will quickly call  free (elt).  */

      l = next;
    }
  if (!(wq->wq_list.wql_next == &wq->wq_list
        && wq->wq_list.wql_prev == &wq->wq_list))
    abort ();
}

int
glwthread_cond_init (glwthread_cond_t *cond)
{
  InitializeCriticalSection (&cond->lock);
  glwthread_waitqueue_init (&cond->waiters);

  cond->guard.done = 1;
  return 0;
}

int
glwthread_cond_wait (glwthread_cond_t *cond,
                     void *mutex, int (*mutex_lock) (void *), int (*mutex_unlock) (void *))
{
  if (!cond->guard.done)
    {
      if (InterlockedIncrement (&cond->guard.started) == 0)
        /* This thread is the first one to need this condition variable.
           Initialize it.  */
        glwthread_cond_init (cond);
      else
        {
          /* Don't let cond->guard.started grow and wrap around.  */
          InterlockedDecrement (&cond->guard.started);
          /* Yield the CPU while waiting for another thread to finish
             initializing this condition variable.  */
          while (!cond->guard.done)
            Sleep (0);
        }
    }

  EnterCriticalSection (&cond->lock);
  {
    struct glwthread_waitqueue_element *elt =
      glwthread_waitqueue_add (&cond->waiters);
    LeaveCriticalSection (&cond->lock);
    if (elt == NULL)
      {
        /* Allocation failure.  Weird.  */
        return EAGAIN;
      }
    else
      {
        HANDLE event = elt->event;
        int err;
        DWORD result;

        /* Now release the mutex and let any other thread take it.  */
        err = mutex_unlock (mutex);
        if (err != 0)
          {
            EnterCriticalSection (&cond->lock);
            glwthread_waitqueue_remove (&cond->waiters, elt);
            LeaveCriticalSection (&cond->lock);
            CloseHandle (event);
            free (elt);
            return err;
          }
        /* POSIX says:
            "If another thread is able to acquire the mutex after the
             about-to-block thread has released it, then a subsequent call to
             pthread_cond_broadcast() or pthread_cond_signal() in that thread
             shall behave as if it were issued after the about-to-block thread
             has blocked."
           This is fulfilled here, because the thread signalling is done
           through SetEvent, not PulseEvent.  */
        /* Wait until another thread signals this event.  */
        result = WaitForSingleObject (event, INFINITE);
        if (result == WAIT_FAILED || result == WAIT_TIMEOUT)
          abort ();
        CloseHandle (event);
        free (elt);
        /* The thread which signalled the event already did the bookkeeping:
           removed us from the waiters.  */
        return mutex_lock (mutex);
      }
  }
}

int
glwthread_cond_timedwait (glwthread_cond_t *cond,
                          void *mutex, int (*mutex_lock) (void *), int (*mutex_unlock) (void *),
                          const struct timespec *abstime)
{
  if (!cond->guard.done)
    {
      if (InterlockedIncrement (&cond->guard.started) == 0)
        /* This thread is the first one to need this condition variable.
           Initialize it.  */
        glwthread_cond_init (cond);
      else
        {
          /* Don't let cond->guard.started grow and wrap around.  */
          InterlockedDecrement (&cond->guard.started);
          /* Yield the CPU while waiting for another thread to finish
             initializing this condition variable.  */
          while (!cond->guard.done)
            Sleep (0);
        }
    }

  {
    struct timeval currtime;

    gettimeofday (&currtime, NULL);
    if (currtime.tv_sec > abstime->tv_sec
        || (currtime.tv_sec == abstime->tv_sec
            && currtime.tv_usec * 1000 >= abstime->tv_nsec))
      return ETIMEDOUT;

    EnterCriticalSection (&cond->lock);
    {
      struct glwthread_waitqueue_element *elt =
        glwthread_waitqueue_add (&cond->waiters);
      LeaveCriticalSection (&cond->lock);
      if (elt == NULL)
        {
          /* Allocation failure.  Weird.  */
          return EAGAIN;
        }
      else
        {
          HANDLE event = elt->event;
          int err;
          DWORD timeout;
          DWORD result;

          /* Now release the mutex and let any other thread take it.  */
          err = mutex_unlock (mutex);
          if (err != 0)
            {
              EnterCriticalSection (&cond->lock);
              glwthread_waitqueue_remove (&cond->waiters, elt);
              LeaveCriticalSection (&cond->lock);
              CloseHandle (event);
              free (elt);
              return err;
            }
          /* POSIX says:
              "If another thread is able to acquire the mutex after the
               about-to-block thread has released it, then a subsequent call to
               pthread_cond_broadcast() or pthread_cond_signal() in that thread
               shall behave as if it were issued after the about-to-block thread
               has blocked."
             This is fulfilled here, because the thread signalling is done
             through SetEvent, not PulseEvent.  */
          /* Wait until another thread signals this event or until the abstime
             passes.  */
          gettimeofday (&currtime, NULL);
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
          result = WaitForSingleObject (event, timeout);
          if (result == WAIT_FAILED)
            abort ();
          if (result == WAIT_TIMEOUT)
            {
              EnterCriticalSection (&cond->lock);
              if (glwthread_waitqueue_remove (&cond->waiters, elt))
                {
                  /* The event was not signaled between the WaitForSingleObject
                     call and the EnterCriticalSection call.  */
                  if (!(WaitForSingleObject (event, 0) == WAIT_TIMEOUT))
                    abort ();
                }
              else
                {
                  /* The event was signaled between the WaitForSingleObject
                     call and the EnterCriticalSection call.  */
                  if (!(WaitForSingleObject (event, 0) == WAIT_OBJECT_0))
                    abort ();
                  /* Produce the right return value.  */
                  result = WAIT_OBJECT_0;
                }
              LeaveCriticalSection (&cond->lock);
            }
          else
            {
              /* The thread which signalled the event already did the
                 bookkeeping: removed us from the waiters.  */
            }
          CloseHandle (event);
          free (elt);
          /* Take the mutex again.  It does not matter whether this is done
             before or after the bookkeeping for WAIT_TIMEOUT.  */
          err = mutex_lock (mutex);
          return (err ? err :
                  result == WAIT_OBJECT_0 ? 0 :
                  result == WAIT_TIMEOUT ? ETIMEDOUT :
                  /* WAIT_FAILED shouldn't happen */ EAGAIN);
        }
    }
  }
}

int
glwthread_cond_signal (glwthread_cond_t *cond)
{
  if (!cond->guard.done)
    return EINVAL;

  EnterCriticalSection (&cond->lock);
  /* POSIX says:
      "The pthread_cond_broadcast() and pthread_cond_signal() functions shall
       have no effect if there are no threads currently blocked on cond."  */
  if (cond->waiters.wq_list.wql_next != &cond->waiters.wq_list)
    glwthread_waitqueue_notify_first (&cond->waiters);
  LeaveCriticalSection (&cond->lock);

  return 0;
}

int
glwthread_cond_broadcast (glwthread_cond_t *cond)
{
  if (!cond->guard.done)
    return EINVAL;

  EnterCriticalSection (&cond->lock);
  /* POSIX says:
      "The pthread_cond_broadcast() and pthread_cond_signal() functions shall
       have no effect if there are no threads currently blocked on cond."
     glwthread_waitqueue_notify_all is a nop in this case.  */
  glwthread_waitqueue_notify_all (&cond->waiters);
  LeaveCriticalSection (&cond->lock);

  return 0;
}

int
glwthread_cond_destroy (glwthread_cond_t *cond)
{
  if (!cond->guard.done)
    return EINVAL;
  if (cond->waiters.wq_list.wql_next != &cond->waiters.wq_list)
    return EBUSY;
  DeleteCriticalSection (&cond->lock);
  cond->guard.done = 0;
  return 0;
}
