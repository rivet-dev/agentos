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

#include <config.h>

#include <localeinfo.h>

#include <limits.h>
#include <locale.h>
#include <stdlib.h>
#include <string.h>
#if GAWK
/* Use ISO C 99 API.  */
# include <wctype.h>
# define char32_t wchar_t
# define mbrtoc32 mbrtowc
# define c32tolower towlower
# define c32toupper towupper
#else
/* Use ISO C 11 + gnulib API.  */
# include <uchar.h>
#endif

/* The sbclen implementation relies on this.  */
static_assert (MB_LEN_MAX <= SCHAR_MAX);

/* Return true if the locale uses UTF-8.  */

static bool
is_using_utf8 (void)
{
  char32_t wc;
  mbstate_t mbs = {0};
  return mbrtoc32 (&wc, "\xc4\x80", 2, &mbs) == 2 && wc == 0x100;
}

/* Return true if the locale is compatible enough with the C locale so
   that the locale is single-byte, bytes are in collating-sequence
   order, and there are no multi-character collating elements.  */

static bool
using_simple_locale (bool multibyte)
{
  /* The native character set is known to be compatible with
     the C locale.  The following test isn't perfect, but it's good
     enough in practice, as only ASCII and EBCDIC are in common use
     and this test correctly accepts ASCII and rejects EBCDIC.  */
  enum { native_c_charset =
    ('\b' == 8 && '\t' == 9 && '\n' == 10 && '\v' == 11 && '\f' == 12
     && '\r' == 13 && ' ' == 32 && '!' == 33 && '"' == 34 && '#' == 35
     && '%' == 37 && '&' == 38 && '\'' == 39 && '(' == 40 && ')' == 41
     && '*' == 42 && '+' == 43 && ',' == 44 && '-' == 45 && '.' == 46
     && '/' == 47 && '0' == 48 && '9' == 57 && ':' == 58 && ';' == 59
     && '<' == 60 && '=' == 61 && '>' == 62 && '?' == 63 && 'A' == 65
     && 'Z' == 90 && '[' == 91 && '\\' == 92 && ']' == 93 && '^' == 94
     && '_' == 95 && 'a' == 97 && 'z' == 122 && '{' == 123 && '|' == 124
     && '}' == 125 && '~' == 126)
  };

  if (!native_c_charset || multibyte)
    return false;

  /* As a heuristic, use strcoll to compare native character order.
     If this agrees with byte order the locale should be simple.
     This heuristic should work for all known practical locales,
     although it would be invalid for artificially-constructed locales
     where the native order is the collating-sequence order but there
     are multi-character collating elements.  */
  for (int i = 0; i < UCHAR_MAX; i++)
    if (0 <= strcoll (((char []) {i, 0}), ((char []) {i + 1, 0})))
      return false;

  return true;
}

/* Initialize *LOCALEINFO from the current locale.  */

void
init_localeinfo (struct localeinfo *localeinfo)
{
  localeinfo->multibyte = MB_CUR_MAX > 1;
  localeinfo->simple = using_simple_locale (localeinfo->multibyte);
  localeinfo->using_utf8 = is_using_utf8 ();

  for (int i = CHAR_MIN; i <= CHAR_MAX; i++)
    {
      char c = i;
      unsigned char uc = i;
      mbstate_t s = {0};
      char32_t wc;
      size_t len = mbrtoc32 (&wc, &c, 1, &s);
      localeinfo->sbclen[uc] = len <= 1 ? 1 : - (int) - len;
      localeinfo->sbctowc[uc] = len <= 1 ? wc : WEOF;
    }
}

/* The set of char32_t values C such that there's a useful locale
   somewhere where C != towupper (C) && C != towlower (towupper (C)).
   For example, 0x00B5 (U+00B5 MICRO SIGN) is in this table, because
   towupper (0x00B5) == 0x039C (U+039C GREEK CAPITAL LETTER MU), and
   towlower (0x039C) == 0x03BC (U+03BC GREEK SMALL LETTER MU).  */
static unsigned short int const lonesome_lower[] =
  {
    0x00B5, 0x0131, 0x017F, 0x01C5, 0x01C8, 0x01CB, 0x01F2, 0x0345,
    0x03C2, 0x03D0, 0x03D1, 0x03D5, 0x03D6, 0x03F0, 0x03F1,

    /* U+03F2 GREEK LUNATE SIGMA SYMBOL lacks a specific uppercase
       counterpart in locales predating Unicode 4.0.0 (April 2003).  */
    0x03F2,

    0x03F5, 0x1E9B, 0x1FBE,
  };

/* Verify that the worst case fits.  This is 1 for towupper, 1 for
   towlower, and 1 for each entry in LONESOME_LOWER.  */
static_assert (1 + 1 + sizeof lonesome_lower / sizeof *lonesome_lower
               <= CASE_FOLDED_BUFSIZE);

/* Find the characters equal to C after case-folding, other than C
   itself, and store them into FOLDED.  Return the number of characters
   stored; this is zero if C is WEOF.  */

int
case_folded_counterparts (wint_t c, char32_t folded[CASE_FOLDED_BUFSIZE])
{
  int i;
  int n = 0;
  wint_t uc = c32toupper (c);
  wint_t lc = c32tolower (uc);
  if (uc != c)
    folded[n++] = uc;
  if (lc != uc && lc != c && c32toupper (lc) == uc)
    folded[n++] = lc;
  for (i = 0; i < sizeof lonesome_lower / sizeof *lonesome_lower; i++)
    {
      wint_t li = lonesome_lower[i];
      if (li != lc && li != uc && li != c && c32toupper (li) == uc)
        folded[n++] = li;
    }
  return n;
}
