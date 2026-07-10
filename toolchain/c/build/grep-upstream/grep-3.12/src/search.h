/* search.c - searching subroutines using dfa, kwset and regex for grep.
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

#ifndef GREP_SEARCH_H
#define GREP_SEARCH_H 1

#include <config.h>

#include <sys/types.h>
#include <stdint.h>
#include <wchar.h>
#include <regex.h>

#include "system.h"
#include "grep.h"
#include "dfa.h"
#include "kwset.h"
#include "xalloc.h"
#include "localeinfo.h"

_GL_INLINE_HEADER_BEGIN
#ifndef SEARCH_INLINE
# define SEARCH_INLINE _GL_INLINE
#endif

/* This must be a signed type.  Each value is the difference in the size
   of a character (in bytes) induced by converting to lower case.
   The vast majority of values are 0, but a few are 1 or -1, so
   technically, two bits may be sufficient.  */
typedef signed char mb_len_map_t;

/* searchutils.c */
extern void wordinit (void);
extern kwset_t kwsinit (bool);
extern idx_t wordchars_size (char const *, char const *) _GL_ATTRIBUTE_PURE;
extern idx_t wordchar_next (char const *, char const *) _GL_ATTRIBUTE_PURE;
extern idx_t wordchar_prev (char const *, char const *, char const *)
  _GL_ATTRIBUTE_PURE;
extern ptrdiff_t mb_goback (char const **, idx_t *, char const *, char const *);

/* dfasearch.c */
extern void *GEAcompile (char *, idx_t, reg_syntax_t, bool);
extern ptrdiff_t EGexecute (void *, char const *, idx_t, idx_t *, char const *);

/* kwsearch.c */
extern void *Fcompile (char *, idx_t, reg_syntax_t, bool);
extern ptrdiff_t Fexecute (void *, char const *, idx_t, idx_t *, char const *);

/* pcresearch.c */
extern void *Pcompile (char *, idx_t, reg_syntax_t, bool);
extern ptrdiff_t Pexecute (void *, char const *, idx_t, idx_t *, char const *);
extern void Pprint_version (void);

/* grep.c */
extern struct localeinfo localeinfo;
extern void fgrep_to_grep_pattern (char **, idx_t *);

/* Return the number of bytes in the character at the start of S, which
   is of size N.  N must be positive.  MBS is the conversion state.
   This acts like mbrlen, except it returns -1 and -2 instead of
   (size_t) -1 and (size_t) -2.  */
SEARCH_INLINE ptrdiff_t
imbrlen (char const *s, idx_t n, mbstate_t *mbs)
{
  size_t len = mbrlen (s, n, mbs);

  /* Convert result to ptrdiff_t portably, even on oddball platforms.
     When optimizing, this typically uses no machine instructions.  */
  if (len <= MB_LEN_MAX)
    return len;
  ptrdiff_t neglen = -len;
  return -neglen;
}

/* Return the number of bytes in the character at the start of S, which
   is of size N.  N must be positive.  MBS is the conversion state.
   This acts like mbrlen, except it returns 1 when mbrlen would return 0,
   it returns -1 and -2 instead of (size_t) -1 and (size_t) -2,
   and it is typically faster because of the cache.  */
SEARCH_INLINE ptrdiff_t
mb_clen (char const *s, idx_t n, mbstate_t *mbs)
{
  signed char len = localeinfo.sbclen[to_uchar (*s)];
  return len == -2 ? imbrlen (s, n, mbs) : len;
}

extern char const *input_filename (void);

_GL_INLINE_HEADER_END

#endif /* GREP_SEARCH_H */
