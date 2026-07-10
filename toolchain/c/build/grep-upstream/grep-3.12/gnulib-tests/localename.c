/* Determine name of the currently selected locale.
   Copyright (C) 1995-2025 Free Software Foundation, Inc.

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

/* Don't use __attribute__ __nonnull__ in this compilation unit.  Otherwise gcc
   optimizes away the locale == NULL tests below in duplocale() and freelocale(),
   or xlclang reports -Wtautological-pointer-compare warnings for these tests.
 */
#define _GL_ARG_NONNULL(params)

#include <config.h>

/* Specification.  */
#include "localename.h"

#include <limits.h>
#include <stdlib.h>
#include <locale.h>
#include <string.h>

#include "flexmember.h"
#include "glthread/lock.h"
#include "thread-optim.h"


/* Define a local struniq() function.  */
#include "struniq.h"

const char *
gl_locale_name_thread (int category, const char *categoryname)
{
  if (category == LC_ALL)
    /* Invalid argument.  */
    abort ();
  const char *name = gl_locale_name_thread_unsafe (category, categoryname);
  if (name != NULL)
    return struniq (name);
  return NULL;
}

const char *
gl_locale_name_posix (int category, const char *categoryname)
{
  if (category == LC_ALL)
    /* Invalid argument.  */
    abort ();
  const char *name = gl_locale_name_posix_unsafe (category, categoryname);
  if (name != NULL)
    return struniq (name);
  return NULL;
}

/* Determine the current locale's name, and canonicalize it into XPG syntax
     language[_territory][.codeset][@modifier]
   The codeset part in the result is not reliable; the locale_charset()
   should be used for codeset information instead.
   The result must not be freed; it is statically allocated.  */

const char *
gl_locale_name (int category, const char *categoryname)
{
  const char *retval;

  if (category == LC_ALL)
    /* Invalid argument.  */
    abort ();

  retval = gl_locale_name_thread (category, categoryname);
  if (retval != NULL)
    return retval;

  retval = gl_locale_name_posix (category, categoryname);
  if (retval != NULL)
    return retval;

  return gl_locale_name_default ();
}
