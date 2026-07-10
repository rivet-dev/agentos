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

/* Written by Ulrich Drepper <drepper@gnu.org>, 1995.  */

#include <config.h>

/* Specification.  */
#include "localename.h"

#include <stdlib.h>
#include <string.h>

const char *
gl_locale_name_environ (_GL_UNUSED int category, const char *categoryname)
{
  const char *retval;

  /* Setting of LC_ALL overrides all other.  */
  retval = getenv ("LC_ALL");
  if (retval != NULL && retval[0] != '\0')
    return retval;
  /* Next comes the name of the desired category.  */
  retval = getenv (categoryname);
  if (retval != NULL && retval[0] != '\0')
    return retval;
  /* Last possibility is the LANG environment variable.  */
  retval = getenv ("LANG");
  if (retval != NULL && retval[0] != '\0')
    {
#if HAVE_CFPREFERENCESCOPYAPPVALUE
      /* Mac OS X 10.2 or newer.
         Ignore invalid LANG value set by the Terminal application.  */
      if (strcmp (retval, "UTF-8") != 0)
#endif
#if defined __CYGWIN__
      /* Cygwin.
         Ignore dummy LANG value set by ~/.profile.  */
      if (strcmp (retval, "C.UTF-8") != 0)
#endif
        return retval;
    }

  return NULL;
}
