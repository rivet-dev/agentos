# pthread-rwlock.m4
# serial 8
dnl Copyright (C) 2019-2025 Free Software Foundation, Inc.
dnl This file is free software; the Free Software Foundation
dnl gives unlimited permission to copy and/or distribute it,
dnl with or without modifications, as long as this notice is preserved.
dnl This file is offered as-is, without any warranty.

AC_DEFUN([gl_PTHREAD_RWLOCK],
[
  AC_REQUIRE([gl_PTHREAD_H])
  AC_REQUIRE([AC_CANONICAL_HOST])

  if { case "$host_os" in mingw* | windows*) true;; *) false;; esac; } \
     && test $gl_threads_api = windows; then
    dnl Choose function names that don't conflict with the mingw-w64 winpthreads
    dnl library.
    REPLACE_PTHREAD_RWLOCK_INIT=1
    REPLACE_PTHREAD_RWLOCKATTR_INIT=1
    REPLACE_PTHREAD_RWLOCKATTR_DESTROY=1
    REPLACE_PTHREAD_RWLOCK_RDLOCK=1
    REPLACE_PTHREAD_RWLOCK_WRLOCK=1
    REPLACE_PTHREAD_RWLOCK_TRYRDLOCK=1
    REPLACE_PTHREAD_RWLOCK_TRYWRLOCK=1
    REPLACE_PTHREAD_RWLOCK_TIMEDRDLOCK=1
    REPLACE_PTHREAD_RWLOCK_TIMEDWRLOCK=1
    REPLACE_PTHREAD_RWLOCK_UNLOCK=1
    REPLACE_PTHREAD_RWLOCK_DESTROY=1
  else
    if test $HAVE_PTHREAD_H = 0; then
      HAVE_PTHREAD_RWLOCK_INIT=0
      HAVE_PTHREAD_RWLOCKATTR_INIT=0
      HAVE_PTHREAD_RWLOCKATTR_DESTROY=0
      HAVE_PTHREAD_RWLOCK_RDLOCK=0
      HAVE_PTHREAD_RWLOCK_WRLOCK=0
      HAVE_PTHREAD_RWLOCK_TRYRDLOCK=0
      HAVE_PTHREAD_RWLOCK_TRYWRLOCK=0
      HAVE_PTHREAD_RWLOCK_TIMEDRDLOCK=0
      HAVE_PTHREAD_RWLOCK_TIMEDWRLOCK=0
      HAVE_PTHREAD_RWLOCK_UNLOCK=0
      HAVE_PTHREAD_RWLOCK_DESTROY=0
    else
      dnl On Mac OS X 10.4, the pthread_rwlock_* functions exist but are not
      dnl usable because PTHREAD_RWLOCK_INITIALIZER is not defined.
      dnl On Android 4.3, the pthread_rwlock_* functions are declared in
      dnl <pthread.h> but don't exist in libc.
      AC_CACHE_CHECK([for pthread_rwlock_init],
        [gl_cv_func_pthread_rwlock_init],
        [case "$host_os" in
           darwin*)
             AC_COMPILE_IFELSE(
               [AC_LANG_SOURCE(
                  [[#include <pthread.h>
                    pthread_rwlock_t l = PTHREAD_RWLOCK_INITIALIZER;
                  ]])],
               [gl_cv_func_pthread_rwlock_init=yes],
               [gl_cv_func_pthread_rwlock_init=no])
             ;;
           *)
             saved_LIBS="$LIBS"
             LIBS="$LIBS $LIBPMULTITHREAD"
             AC_LINK_IFELSE(
               [AC_LANG_SOURCE(
                  [[extern
                    #ifdef __cplusplus
                    "C"
                    #endif
                    int pthread_rwlock_init (void);
                    int main ()
                    {
                      return pthread_rwlock_init ();
                    }
                  ]])],
               [gl_cv_func_pthread_rwlock_init=yes],
               [gl_cv_func_pthread_rwlock_init=no])
             LIBS="$saved_LIBS"
             ;;
         esac
        ])
      if test $gl_cv_func_pthread_rwlock_init = no; then
        REPLACE_PTHREAD_RWLOCK_INIT=1
        REPLACE_PTHREAD_RWLOCKATTR_INIT=1
        REPLACE_PTHREAD_RWLOCKATTR_DESTROY=1
        REPLACE_PTHREAD_RWLOCK_RDLOCK=1
        REPLACE_PTHREAD_RWLOCK_WRLOCK=1
        REPLACE_PTHREAD_RWLOCK_TRYRDLOCK=1
        REPLACE_PTHREAD_RWLOCK_TRYWRLOCK=1
        REPLACE_PTHREAD_RWLOCK_TIMEDRDLOCK=1
        REPLACE_PTHREAD_RWLOCK_TIMEDWRLOCK=1
        REPLACE_PTHREAD_RWLOCK_UNLOCK=1
        REPLACE_PTHREAD_RWLOCK_DESTROY=1
        AC_DEFINE([PTHREAD_RWLOCK_UNIMPLEMENTED], [1],
          [Define if all pthread_rwlock* functions don't exist.])
      else
        dnl On Mac OS X 10.5, FreeBSD 5.2.1, OpenBSD 3.8, AIX 5.1, HP-UX 11,
        dnl IRIX 6.5, Solaris 9, Cygwin, the pthread_rwlock_timed*lock functions
        dnl don't exist, although the other pthread_rwlock* functions exist.
        AC_CHECK_DECL([pthread_rwlock_timedrdlock], ,
          [HAVE_PTHREAD_RWLOCK_TIMEDRDLOCK=0
           HAVE_PTHREAD_RWLOCK_TIMEDWRLOCK=0
           AC_DEFINE([PTHREAD_RWLOCK_LACKS_TIMEOUT], [1],
             [Define if the functions pthread_rwlock_timedrdlock and pthread_rwlock_timedwrlock don't exist.])
          ],
          [[#include <pthread.h>]])
        dnl In glibc ≥ 2.25 on Linux, test-pthread-rwlock-waitqueue reports
        dnl "This implementation always prefers readers.", and this wait queue
        dnl handling is unsuitable, because it leads to writer starvation:
        dnl On machines with 8 or more CPUs, test-pthread-rwlock may never
        dnl terminate. See
        dnl <https://lists.gnu.org/archive/html/bug-gnulib/2024-06/msg00291.html>
        dnl <https://lists.gnu.org/archive/html/bug-gnulib/2024-07/msg00081.html>
        dnl for details.
        AC_CACHE_CHECK([for reasonable pthread_rwlock wait queue handling],
          [gl_cv_func_pthread_rwlock_good_waitqueue],
          [case "$host_os" in
             linux*-gnu*)
               saved_LIBS="$LIBS"
               LIBS="$LIBS $LIBPMULTITHREAD"
               AC_RUN_IFELSE(
                 [AC_LANG_SOURCE([[
/* This test is a simplified variant of tests/test-pthread-rwlock-waitqueue.c.  */
#include <pthread.h>
#include <stdbool.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <time.h>

#if defined __hppa
# define STEP_INTERVAL 20000000 /* nanoseconds */
#else
# define STEP_INTERVAL 10000000 /* nanoseconds */
#endif

static pthread_rwlock_t lock;

static pthread_rwlock_t sprintf_lock;

struct locals
{
  const char *name;
  unsigned int wait_before;
  unsigned int wait_after;
  char *result;
};

static void *
reader_func (void *arg)
{
  struct locals *l = arg;
  int err;

  if (l->wait_before > 0)
    {
      struct timespec duration;
      duration.tv_sec  = l->wait_before / 1000000000;
      duration.tv_nsec = l->wait_before % 1000000000;
      nanosleep (&duration, NULL);
    }
  err = pthread_rwlock_rdlock (&lock);
  if (err)
    {
      fprintf (stderr, "pthread_rwlock_rdlock failed, error = %d\n", err);
      abort ();
    }
  if (pthread_rwlock_wrlock (&sprintf_lock))
    {
      fprintf (stderr, "pthread_rwlock_wrlock on sprintf_lock failed\n");
      abort ();
    }
  sprintf (l->result + strlen (l->result), " %s", l->name);
  if (pthread_rwlock_unlock (&sprintf_lock))
    {
      fprintf (stderr, "pthread_rwlock_unlock on sprintf_lock failed\n");
      abort ();
    }
  if (l->wait_after > 0)
    {
      struct timespec duration;
      duration.tv_sec  = l->wait_after / 1000000000;
      duration.tv_nsec = l->wait_after % 1000000000;
      nanosleep (&duration, NULL);
    }
  err = pthread_rwlock_unlock (&lock);
  if (err)
    {
      fprintf (stderr, "pthread_rwlock_unlock failed, error = %d\n", err);
      abort ();
    }

  return NULL;
}

static void *
writer_func (void *arg)
{
  struct locals *l = arg;
  int err;

  if (l->wait_before > 0)
    {
      struct timespec duration;
      duration.tv_sec  = l->wait_before / 1000000000;
      duration.tv_nsec = l->wait_before % 1000000000;
      nanosleep (&duration, NULL);
    }
  err = pthread_rwlock_wrlock (&lock);
  if (err)
    {
      fprintf (stderr, "pthread_rwlock_rdlock failed, error = %d\n", err);
      abort ();
    }
  if (pthread_rwlock_wrlock (&sprintf_lock))
    {
      fprintf (stderr, "pthread_rwlock_wrlock on sprintf_lock failed\n");
      abort ();
    }
  sprintf (l->result + strlen (l->result), " %s", l->name);
  if (pthread_rwlock_unlock (&sprintf_lock))
    {
      fprintf (stderr, "pthread_rwlock_unlock on sprintf_lock failed\n");
      abort ();
    }
  if (l->wait_after > 0)
    {
      struct timespec duration;
      duration.tv_sec  = l->wait_after / 1000000000;
      duration.tv_nsec = l->wait_after % 1000000000;
      nanosleep (&duration, NULL);
    }
  err = pthread_rwlock_unlock (&lock);
  if (err)
    {
      fprintf (stderr, "pthread_rwlock_unlock failed, error = %d\n", err);
      abort ();
    }

  return NULL;
}

static const char *
do_test (const char *rw_string)
{
  size_t n = strlen (rw_string);
  int err;
  char resultbuf[100];

  char **names = (char **) malloc (n * sizeof (char *));
  for (size_t i = 0; i < n; i++)
    {
      char name[12];
      sprintf (name, "%c%u", rw_string[i], (unsigned int) (i+1));
      names[i] = strdup (name);
    }

  resultbuf[0] = '\0';

  /* Create the threads.  */
  struct locals *locals = (struct locals *) malloc (n * sizeof (struct locals));
  pthread_t *threads = (pthread_t *) malloc (n * sizeof (pthread_t));
  for (size_t i = 0; i < n; i++)
    {
      locals[i].name = names[i];
      locals[i].wait_before = i * STEP_INTERVAL;
      locals[i].wait_after  = (i == 0 ? n * STEP_INTERVAL : 0);
      locals[i].result = resultbuf;
      err = pthread_create (&threads[i], NULL,
                            rw_string[i] == 'R' ? reader_func :
                            rw_string[i] == 'W' ? writer_func :
                            (abort (), (void * (*) (void *)) NULL),
                            &locals[i]);
      if (err)
        {
          fprintf (stderr, "pthread_create failed to create thread %u, error = %d\n",
                   (unsigned int) (i+1), err);
          abort ();
        }
    }

  /* Wait until the threads are done.  */
  for (size_t i = 0; i < n; i++)
    {
      void *retcode;
      err = pthread_join (threads[i], &retcode);
      if (err)
        {
          fprintf (stderr, "pthread_join failed to wait for thread %u, error = %d\n",
                   (unsigned int) (i+1), err);
          abort ();
        }
    }

  /* Clean up.  */
  free (threads);
  free (locals);
  for (size_t i = 0; i < n; i++)
    free (names[i]);
  free (names);

  return strdup (resultbuf);
}

static bool
startswith (const char *str, const char *prefix)
{
  return strncmp (str, prefix, strlen (prefix)) == 0;
}

static int
find_wait_queue_handling (void)
{
  bool final_r_prefers_readers = true;
  bool final_w_prefers_readers = true;

  /* Perform the test a few times, so that in case of a non-deterministic
     behaviour that happens to look like deterministic in one round, we get
     a higher probability of finding that it is non-deterministic.  */
  for (int repeat = 3; repeat > 0; repeat--)
    {
      bool r_prefers_readers = false;
      bool w_prefers_readers = false;

      {
        const char * RWR = do_test ("RWR");
        const char * RWRR = do_test ("RWRR");
        const char * RWRW = do_test ("RWRW");
        const char * RWWR = do_test ("RWWR");
        const char * RWRRR = do_test ("RWRRR");
        const char * RWRRW = do_test ("RWRRW");
        const char * RWRWR = do_test ("RWRWR");
        const char * RWRWW = do_test ("RWRWW");
        const char * RWWRR = do_test ("RWWRR");
        const char * RWWRW = do_test ("RWWRW");
        const char * RWWWR = do_test ("RWWWR");

        if (   startswith (RWR, " R1 R")
            && startswith (RWRR, " R1 R")
            && startswith (RWRW, " R1 R")
            && startswith (RWWR, " R1 R")
            && startswith (RWRRR, " R1 R")
            && startswith (RWRRW, " R1 R")
            && startswith (RWRWR, " R1 R")
            && startswith (RWRWW, " R1 R")
            && startswith (RWWRR, " R1 R")
            && startswith (RWWRW, " R1 R")
            && startswith (RWWWR, " R1 R"))
          r_prefers_readers = true;
      }

      {
        const char * WRR = do_test ("WRR");
        const char * WRW = do_test ("WRW");
        const char * WWR = do_test ("WWR");
        const char * WRRR = do_test ("WRRR");
        const char * WRRW = do_test ("WRRW");
        const char * WRWR = do_test ("WRWR");
        const char * WRWW = do_test ("WRWW");
        const char * WWRR = do_test ("WWRR");
        const char * WWRW = do_test ("WWRW");
        const char * WWWR = do_test ("WWWR");
        const char * WRRRR = do_test ("WRRRR");
        const char * WRRRW = do_test ("WRRRW");
        const char * WRRWR = do_test ("WRRWR");
        const char * WRRWW = do_test ("WRRWW");
        const char * WRWRR = do_test ("WRWRR");
        const char * WRWRW = do_test ("WRWRW");
        const char * WRWWR = do_test ("WRWWR");
        const char * WRWWW = do_test ("WRWWW");
        const char * WWRRR = do_test ("WWRRR");
        const char * WWRRW = do_test ("WWRRW");
        const char * WWRWR = do_test ("WWRWR");
        const char * WWRWW = do_test ("WWRWW");
        const char * WWWRR = do_test ("WWWRR");
        const char * WWWRW = do_test ("WWWRW");
        const char * WWWWR = do_test ("WWWWR");

        if (   startswith (WRR, " W1 R")
            && startswith (WRW, " W1 R")
            && startswith (WWR, " W1 R")
            && startswith (WRRR, " W1 R")
            && startswith (WRRW, " W1 R")
            && startswith (WRWR, " W1 R")
            && startswith (WRWW, " W1 R")
            && startswith (WWRR, " W1 R")
            && startswith (WWRW, " W1 R")
            && startswith (WWWR, " W1 R")
            && startswith (WRRRR, " W1 R")
            && startswith (WRRRW, " W1 R")
            && startswith (WRRWR, " W1 R")
            && startswith (WRRWW, " W1 R")
            && startswith (WRWRR, " W1 R")
            && startswith (WRWRW, " W1 R")
            && startswith (WRWWR, " W1 R")
            && startswith (WRWWW, " W1 R")
            && startswith (WWRRR, " W1 R")
            && startswith (WWRRW, " W1 R")
            && startswith (WWRWR, " W1 R")
            && startswith (WWRWW, " W1 R")
            && startswith (WWWRR, " W1 R")
            && startswith (WWWRW, " W1 R")
            && startswith (WWWWR, " W1 R"))
          w_prefers_readers = true;
      }

      final_r_prefers_readers &= r_prefers_readers;
      final_w_prefers_readers &= w_prefers_readers;
    }

  /* The wait queue handling is unsuitable if it always prefers readers,
     because it leads to writer starvation: On machines with 8 or more CPUs,
     test-pthread-rwlock may never terminate.  */
  return final_r_prefers_readers && final_w_prefers_readers;
}

int
main ()
{
  /* Initialize the sprintf_lock.  */
  if (pthread_rwlock_init (&sprintf_lock, NULL))
    {
      fprintf (stderr, "pthread_rwlock_init failed\n");
      abort ();
    }

  /* Find the wait queue handling of a default-initialized lock.  */
  if (pthread_rwlock_init (&lock, NULL))
    {
      fprintf (stderr, "pthread_rwlock_init failed\n");
      abort ();
    }
  {
    int fail = find_wait_queue_handling ();
    return fail;
  }
}
                    ]])
                 ],
                 [gl_cv_func_pthread_rwlock_good_waitqueue=yes],
                 [gl_cv_func_pthread_rwlock_good_waitqueue=no],
                 [dnl Guess no on glibc/Linux.
                  gl_cv_func_pthread_rwlock_good_waitqueue="guessing no"
                 ])
               LIBS="$saved_LIBS"
               ;;
             *) dnl Guess yes on other platforms.
                gl_cv_func_pthread_rwlock_good_waitqueue="guessing yes"
                ;;
           esac
          ])
        case "$gl_cv_func_pthread_rwlock_good_waitqueue" in
          *yes) ;;
          *no)
            REPLACE_PTHREAD_RWLOCK_INIT=1
            REPLACE_PTHREAD_RWLOCKATTR_INIT=1
            AC_DEFINE([PTHREAD_RWLOCK_BAD_WAITQUEUE], [1],
              [Define if the pthread_rwlock wait queue handling is not reasonable.])
            ;;
        esac
      fi
    fi
  fi
])
