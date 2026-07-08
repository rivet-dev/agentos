/* pcresearch.c - searching subroutines using PCRE for grep.
   Copyright 2000, 2007, 2009-2025 Free Software Foundation, Inc.

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

#include <search.h>
#include "die.h"

#include <stdckdint.h>

#define PCRE2_CODE_UNIT_WIDTH 8
#include <pcre2.h>

/* For older PCRE2.  */
#ifndef PCRE2_SIZE_MAX
# define PCRE2_SIZE_MAX SIZE_MAX
#endif
#ifndef PCRE2_CONFIG_DEPTHLIMIT
# define PCRE2_CONFIG_DEPTHLIMIT PCRE2_CONFIG_RECURSIONLIMIT
# define PCRE2_ERROR_DEPTHLIMIT PCRE2_ERROR_RECURSIONLIMIT
# define pcre2_set_depth_limit pcre2_set_recursion_limit
#endif
#ifndef PCRE2_EXTRA_ASCII_BSD
# define PCRE2_EXTRA_ASCII_BSD 0
#endif

/* Use PCRE2_MATCH_INVALID_UTF if supported and not buggy;
   see <https://github.com/PCRE2Project/pcre2/issues/224>.
   Assume the bug will be fixed after PCRE2 10.42.  */
#if defined PCRE2_MATCH_INVALID_UTF && 10 < PCRE2_MAJOR + (42 < PCRE2_MINOR)
enum { MATCH_INVALID_UTF = PCRE2_MATCH_INVALID_UTF };
#else
enum { MATCH_INVALID_UTF = 0 };
#endif

struct pcre_comp
{
  /* General context for PCRE operations.  */
  pcre2_general_context *gcontext;

  /* Compiled internal form of a Perl regular expression.  */
  pcre2_code *cre;

  /* Match context and data block.  */
  pcre2_match_context *mcontext;
  pcre2_match_data *data;

  /* The JIT stack and its maximum size.  */
  pcre2_jit_stack *jit_stack;
  idx_t jit_stack_size;

  /* Table, indexed by ! (flag & PCRE2_NOTBOL), of whether the empty
     string matches when that flag is used.  */
  int empty_match[2];
};

/* Memory allocation functions for PCRE.  */
static void *
private_malloc (PCRE2_SIZE size, _GL_UNUSED void *unused)
{
  if (IDX_MAX < size)
    xalloc_die ();
  return ximalloc (size);
}
static void
private_free (void *ptr, _GL_UNUSED void *unused)
{
  free (ptr);
}

void
Pprint_version (void)
{
  char *buf = ximalloc (pcre2_config (PCRE2_CONFIG_VERSION, nullptr));
  pcre2_config (PCRE2_CONFIG_VERSION, buf);
  printf (_("\ngrep -P uses PCRE2 %s\n"), buf);
  free (buf);
}

/* Match the already-compiled PCRE pattern against the data in SUBJECT,
   of size SEARCH_BYTES and starting with offset SEARCH_OFFSET, with
   options OPTIONS.
   Return the (nonnegative) match count or a (negative) error number.  */
static int
jit_exec (struct pcre_comp *pc, char const *subject, idx_t search_bytes,
          idx_t search_offset, int options)
{
  while (true)
    {
      /* STACK_GROWTH_RATE is taken from PCRE's src/pcre2_jit_compile.c.
         Going over the jitstack_max limit could trigger an int
         overflow bug.  */
      int STACK_GROWTH_RATE = 8192;
      idx_t jitstack_max = MIN (IDX_MAX, SIZE_MAX - (STACK_GROWTH_RATE - 1));

      int e = pcre2_match (pc->cre, (PCRE2_SPTR) subject, search_bytes,
                           search_offset, options, pc->data, pc->mcontext);
      if (e == PCRE2_ERROR_JIT_STACKLIMIT
          && pc->jit_stack_size <= jitstack_max / 2)
        {
          idx_t old_size = pc->jit_stack_size;
          idx_t new_size = pc->jit_stack_size = old_size * 2;
          pcre2_jit_stack_free (pc->jit_stack);
          pc->jit_stack = pcre2_jit_stack_create (old_size, new_size,
                                                  pc->gcontext);
          if (!pc->jit_stack)
            xalloc_die ();
          if (!pc->mcontext)
            pc->mcontext = pcre2_match_context_create (pc->gcontext);
          pcre2_jit_stack_assign (pc->mcontext, nullptr, pc->jit_stack);
        }
      else if (e == PCRE2_ERROR_DEPTHLIMIT)
        {
          uint32_t lim;
          pcre2_config (PCRE2_CONFIG_DEPTHLIMIT, &lim);
          if (ckd_mul (&lim, lim, 2))
            return e;
          if (!pc->mcontext)
            pc->mcontext = pcre2_match_context_create (pc->gcontext);
          pcre2_set_depth_limit (pc->mcontext, lim);
        }
      else
        return e;
    }
}

/* Return true if E is an error code for bad UTF-8.  */
static bool
bad_utf8_from_pcre2 (int e)
{
  return PCRE2_ERROR_UTF8_ERR21 <= e && e <= PCRE2_ERROR_UTF8_ERR1;
}

/* Compile the -P style PATTERN, containing SIZE bytes that are
   followed by '\n'.  Return a description of the compiled pattern.  */

void *
Pcompile (char *pattern, idx_t size, reg_syntax_t ignored, bool exact)
{
  PCRE2_SIZE e;
  int ec;
  int flags = PCRE2_DOLLAR_ENDONLY | (match_icase ? PCRE2_CASELESS : 0);
  char *patlim = pattern + size;
  struct pcre_comp *pc = ximalloc (sizeof *pc);
  pcre2_general_context *gcontext = pc->gcontext
    = pcre2_general_context_create (private_malloc, private_free, nullptr);
  pcre2_compile_context *ccontext = pcre2_compile_context_create (gcontext);

  if (localeinfo.multibyte)
    {
      uint32_t unicode;
      if (pcre2_config (PCRE2_CONFIG_UNICODE, &unicode) < 0 || !unicode)
        die (EXIT_TROUBLE, 0,
             _("-P supports only unibyte locales on this platform"));
      if (! localeinfo.using_utf8)
        die (EXIT_TROUBLE, 0, _("-P supports only unibyte and UTF-8 locales"));

      flags |= PCRE2_UTF;

      /* If supported, consider invalid UTF-8 as a barrier not an error.  */
      flags |= MATCH_INVALID_UTF;

      /* If PCRE2_EXTRA_ASCII_BSD is available, use PCRE2_UCP
         so that \d does not have the undesirable effect of matching
         non-ASCII digits.  Otherwise (i.e., with PCRE2 10.42 and earlier),
         escapes like \w have only their ASCII interpretations,
         but that's better than the confusion that would ensue if \d
         matched non-ASCII digits.  */
      flags |= PCRE2_EXTRA_ASCII_BSD ? PCRE2_UCP : 0;

#if 0
      /* Do not match individual code units but only UTF-8.  */
      flags |= PCRE2_NEVER_BACKSLASH_C;
#endif
    }

  /* FIXME: Remove this restriction.  */
  if (rawmemchr (pattern, '\n') != patlim)
    die (EXIT_TROUBLE, 0, _("the -P option only supports a single pattern"));

#ifdef PCRE2_EXTRA_MATCH_LINE
  uint32_t extra_options = (PCRE2_EXTRA_ASCII_BSD
                            | (match_lines ? PCRE2_EXTRA_MATCH_LINE : 0));
  pcre2_set_compile_extra_options (ccontext, extra_options);
#endif

  void *re_storage = nullptr;
  if (match_lines)
    {
#ifndef PCRE2_EXTRA_MATCH_LINE
      static char const *const xprefix = "^(?:";
      static char const *const xsuffix = ")$";
      idx_t re_size = size + strlen (xprefix) + strlen (xsuffix);
      char *re = re_storage = ximalloc (re_size);
      char *rez = mempcpy (re, xprefix, strlen (xprefix));
      rez = mempcpy (rez, pattern, size);
      memcpy (rez, xsuffix, strlen (xsuffix));
      pattern = re;
      size = re_size;
#endif
    }
  else if (match_words)
    {
      /* PCRE2_EXTRA_MATCH_WORD is incompatible with grep -w;
         do things the grep way.  */
      static char const *const wprefix = "(?<!\\w)(?:";
      static char const *const wsuffix = ")(?!\\w)";
      idx_t re_size = size + strlen (wprefix) + strlen (wsuffix);
      char *re = re_storage = ximalloc (re_size);
      char *rez = mempcpy (re, wprefix, strlen (wprefix));
      rez = mempcpy (rez, pattern, size);
      memcpy (rez, wsuffix, strlen (wsuffix));
      pattern = re;
      size = re_size;
    }

  if (!localeinfo.multibyte)
    pcre2_set_character_tables (ccontext, pcre2_maketables (gcontext));

  pc->cre = pcre2_compile ((PCRE2_SPTR) pattern, size, flags,
                           &ec, &e, ccontext);
  if (!pc->cre)
    {
      enum { ERRBUFSIZ = 256 }; /* Taken from pcre2grep.c ERRBUFSIZ.  */
      PCRE2_UCHAR8 ep[ERRBUFSIZ];
      pcre2_get_error_message (ec, ep, sizeof ep);
      die (EXIT_TROUBLE, 0, "%s", ep);
    }

  free (re_storage);
  pcre2_compile_context_free (ccontext);

  pc->mcontext = nullptr;
  pc->data = pcre2_match_data_create_from_pattern (pc->cre, gcontext);

  /* Ignore any failure return from pcre2_jit_compile, as that merely
     means JIT won't be used during matching.  */
  pcre2_jit_compile (pc->cre, PCRE2_JIT_COMPLETE);

  /* The PCRE documentation says that a 32 KiB stack is the default.  */
  pc->jit_stack = nullptr;
  pc->jit_stack_size = 32 << 10;

  pc->empty_match[false] = jit_exec (pc, "", 0, 0, PCRE2_NOTBOL);
  pc->empty_match[true] = jit_exec (pc, "", 0, 0, 0);

  return pc;
}

ptrdiff_t
Pexecute (void *vcp, char const *buf, idx_t size, idx_t *match_size,
          char const *start_ptr)
{
  char const *p = start_ptr ? start_ptr : buf;
  bool bol = p[-1] == eolbyte;
  char const *line_start = buf;
  int e = PCRE2_ERROR_NOMATCH;
  char const *line_end;
  struct pcre_comp *pc = vcp;
  PCRE2_SIZE *sub = pcre2_get_ovector_pointer (pc->data);

  /* The search address to pass to PCRE.  This is the start of
     the buffer, or just past the most-recently discovered encoding
     error or line end.  */
  char const *subject = buf;

  do
    {
      /* Search line by line.  Although this formerly used something like
         PCRE2_MULTILINE for performance, the performance wasn't always
         better and the correctness issues were too puzzling.  See
         Bug#22655.  */
      line_end = rawmemchr (p, eolbyte);
      if (PCRE2_SIZE_MAX < line_end - p)
        die (EXIT_TROUBLE, 0, _("exceeded PCRE's line length limit"));

      for (;;)
        {
          /* Skip past bytes that are easily determined to be encoding
             errors, treating them as data that cannot match.  This is
             faster than having PCRE check them.  */
          while (localeinfo.sbclen[to_uchar (*p)] == -1)
            {
              p++;
              subject = p;
              bol = false;
            }

          idx_t search_offset = p - subject;

          /* Check for an empty match; this is faster than letting
             PCRE do it.  */
          if (p == line_end)
            {
              sub[0] = sub[1] = search_offset;
              e = pc->empty_match[bol];
              break;
            }

          int options = 0;
          if (!bol)
            options |= PCRE2_NOTBOL;

          e = jit_exec (pc, subject, line_end - subject,
                        search_offset, options);
          if (MATCH_INVALID_UTF || !bad_utf8_from_pcre2 (e))
            break;

          idx_t valid_bytes = pcre2_get_startchar (pc->data);

          if (search_offset <= valid_bytes)
            {
              /* Try to match the string before the encoding error.  */
              if (valid_bytes == 0)
                {
                  /* Handle the empty-match case specially, for speed.
                     This optimization is valid if VALID_BYTES is zero,
                     which means SEARCH_OFFSET is also zero.  */
                  sub[0] = valid_bytes;
                  sub[1] = 0;
                  e = pc->empty_match[bol];
                }
              else
                e = jit_exec (pc, subject, valid_bytes, search_offset,
                              options | PCRE2_NO_UTF_CHECK | PCRE2_NOTEOL);

              if (e != PCRE2_ERROR_NOMATCH)
                break;

              /* Treat the encoding error as data that cannot match.  */
              p = subject + valid_bytes + 1;
              bol = false;
            }

          subject += valid_bytes + 1;
        }

      if (e != PCRE2_ERROR_NOMATCH)
        break;
      bol = true;
      p = subject = line_start = line_end + 1;
    }
  while (p < buf + size);

  if (e <= 0)
    {
      switch (e)
        {
        case PCRE2_ERROR_NOMATCH:
          break;

        case PCRE2_ERROR_NOMEMORY:
          die (EXIT_TROUBLE, 0, _("%s: memory exhausted"), input_filename ());

        case PCRE2_ERROR_JIT_STACKLIMIT:
          die (EXIT_TROUBLE, 0, _("%s: exhausted PCRE JIT stack"),
               input_filename ());

        case PCRE2_ERROR_MATCHLIMIT:
          die (EXIT_TROUBLE, 0, _("%s: exceeded PCRE's backtracking limit"),
               input_filename ());

        case PCRE2_ERROR_DEPTHLIMIT:
          die (EXIT_TROUBLE, 0,
               _("%s: exceeded PCRE's nested backtracking limit"),
               input_filename ());

        case PCRE2_ERROR_RECURSELOOP:
          die (EXIT_TROUBLE, 0, _("%s: PCRE detected recurse loop"),
               input_filename ());

#ifdef PCRE2_ERROR_HEAPLIMIT
        case PCRE2_ERROR_HEAPLIMIT:
          die (EXIT_TROUBLE, 0, _("%s: exceeded PCRE's heap limit"),
               input_filename ());
#endif

        default:
          /* For now, we lump all remaining PCRE failures into this basket.
             If anyone cares to provide sample grep usage that can trigger
             particular PCRE errors, we can add to the list (above) of more
             detailed diagnostics.  */
          die (EXIT_TROUBLE, 0, _("%s: internal PCRE error: %d"),
               input_filename (), e);
        }

      return -1;
    }
  else
    {
      char const *matchbeg = subject + sub[0];
      char const *matchend = subject + sub[1];
      char const *beg;
      char const *end;
      if (start_ptr)
        {
          beg = matchbeg;
          end = matchend;
        }
      else
        {
          beg = line_start;
          end = line_end + 1;
        }
      *match_size = end - beg;
      return beg - buf;
    }
}
