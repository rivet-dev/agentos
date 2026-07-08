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

/* Written by Yoann Vandoorselaere <yoann@prelude-ids.org>, 2008.
   Based on Bruno Haible <bruno@clisp.org> lock.h */

#ifndef _WINDOWS_COND_H
#define _WINDOWS_COND_H

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
        }
        glwthread_linked_waitqueue_t;

typedef struct
        {
          glwthread_initguard_t guard; /* protects the initialization */
          CRITICAL_SECTION lock; /* protects the remaining fields */
          glwthread_linked_waitqueue_t waiters; /* waiting threads */
        }
        glwthread_cond_t;

#define GLWTHREAD_COND_INIT { GLWTHREAD_INITGUARD_INIT }

#ifdef __cplusplus
extern "C" {
#endif

extern int glwthread_cond_init (glwthread_cond_t *cond);
/* Here, to cope with the various types of mutexes, the mutex is a 'void *', and
   the caller needs to pass the corresponding *_lock and *_unlock functions.  */
extern int glwthread_cond_wait (glwthread_cond_t *cond,
                                void *mutex,
                                int (*mutex_lock) (void *),
                                int (*mutex_unlock) (void *));
extern int glwthread_cond_timedwait (glwthread_cond_t *cond,
                                     void *mutex,
                                     int (*mutex_lock) (void *),
                                     int (*mutex_unlock) (void *),
                                     const struct timespec *abstime);
extern int glwthread_cond_signal (glwthread_cond_t *cond);
extern int glwthread_cond_broadcast (glwthread_cond_t *cond);
extern int glwthread_cond_destroy (glwthread_cond_t *cond);

#ifdef __cplusplus
}
#endif

#endif /* _WINDOWS_COND_H */
