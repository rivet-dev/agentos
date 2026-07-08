/* Multi-byte characters, Error encodings, and Lengths (MCELs)
   Copyright 2023-2025 Free Software Foundation, Inc.

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

/* Written by Paul Eggert.  */

/* The macros in this file implement multi-byte character representation
   and forward iteration through a multi-byte string.
   They are simpler and can be faster than the mbiter family.
   However, they do not support obsolescent encodings like CP864,
   EBCDIC, Johab, and Shift JIS that glibc also does not support,
   and it is up to the caller to coalesce encoding-error bytes if desired.

   The mcel_scan function lets code iterate through an array of bytes,
   supporting character encodings in practical use
   more simply than using plain mbrtoc32.

   Instead of this single-byte code:

      char *p = ..., *lim = ...;
      for (; p < lim; p++)
        process (*p);

   You can use this multi-byte code:

      char *p = ..., *lim = ...;
      for (mcel_t g; p < lim; p += g.len)
        {
          g = mcel_scan (p, lim);
          process (g);
        }

   You can select from G using G.ch, G.err, and G.len.
   G is an encoding error if G.err is nonzero, a character otherwise.

   The mcel_scanz function is similar except it works with a
   string of unknown but positive length that is terminated with '\0'.
   Instead of this single-byte code:

      char *p = ...;
      for (; *p; p++)
        process (*p);

   You can use this multi-byte code:

      char *p = ...;
      for (mcel_t g; *p; p += g.len)
        {
          g = mcel_scanz (p);
          process (g);
        }

   mcel_scant (P, TERMINATOR) is like mcel_scanz (P) except the
   string is terminated by TERMINATOR.  The C standard says that the
   TERMINATORs '\0', '\r', '\n', '.', '/' are safe, as they cannot be
   a part (even a trailing byte) of a multi-byte character.
   In practice TERMINATOR is safe if 0 <= TERMINATOR <= 0x2f (ASCII '/').

   mcel_ch (CH, LEN) and mcel_err (ERR) construct mcel_t values.

   mcel_cmp (G1, G2) compares two mcel_t values lexicographically by
   character or by encoding byte value, with encoding bytes sorting
   after characters.

   Calls like c32isalpha (G.ch) test G; they return false for encoding
   errors since calls like c32isalpha (0) return false.  Calls like
   mcel_tocmp (c32tolower, G1, G2) are like mcel_cmp (G1, G2),
   but transliterate first.

   Although ISO C and POSIX allow encodings that have shift states or
   that can produce multiple characters from an indivisible byte sequence,
   POSIX does not require support for these encodings,
   they are not in practical use on GNUish platforms,
   and omitting support for them simplifies the API.  */

#ifndef _MCEL_H
#define _MCEL_H 1

#if !_GL_CONFIG_H_INCLUDED
 #error "Please include config.h first."
#endif

#include <verify.h>

#include <limits.h>
#include <stddef.h>
#include <uchar.h>

_GL_INLINE_HEADER_BEGIN
#ifndef MCEL_INLINE
# define MCEL_INLINE _GL_INLINE
#endif

#ifdef __cplusplus
extern "C" {
#endif


/* Pacify GCC re type limits.  */
#if 4 < __GNUC__ + (3 <= __GNUC_MINOR__) && !defined __clang__
# pragma GCC diagnostic ignored "-Wtype-limits"
#endif

/* The maximum multi-byte character length supported on any platform.
   This can be less than MB_LEN_MAX because many platforms have a
   large MB_LEN_MAX to allow for stateful encodings, and mcel does not
   support these encodings.  MCEL_LEN_MAX is enough for UTF-8, EUC,
   Shift-JIS, GB18030, etc.  In all multi-byte encodings supported by glibc,
   0 < MB_CUR_MAX <= MCEL_LEN_MAX <= MB_LEN_MAX.  */
enum { MCEL_LEN_MAX = MB_LEN_MAX < 4 ? MB_LEN_MAX : 4 };

/* Bounds for mcel_t members.  */
enum { MCEL_CHAR_MAX = 0x10FFFF };
enum { MCEL_ERR_MIN = 0x80 };

/* mcel_t is a type representing a character CH or an encoding error byte ERR,
   along with a count of the LEN bytes that represent CH or ERR.
   If ERR is zero, CH is a valid character and 0 < LEN <= MCEL_LEN_MAX;
   otherwise ERR is an encoding error byte, MCEL_ERR_MIN <= ERR,
   CH == 0, and LEN == 1.  */
typedef struct
{
  char32_t ch;
  unsigned char err;
  unsigned char len;
} mcel_t;

/* Every multi-byte character length fits in mcel_t's LEN.  */
static_assert (MB_LEN_MAX <= UCHAR_MAX);

/* Shifting an encoding error byte left by this value
   suffices to sort encoding errors after characters.  */
enum { MCEL_ERR_SHIFT = 14 };
static_assert (MCEL_CHAR_MAX < MCEL_ERR_MIN << MCEL_ERR_SHIFT);

/* Unsigned char promotes to int.  */
static_assert (UCHAR_MAX <= INT_MAX);

/* Bytes have 8 bits, as POSIX requires.  */
static_assert (CHAR_BIT == 8);

#ifndef _GL_LIKELY
/* Rely on __builtin_expect, as provided by the module 'builtin-expect'.  */
# define _GL_LIKELY(cond) __builtin_expect ((cond), 1)
# define _GL_UNLIKELY(cond) __builtin_expect ((cond), 0)
#endif

/* mcel_t constructors.  */
MCEL_INLINE mcel_t
mcel_ch (char32_t ch, size_t len)
{
  assume (0 < len);
  assume (len <= MCEL_LEN_MAX);
  assume (ch <= MCEL_CHAR_MAX);
  return (mcel_t) {.ch = ch, .len = len};
}
MCEL_INLINE mcel_t
mcel_err (unsigned char err)
{
  assume (MCEL_ERR_MIN <= err);
  return (mcel_t) {.err = err, .len = 1};
}

/* Compare C1 and C2, with encoding errors sorting after characters.
   Return <0, 0, >0 for <, =, >.  */
MCEL_INLINE int
mcel_cmp (mcel_t c1, mcel_t c2)
{
  int ch1 = c1.ch, ch2 = c2.ch;
  return ((c1.err - c2.err) * (1 << MCEL_ERR_SHIFT)) + (ch1 - ch2);
}

/* Apply the uchar translator TO to C1 and C2 and compare the results,
   with encoding errors sorting after characters,
   Return <0, 0, >0 for <, =, >.  */
MCEL_INLINE int
mcel_tocmp (wint_t (*to) (wint_t), mcel_t c1, mcel_t c2)
{
  int cmp = mcel_cmp (c1, c2);
  if (_GL_LIKELY ((c1.err - c2.err) | !cmp))
    return cmp;
  int ch1 = to (c1.ch), ch2 = to (c2.ch);
  return ch1 - ch2;
}

/* Whether C represents itself as a Unicode character
   when it is the first byte of a single- or multi-byte character.
   These days it is safe to assume ASCII, so do not support
   obsolescent encodings like CP864, EBCDIC, Johab, and Shift JIS.  */
MCEL_INLINE bool
mcel_isbasic (char c)
{
  return _GL_LIKELY (0 <= c && c < MCEL_ERR_MIN);
}

/* With mcel there should be no need for the performance overhead of
   replacing glibc mbrtoc32, as callers shouldn't care whether the
   C locale treats a byte with the high bit set as an encoding error.  */
#ifdef __GLIBC__
# undef mbrtoc32
#endif

/* Scan bytes from P inclusive to LIM exclusive.  P must be less than LIM.
   Return the character or encoding error starting at P.  */
MCEL_INLINE mcel_t
mcel_scan (char const *p, char const *lim)
{
  /* Handle ASCII quickly to avoid the overhead of calling mbrtoc32.
     In supported encodings, the first byte of a multi-byte character
     cannot be an ASCII byte.  */
  char c = *p;
  if (mcel_isbasic (c))
    return mcel_ch (c, 1);

  /* An initial mbstate_t; initialization optimized for some platforms.
     For details about these and other platforms, see wchar.in.h.  */
#if (defined __GLIBC__ && 2 < __GLIBC__ + (2 <= __GLIBC_MINOR__) \
     && !defined __UCLIBC__)
  /* Although only a trivial optimization, it's worth it for GNU.  */
  mbstate_t mbs; mbs.__count = 0;
#elif (defined __FreeBSD__ || defined __DragonFly__ || defined __OpenBSD__ \
       || (defined __APPLE__ && defined __MACH__))
  /* These platforms have 128-byte mbstate_t.  What were they thinking?
     Initialize just for supported encodings (UTF-8, EUC, etc.).
     Avoid memset because some compilers generate function call code.  */
  struct mbhidden { char32_t ch; int utf8_want, euc_want; }
    _GL_ATTRIBUTE_MAY_ALIAS;
  union { mbstate_t m; struct mbhidden s; } u;
  u.s.ch = u.s.utf8_want = u.s.euc_want = 0;
# define mbs u.m
#elif defined __NetBSD__
  /* Experiments on both 32- and 64-bit NetBSD platforms have
     shown that it doesn't work to clear fewer than 24 bytes.  */
  struct mbhidden { long long int a, b, c; } _GL_ATTRIBUTE_MAY_ALIAS;
  union { mbstate_t m; struct mbhidden s; } u;
  u.s.a = u.s.b = u.s.c = 0;
# define mbs u.m
#else
  /* mbstate_t has unknown structure or is not worth optimizing.  */
  mbstate_t mbs = {0};
#endif

  char32_t ch;
  size_t len = mbrtoc32 (&ch, p, lim - p, &mbs);

#undef mbs

  /* Any LEN with top bit set is an encoding error, as LEN == (size_t) -3
     is not supported and MB_LEN_MAX is small.  */
  if (_GL_UNLIKELY ((size_t) -1 / 2 < len))
    return mcel_err (c);

  /* A multi-byte character.  LEN must be positive,
     as *P != '\0' and shift sequences are not supported.  */
  return mcel_ch (ch, len);
}

/* Scan bytes from P, a byte sequence terminated by TERMINATOR.
   If *P == TERMINATOR, scan just that byte; otherwise scan
   bytes up to but not including TERMINATOR.
   TERMINATOR must be ASCII, and should be '\0', '\r', '\n', '.', or '/'.
   Return the character or encoding error starting at P.  */
MCEL_INLINE mcel_t
mcel_scant (char const *p, char terminator)
{
  /* Handle ASCII quickly for speed.  */
  if (mcel_isbasic (*p))
    return mcel_ch (*p, 1);

  /* Defer to mcel_scan for non-ASCII.  Compute length with code that
     is typically faster than strnlen.  */
  char const *lim = p + 1;
  for (int i = 0; i < MCEL_LEN_MAX - 1; i++)
    lim += *lim != terminator;
  return mcel_scan (p, lim);
}

/* Scan bytes from P, a byte sequence terminated by '\0'.
   If *P == '\0', scan just that byte; otherwise scan
   bytes up to but not including '\0'.
   Return the character or encoding error starting at P.  */
MCEL_INLINE mcel_t
mcel_scanz (char const *p)
{
  return mcel_scant (p, '\0');
}


#ifdef __cplusplus
}
#endif

_GL_INLINE_HEADER_END

#endif /* _MCEL_H */
