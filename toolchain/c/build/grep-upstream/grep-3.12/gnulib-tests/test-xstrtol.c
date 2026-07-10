/* Test of xstrtol module.
   Copyright (C) 1995-1996, 1998-2001, 2003-2025 Free Software Foundation, Inc.

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

#include <inttypes.h>
#include <stdlib.h>
#include <stdio.h>

#include <error.h>
#include "macros.h"
#include "xstrtol-error.h"

#ifndef __xstrtol
# define __xstrtol xstrtol
# define __strtol_t long int
# define __spec "ld"
# define is_GNULIB_strtol GNULIB_defined_strtol_function
#endif

/* Don't show the program name in error messages.  */
static void
print_no_progname (void)
{
}

int
main (int argc, char **argv)
{
  if (argc > 1)
    {
      /* Test cases given on the command line.  */
      int i;

      error_print_progname = print_no_progname;

      for (i = 1; i < argc; i++)
        {
          char *p;
          __strtol_t val;
          strtol_error s_err = __xstrtol (argv[i], &p, 0, &val, "bckMw0");
          if (s_err == LONGINT_OK)
            {
              printf ("%s->%" __spec " (%s)\n", argv[i], val, p);
            }
          else
            {
              xstrtol_fatal (s_err, -2, 'X', NULL, argv[i]);
            }
        }

      return 0;
    }
  else
    {
      /* Miscellaneous test cases.  */

      /* Test an invalid base (undefined behaviour, as documented in xstrtol.h).
         Reported by Alejandro Colomar.  */
#if !(defined __CYGWIN__ || defined _MSC_VER)
      {
        const char input[] = "k";
        char *endp = NULL;
        __strtol_t val = -17;
        strtol_error s_err = __xstrtol (input, &endp, -1, &val, "k");
# if !(defined __GLIBC__ || is_GNULIB_strtol)
        ASSERT (s_err == LONGINT_OK);
        ASSERT (endp == input + 1);
        ASSERT (val == 1024);
# else
        ASSERT (s_err == LONGINT_INVALID);
        ASSERT (val == -17);
# endif
      }
#endif

      return test_exit_status;
    }
}
