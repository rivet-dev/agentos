/* Test <mcel.h>
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

#include <config.h>

#include <mcel.h>

#include <locale.h>

#include "macros.h"

static wint_t
to_ascii (wint_t c)
{
  return c & 0x7f;
}

static int
sgn (int i)
{
  return (i > 0) - (i < 0);
}

static void
test_mcel_vs_mbrtoc32 (unsigned char uc, mcel_t c, size_t n, char32_t ch)
{
  ASSERT (!c.err == (n <= MB_LEN_MAX));
  ASSERT (c.err
          ? c.err == uc && c.ch == 0 && c.len == 1
          : c.ch == ch && c.len == (n ? n : 1));
}

int
main (int argc, char *argv[])
{
  /* configure should already have checked that the locale is supported.  */
  if (setlocale (LC_ALL, "") == NULL)
    return 1;

  mcel_t prev;
  for (int ch = 0; ch < 0x80; ch++)
    {
      mcel_t c = mcel_ch (ch, 1);
      ASSERT (c.ch == ch);
      ASSERT (c.len == 1);
      ASSERT (!c.err);
      ASSERT (mcel_cmp (c, c) == 0);
      ASSERT (mcel_tocmp (to_ascii, c, c) == 0);
      if (ch)
        {
          ASSERT (mcel_cmp (prev, c) < 0);
          ASSERT (mcel_cmp (c, prev) > 0);
          ASSERT (mcel_tocmp (to_ascii, prev, c) < 0);
          ASSERT (mcel_tocmp (to_ascii, c, prev) > 0);
        }
      ASSERT (mcel_isbasic (ch));
      prev = c;
    }
  for (char ch = CHAR_MIN; ; ch++)
    {
      ASSERT (mcel_isbasic (ch) == (0 <= ch && ch <= 0x7f));
      if (ch == CHAR_MAX)
        break;
    }

  if (argc > 1 && argv[1][0] == '5')
    {
      /* Locale encoding is GB18030.  */
      #if (defined __GLIBC__ && __GLIBC__ == 2 && __GLIBC_MINOR__ >= 13 && __GLIBC_MINOR__ <= 15) || (GL_CHAR32_T_IS_UNICODE && (defined __FreeBSD__ || defined __NetBSD__ || defined __sun))
      if (test_exit_status != EXIT_SUCCESS)
        return test_exit_status;
      fputs ("Skipping test: The GB18030 converter in this system's iconv is broken.\n", stderr);
      return 77;
      #endif
    }

  for (int ch = 0x80; ch < 0x200; ch++)
    {
      mcel_t c = mcel_ch (ch, 2);
      ASSERT (c.ch == ch);
      ASSERT (c.len == 2);
      ASSERT (!c.err);
      ASSERT (mcel_cmp (c, c) == 0);
      ASSERT (mcel_tocmp (to_ascii, c, c) == 0);
      ASSERT (mcel_cmp (prev, c) < 0);
      ASSERT (mcel_cmp (c, prev) > 0);
      ASSERT (mcel_tocmp (to_ascii, c, c) == 0);
      int cmp = to_ascii (c.ch) ? -1 : 1;
      ASSERT (sgn (mcel_tocmp (to_ascii, prev, c)) == cmp);
      ASSERT (sgn (mcel_tocmp (to_ascii, c, prev)) == -cmp);
      prev = c;
    }
  for (unsigned char err = 0x80; ; err++)
    {
      mcel_t c = mcel_err (err);
      ASSERT (!c.ch);
      ASSERT (c.len == 1);
      ASSERT (c.err == err);
      ASSERT (mcel_cmp (c, c) == 0);
      ASSERT (mcel_cmp (prev, c) < 0);
      ASSERT (mcel_cmp (c, prev) > 0);
      ASSERT (mcel_tocmp (to_ascii, c, c) == 0);
      ASSERT (mcel_tocmp (to_ascii, prev, c) < 0);
      ASSERT (mcel_tocmp (to_ascii, c, prev) > 0);
      prev = c;
      if (err == (unsigned char) -1)
        break;
    }

  for (int i = CHAR_MIN; i <= CHAR_MAX; i++)
    for (int j = CHAR_MIN; j <= CHAR_MAX; j++)
      for (int k = CHAR_MIN; k <= CHAR_MAX; k++)
        {
          char const ijk[] = {i, j, k};
          mbstate_t mbs = {0};
          char32_t ch;
          size_t n = mbrtoc32 (&ch, ijk, sizeof ijk, &mbs);
          mcel_t c = mcel_scan (ijk, ijk + sizeof ijk);
          test_mcel_vs_mbrtoc32 (i, c, n, ch);

          static char const terminator[] = "\r\n./";
          for (int ti = 0; ti < sizeof terminator; ti++)
            {
              char t = terminator[ti];
              if (i == t || j == t || k == t)
                continue;
              char const ijkt[] = {i, j, k, t};
              mcel_t d = mcel_scant (ijkt, t);
              ASSERT (c.ch == d.ch && c.err == d.err && c.len == d.len);
              if (!t)
                {
                  mcel_t z = mcel_scanz (ijkt);
                  ASSERT (d.ch == z.ch && d.err == z.err && d.len == z.len);
                }
            }
        }

  return test_exit_status;
}
