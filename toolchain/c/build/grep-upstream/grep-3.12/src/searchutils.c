/* searchutils.c - helper subroutines for grep's matchers.
   Copyright 1992, 1998, 2000, 2007, 2009-2025 Free Software Foundation, Inc.

   This program is free software; you can redistribute it and/or modify
   it under the terms of the GNU General Public License as published by
   the Free Software Foundation; either version 3, or (at your option)
   any later version.

   This program is distributed in the hope that it will be useful,
   but WITHOUT ANY WARRANTY; without even the implied warranty of
   MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE.  See the
   GNU General Public License for more details.

   You should have received a copy of the GNU General Public License
   along with this program.  If not, see <https://www.gnu.org/licenses/>.  */

#include <config.h>

#define SEARCH_INLINE _GL_EXTERN_INLINE
#define SYSTEM_INLINE _GL_EXTERN_INLINE
#include <search.h>

#include <uchar.h>

/* For each byte B, sbwordchar[B] is true if B is a single-byte
   character that is a word constituent, and is false otherwise.  */
static bool sbwordchar[NCHAR];

/* Whether -w considers WC to be a word constituent.  */
static bool
wordchar (wint_t wc)
{
  return wc == L'_' || c32isalnum (wc);
}

void
wordinit (void)
{
  for (int i = 0; i < NCHAR; i++)
    sbwordchar[i] = wordchar (localeinfo.sbctowc[i]);
}

kwset_t
kwsinit (bool mb_trans)
{
  char *trans = nullptr;

  if (match_icase && (MB_CUR_MAX == 1 || mb_trans))
    {
      trans = ximalloc (NCHAR);
      /* If I is a single-byte character that becomes a different
         single-byte character when uppercased, set trans[I]
         to that character.  Otherwise, set trans[I] to I.  */
      for (int i = 0; i < NCHAR; i++)
        trans[i] = toupper (i);
    }

  return kwsalloc (trans);
}

/* Return the number of bytes needed to go back to the start of a
   multibyte character in a buffer.  The buffer starts at *MB_START.
   (See below for MBCLEN's role.)  The multibyte character contains
   the byte addressed by CUR.  The buffer ends just before END, which
   must not be less than CUR.

   If CUR is no larger than *MB_START, return CUR - *MB_START without
   modifying *MB_START or dealing with MBCLEN.  Otherwise, update
   *MB_START to point to the first multibyte character starting on or
   after CUR, and if MBCLEN is nonnull then deal with MBCLEN as follows:

     - If this function returns 0 and the locale is multibyte and is
       not UTF-8, set *MBCLEN to the number of bytes in the multibyte
       character containing the byte addressed by (CUR - 1).

     - Otherwise, possibly set *MBCLEN to an unspecified value.

   *MB_START should point to the start of a multibyte character, or to
   an encoding-error byte.

   *END should be a sentinel byte - one of '\0', '\r', '\n', '.', '/',
   which POSIX says cannot be part of any other character.  Also,
   there should be a byte string immediately before *MB_START that
   contains a sentinel byte.  This means it is OK to scan backwards
   before *MB_START as long as the scan stops at a sentinel byte, and
   similarly it is OK to scan forwards from CUR (without checking END)
   so long as the scan stops at a sentinel byte.

   Treat encoding errors as if they were single-byte characters.  */
ptrdiff_t
mb_goback (char const **mb_start, idx_t *mbclen, char const *cur,
           char const *end)
{
  const char *p = *mb_start;
  const char *p0 = p;

  if (cur <= p)
    return cur - p;

  if (localeinfo.using_utf8)
    {
      /* UTF-8 permits scanning backward to the previous character.
         Start by assuming CUR is at a character boundary.  */
      p = cur;

      if ((*cur & 0xc0) == 0x80)
        for (int i = 1; i <= 3; i++)
          if ((cur[-i] & 0xc0) != 0x80)
            {
              /* True if the length implied by the putative byte 1 at
                 CUR[-I] extends at least through *CUR.  */
              bool long_enough = (~cur[-i] & 0xff) >> (7 - i) == 0;

              if (long_enough)
                {
                  mbstate_t mbs; mbszero (&mbs);
                  ptrdiff_t clen = imbrlen (cur - i, end - (cur - i), &mbs);
                  if (0 <= clen)
                    {
                      /* This multibyte character contains *CUR.  */
                      p0 = cur - i;
                      p = p0 + clen;
                    }
                }
              break;
            }
    }
  else
    {
      /* In non-UTF-8 encodings, to find character boundaries one must
         in general scan forward from the start of the buffer.  */
      mbstate_t mbs; mbszero (&mbs);
      ptrdiff_t clen;

      do
        {
          clen = mb_clen (p, end - p, &mbs);

          if (clen < 0)
            {
              /* An invalid sequence, or a truncated multibyte character.
                 Treat it as a single byte character.  */
              clen = 1;
              memset (&mbs, 0, sizeof mbs);
            }
          p0 = p;
          p += clen;
        }
      while (p < cur);

      if (mbclen)
        *mbclen = clen;
    }

  *mb_start = p;
  return p == cur ? 0 : cur - p0;
}

/* Examine the start of BUF (which goes to END) for word constituents.
   If COUNTALL, examine as many as possible; otherwise, examine at most one.
   Return the total number of bytes in the examined characters.  */
static idx_t
wordchars_count (char const *buf, char const *end, bool countall)
{
  mbstate_t mbs; mbszero (&mbs);
  char const *p = buf;
  while (p < end)
    {
      unsigned char b = *p;
      if (sbwordchar[b])
        p++;
      else if (localeinfo.sbclen[b] != -2)
        break;
      else
        {
          char32_t wc = 0;
          size_t wcbytes = mbrtoc32 (&wc, p, end - p, &mbs);
          if (!wordchar (wc))
            break;
          p += wcbytes + !wcbytes;
        }
      if (!countall)
        break;
    }
  return p - buf;
}

/* Examine the start of BUF for the longest prefix containing just
   word constituents.  Return the total number of bytes in the prefix.
   The buffer ends at END.  */
idx_t
wordchars_size (char const *buf, char const *end)
{
  return wordchars_count (buf, end, true);
}

/* If BUF starts with a word constituent, return the number of bytes
   used to represent it; otherwise, return zero.  The buffer ends at END.  */
idx_t
wordchar_next (char const *buf, char const *end)
{
  return wordchars_count (buf, end, false);
}

/* In the buffer BUF, return nonzero if the character whose encoding
   contains the byte before CUR is a word constituent.  The buffer
   ends at END.  */
idx_t
wordchar_prev (char const *buf, char const *cur, char const *end)
{
  if (buf == cur)
    return 0;
  unsigned char b = *--cur;
  if (! localeinfo.multibyte || localeinfo.using_utf8 & ~(b >> 7))
    return sbwordchar[b];
  char const *p = buf;
  cur -= mb_goback (&p, nullptr, cur, end);
  return wordchar_next (cur, end);
}
