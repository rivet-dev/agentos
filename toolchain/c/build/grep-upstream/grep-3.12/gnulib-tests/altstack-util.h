/* Some auxiliary stuff for defining an alternate stack.
   Copyright (C) 2010-2025 Free Software Foundation, Inc.

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

/* Written by Eric Blake and Bruno Haible.  */

#include <stdint.h> /* uintptr_t */
#include <string.h> /* for memset */

#define MYSTACK_SIZE (1 << 24)

/* glibc says: Users should use SIGSTKSZ as the size of user-supplied
   buffers.  We want to detect stack overflow of the alternate stack
   in a nicer manner than just crashing, so we overallocate in
   comparison to what we hand libsigsegv.  Also, we intentionally hand
   an unaligned pointer, to ensure the alternate stack still ends up
   aligned.  */
#define MYSTACK_CRUMPLE_ZONE 8192
static char mystack_storage[MYSTACK_SIZE + 2 * MYSTACK_CRUMPLE_ZONE + 31];
static char *mystack; /* MYSTACK_SIZE bytes in the middle of storage. */

static void
prepare_alternate_stack (void)
{
#ifdef SIGSTKSZ
  if (MYSTACK_SIZE < SIGSTKSZ)
    {
      size_t size = SIGSTKSZ;
      printf ("SIGSTKSZ=%zu exceeds MYSTACK_SIZE=%d\n", size, MYSTACK_SIZE);
      exit (1);
    }
#endif
  memset (mystack_storage, 's', sizeof mystack_storage);
  mystack = (char *) ((uintptr_t) (mystack_storage + MYSTACK_CRUMPLE_ZONE) | 31);
}

static void
check_alternate_stack_no_overflow (void)
{
  unsigned int i;

  for (i = MYSTACK_CRUMPLE_ZONE; i > 0; i--)
    if (*(mystack - i) != 's')
      {
        printf ("Alternate stack was exceeded by %u bytes!!\n", i);
        exit (1);
      }
  for (i = MYSTACK_CRUMPLE_ZONE; i > 0; i--)
    if (*(mystack + MYSTACK_SIZE - 1 + i) != 's')
      {
        printf ("Alternate stack was exceeded by %u bytes!!\n", i);
        exit (1);
      }
}
