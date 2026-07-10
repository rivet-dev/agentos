/* A more useful interface to strtol.

   Copyright (C) 1995-1996, 1998-2001, 2003-2007, 2009-2025 Free Software
   Foundation, Inc.

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

/* Written by Jim Meyering. */

#ifndef __strtol
# define __strtol strtol
# define __strtol_t long int
# define __xstrtol xstrtol
# define STRTOL_T_MINIMUM LONG_MIN
# define STRTOL_T_MAXIMUM LONG_MAX
#endif

#include <config.h>

#include "xstrtol.h"

#include <ctype.h>
#include <errno.h>
#include <limits.h>
#include <stdckdint.h>
#include <stdlib.h>
#include <string.h>

#if XSTRTOL_INCLUDE_INTTYPES_H
# include <inttypes.h>
#endif

#include "intprops.h"

static strtol_error
bkm_scale (__strtol_t *x, int scale_factor)
{
  __strtol_t scaled;
  if (ckd_mul (&scaled, *x, scale_factor))
    {
      *x = *x < 0 ? TYPE_MINIMUM (__strtol_t) : TYPE_MAXIMUM (__strtol_t);
      return LONGINT_OVERFLOW;
    }

  *x = scaled;

  return LONGINT_OK;
}

static strtol_error
bkm_scale_by_power (__strtol_t *x, int base, int power)
{
  strtol_error err = LONGINT_OK;
  while (power--)
    err |= bkm_scale (x, base);
  return err;
}

strtol_error
__xstrtol (char const *nptr, char **endptr, int base,
           __strtol_t *val, char const *valid_suffixes)
{
  char *t_ptr;
  char **p = endptr ? endptr : &t_ptr;

  if (! TYPE_SIGNED (__strtol_t))
    {
      char const *q = nptr;
      unsigned char ch = *q;
      while (isspace (ch))
        ch = *++q;
      if (ch == '-')
        {
          *p = (char *) nptr;
          return LONGINT_INVALID;
        }
    }

  errno = 0;
  __strtol_t tmp = __strtol (nptr, p, base);
  strtol_error err = LONGINT_OK;

  if (*p == nptr)
    {
      /* If there is no number but there is a valid suffix, assume the
         number is 1.  The string is invalid otherwise.  */
      if (! (valid_suffixes && *nptr && strchr (valid_suffixes, *nptr)))
        return LONGINT_INVALID;
      tmp = 1;
    }
  else if (errno != 0)
    {
      if (errno != ERANGE)
        return LONGINT_INVALID;
      err = LONGINT_OVERFLOW;
    }

  /* Let valid_suffixes == NULL mean "allow any suffix".  */
  /* FIXME: update all callers except the ones that allow suffixes
     after the number, changing last parameter NULL to "".  */
  if (!valid_suffixes)
    {
      *val = tmp;
      return err;
    }

  if (**p != '\0')
    {
      int xbase = 1024;
      int suffixes = 1;
      strtol_error overflow;

      if (!strchr (valid_suffixes, **p))
        {
          *val = tmp;
          return err | LONGINT_INVALID_SUFFIX_CHAR;
        }

      switch (**p)
        {
        case 'E': case 'G': case 'g': case 'k': case 'K': case 'M': case 'm':
        case 'P': case 'Q': case 'R': case 'T': case 't': case 'Y': case 'Z':

          /* The "valid suffix" '0' is a special flag meaning that
             an optional second suffix is allowed, which can change
             the base.  A suffix "B" (e.g. "100MB") stands for a power
             of 1000, whereas a suffix "iB" (e.g. "100MiB") stands for
             a power of 1024.  If no suffix (e.g. "100M"), assume
             power-of-1024.  */

          if (strchr (valid_suffixes, '0'))
            switch (p[0][1])
              {
              case 'i':
                if (p[0][2] == 'B')
                  suffixes += 2;
                break;

              case 'B':
              case 'D': /* 'D' is obsolescent */
                xbase = 1000;
                suffixes++;
                break;
              }
        }

      switch (**p)
        {
        case 'b':
          overflow = bkm_scale (&tmp, 512);
          break;

        case 'B':
          /* This obsolescent first suffix is distinct from the 'B'
             second suffix above.  E.g., 'tar -L 1000B' means change
             the tape after writing 1000 KiB of data.  */
          overflow = bkm_scale (&tmp, 1024);
          break;

        case 'c':
          overflow = LONGINT_OK;
          break;

        case 'E': /* exa or exbi */
          overflow = bkm_scale_by_power (&tmp, xbase, 6);
          break;

        case 'G': /* giga or gibi */
        case 'g': /* 'g' is undocumented; for compatibility only */
          overflow = bkm_scale_by_power (&tmp, xbase, 3);
          break;

        case 'k': /* kilo */
        case 'K': /* kibi */
          overflow = bkm_scale_by_power (&tmp, xbase, 1);
          break;

        case 'M': /* mega or mebi */
        case 'm': /* 'm' is undocumented; for compatibility only */
          overflow = bkm_scale_by_power (&tmp, xbase, 2);
          break;

        case 'P': /* peta or pebi */
          overflow = bkm_scale_by_power (&tmp, xbase, 5);
          break;

        case 'Q': /* quetta or 2**100 */
          overflow = bkm_scale_by_power (&tmp, xbase, 10);
          break;

        case 'R': /* ronna or 2**90 */
          overflow = bkm_scale_by_power (&tmp, xbase, 9);
          break;

        case 'T': /* tera or tebi */
        case 't': /* 't' is undocumented; for compatibility only */
          overflow = bkm_scale_by_power (&tmp, xbase, 4);
          break;

        case 'w':
          overflow = bkm_scale (&tmp, 2);
          break;

        case 'Y': /* yotta or 2**80 */
          overflow = bkm_scale_by_power (&tmp, xbase, 8);
          break;

        case 'Z': /* zetta or 2**70 */
          overflow = bkm_scale_by_power (&tmp, xbase, 7);
          break;

        default:
          *val = tmp;
          return err | LONGINT_INVALID_SUFFIX_CHAR;
        }

      err |= overflow;
      *p += suffixes;
      if (**p)
        err |= LONGINT_INVALID_SUFFIX_CHAR;
    }

  *val = tmp;
  return err;
}
