/* Test of conversion of multibyte character to 32-bit wide character.
   Copyright (C) 2023-2025 Free Software Foundation, Inc.

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

/* Written by Bruno Haible <bruno@clisp.org>, 2023.  */

#include <config.h>

#include <uchar.h>

#include <locale.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <uchar.h>
#include <wchar.h>

#include "macros.h"

int
main (int argc, char *argv[])
{
  /* The only locales in which mbrtoc32 may map a multibyte character to a
     sequence of two or more Unicode characters are those with BIG5-HKSCS
     encoding.  See
     <https://lists.gnu.org/archive/html/bug-gnulib/2023-06/msg00134.html>
     <https://lists.gnu.org/archive/html/bug-gnulib/2023-07/msg00014.html>  */
  if (setlocale (LC_ALL, "zh_HK.BIG5-HKSCS") == NULL)
    {
      fprintf (stderr, "Skipping test: found no locale with BIG5-HKSCS encoding.\n");
      return 77;
    }

  /* The problematic BIG5-HKSCS characters are:

       input         maps to                          name
       -----         -------                          ----
     0x88 0x62    U+00CA U+0304    LATIN CAPITAL LETTER E WITH CIRCUMFLEX AND MACRON
     0x88 0x64    U+00CA U+030C    LATIN CAPITAL LETTER E WITH CIRCUMFLEX AND CARON
     0x88 0xA3    U+00EA U+0304    LATIN SMALL LETTER E WITH CIRCUMFLEX AND MACRON
     0x88 0xA5    U+00EA U+030C    LATIN SMALL LETTER E WITH CIRCUMFLEX AND CARON

     Test one of them.
     See <https://sourceware.org/bugzilla/show_bug.cgi?id=30611>.  */
  mbstate_t state;
  memset (&state, '\0', sizeof (mbstate_t));
  char32_t c32 = (char32_t) 0xBADFACE;
  size_t ret = mbrtoc32 (&c32, "\210\142", 2, &state);
  /* It is OK if this conversion fails.  */
  if (ret != (size_t)(-1))
    {
      /* mbrtoc32 being regular, means that STATE is in an initial state.  */
      ASSERT (mbsinit (&state));
      ret = mbrtoc32 (&c32, "", 0, &state);
      /* mbrtoc32 being regular, means that it returns (size_t)(-2), not
         (size_t)(-3), here.  */
      ASSERT (ret == (size_t)(-2));
      ret = mbrtoc32 (&c32, "", 1, &state);
      /* mbrtoc32 being regular, means that it returns the null 32-bit wide
         character, here, not any remnant from the previous multibyte
         character.  */
      ASSERT (ret == 0);
      ASSERT (c32 == 0);
    }

  return test_exit_status;
}
