/* A more useful interface to strtol.

   Copyright (C) 1995-1996, 1998-1999, 2001-2004, 2006-2025 Free Software
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

#ifndef XSTRTOL_H_
#define XSTRTOL_H_ 1

/* Get intmax_t, uintmax_t.  */
#include <stdint.h>

#ifdef __cplusplus
extern "C" {
#endif


enum strtol_error
  {
    LONGINT_OK = 0,

    /* These two values can be ORed together, to indicate that both
       errors occurred.  */
    LONGINT_OVERFLOW = 1,
    LONGINT_INVALID_SUFFIX_CHAR = 2,

    LONGINT_INVALID_SUFFIX_CHAR_WITH_OVERFLOW = (LONGINT_INVALID_SUFFIX_CHAR
                                                 | LONGINT_OVERFLOW),
    LONGINT_INVALID = 4
  };
typedef enum strtol_error strtol_error;

/* Act like the system's strtol (NPTR, ENDPTR, BASE) except:
   - The TYPE of the result might be something other than long int.
   - Return strtol_error, and store any result through an additional
     TYPE *VAL pointer instead of returning the result.
   - If TYPE is unsigned, reject leading '-'.
   - Behavior is undefined if BASE is negative, 1, or greater than 36.
     (In this respect xstrtol acts like the C standard, not like POSIX.)
   - Accept an additional char const *VALID_SUFFIXES pointer to a
     possibly-empty string containing allowed numeric suffixes,
     which multiply the value.  These include SI suffixes like 'k' and 'M';
     these normally stand for powers of 1024, but if VALID_SUFFIXES also
     includes '0' they can be followed by "B" to stand for the usual
     SI powers of 1000 (or by "iB" to stand for powers of 1024 as before).
     Other supported suffixes include 'K' for 1024 or 1000, 'b' for 512,
     'c' for 1, and 'w' for 2.
   - Suppose that after the initial whitespace, the number is missing
     but there is a valid suffix.  Then the number is treated as 1.  */

#define _DECLARE_XSTRTOL(name, type) \
  strtol_error name (char const *restrict /*nptr*/,             \
                     char **restrict /*endptr*/,                \
                     int /*base*/,                              \
                     type *restrict /*val*/,                    \
                     char const *restrict /*valid_suffixes*/);
_DECLARE_XSTRTOL (xstrtol, long int)
_DECLARE_XSTRTOL (xstrtoul, unsigned long int)
_DECLARE_XSTRTOL (xstrtoll, long long int)
_DECLARE_XSTRTOL (xstrtoull, unsigned long long int)
_DECLARE_XSTRTOL (xstrtoimax, intmax_t)
_DECLARE_XSTRTOL (xstrtoumax, uintmax_t)


#ifdef __cplusplus
}
#endif

#endif /* not XSTRTOL_H_ */
