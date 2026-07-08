/* Test of <sys/types.h> substitute.
   Copyright (C) 2011-2025 Free Software Foundation, Inc.

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

/* Written by Bruno Haible <bruno@clisp.org>, 2011.  */

#include <config.h>

#include <sys/types.h>

/* Check that the most important types are defined.  */
pid_t t1;
size_t t2;
ssize_t t3;
off_t t4;
mode_t t5;
off64_t t6;
blksize_t t7;
blkcnt_t t8;

#include "intprops.h"

/* POSIX requires that pid_t is a signed integer type.  */
static_assert (TYPE_SIGNED (pid_t));

/* POSIX requires that size_t is an unsigned integer type.  */
static_assert (! TYPE_SIGNED (size_t));

/* POSIX requires that ssize_t is a signed integer type.  */
static_assert (TYPE_SIGNED (ssize_t));

/* POSIX requires that off_t is a signed integer type.  */
static_assert (TYPE_SIGNED (off_t));
static_assert (TYPE_SIGNED (off64_t));

/* POSIX requires that blksize_t is a signed integer type.  */
#if !defined __ANDROID__
static_assert (TYPE_SIGNED (blksize_t));
#endif

/* POSIX requires that blkcnt_t is a signed integer type.  */
#if !defined __ANDROID__
static_assert (TYPE_SIGNED (blkcnt_t));
#endif

int
main (void)
{
  return 0;
}
