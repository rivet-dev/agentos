/* locale information

   Copyright 2016-2025 Free Software Foundation, Inc.

   This program is free software; you can redistribute it and/or modify
   it under the terms of the GNU General Public License as published by
   the Free Software Foundation, either version 3, or (at your option)
   any later version.

   This program is distributed in the hope that it will be useful,
   but WITHOUT ANY WARRANTY; without even the implied warranty of
   MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE.  See the
   GNU General Public License for more details.

   You should have received a copy of the GNU General Public License
   along with this program.  If not, see <https://www.gnu.org/licenses/>.  */

/* Written by Paul Eggert.  */

#include <limits.h>
#include <wchar.h>
#if GAWK
/* Use ISO C 99 API.  */
# define char32_t wchar_t
#else
/* Use ISO C 11 + gnulib API.  */
# include <uchar.h>
#endif

#ifdef __cplusplus
extern "C" {
#endif


struct localeinfo
{
  /* MB_CUR_MAX > 1.  */
  bool multibyte;

  /* The locale is simple, like the C locale.  These locales can be
     processed more efficiently, as they are single-byte, their native
     character set is in collating-sequence order, and they do not
     have multi-character collating elements.  */
  bool simple;

  /* The locale uses UTF-8.  */
  bool using_utf8;

  /* An array indexed by byte values B that contains 1 if B is a
     single-byte character, -1 if B is an encoding error, and -2 if B
     is the leading byte of a multibyte character that contains more
     than one byte.  */
  signed char sbclen[UCHAR_MAX + 1];

  /* An array indexed by byte values B that contains the corresponding
     32-bit wide character (if any) for B if sbclen[B] == 1.  WEOF means
     the byte is not a valid single-byte character, i.e., sbclen[B] == -1
     or -2.  */
  wint_t sbctowc[UCHAR_MAX + 1];
};

extern void init_localeinfo (struct localeinfo *);

/* Maximum number of characters that can be the case-folded
   counterparts of a single character, not counting the character
   itself.  This is a generous upper bound.  */
enum { CASE_FOLDED_BUFSIZE = 32 };

extern int case_folded_counterparts (wint_t, char32_t[CASE_FOLDED_BUFSIZE]);


#ifdef __cplusplus
}
#endif
