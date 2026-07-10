/* Test regular expressions
   Copyright 1996-2001, 2003-2025 Free Software Foundation, Inc.

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

#include "regex.h"

#include <locale.h>
#include <limits.h>
#include <stdarg.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <wctype.h>
#if HAVE_DECL_ALARM
# include <unistd.h>
# include <signal.h>
#endif

#include "localcharset.h"

static int exit_status;

static void
report_error (char const *format, ...)
{
  va_list args;
  va_start (args, format);
  fprintf (stderr, "test-regex: ");
  vfprintf (stderr, format, args);
  fprintf (stderr, "\n");
  va_end (args);
  exit_status = 1;
}

/* Check whether it's really a UTF-8 locale.
   On mingw, setlocale (LC_ALL, "en_US.UTF-8") succeeds but returns
   "English_United States.1252", with locale_charset () returning "CP1252".  */
static int
really_utf8 (void)
{
  return strcmp (locale_charset (), "UTF-8") == 0;
}

/* Tests supposed to match; copied from glibc posix/bug-regex11.c.  */
static struct
{
  const char *pattern;
  const char *string;
  int flags, nmatch;
  regmatch_t rm[5];
} const tests[] = {
  /* Test for newline handling in regex.  */
  { "[^~]*~", "\nx~y", 0, 2, { { 0, 3 }, { -1, -1 } } },
  /* Other tests.  */
  { "a(.*)b", "a b", REG_EXTENDED, 2, { { 0, 3 }, { 1, 2 } } },
  { ".*|\\([KIO]\\)\\([^|]*\\).*|?[KIO]", "10~.~|P|K0|I10|O16|?KSb", 0, 3,
    { { 0, 21 }, { 15, 16 }, { 16, 18 } } },
  { ".*|\\([KIO]\\)\\([^|]*\\).*|?\\1", "10~.~|P|K0|I10|O16|?KSb", 0, 3,
    { { 0, 21 }, { 8, 9 }, { 9, 10 } } },
  { "^\\(a*\\)\\1\\{9\\}\\(a\\{0,9\\}\\)\\([0-9]*;.*[^a]\\2\\([0-9]\\)\\)",
    "a1;;0a1aa2aaa3aaaa4aaaaa5aaaaaa6aaaaaaa7aaaaaaaa8aaaaaaaaa9aa2aa1a0", 0,
    5, { { 0, 67 }, { 0, 0 }, { 0, 1 }, { 1, 67 }, { 66, 67 } } },
  /* Test for BRE expression anchoring.  POSIX says just that this may match;
     in glibc regex it always matched, so avoid changing it.  */
  { "\\(^\\|foo\\)bar", "bar", 0, 2, { { 0, 3 }, { -1, -1 } } },
  { "\\(foo\\|^\\)bar", "bar", 0, 2, { { 0, 3 }, { -1, -1 } } },
  /* In ERE this must be treated as an anchor.  */
  { "(^|foo)bar", "bar", REG_EXTENDED, 2, { { 0, 3 }, { -1, -1 } } },
  { "(foo|^)bar", "bar", REG_EXTENDED, 2, { { 0, 3 }, { -1, -1 } } },
  /* Here ^ cannot be treated as an anchor according to POSIX.  */
  { "(^|foo)bar", "(^|foo)bar", 0, 2, { { 0, 10 }, { -1, -1 } } },
  { "(foo|^)bar", "(foo|^)bar", 0, 2, { { 0, 10 }, { -1, -1 } } },
  /* More tests on backreferences.  */
  { "()\\1", "x", REG_EXTENDED, 2, { { 0, 0 }, { 0, 0 } } },
  { "()x\\1", "x", REG_EXTENDED, 2, { { 0, 1 }, { 0, 0 } } },
  { "()\\1*\\1*", "", REG_EXTENDED, 2, { { 0, 0 }, { 0, 0 } } },
  { "([0-9]).*\\1(a*)", "7;7a6", REG_EXTENDED, 3, { { 0, 4 }, { 0, 1 }, { 3, 4 } } },
  { "([0-9]).*\\1(a*)", "7;7a", REG_EXTENDED, 3, { { 0, 4 }, { 0, 1 }, { 3, 4 } } },
  { "(b)()c\\1", "bcb", REG_EXTENDED, 3, { { 0, 3 }, { 0, 1 }, { 1, 1 } } },
  { "()(b)c\\2", "bcb", REG_EXTENDED, 3, { { 0, 3 }, { 0, 0 }, { 0, 1 } } },
  { "a(b)()c\\1", "abcb", REG_EXTENDED, 3, { { 0, 4 }, { 1, 2 }, { 2, 2 } } },
  { "a()(b)c\\2", "abcb", REG_EXTENDED, 3, { { 0, 4 }, { 1, 1 }, { 1, 2 } } },
  { "()(b)\\1c\\2", "bcb", REG_EXTENDED, 3, { { 0, 3 }, { 0, 0 }, { 0, 1 } } },
  { "(b())\\2\\1", "bbbb", REG_EXTENDED, 3, { { 0, 2 }, { 0, 1 }, { 1, 1 } } },
  { "a()(b)\\1c\\2", "abcb", REG_EXTENDED, 3, { { 0, 4 }, { 1, 1 }, { 1, 2 } } },
  { "a()d(b)\\1c\\2", "adbcb", REG_EXTENDED, 3, { { 0, 5 }, { 1, 1 }, { 2, 3 } } },
  { "a(b())\\2\\1", "abbbb", REG_EXTENDED, 3, { { 0, 3 }, { 1, 2 }, { 2, 2 } } },
  { "(bb())\\2\\1", "bbbb", REG_EXTENDED, 3, { { 0, 4 }, { 0, 2 }, { 2, 2 } } },
  { "^([^,]*),\\1,\\1$", "a,a,a", REG_EXTENDED, 2, { { 0, 5 }, { 0, 1 } } },
  { "^([^,]*),\\1,\\1$", "ab,ab,ab", REG_EXTENDED, 2, { { 0, 8 }, { 0, 2 } } },
  { "^([^,]*),\\1,\\1,\\1$", "abc,abc,abc,abc", REG_EXTENDED, 2,
    { { 0, 15 }, { 0, 3 } } },
  { "^(.?)(.?)(.?)(.?)(.?).?\\5\\4\\3\\2\\1$",
    "level", REG_NOSUB | REG_EXTENDED, 0, { { -1, -1 } } },
  { "^(.?)(.?)(.?)(.?)(.?)(.?)(.?)(.?)(.).?\\9\\8\\7\\6\\5\\4\\3\\2\\1$|^.?$",
    "level", REG_NOSUB | REG_EXTENDED, 0, { { -1, -1 } } },
  { "^(.?)(.?)(.?)(.?)(.?)(.?)(.?)(.?)(.).?\\9\\8\\7\\6\\5\\4\\3\\2\\1$|^.?$",
    "abcdedcba", REG_EXTENDED, 1, { { 0, 9 } } },
  { "^(.?)(.?)(.?)(.?)(.?)(.?)(.?)(.?)(.).?\\9\\8\\7\\6\\5\\4\\3\\2\\1$|^.?$",
    "ababababa", REG_EXTENDED, 1, { { 0, 9 } } },
  { "^(.?)(.?)(.?)(.?)(.?)(.?)(.?)(.?)(.?).?\\9\\8\\7\\6\\5\\4\\3\\2\\1$",
    "level", REG_NOSUB | REG_EXTENDED, 0, { { -1, -1 } } },
  { "^(.?)(.?)(.?)(.?)(.?)(.?)(.?)(.?)(.?).?\\9\\8\\7\\6\\5\\4\\3\\2\\1$",
    "ababababa", REG_EXTENDED, 1, { { 0, 9 } } },
  /* Test for *+ match.  */
  { "^a*+(.)", "ab", REG_EXTENDED, 2, { { 0, 2 }, { 1, 2 } } },
  /* Test for ** match.  */
  { "^(a*)*(.)", "ab", REG_EXTENDED, 3, { { 0, 2 }, { 0, 1 }, { 1, 2 } } },
};

static void
bug_regex11 (void)
{
  regex_t re;
  regmatch_t rm[5];
  size_t i;
  int n;

  for (i = 0; i < sizeof (tests) / sizeof (tests[0]); ++i)
    {
      n = regcomp (&re, tests[i].pattern, tests[i].flags);
      if (n != 0)
        {
          char buf[500];
          regerror (n, &re, buf, sizeof (buf));
          report_error ("%s: regcomp %zd failed: %s", tests[i].pattern, i, buf);
          continue;
        }

      if (regexec (&re, tests[i].string, tests[i].nmatch, rm, 0))
        {
          report_error ("%s: regexec %zd failed", tests[i].pattern, i);
          regfree (&re);
          continue;
        }

      for (n = 0; n < tests[i].nmatch; ++n)
        if (rm[n].rm_so != tests[i].rm[n].rm_so
              || rm[n].rm_eo != tests[i].rm[n].rm_eo)
          {
            if (tests[i].rm[n].rm_so == -1 && tests[i].rm[n].rm_eo == -1)
              break;
            report_error ("%s: regexec %zd match failure rm[%d] %d..%d",
                          tests[i].pattern, i, n,
                          (int) rm[n].rm_so, (int) rm[n].rm_eo);
            break;
          }

      regfree (&re);
    }
}

int
main (void)
{
  static struct re_pattern_buffer regex;
  unsigned char folded_chars[UCHAR_MAX + 1];
  int i;
  const char *s;
  struct re_registers regs;

#if HAVE_DECL_ALARM
  /* In case a bug causes glibc to go into an infinite loop.
     The tests should take less than 10 s on a reasonably modern CPU.  */
  int alarm_value = 1000;
  signal (SIGALRM, SIG_DFL);
  alarm (alarm_value);
#endif

  bug_regex11 ();

  if (setlocale (LC_ALL, "en_US.UTF-8"))
    {
      {
        /* https://sourceware.org/ml/libc-hacker/2006-09/msg00008.html
           This test needs valgrind to catch the bug on Debian
           GNU/Linux 3.1 x86, but it might catch the bug better
           on other platforms and it shouldn't hurt to try the
           test here.  */
        static char const pat[] = "insert into";
        static char const data[] =
          "\xFF\0\x12\xA2\xAA\xC4\xB1,K\x12\xC4\xB1*\xACK";
        re_set_syntax (RE_SYNTAX_GREP | RE_HAT_LISTS_NOT_NEWLINE
                       | RE_ICASE);
        memset (&regex, 0, sizeof regex);
        s = re_compile_pattern (pat, sizeof pat - 1, &regex);
        if (s)
          report_error ("%s: %s", pat, s);
        else
          {
            memset (&regs, 0, sizeof regs);
            i = re_search (&regex, data, sizeof data - 1,
                           0, sizeof data - 1, &regs);
            if (i != -1)
              report_error ("re_search '%s' on '%s' returned %d",
                            pat, data, i);
            regfree (&regex);
            free (regs.start);
            free (regs.end);
          }
      }

      if (really_utf8 ())
        {
          /* This test is from glibc bug 15078.
             The test case is from Andreas Schwab in
             <https://sourceware.org/ml/libc-alpha/2013-01/msg00967.html>.
          */
          static char const pat[] = "[^x]x";
          static char const data[] =
            /* <U1000><U103B><U103D><U1014><U103A><U102F><U1015><U103A> */
            "\xe1\x80\x80"
            "\xe1\x80\xbb"
            "\xe1\x80\xbd"
            "\xe1\x80\x94"
            "\xe1\x80\xba"
            "\xe1\x80\xaf"
            "\xe1\x80\x95"
            "\xe1\x80\xba"
            "x";
          re_set_syntax (0);
          memset (&regex, 0, sizeof regex);
          s = re_compile_pattern (pat, sizeof pat - 1, &regex);
          if (s)
            report_error ("%s: %s", pat, s);
          else
            {
              memset (&regs, 0, sizeof regs);
              i = re_search (&regex, data, sizeof data - 1,
                             0, sizeof data - 1, NULL);
              if (i != 0 && i != 21)
                report_error ("re_search '%s' on '%s' returned %d",
                              pat, data, i);
              regfree (&regex);
              free (regs.start);
              free (regs.end);
            }
        }

      if (! setlocale (LC_ALL, "C"))
        {
          report_error ("setlocale \"C\" failed");
          return exit_status;
        }
    }

  if (setlocale (LC_ALL, "tr_TR.UTF-8"))
    {
      if (really_utf8 () && towupper (L'i') == 0x0130 /* U+0130; see below.  */)
        {
          re_set_syntax (RE_SYNTAX_GREP | RE_ICASE);
          memset (&regex, 0, sizeof regex);
          static char const pat[] = "i";
          s = re_compile_pattern (pat, sizeof pat - 1, &regex);
          if (s)
            report_error ("%s: %s", pat, s);
          else
            {
              /* UTF-8 encoding of U+0130 LATIN CAPITAL LETTER I WITH DOT ABOVE.
                 In Turkish, this is the upper-case equivalent of ASCII "i".
                 Older versions of Gnulib failed to match "i" to U+0130 when
                 ignoring case in Turkish <https://bugs.gnu.org/43577>.  */
              static char const data[] = "\xc4\xb0";

              memset (&regs, 0, sizeof regs);
              i = re_search (&regex, data, sizeof data - 1, 0, sizeof data - 1,
                             &regs);
              if (i != 0)
                report_error ("re_search '%s' on '%s' returned %d",
                              pat, data, i);
              regfree (&regex);
              free (regs.start);
              free (regs.end);
            }
        }

      if (! setlocale (LC_ALL, "C"))
        {
          report_error ("setlocale \"C\" failed");
          return exit_status;
        }
    }

  /* This test is from glibc bug 3957, reported by Andrew Mackey.  */
  re_set_syntax (RE_SYNTAX_EGREP | RE_HAT_LISTS_NOT_NEWLINE);
  memset (&regex, 0, sizeof regex);
  static char const pat_3957[] = "a[^x]b";
  s = re_compile_pattern (pat_3957, sizeof pat_3957 - 1, &regex);
  if (s)
    report_error ("%s: %s", pat_3957, s);
  else
    {
      /* This should fail, but succeeds for glibc-2.5.  */
      memset (&regs, 0, sizeof regs);
      static char const data[] = "a\nb";
      i = re_search (&regex, data, sizeof data - 1, 0, sizeof data - 1, &regs);
      if (i != -1)
        report_error ("re_search '%s' on '%s' returned %d",
                      pat_3957, data, i);
      regfree (&regex);
      free (regs.start);
      free (regs.end);
    }

  /* This regular expression is from Spencer ere test number 75
     in grep-2.3.  */
  re_set_syntax (RE_SYNTAX_POSIX_EGREP);
  memset (&regex, 0, sizeof regex);
  for (i = 0; i <= UCHAR_MAX; i++)
    folded_chars[i] = i;
  regex.translate = folded_chars;
  static char const pat75[] = "a[[:@:>@:]]b\n";
  s = re_compile_pattern (pat75, sizeof pat75 - 1, &regex);
  /* This should fail with _Invalid character class name_ error.  */
  if (!s)
    {
      report_error ("re_compile_pattern: failed to reject '%s'", pat75);
      regfree (&regex);
    }

  /* Ensure that [b-a] is diagnosed as invalid, when
     using RE_NO_EMPTY_RANGES. */
  re_set_syntax (RE_SYNTAX_POSIX_EGREP | RE_NO_EMPTY_RANGES);
  memset (&regex, 0, sizeof regex);
  static char const pat_b_a[] = "a[b-a]";
  s = re_compile_pattern (pat_b_a, sizeof pat_b_a - 1, &regex);
  if (s == NULL)
    {
      report_error ("re_compile_pattern: failed to reject '%s'", pat_b_a);
      regfree (&regex);
    }

  /* This should succeed, but does not for glibc-2.1.3.  */
  memset (&regex, 0, sizeof regex);
  static char const pat_213[] = "{1";
  s = re_compile_pattern (pat_213, sizeof pat_213 - 1, &regex);
  if (s)
    report_error ("%s: %s", pat_213, s);
  else
    regfree (&regex);

  /* The following example is derived from a problem report
     against gawk from Jorge Stolfi <stolfi@ic.unicamp.br>.  */
  memset (&regex, 0, sizeof regex);
  static char const pat_stolfi[] = "[an\371]*n";
  s = re_compile_pattern (pat_stolfi, sizeof pat_stolfi - 1, &regex);
  if (s)
    report_error ("%s: %s", pat_stolfi, s);
  /* This should match, but does not for glibc-2.2.1.  */
  else
    {
      memset (&regs, 0, sizeof regs);
      static char const data[] = "an";
      i = re_match (&regex, data, sizeof data - 1, 0, &regs);
      if (i != 2)
        report_error ("re_match '%s' on '%s' at 2 returned %d",
                      pat_stolfi, data, i);
      regfree (&regex);
      free (regs.start);
      free (regs.end);
    }

  memset (&regex, 0, sizeof regex);
  static char const pat_x[] = "x";
  s = re_compile_pattern (pat_x, sizeof pat_x - 1, &regex);
  if (s)
    report_error ("%s: %s", pat_x, s);
  /* glibc-2.2.93 does not work with a negative RANGE argument.  */
  else
    {
      memset (&regs, 0, sizeof regs);
      static char const data[] = "wxy";
      i = re_search (&regex, data, sizeof data - 1, 2, -2, &regs);
      if (i != 1)
        report_error ("re_search '%s' on '%s' returned %d", pat_x, data, i);
      regfree (&regex);
      free (regs.start);
      free (regs.end);
    }

  /* The version of regex.c in older versions of gnulib
     ignored RE_ICASE.  Detect that problem too.  */
  re_set_syntax (RE_SYNTAX_EMACS | RE_ICASE);
  memset (&regex, 0, sizeof regex);
  s = re_compile_pattern (pat_x, 1, &regex);
  if (s)
    report_error ("%s: %s", pat_x, s);
  else
    {
      memset (&regs, 0, sizeof regs);
      static char const data[] = "WXY";
      i = re_search (&regex, data, sizeof data - 1, 0, 3, &regs);
      if (i < 0)
        report_error ("re_search '%s' on '%s' returned %d", pat_x, data, i);
      regfree (&regex);
      free (regs.start);
      free (regs.end);
    }

  /* glibc bug 11053.  */
  re_set_syntax (RE_SYNTAX_POSIX_BASIC);
  memset (&regex, 0, sizeof regex);
  static char const pat_sub2[] = "\\(a*\\)*a*\\1";
  s = re_compile_pattern (pat_sub2, sizeof pat_sub2 - 1, &regex);
  if (s)
    report_error ("%s: %s", pat_sub2, s);
  else
    {
      memset (&regs, 0, sizeof regs);
      static char const data[] = "a";
      int datalen = sizeof data - 1;
      i = re_search (&regex, data, datalen, 0, datalen, &regs);
      if (i != 0)
        report_error ("re_search '%s' on '%s' returned %d", pat_sub2, data, i);
      else if (regs.num_regs < 2)
        report_error ("re_search '%s' on '%s' returned only %d registers",
                      pat_sub2, data, (int) regs.num_regs);
      else if (! (regs.start[0] == 0 && regs.end[0] == 1))
        report_error ("re_search '%s' on '%s' returned wrong match [%d,%d)",
                      pat_sub2, data, (int) regs.start[0], (int) regs.end[0]);
      else if (! (regs.start[1] == 0 && regs.end[1] == 0))
        report_error ("re_search '%s' on '%s' returned wrong submatch [%d,%d)",
                      pat_sub2, data, (int) regs.start[1], (int) regs.end[1]);
      regfree (&regex);
      free (regs.start);
      free (regs.end);
    }

  /* Catch a bug reported by Vin Shelton in
     https://lists.gnu.org/r/bug-coreutils/2007-06/msg00089.html
     */
  re_set_syntax (RE_SYNTAX_POSIX_BASIC
                 & ~RE_CONTEXT_INVALID_DUP
                 & ~RE_NO_EMPTY_RANGES);
  static char const pat_shelton[] = "[[:alnum:]_-]\\\\+$";
  s = re_compile_pattern (pat_shelton, sizeof pat_shelton - 1, &regex);
  if (s)
    report_error ("%s: %s", pat_shelton, s);
  else
    regfree (&regex);

  /* REG_STARTEND was added to glibc on 2004-01-15.
     Reject older versions.  */
  if (REG_STARTEND == 0)
    report_error ("REG_STARTEND is zero");

  /* Matching with the compiled form of this regexp would provoke
     an assertion failure prior to glibc-2.28:
       regexec.c:1375: pop_fail_stack: Assertion 'num >= 0' failed
     With glibc-2.28, compilation fails and reports the invalid
     back reference.  */
  re_set_syntax (RE_SYNTAX_POSIX_EGREP);
  memset (&regex, 0, sizeof regex);
  static char const pat_badback[] = "0|()0|\\1|0";
  s = re_compile_pattern (pat_badback, sizeof pat_badback, &regex);
  if (!s && re_search (&regex, "x", 1, 0, 1, &regs) != -1)
    s = "mishandled invalid back reference";
  if (s && strcmp (s, "Invalid back reference") != 0)
    report_error ("%s: %s", pat_badback, s);

#if 0
  /* It would be nice to reject hosts whose regoff_t values are too
     narrow (including glibc on hosts with 64-bit ptrdiff_t and
     32-bit int), but we should wait until glibc implements this
     feature.  Otherwise, support for equivalence classes and
     multibyte collation symbols would always be broken except
     when compiling --without-included-regex.   */
  if (sizeof (regoff_t) < sizeof (ptrdiff_t)
      || sizeof (regoff_t) < sizeof (ssize_t))
    report_error ("regoff_t values are too narrow");
#endif

  return exit_status;
}
