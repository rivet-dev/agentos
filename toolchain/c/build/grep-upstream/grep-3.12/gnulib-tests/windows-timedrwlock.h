/* Timed read-write locks (native Windows implementation).
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

#ifndef _WINDOWS_TIMEDRWLOCK_H
#define _WINDOWS_TIMEDRWLOCK_H

#define WIN32_LEAN_AND_MEAN  /* avoid including junk */
#include <windows.h>

#include <time.h>

#include "windows-initguard.h"

#ifndef _glwthread_linked_waitqueue_link_defined
#define _glwthread_linked_waitqueue_link_defined
struct glwthread_waitqueue_link
{
  struct glwthread_waitqueue_link *wql_next;
  struct glwthread_waitqueue_link *wql_prev;
};
#endif /* _glwthread_linked_waitqueue_link_defined */
typedef struct
        {
          struct glwthread_waitqueue_link wq_list; /* circular list of waiting threads */
          unsigned int count; /* number of waiting threads */
        }
        glwthread_clinked_waitqueue_t;

typedef struct
        {
          glwthread_initguard_t guard; /* protects the initialization */
          CRITICAL_SECTION lock; /* protects the remaining fields */
          glwthread_clinked_waitqueue_t waiting_readers; /* waiting readers */
          glwthread_clinked_waitqueue_t waiting_writers; /* waiting writers */
          int runcount; /* number of readers running, or -1 when a writer runs */
        }
        glwthread_timedrwlock_t;

#define GLWTHREAD_TIMEDRWLOCK_INIT { GLWTHREAD_INITGUARD_INIT }

#ifdef __cplusplus
extern "C" {
#endif

extern void glwthread_timedrwlock_init (glwthread_timedrwlock_t *lock);
extern int glwthread_timedrwlock_rdlock (glwthread_timedrwlock_t *lock);
extern int glwthread_timedrwlock_wrlock (glwthread_timedrwlock_t *lock);
extern int glwthread_timedrwlock_tryrdlock (glwthread_timedrwlock_t *lock);
extern int glwthread_timedrwlock_trywrlock (glwthread_timedrwlock_t *lock);
extern int glwthread_timedrwlock_timedrdlock (glwthread_timedrwlock_t *lock,
                                              const struct timespec *abstime);
extern int glwthread_timedrwlock_timedwrlock (glwthread_timedrwlock_t *lock,
                                              const struct timespec *abstime);
extern int glwthread_timedrwlock_unlock (glwthread_timedrwlock_t *lock);
extern int glwthread_timedrwlock_destroy (glwthread_timedrwlock_t *lock);

#ifdef __cplusplus
}
#endif

#endif /* _WINDOWS_TIMEDRWLOCK_H */
