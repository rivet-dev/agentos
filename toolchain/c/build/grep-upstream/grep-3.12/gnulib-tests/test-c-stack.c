/* Test of c-stack module.
   Copyright (C) 2002, 2004, 2006, 2008-2025 Free Software Foundation, Inc.

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

#include <config.h>

#include "c-stack.h"

#include "exitfail.h"
#include <stdio.h>
#if HAVE_SETRLIMIT
/* At least FreeBSD 5.0 needs extra headers before <sys/resource.h>
   will compile.  */
# include <sys/types.h>
# include <sys/time.h>
# include <sys/resource.h>
#endif

#include "macros.h"

/* Skip this test when an address sanitizer is in use.  */
#ifndef __has_feature
# define __has_feature(a) 0
#endif
#if defined __SANITIZE_ADDRESS__ || __has_feature (address_sanitizer)

int
main (int argc, char **argv)
{
  fputs ("skipping test: address sanitizer in use\n", stderr);
  return 77;
}

#else

static volatile int *
recurse_1 (volatile int n, volatile int *p)
{
  if (n >= 0)
    *recurse_1 (n + 1, p) += n;
  return p;
}

static int
recurse (volatile int n)
{
  int sum = 0;
  return *recurse_1 (n, &sum);
}

int
main (int argc, char **argv)
{
# if HAVE_SETRLIMIT && defined RLIMIT_STACK
  /* Before starting the endless recursion, try to be friendly to the
     user's machine.  On some Linux 2.2.x systems, there is no stack
     limit for user processes at all.  We don't want to kill such
     systems.  */
  struct rlimit rl;
  rl.rlim_cur = rl.rlim_max = 0x100000; /* 1 MB */
  setrlimit (RLIMIT_STACK, &rl);
# endif

  if (c_stack_action (NULL) == 0)
    {
      if (1 < argc)
        {
          exit_failure = 77;
          ++*argv[argc]; /* Intentionally dereference NULL.  */
        }
      return recurse (0);
    }
  fputs ("skipping test: ", stderr);
  perror ("c_stack_action");
  return 77;
}

#endif
