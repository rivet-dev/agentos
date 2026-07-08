/* Test of wmemchr() function.
   Copyright (C) 2024-2025 Free Software Foundation, Inc.

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

/* Specification.  */
#include <wchar.h>

#include <stddef.h>

#include "macros.h"

/* Test the library, not the compiler+library.  */
static wchar_t *
lib_wmemchr (wchar_t const *s, wchar_t wc, size_t n)
{
  return wmemchr (s, wc, n);
}
static wchar_t *(*volatile volatile_wmemchr) (wchar_t const *, wchar_t, size_t)
  = lib_wmemchr;
#undef wmemchr
#define wmemchr volatile_wmemchr

int
main (void)
{
  /* Test zero-length operations on NULL pointers, allowed by
     <https://www.open-std.org/jtc1/sc22/wg14/www/docs/n3322.pdf>.  */
  ASSERT (wmemchr (NULL, L'?', 0) == NULL);

  return test_exit_status;
}
