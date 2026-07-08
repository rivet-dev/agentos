/* dfasearch.c - searching subroutines using dfa and regex for grep.
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

/* Written August 1992 by Mike Haertel. */

#include <config.h>
#include "intprops.h"
#include <search.h>
#include "die.h"
#include <error.h>

struct dfa_comp
{
  /* KWset compiled pattern.  For GEAcompile, we compile
     a list of strings, at least one of which is known to occur in
     any string matching the regexp. */
  kwset_t kwset;

  /* DFA compiled regexp. */
  struct dfa *dfa;

  /* Regex compiled regexps. */
  struct re_pattern_buffer *patterns;
  idx_t pcount;
  struct re_registers regs;

  /* Number of compiled fixed strings known to exactly match the regexp.
     If kwsexec returns < kwset_exact_matches, then we don't need to
     call the regexp matcher at all. */
  idx_t kwset_exact_matches;

  bool begline;
};

void
dfaerror (char const *mesg)
{
  die (EXIT_TROUBLE, 0, "%s", mesg);
}

void
dfawarn (char const *mesg)
{
  error (0, 0, _("warning: %s"), mesg);
}

/* If the DFA turns out to have some set of fixed strings one of
   which must occur in the match, then we build a kwset matcher
   to find those strings, and thus quickly filter out impossible
   matches. */
static void
kwsmusts (struct dfa_comp *dc)
{
  struct dfamust *dm = dfamust (dc->dfa);
  if (!dm)
    return;
  dc->kwset = kwsinit (false);
  if (dm->exact)
    {
      /* Prepare a substring whose presence implies a match.
         The kwset matcher will return the index of the matching
         string that it chooses. */
      ++dc->kwset_exact_matches;
      idx_t old_len = strlen (dm->must);
      idx_t new_len = old_len + dm->begline + dm->endline;
      char *must = ximalloc (new_len);
      char *mp = must;
      *mp = eolbyte;
      mp += dm->begline;
      dc->begline |= dm->begline;
      memcpy (mp, dm->must, old_len);
      if (dm->endline)
        mp[old_len] = eolbyte;
      kwsincr (dc->kwset, must, new_len);
      free (must);
    }
  else
    {
      /* Otherwise, filtering with this substring should help reduce the
         search space, but we'll still have to use the regexp matcher.  */
      kwsincr (dc->kwset, dm->must, strlen (dm->must));
    }
  kwsprep (dc->kwset);
  dfamustfree (dm);
}

/* Return true if KEYS, of length LEN, might contain a back-reference.
   Return false if KEYS cannot contain a back-reference.
   BS_SAFE is true of encodings where a backslash cannot appear as the
   last byte of a multibyte character.  */
static bool _GL_ATTRIBUTE_PURE
possible_backrefs_in_pattern (char const *keys, idx_t len, bool bs_safe)
{
  /* Normally a backslash, but in an unsafe encoding this is a non-char
     value so that the comparison below always fails, because if there
     are two adjacent '\' bytes, the first might be the last byte of a
     multibyte character.  */
  int second_backslash = bs_safe ? '\\' : CHAR_MAX + 1;

  /* This code can return true even if KEYS lacks a back-reference, for
     patterns like [\2], or for encodings where '\' appears as the last
     byte of a multibyte character.  However, false alarms should be
     rare and do not affect correctness.  */

  /* Do not look for a backslash in the pattern's last byte, since it
     can't be part of a back-reference and this streamlines the code.  */
  len--;

  if (0 <= len)
    {
      char const *lim = keys + len;
      for (char const *p = keys; (p = memchr (p, '\\', lim - p)); p++)
        {
          if ('1' <= p[1] && p[1] <= '9')
            return true;
          if (p[1] == second_backslash)
            {
              p++;
              if (p == lim)
                break;
            }
        }
    }
  return false;
}

static bool
regex_compile (struct dfa_comp *dc, char const *p, idx_t len,
               idx_t pcount, idx_t lineno, reg_syntax_t syntax_bits,
               bool syntax_only)
{
  struct re_pattern_buffer pat;
  pat.buffer = nullptr;
  pat.allocated = 0;

  /* Do not use a fastmap with -i, to work around glibc Bug#20381.  */
  static_assert (UCHAR_MAX < IDX_MAX);
  idx_t uchar_max = UCHAR_MAX;
  pat.fastmap = syntax_only | match_icase ? nullptr : ximalloc (uchar_max + 1);

  pat.translate = nullptr;

  if (syntax_only)
    re_set_syntax (syntax_bits | RE_NO_SUB);
  else
    re_set_syntax (syntax_bits);

  char const *err = re_compile_pattern (p, len, &pat);
  if (!err)
    {
      if (syntax_only)
        regfree (&pat);
      else
        dc->patterns[pcount] = pat;

      return true;
    }

  free (pat.fastmap);

  /* Emit a filename:lineno: prefix for patterns taken from files.  */
  idx_t pat_lineno;
  char const *pat_filename
    = lineno < 0 ? "" : pattern_file_name (lineno, &pat_lineno);

  if (*pat_filename == '\0')
    error (0, 0, "%s", err);
  else
    {
      ptrdiff_t n = pat_lineno;
      error (0, 0, "%s:%td: %s", pat_filename, n, err);
    }

  return false;
}

/* Compile PATTERN, containing SIZE bytes that are followed by '\n'.
   SYNTAX_BITS specifies whether PATTERN uses style -G, -E, or -A.
   Return a description of the compiled pattern.  */

void *
GEAcompile (char *pattern, idx_t size, reg_syntax_t syntax_bits,
            bool exact)
{
  char *motif;
  struct dfa_comp *dc = xcalloc (1, sizeof (*dc));

  dc->dfa = dfaalloc ();

  if (match_icase)
    syntax_bits |= RE_ICASE;
  int dfaopts = (DFA_CONFUSING_BRACKETS_ERROR | DFA_STRAY_BACKSLASH_WARN
                 | DFA_PLUS_WARN
                 | (syntax_bits & RE_CONTEXT_INDEP_OPS ? DFA_STAR_WARN : 0)
                 | (eolbyte ? 0 : DFA_EOL_NUL));
  dfasyntax (dc->dfa, &localeinfo, syntax_bits, dfaopts);
  bool bs_safe = !localeinfo.multibyte | localeinfo.using_utf8;

  /* For GNU regex, pass the patterns separately to detect errors like
     "[\nallo\n]\n", where the patterns are "[", "allo" and "]", and
     this should be a syntax error.  The same for backref, where the
     backref should be local to each pattern.  */
  char const *p = pattern;
  char const *patlim = pattern + size;
  bool compilation_failed = false;

  dc->patterns = xmalloc (sizeof *dc->patterns);
  dc->patterns++;
  dc->pcount = 0;
  idx_t palloc = 1;

  char const *prev = pattern;

  /* Buffer containing back-reference-free patterns.  */
  char *buf = nullptr;
  idx_t buflen = 0;
  idx_t bufalloc = 0;

  idx_t lineno = 0;

  do
    {
      char const *sep = rawmemchr (p, '\n');
      idx_t len = sep - p;

      bool backref = possible_backrefs_in_pattern (p, len, bs_safe);

      if (backref && prev < p)
        {
          idx_t prevlen = p - prev;
          ptrdiff_t bufshortage = buflen - bufalloc + prevlen;
          if (0 < bufshortage)
            buf = xpalloc (buf, &bufalloc, bufshortage, -1, 1);
          memcpy (buf + buflen, prev, prevlen);
          buflen += prevlen;
        }

      /* Ensure room for at least two more patterns.  The extra one is
         for the regex_compile that may be executed after this loop
         exits, and its (unused) slot is patterns[-1] until then.  */
      ptrdiff_t shortage = dc->pcount - palloc + 2;
      if (0 < shortage)
        {
          dc->patterns = xpalloc (dc->patterns - 1, &palloc, shortage, -1,
                                  sizeof *dc->patterns);
          dc->patterns++;
        }

      if (!regex_compile (dc, p, len, dc->pcount, lineno, syntax_bits,
                          !backref))
        compilation_failed = true;

      p = sep + 1;
      lineno++;

      if (backref)
        {
          dc->pcount++;
          prev = p;
        }
    }
  while (p <= patlim);

  if (compilation_failed)
    exit (EXIT_TROUBLE);

  if (patlim < prev)
    buflen--;
  else if (pattern < prev)
    {
      idx_t prevlen = patlim - prev;
      buf = xirealloc (buf, buflen + prevlen);
      memcpy (buf + buflen, prev, prevlen);
      buflen += prevlen;
    }
  else
    {
      buf = pattern;
      buflen = size;
    }

  /* In the match_words and match_lines cases, we use a different pattern
     for the DFA matcher that will quickly throw out cases that won't work.
     Then if DFA succeeds we do some hairy stuff using the regex matcher
     to decide whether the match should really count. */
  if (match_words || match_lines)
    {
      static char const line_beg_no_bk[] = "^(";
      static char const line_end_no_bk[] = ")$";
      static char const word_beg_no_bk[] = "(^|[^[:alnum:]_])(";
      static char const word_end_no_bk[] = ")([^[:alnum:]_]|$)";
      static char const line_beg_bk[] = "^\\(";
      static char const line_end_bk[] = "\\)$";
      static char const word_beg_bk[] = "\\(^\\|[^[:alnum:]_]\\)\\(";
      static char const word_end_bk[] = "\\)\\([^[:alnum:]_]\\|$\\)";
      int bk = !(syntax_bits & RE_NO_BK_PARENS);
      idx_t bracket_bytes = sizeof word_beg_bk - 1 + sizeof word_end_bk;
      char *n = ximalloc (size + bracket_bytes);

      strcpy (n, match_lines ? (bk ? line_beg_bk : line_beg_no_bk)
                             : (bk ? word_beg_bk : word_beg_no_bk));
      idx_t total = strlen (n);
      memcpy (n + total, pattern, size);
      total += size;
      strcpy (n + total, match_lines ? (bk ? line_end_bk : line_end_no_bk)
                                     : (bk ? word_end_bk : word_end_no_bk));
      total += strlen (n + total);
      pattern = motif = n;
      size = total;
    }
  else
    motif = nullptr;

  dfaparse (pattern, size, dc->dfa);
  kwsmusts (dc);
  dfacomp (nullptr, 0, dc->dfa, 1);

  if (buf)
    {
      if (exact || !dfasupported (dc->dfa))
        {
          dc->patterns--;
          dc->pcount++;

          if (!regex_compile (dc, buf, buflen, 0, -1, syntax_bits, false))
            abort ();
        }

      if (buf != pattern)
        free (buf);
    }

  free (motif);

  return dc;
}

ptrdiff_t
EGexecute (void *vdc, char const *buf, idx_t size, idx_t *match_size,
           char const *start_ptr)
{
  char const *buflim, *beg, *end, *ptr, *match, *best_match, *mb_start;
  char eol = eolbyte;
  regoff_t start;
  idx_t len, best_len;
  struct kwsmatch kwsm;
  idx_t i;
  struct dfa_comp *dc = vdc;
  struct dfa *superset = dfasuperset (dc->dfa);
  bool dfafast = dfaisfast (dc->dfa);

  mb_start = buf;
  buflim = buf + size;

  for (beg = end = buf; end < buflim; beg = end)
    {
      end = buflim;

      if (!start_ptr)
        {
          char const *next_beg, *dfa_beg = beg;
          idx_t count = 0;
          bool exact_kwset_match = false;
          bool backref = false;

          /* Try matching with KWset, if it's defined.  */
          if (dc->kwset)
            {
              char const *prev_beg;

              /* Find a possible match using the KWset matcher.  */
              ptrdiff_t offset = kwsexec (dc->kwset, beg - dc->begline,
                                          buflim - beg + dc->begline,
                                          &kwsm, true);
              if (offset < 0)
                return offset;
              match = beg + offset;
              prev_beg = beg;

              /* Narrow down to the line containing the possible match.  */
              beg = memrchr (buf, eol, match - buf);
              beg = beg ? beg + 1 : buf;
              dfa_beg = beg;

              /* Determine the end pointer to give the DFA next.  Typically
                 this is after the first newline after MATCH; but if the KWset
                 match is not exact, the DFA is fast, and the offset from
                 PREV_BEG is less than 64 or (MATCH - PREV_BEG), this is the
                 greater of the latter two values; this temporarily prefers
                 the DFA to KWset.  */
              exact_kwset_match = kwsm.index < dc->kwset_exact_matches;
              if (exact_kwset_match || !dfafast
                  || MAX (16, match - beg) < (match - prev_beg) >> 2)
                {
                  end = rawmemchr (match, eol);
                  end++;
                }
              else if (MAX (16, match - beg) < (buflim - prev_beg) >> 2)
                {
                  end = rawmemchr (prev_beg + 4 * MAX (16, match - beg), eol);
                  end++;
                }
              else
                end = buflim;

              if (exact_kwset_match)
                {
                  if (!localeinfo.multibyte | localeinfo.using_utf8)
                    goto success;
                  if (mb_start < beg)
                    mb_start = beg;
                  if (mb_goback (&mb_start, nullptr, match, buflim) == 0)
                    goto success;
                  /* The matched line starts in the middle of a multibyte
                     character.  Perform the DFA search starting from the
                     beginning of the next character.  */
                  dfa_beg = mb_start;
                }
            }

          /* Try matching with the superset of DFA, if it's defined.  */
          if (superset && !exact_kwset_match)
            {
              /* Keep using the superset while it reports multiline
                 potential matches; this is more likely to be fast
                 than falling back to KWset would be.  */
              next_beg = dfaexec (superset, dfa_beg, (char *) end, 0,
                                  &count, nullptr);
              if (!next_beg || next_beg == end)
                continue;

              /* Narrow down to the line we've found.  */
              if (count != 0)
                {
                  beg = memrchr (buf, eol, next_beg - buf);
                  beg++;
                  dfa_beg = beg;
                }
              end = rawmemchr (next_beg, eol);
              end++;

              count = 0;
            }

          /* Try matching with DFA.  */
          next_beg = dfaexec (dc->dfa, dfa_beg, (char *) end, 0, &count,
                              &backref);

          /* If there's no match, or if we've matched the sentinel,
             we're done.  */
          if (!next_beg || next_beg == end)
            continue;

          /* Narrow down to the line we've found.  */
          if (count != 0)
            {
              beg = memrchr (buf, eol, next_beg - buf);
              beg++;
            }
          end = rawmemchr (next_beg, eol);
          end++;

          /* Successful, no back-references encountered! */
          if (!backref)
            goto success;
          ptr = beg;
        }
      else
        {
          /* We are looking for the leftmost (then longest) exact match.
             We will go through the outer loop only once.  */
          ptr = start_ptr;
        }

      /* If the "line" is longer than the maximum regexp offset,
         die as if we've run out of memory.  */
      if (TYPE_MAXIMUM (regoff_t) < end - beg - 1)
        xalloc_die ();

      /* Run the possible match through Regex.  */
      best_match = end;
      best_len = 0;
      for (i = 0; i < dc->pcount; i++)
        {
          dc->patterns[i].not_eol = 0;
          dc->patterns[i].newline_anchor = eolbyte == '\n';
          start = re_search (&dc->patterns[i], beg, end - beg - 1,
                             ptr - beg, end - ptr - 1, &dc->regs);
          if (start < -1)
            xalloc_die ();
          else if (0 <= start)
            {
              len = dc->regs.end[0] - start;
              match = beg + start;
              if (match > best_match)
                continue;
              if (start_ptr && !match_words)
                goto assess_pattern_match;
              if ((!match_lines && !match_words)
                  || (match_lines && len == end - ptr - 1))
                {
                  match = ptr;
                  len = end - ptr;
                  goto assess_pattern_match;
                }
              /* If -w and not -x, check whether the match aligns with
                 word boundaries.  Do this iteratively because:
                 (a) the line may contain more than one occurrence of the
                 pattern, and
                 (b) Several alternatives in the pattern might be valid at a
                 given point, and we may need to consider a shorter one to
                 find a word boundary.  */
              if (!match_lines && match_words)
                while (match <= best_match)
                  {
                    regoff_t shorter_len = 0;
                    if (! wordchar_next (match + len, end - 1)
                        && ! wordchar_prev (beg, match, end - 1))
                      goto assess_pattern_match;
                    if (len > 0)
                      {
                        /* Try a shorter length anchored at the same place. */
                        --len;
                        dc->patterns[i].not_eol = 1;
                        shorter_len = re_match (&dc->patterns[i], beg,
                                                match + len - ptr, match - beg,
                                                &dc->regs);
                        if (shorter_len < -1)
                          xalloc_die ();
                      }
                    if (0 < shorter_len)
                      len = shorter_len;
                    else
                      {
                        /* Try looking further on. */
                        if (match == end - 1)
                          break;
                        match++;
                        dc->patterns[i].not_eol = 0;
                        start = re_search (&dc->patterns[i], beg, end - beg - 1,
                                           match - beg, end - match - 1,
                                           &dc->regs);
                        if (start < 0)
                          {
                            if (start < -1)
                              xalloc_die ();
                            break;
                          }
                        len = dc->regs.end[0] - start;
                        match = beg + start;
                      }
                  } /* while (match <= best_match) */
              continue;
            assess_pattern_match:
              if (!start_ptr)
                {
                  /* Good enough for a non-exact match.
                     No need to look at further patterns, if any.  */
                  goto success;
                }
              if (match < best_match || (match == best_match && len > best_len))
                {
                  /* Best exact match:  leftmost, then longest.  */
                  best_match = match;
                  best_len = len;
                }
            } /* if re_search >= 0 */
        } /* for Regex patterns.  */
        if (best_match < end)
          {
            /* We have found an exact match.  We were just
               waiting for the best one (leftmost then longest).  */
            beg = best_match;
            len = best_len;
            goto success_in_len;
          }
    } /* for (beg = end ..) */

  return -1;

 success:
  len = end - beg;
 success_in_len:;
  *match_size = len;
  return beg - buf;
}
