/* Test the stack overflow handler.
   Copyright (C) 2002-2025 Free Software Foundation, Inc.

   This program is free software: you can redistribute it and/or modify
   it under the terms of the GNU General Public License as published by
   the Free Software Foundation; either version 2 of the License, or
   (at your option) any later version.

   This program is distributed in the hope that it will be useful,
   but WITHOUT ANY WARRANTY; without even the implied warranty of
   MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE.  See the
   GNU General Public License for more details.

   You should have received a copy of the GNU General Public License
   along with this program.  If not, see <https://www.gnu.org/licenses/>.  */

/* Written by Bruno Haible and Eric Blake.  */

#include <config.h>

/* On GNU/Hurd, when compiling with -D_FORTIFY_SOURCE=2, avoid an error
   "*** longjmp causes uninitialized stack frame ***: terminated".
   Cf. <https://sourceware.org/bugzilla/show_bug.cgi?id=32522>  */
#ifdef __gnu_hurd__
# undef _FORTIFY_SOURCE
# undef __USE_FORTIFY_LEVEL
#endif

/* Specification.  */
#include "sigsegv.h"

#include <stdio.h>
#include <limits.h>

/* Skip this test when an address sanitizer is in use.  */
#ifndef __has_feature
# define __has_feature(a) 0
#endif
#if defined __SANITIZE_ADDRESS__ || __has_feature (address_sanitizer)
# undef HAVE_STACK_OVERFLOW_RECOVERY
#endif

#if HAVE_STACK_OVERFLOW_RECOVERY

# if defined _WIN32 && !defined __CYGWIN__
  /* Windows doesn't have sigset_t.  */
  typedef int sigset_t;
#  define sigemptyset(set)
#  define sigprocmask(how,set,oldset)
# endif

# include <stddef.h> /* needed for NULL on SunOS4 */
# include <stdlib.h> /* for abort, exit */
# include <signal.h>
# include <setjmp.h>
# if HAVE_SETRLIMIT
#  include <sys/types.h>
#  include <sys/time.h>
#  include <sys/resource.h>
# endif
# include "altstack-util.h"

static jmp_buf mainloop;
static sigset_t mainsigset;

static volatile int pass = 0;

static volatile char *stack_lower_bound;
static volatile char *stack_upper_bound;

static void
stackoverflow_handler_continuation (void *arg1, void *arg2, void *arg3)
{
#if defined __NetBSD__ && defined __i386__
  /* On NetBSD 10.0/i386, when built as part of a testdir-all (but not as part
     of a testdir for just the module 'sigsegv'!) this program crashes.  The
     cause is that:
       - The alternate stack is not aligned (which is intentional, see
         altstack-util.h) and NetBSD does not align the stack pointer while
         switching to the alternate stack.
       - When %esp is not aligned, the dynamic linker crashes in function
         _rtld_bind while resolving the symbol 'longjmp'.
     We would around this by aligning the stack pointer, to a multiple of 8.  */
  int *argp;
  __asm__ __volatile__ ("movl %1,%0" : "=r" (argp) : "r" (&arg1));
  unsigned long sp;
  __asm__ __volatile__ ("movl %%esp,%0" : "=r" (sp));
  sp &= ~7UL;
  __asm__ __volatile__ ("movl %0,%%esp" : : "r" (sp));
  int arg = *argp;
#else
  int arg = (int) (long) arg1;
#endif
  longjmp (mainloop, arg);
}

static void
stackoverflow_handler (int emergency, stackoverflow_context_t scp)
{
  char dummy;
  volatile char *addr = &dummy;
  if (!(addr >= stack_lower_bound && addr <= stack_upper_bound))
    abort ();
  pass++;
  printf ("Stack overflow %d caught.\n", pass);
  sigprocmask (SIG_SETMASK, &mainsigset, NULL);
  sigsegv_leave_handler (stackoverflow_handler_continuation,
                         (void *) (long) (emergency ? -1 : pass), NULL, NULL);
}

static volatile int *
recurse_1 (int n, volatile int *p)
{
  if (n < INT_MAX)
    *recurse_1 (n + 1, p) += n;
  return p;
}

static int
recurse (volatile int n)
{
  return *recurse_1 (n, &n);
}

int
main ()
{
  sigset_t emptyset;

# if HAVE_SETRLIMIT && defined RLIMIT_STACK
  /* Before starting the endless recursion, try to be friendly to the user's
     machine.  On some Linux 2.2.x systems, there is no stack limit for user
     processes at all.  We don't want to kill such systems.  */
  struct rlimit rl;
  rl.rlim_cur = rl.rlim_max = 0x100000; /* 1 MB */
  setrlimit (RLIMIT_STACK, &rl);
# endif

  /* Prepare the storage for the alternate stack.  */
  prepare_alternate_stack ();

  /* Install the stack overflow handler.  */
  if (stackoverflow_install_handler (&stackoverflow_handler,
                                     mystack, MYSTACK_SIZE)
      < 0)
    exit (2);
  stack_lower_bound = mystack;
  stack_upper_bound = mystack + MYSTACK_SIZE - 1;

  /* Save the current signal mask.  */
  sigemptyset (&emptyset);
  sigprocmask (SIG_BLOCK, &emptyset, &mainsigset);

  /* Provoke two stack overflows in a row.  */
  switch (setjmp (mainloop))
    {
    case -1:
      printf ("emergency exit\n"); exit (1);
    case 0: case 1:
      printf ("Starting recursion pass %d.\n", pass + 1);
      recurse (0);
      printf ("no endless recursion?!\n"); exit (1);
    case 2:
      break;
    default:
      abort ();
    }

  /* Validate that the alternate stack did not overflow.  */
  check_alternate_stack_no_overflow ();

  printf ("Test passed.\n");
  exit (0);
}

#else

int
main ()
{
  return 77;
}

#endif
