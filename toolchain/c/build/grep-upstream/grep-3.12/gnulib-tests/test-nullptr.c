/* Test nullptr.
   Copyright 2023-2025 Free Software Foundation, Inc.

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

/* Written by Bruno Haible.  */

#include <config.h>

int *my_null = nullptr;

#if 0 /* not yet */
/* Header that is supposed to define nullptr_t.  */
# include <stddef.h>
#endif

#include <stdarg.h>

#include "macros.h"

#if 0 /* not yet */

static void
simple_callee (nullptr_t x)
{
  ASSERT (x == NULL);
}

#endif

static void
varargs_callee (const char *first, ...)
{
  va_list args;
  const char *arg;

  ASSERT (first[0] == 't');
  va_start (args, first);

  arg = va_arg (args, const char *);
  ASSERT (arg == NULL);

  arg = va_arg (args, const char *);
  ASSERT (arg[0] == 'f');

  arg = va_arg (args, const char *);
  ASSERT (arg == NULL);

  va_end (args);
}

int
main ()
{
  varargs_callee ("type", nullptr, "foo", nullptr);

  return test_exit_status;
}
