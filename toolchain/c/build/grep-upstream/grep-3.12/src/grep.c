/* grep.c - main driver file for grep.
   Copyright (C) 1992, 1997-2002, 2004-2025 Free Software Foundation, Inc.

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

/* Written July 1992 by Mike Haertel.  */

#include <config.h>
#include <sys/types.h>
#include <sys/stat.h>
#include <uchar.h>
#include <wchar.h>
#include <inttypes.h>
#include <stdarg.h>
#include <stdckdint.h>
#include <stdint.h>
#include <stdio.h>
#include "system.h"

#include "argmatch.h"
#include "c-ctype.h"
#include "c-stack.h"
#include "closeout.h"
#include "colorize.h"
#include "die.h"
#include <error.h>
#include "exclude.h"
#include "exitfail.h"
#include "fcntl-safer.h"
#include "fts_.h"
#include <getopt.h>
#include "grep.h"
#include "hash.h"
#include "intprops.h"
#include "safe-read.h"
#include <search.h>
#include "c-strcase.h"
#include "version-etc.h"
#include "xalloc.h"
#include "xbinary-io.h"
#include "xstrtol.h"

enum { SEP_CHAR_SELECTED = ':' };
enum { SEP_CHAR_REJECTED = '-' };
static char const SEP_STR_GROUP[] = "--";

/* When stdout is connected to a regular file, save its stat
   information here, so that we can automatically skip it, thus
   avoiding a potential (racy) infinite loop.  */
static struct stat out_stat;

/* if non-zero, display usage information and exit */
static int show_help;

/* Print the version on standard output and exit.  */
static bool show_version;

/* Suppress diagnostics for nonexistent or unreadable files.  */
static bool suppress_errors;

/* If nonzero, use color markers.  */
static int color_option;

/* Show only the part of a line matching the expression. */
static bool only_matching;

/* If nonzero, make sure first content char in a line is on a tab stop. */
static bool align_tabs;

/* Print width of line numbers and byte offsets.  Nonzero if ALIGN_TABS.  */
static int offset_width;

/* An entry in the PATLOC array saying where patterns came from.  */
struct patloc
  {
    /* Line number of the pattern in PATTERN_ARRAY.  Line numbers
       start at 0, and each pattern is terminated by '\n'.  */
    idx_t lineno;

    /* Input location of the pattern.  The FILENAME "-" represents
       standard input, and "" represents the command line.  FILELINE is
       origin-1 for files and is irrelevant for the command line.  */
    char const *filename;
    idx_t fileline;
  };

/* The array of pattern locations.  The concatenation of all patterns
   is stored in a single array, KEYS.  Given the invocation
   'grep -f <(seq 5) -f <(seq 6) -f <(seq 3)', there will initially be
   28 bytes in KEYS.  After duplicate patterns are removed, KEYS
   will have 12 bytes and PATLOC will be {0,x,1}, {10,y,1}
   where x, y and z are just place-holders for shell-generated names
   since and z is omitted as it contains only duplicates.  Sometimes
   removing duplicates will grow PATLOC, since each run of
   removed patterns not at a file start or end requires another
   PATLOC entry for the first non-removed pattern.  */
static struct patloc *patloc;
static idx_t patlocs_allocated, patlocs_used;

/* Pointer to the array of patterns, each terminated by newline.  */
static char *pattern_array;

/* The number of unique patterns seen so far.  */
static idx_t n_patterns;

/* Hash table of patterns seen so far.  */
static Hash_table *pattern_table;

/* Hash and compare newline-terminated patterns for textual equality.
   Patterns are represented by origin-1 offsets into PATTERN_ARRAY,
   cast to void *.  The origin-1 is so that the first pattern offset
   does not appear to be a null pointer when cast to void *.  */
static size_t _GL_ATTRIBUTE_PURE
hash_pattern (void const *pat, size_t n_buckets)
{
  /* This uses the djb2 algorithm, except starting with a larger prime
     in place of djb2's 5381, if size_t is wide enough.  The primes
     are taken from the primeth recurrence sequence
     <https://oeis.org/A007097>.  h15, h32 and h64 are the largest
     sequence members that fit into 15, 32 and 64 bits, respectively.
     Since any H will do, hashing works correctly on oddball machines
     where size_t has some other width.  */
  uint_fast64_t h15 = 5381, h32 = 3657500101, h64 = 4123221751654370051;
  size_t h = h64 <= SIZE_MAX ? h64 : h32 <= SIZE_MAX ? h32 : h15;
  intptr_t pat_offset = (intptr_t) pat - 1;
  unsigned char const *s = (unsigned char const *) pattern_array + pat_offset;
  for ( ; *s != '\n'; s++)
    h = h * 33 ^ *s;
  return h % n_buckets;
}
static bool _GL_ATTRIBUTE_PURE
compare_patterns (void const *a, void const *b)
{
  intptr_t a_offset = (intptr_t) a - 1;
  intptr_t b_offset = (intptr_t) b - 1;
  char const *p = pattern_array + a_offset;
  char const *q = pattern_array + b_offset;
  for (; *p == *q; p++, q++)
    if (*p == '\n')
      return true;
  return false;
}

/* Update KEYS to remove duplicate patterns, and return the number of
   bytes in the resulting KEYS.  KEYS contains a sequence of patterns
   each terminated by '\n'.  The first DUPFREE_SIZE bytes are a
   sequence of patterns with no duplicates; SIZE is the total number
   of bytes in KEYS.  If some patterns past the first DUPFREE_SIZE
   bytes are not duplicates, update PATLOCS accordingly.  */
static idx_t
update_patterns (char *keys, idx_t dupfree_size, idx_t size,
                 char const *filename)
{
  char *dst = keys + dupfree_size;
  idx_t fileline = 1;
  int prev_inserted = 0;

  char const *srclim = keys + size;
  idx_t patsize;
  for (char const *src = keys + dupfree_size; src < srclim; src += patsize)
    {
      char const *patend = rawmemchr (src, '\n');
      patsize = patend + 1 - src;
      memmove (dst, src, patsize);

      intptr_t dst_offset_1 = dst - keys + 1;
      int inserted = hash_insert_if_absent (pattern_table,
                                            (void *) dst_offset_1, nullptr);
      if (inserted)
        {
          if (inserted < 0)
            xalloc_die ();
          dst += patsize;

          /* Add a PATLOCS entry unless this input line is simply the
             next one in the same file.  */
          if (!prev_inserted)
            {
              if (patlocs_used == patlocs_allocated)
                patloc = xpalloc (patloc, &patlocs_allocated, 1, -1,
                                  sizeof *patloc);
              patloc[patlocs_used++]
                = (struct patloc) { .lineno = n_patterns,
                                    .filename = filename,
                                    .fileline = fileline };
            }
          n_patterns++;
        }

      prev_inserted = inserted;
      fileline++;
    }

  return dst - keys;
}

/* Map LINENO, the origin-0 line number of one of the input patterns,
   to the name of the file from which it came.  Return "-" if it was
   read from stdin, "" if it was specified on the command line.
   Set *NEW_LINENO to the origin-1 line number of PATTERN in the file,
   or to an unspecified value if PATTERN came from the command line.  */
char const * _GL_ATTRIBUTE_PURE
pattern_file_name (idx_t lineno, idx_t *new_lineno)
{
  idx_t i;
  for (i = 1; i < patlocs_used; i++)
    if (lineno < patloc[i].lineno)
      break;
  *new_lineno = lineno - patloc[i - 1].lineno + patloc[i - 1].fileline;
  return patloc[i - 1].filename;
}

#if HAVE_ASAN
/* Record the starting address and length of the sole poisoned region,
   so that we can unpoison it later, just before each following read.  */
static void const *poison_buf;
static idx_t poison_len;

static void
clear_asan_poison (void)
{
  if (poison_buf)
    __asan_unpoison_memory_region (poison_buf, poison_len);
}

static void
asan_poison (void const *addr, idx_t size)
{
  poison_buf = addr;
  poison_len = size;

  __asan_poison_memory_region (poison_buf, poison_len);
}
#else
static void clear_asan_poison (void) { }
static void asan_poison (void const volatile *addr, idx_t size) { }
#endif

/* The group separator used when context is requested. */
static const char *group_separator = SEP_STR_GROUP;

/* The context and logic for choosing default --color screen attributes
   (foreground and background colors, etc.) are the following.
      -- There are eight basic colors available, each with its own
         nominal luminosity to the human eye and foreground/background
         codes (black [0 %, 30/40], blue [11 %, 34/44], red [30 %, 31/41],
         magenta [41 %, 35/45], green [59 %, 32/42], cyan [70 %, 36/46],
         yellow [89 %, 33/43], and white [100 %, 37/47]).
      -- Sometimes, white as a background is actually implemented using
         a shade of light gray, so that a foreground white can be visible
         on top of it (but most often not).
      -- Sometimes, black as a foreground is actually implemented using
         a shade of dark gray, so that it can be visible on top of a
         background black (but most often not).
      -- Sometimes, more colors are available, as extensions.
      -- Other attributes can be selected/deselected (bold [1/22],
         underline [4/24], standout/inverse [7/27], blink [5/25], and
         invisible/hidden [8/28]).  They are sometimes implemented by
         using colors instead of what their names imply; e.g., bold is
         often achieved by using brighter colors.  In practice, only bold
         is really available to us, underline sometimes being mapped by
         the terminal to some strange color choice, and standout best
         being left for use by downstream programs such as less(1).
      -- We cannot assume that any of the extensions or special features
         are available for the purpose of choosing defaults for everyone.
      -- The most prevalent default terminal backgrounds are pure black
         and pure white, and are not necessarily the same shades of
         those as if they were selected explicitly with SGR sequences.
         Some terminals use dark or light pictures as default background,
         but those are covered over by an explicit selection of background
         color with an SGR sequence; their users will appreciate their
         background pictures not be covered like this, if possible.
      -- Some uses of colors attributes is to make some output items
         more understated (e.g., context lines); this cannot be achieved
         by changing the background color.
      -- For these reasons, the grep color defaults should strive not
         to change the background color from its default, unless it's
         for a short item that should be highlighted, not understated.
      -- The grep foreground color defaults (without an explicitly set
         background) should provide enough contrast to be readable on any
         terminal with either a black (dark) or white (light) background.
         This only leaves red, magenta, green, and cyan (and their bold
         counterparts) and possibly bold blue.  */
/* The color strings used for matched text.
   The user can overwrite them using the GREP_COLORS environment variable.  */
static const char *selected_match_color = "01;31";	/* bold red */
static const char *context_match_color  = "01;31";	/* bold red */

/* Other colors.  Defaults look damn good.  */
static const char *filename_color = "35";	/* magenta */
static const char *line_num_color = "32";	/* green */
static const char *byte_num_color = "32";	/* green */
static const char *sep_color      = "36";	/* cyan */
static const char *selected_line_color = "";	/* default color pair */
static const char *context_line_color  = "";	/* default color pair */

/* Select Graphic Rendition (SGR, "\33[...m") strings.  */
/* Also Erase in Line (EL) to Right ("\33[K") by default.  */
/*    Why have EL to Right after SGR?
         -- The behavior of line-wrapping when at the bottom of the
            terminal screen and at the end of the current line is often
            such that a new line is introduced, entirely cleared with
            the current background color which may be different from the
            default one (see the boolean back_color_erase terminfo(5)
            capability), thus scrolling the display by one line.
            The end of this new line will stay in this background color
            even after reverting to the default background color with
            "\33[m', unless it is explicitly cleared again with "\33[K"
            (which is the behavior the user would instinctively expect
            from the whole thing).  There may be some unavoidable
            background-color flicker at the end of this new line because
            of this (when timing with the monitor's redraw is just right).
         -- The behavior of HT (tab, "\t") is usually the same as that of
            Cursor Forward Tabulation (CHT) with a default parameter
            of 1 ("\33[I"), i.e., it performs pure movement to the next
            tab stop, without any clearing of either content or screen
            attributes (including background color); try
               printf 'asdfqwerzxcv\rASDF\tZXCV\n'
            in a bash(1) shell to demonstrate this.  This is not what the
            user would instinctively expect of HT (but is ok for CHT).
            The instinctive behavior would include clearing the terminal
            cells that are skipped over by HT with blank cells in the
            current screen attributes, including background color;
            the boolean dest_tabs_magic_smso terminfo(5) capability
            indicates this saner behavior for HT, but only some rare
            terminals have it (although it also indicates a special
            glitch with standout mode in the Teleray terminal for which
            it was initially introduced).  The remedy is to add "\33K"
            after each SGR sequence, be it START (to fix the behavior
            of any HT after that before another SGR) or END (to fix the
            behavior of an HT in default background color that would
            follow a line-wrapping at the bottom of the screen in another
            background color, and to complement doing it after START).
            Piping grep's output through a pager such as less(1) avoids
            any HT problems since the pager performs tab expansion.

      Generic disadvantages of this remedy are:
         -- Some very rare terminals might support SGR but not EL (nobody
            will use "grep --color" on a terminal that does not support
            SGR in the first place).
         -- Having these extra control sequences might somewhat complicate
            the task of any program trying to parse "grep --color"
            output in order to extract structuring information from it.
      A specific disadvantage to doing it after SGR START is:
         -- Even more possible background color flicker (when timing
            with the monitor's redraw is just right), even when not at the
            bottom of the screen.
      There are no additional disadvantages specific to doing it after
      SGR END.

      It would be impractical for GNU grep to become a full-fledged
      terminal program linked against ncurses or the like, so it will
      not detect terminfo(5) capabilities.  */
static const char *sgr_start = "\33[%sm\33[K";
static const char *sgr_end   = "\33[m\33[K";

/* SGR utility functions.  */
static void
pr_sgr_start (char const *s)
{
  if (*s)
    print_start_colorize (sgr_start, s);
}
static void
pr_sgr_end (char const *s)
{
  if (*s)
    print_end_colorize (sgr_end);
}
static void
pr_sgr_start_if (char const *s)
{
  if (color_option)
    pr_sgr_start (s);
}
static void
pr_sgr_end_if (char const *s)
{
  if (color_option)
    pr_sgr_end (s);
}

struct color_cap
  {
    const char *name;
    const char **var;
    void (*fct) (void);
  };

static void
color_cap_mt_fct (void)
{
  /* Our caller just set selected_match_color.  */
  context_match_color = selected_match_color;
}

static void
color_cap_rv_fct (void)
{
  /* By this point, it was 1 (or already -1).  */
  color_option = -1;  /* That's still != 0.  */
}

static void
color_cap_ne_fct (void)
{
  sgr_start = "\33[%sm";
  sgr_end   = "\33[m";
}

/* For GREP_COLORS.  */
static const struct color_cap color_dict[] =
  {
    { "mt", &selected_match_color, color_cap_mt_fct }, /* both ms/mc */
    { "ms", &selected_match_color, nullptr }, /* selected matched text */
    { "mc", &context_match_color,  nullptr }, /* context matched text */
    { "fn", &filename_color,       nullptr }, /* filename */
    { "ln", &line_num_color,       nullptr }, /* line number */
    { "bn", &byte_num_color,       nullptr }, /* byte (sic) offset */
    { "se", &sep_color,            nullptr }, /* separator */
    { "sl", &selected_line_color,  nullptr }, /* selected lines */
    { "cx", &context_line_color,   nullptr }, /* context lines */
    { "rv", nullptr,               color_cap_rv_fct }, /* -v reverses sl/cx */
    { "ne", nullptr,               color_cap_ne_fct }, /* no EL on SGR_* */
    { nullptr, nullptr,            nullptr }
  };

/* Saved errno value from failed output functions on stdout.
   prline polls this to decide whether to die.
   Setting it to nonzero just before exiting can prevent clean_up_stdout
   from misbehaving on a buggy OS where 'close (STDOUT_FILENO)' fails
   with EACCES.  */
static int stdout_errno;

static void
putchar_errno (int c)
{
  if (putchar (c) < 0)
    stdout_errno = errno;
}

static void
fputs_errno (char const *s)
{
  if (fputs (s, stdout) < 0)
    stdout_errno = errno;
}

static void _GL_ATTRIBUTE_FORMAT_PRINTF_STANDARD (1, 2)
printf_errno (char const *format, ...)
{
  va_list ap;
  va_start (ap, format);
  if (vfprintf (stdout, format, ap) < 0)
    stdout_errno = errno;
  va_end (ap);
}

static void
fwrite_errno (void const *ptr, idx_t size, idx_t nmemb)
{
  if (fwrite (ptr, size, nmemb, stdout) != nmemb)
    stdout_errno = errno;
}

static void
fflush_errno (void)
{
  if (fflush (stdout) != 0)
    stdout_errno = errno;
}

static struct exclude *excluded_patterns[2];
static struct exclude *excluded_directory_patterns[2];
/* Short options.  */
static char const short_options[] =
"0123456789A:B:C:D:EFGHIPTUVX:abcd:e:f:hiLlm:noqRrsuvwxyZz";

/* Non-boolean long options that have no corresponding short equivalents.  */
enum
{
  BINARY_FILES_OPTION = CHAR_MAX + 1,
  COLOR_OPTION,
  EXCLUDE_DIRECTORY_OPTION,
  EXCLUDE_OPTION,
  EXCLUDE_FROM_OPTION,
  GROUP_SEPARATOR_OPTION,
  INCLUDE_OPTION,
  LINE_BUFFERED_OPTION,
  LABEL_OPTION,
  NO_IGNORE_CASE_OPTION
};

/* Long options equivalences. */
static struct option const long_options[] =
{
  {"basic-regexp",    no_argument, nullptr, 'G'},
  {"extended-regexp", no_argument, nullptr, 'E'},
  {"fixed-regexp",    no_argument, nullptr, 'F'},
  {"fixed-strings",   no_argument, nullptr, 'F'},
  {"perl-regexp",     no_argument, nullptr, 'P'},
  {"after-context", required_argument, nullptr, 'A'},
  {"before-context", required_argument, nullptr, 'B'},
  {"binary-files", required_argument, nullptr, BINARY_FILES_OPTION},
  {"byte-offset", no_argument, nullptr, 'b'},
  {"context", required_argument, nullptr, 'C'},
  {"color", optional_argument, nullptr, COLOR_OPTION},
  {"colour", optional_argument, nullptr, COLOR_OPTION},
  {"count", no_argument, nullptr, 'c'},
  {"devices", required_argument, nullptr, 'D'},
  {"directories", required_argument, nullptr, 'd'},
  {"exclude", required_argument, nullptr, EXCLUDE_OPTION},
  {"exclude-from", required_argument, nullptr, EXCLUDE_FROM_OPTION},
  {"exclude-dir", required_argument, nullptr, EXCLUDE_DIRECTORY_OPTION},
  {"file", required_argument, nullptr, 'f'},
  {"files-with-matches", no_argument, nullptr, 'l'},
  {"files-without-match", no_argument, nullptr, 'L'},
  {"group-separator", required_argument, nullptr, GROUP_SEPARATOR_OPTION},
  {"help", no_argument, &show_help, 1},
  {"include", required_argument, nullptr, INCLUDE_OPTION},
  {"ignore-case", no_argument, nullptr, 'i'},
  {"no-ignore-case", no_argument, nullptr, NO_IGNORE_CASE_OPTION},
  {"initial-tab", no_argument, nullptr, 'T'},
  {"label", required_argument, nullptr, LABEL_OPTION},
  {"line-buffered", no_argument, nullptr, LINE_BUFFERED_OPTION},
  {"line-number", no_argument, nullptr, 'n'},
  {"line-regexp", no_argument, nullptr, 'x'},
  {"max-count", required_argument, nullptr, 'm'},

  {"no-filename", no_argument, nullptr, 'h'},
  {"no-group-separator", no_argument, nullptr, GROUP_SEPARATOR_OPTION},
  {"no-messages", no_argument, nullptr, 's'},
  {"null", no_argument, nullptr, 'Z'},
  {"null-data", no_argument, nullptr, 'z'},
  {"only-matching", no_argument, nullptr, 'o'},
  {"quiet", no_argument, nullptr, 'q'},
  {"recursive", no_argument, nullptr, 'r'},
  {"dereference-recursive", no_argument, nullptr, 'R'},
  {"regexp", required_argument, nullptr, 'e'},
  {"invert-match", no_argument, nullptr, 'v'},
  {"silent", no_argument, nullptr, 'q'},
  {"text", no_argument, nullptr, 'a'},
  {"binary", no_argument, nullptr, 'U'},
  {"version", no_argument, nullptr, 'V'},
  {"with-filename", no_argument, nullptr, 'H'},
  {"word-regexp", no_argument, nullptr, 'w'},
  {0, 0, 0, 0}
};

/* Define flags declared in grep.h. */
bool match_icase;
bool match_words;
bool match_lines;
char eolbyte;

/* For error messages. */
/* The input file name, or (if standard input) null or a --label argument.  */
static char const *filename;
/* Omit leading "./" from file names in diagnostics.  */
static bool omit_dot_slash;
static bool errseen;

/* True if output from the current input file has been suppressed
   because an output line had an encoding error.  */
static bool encoding_error_output;

enum directories_type
  {
    READ_DIRECTORIES = 2,
    RECURSE_DIRECTORIES,
    SKIP_DIRECTORIES
  };

/* How to handle directories.  */
static char const *const directories_args[] =
{
  "read", "recurse", "skip", nullptr
};
static enum directories_type const directories_types[] =
{
  READ_DIRECTORIES, RECURSE_DIRECTORIES, SKIP_DIRECTORIES
};
ARGMATCH_VERIFY (directories_args, directories_types);

static enum directories_type directories = READ_DIRECTORIES;

enum { basic_fts_options = FTS_CWDFD | FTS_NOSTAT | FTS_TIGHT_CYCLE_CHECK };
static int fts_options = basic_fts_options | FTS_COMFOLLOW | FTS_PHYSICAL;

/* How to handle devices. */
static enum
  {
    READ_COMMAND_LINE_DEVICES,
    READ_DEVICES,
    SKIP_DEVICES
  } devices = READ_COMMAND_LINE_DEVICES;

static bool grepfile (int, char const *, bool, bool);
static bool grepdesc (int, bool);

static bool
is_device_mode (mode_t m)
{
  return S_ISCHR (m) || S_ISBLK (m) || S_ISSOCK (m) || S_ISFIFO (m);
}

static bool
skip_devices (bool command_line)
{
  return (devices == SKIP_DEVICES
          || ((devices == READ_COMMAND_LINE_DEVICES) & !command_line));
}

/* Return if ST->st_size is defined.  Assume the file is not a
   symbolic link.  */
static bool
usable_st_size (struct stat const *st)
{
  return S_ISREG (st->st_mode) || S_TYPEISSHM (st) || S_TYPEISTMO (st);
}

/* Lame substitutes for SEEK_DATA and SEEK_HOLE on platforms lacking them.
   Do not rely on these finding data or holes if they equal SEEK_SET.  */
#ifndef SEEK_DATA
enum { SEEK_DATA = SEEK_SET };
#endif
#ifndef SEEK_HOLE
enum { SEEK_HOLE = SEEK_SET };
#endif

/* True if lseek with SEEK_CUR or SEEK_DATA failed on the current input.  */
static bool seek_failed;
static bool seek_data_failed;

/* Functions we'll use to search. */
typedef void *(*compile_fp_t) (char *, idx_t, reg_syntax_t, bool);
typedef ptrdiff_t (*execute_fp_t) (void *, char const *, idx_t, idx_t *,
                                   char const *);
static execute_fp_t execute;
static void *compiled_pattern;

char const *
input_filename (void)
{
  if (!filename)
    filename = _("(standard input)");
  return filename;
}

/* Unless requested, diagnose an error about the input file.  */
static void
suppressible_error (int errnum)
{
  if (! suppress_errors)
    error (0, errnum, "%s", input_filename ());
  errseen = true;
}

/* If there has already been a write error, don't bother closing
   standard output, as that might elicit a duplicate diagnostic.  */
static void
clean_up_stdout (void)
{
  if (! stdout_errno)
    close_stdout ();
}

/* A cast to TYPE of VAL.  Use this when TYPE is a pointer type, VAL
   is properly aligned for TYPE, and 'gcc -Wcast-align' cannot infer
   the alignment and would otherwise complain about the cast.  */
#if 4 < __GNUC__ + (6 <= __GNUC_MINOR__)
# define CAST_ALIGNED(type, val)                           \
    ({ __typeof__ (val) val_ = val;                        \
       _Pragma ("GCC diagnostic push")                     \
       _Pragma ("GCC diagnostic ignored \"-Wcast-align\"") \
       (type) val_;                                        \
       _Pragma ("GCC diagnostic pop")                      \
    })
#else
# define CAST_ALIGNED(type, val) ((type) (val))
#endif

/* An unsigned type suitable for fast matching.  */
typedef uintmax_t uword;
static uword const uword_max = UINTMAX_MAX;
enum { uword_size = sizeof (uword) }; /* For when a signed size is wanted.  */

struct localeinfo localeinfo;

/* A mask to test for unibyte characters, with the pattern repeated to
   fill a uword.  For a multibyte character encoding where
   all bytes are unibyte characters, this is 0.  For UTF-8, this is
   0x808080....  For encodings where unibyte characters have no discerned
   pattern, this is all 1s.  The unsigned char C is a unibyte
   character if C & UNIBYTE_MASK is zero.  If the uword W is the
   concatenation of bytes, the bytes are all unibyte characters
   if W & UNIBYTE_MASK is zero.  */
static uword unibyte_mask;

static void
initialize_unibyte_mask (void)
{
  /* For each encoding error I that MASK does not already match,
     accumulate I's most significant 1 bit by ORing it into MASK.
     Although any 1 bit of I could be used, in practice high-order
     bits work better.  */
  unsigned char mask = 0;
  int ms1b = 1;
  for (int i = 1; i <= UCHAR_MAX; i++)
    if ((localeinfo.sbclen[i] != 1) & ! (mask & i))
      {
        while (ms1b * 2 <= i)
          ms1b *= 2;
        mask |= ms1b;
      }

  /* Now MASK will detect any encoding-error byte, although it may
     cry wolf and it may not be optimal.  Build a uword-length mask by
     repeating MASK.  */
  unibyte_mask = uword_max / UCHAR_MAX * mask;
}

/* Skip the easy bytes in a buffer that is guaranteed to have a sentinel
   that is not easy, and return a pointer to the first non-easy byte.
   The easy bytes all have UNIBYTE_MASK off.  */
static char const * _GL_ATTRIBUTE_PURE
skip_easy_bytes (char const *buf)
{
  /* Search a byte at a time until the pointer is aligned, then a
     uword at a time until a match is found, then a byte at a time to
     identify the exact byte.  The uword search may go slightly past
     the buffer end, but that's benign.  */
  char const *p;
  uword const *s;
  for (p = buf; (uintptr_t) p % uword_size != 0; p++)
    if (to_uchar (*p) & unibyte_mask)
      return p;
  for (s = CAST_ALIGNED (uword const *, p); ! (*s & unibyte_mask); s++)
    continue;
  for (p = (char const *) s; ! (to_uchar (*p) & unibyte_mask); p++)
    continue;
  return p;
}

/* Return true if BUF, of size SIZE, has an encoding error.
   BUF must be followed by at least uword_size bytes,
   the first of which may be modified.  */
static bool
buf_has_encoding_errors (char *buf, idx_t size)
{
  if (! unibyte_mask)
    return false;

  mbstate_t mbs; mbszero (&mbs);
  ptrdiff_t clen;

  buf[size] = -1;
  for (char const *p = buf; (p = skip_easy_bytes (p)) < buf + size; p += clen)
    {
      clen = imbrlen (p, buf + size - p, &mbs);
      if (clen < 0)
        return true;
    }

  return false;
}


/* Return true if BUF, of size SIZE, has a null byte.
   BUF must be followed by at least one byte,
   which may be arbitrarily written to or read from.  */
static bool
buf_has_nulls (char *buf, idx_t size)
{
  buf[size] = 0;
  return strlen (buf) != size;
}

/* Return true if a file is known to contain null bytes.
   SIZE bytes have already been read from the file
   with descriptor FD and status ST.  */
static bool
file_must_have_nulls (idx_t size, int fd, struct stat const *st)
{
  /* If the file has holes, it must contain a null byte somewhere.  */
  if (SEEK_HOLE != SEEK_SET && !seek_failed
      && usable_st_size (st) && size < st->st_size)
    {
      off_t cur = size;
      if (O_BINARY || fd == STDIN_FILENO)
        {
          cur = lseek (fd, 0, SEEK_CUR);
          if (cur < 0)
            return false;
        }

      /* Look for a hole after the current location.  */
      off_t hole_start = lseek (fd, cur, SEEK_HOLE);
      if (0 <= hole_start)
        {
          if (lseek (fd, cur, SEEK_SET) < 0)
            suppressible_error (errno);
          if (hole_start < st->st_size)
            return true;
        }
    }

  return false;
}

/* Convert STR to a nonnegative integer, storing the result in *OUT.
   STR must be a valid context length argument; report an error if it
   isn't.  Silently ceiling *OUT at the maximum value, as that is
   practically equivalent to infinity for grep's purposes.  */
static void
context_length_arg (char const *str, intmax_t *out)
{
  switch (xstrtoimax (str, 0, 10, out, ""))
    {
    case LONGINT_OK:
    case LONGINT_OVERFLOW:
      if (0 <= *out)
        break;
      FALLTHROUGH;
    default:
      die (EXIT_TROUBLE, 0, "%s: %s", str,
           _("invalid context length argument"));
    }
}

/* Return the add_exclude options suitable for excluding a file name.
   If COMMAND_LINE, it is a command-line file name.  */
static int
exclude_options (bool command_line)
{
  return EXCLUDE_WILDCARDS | (command_line ? 0 : EXCLUDE_ANCHORED);
}

/* Return true if the file with NAME should be skipped.
   If COMMAND_LINE, it is a command-line argument.
   If IS_DIR, it is a directory.  */
static bool
skipped_file (char const *name, bool command_line, bool is_dir)
{
  struct exclude **pats;
  if (! is_dir)
    pats = excluded_patterns;
  else if (directories == SKIP_DIRECTORIES)
    return true;
  else if (command_line && omit_dot_slash)
    return false;
  else
    pats = excluded_directory_patterns;
  return pats[command_line] && excluded_file_name (pats[command_line], name);
}

/* Hairy buffering mechanism for grep.  The intent is to keep
   all reads aligned on a page boundary and multiples of the
   page size, unless a read yields a partial page.  */

static char *buffer;		/* Base of buffer. */
static idx_t bufalloc;		/* Allocated buffer size, counting slop. */
static int bufdesc;		/* File descriptor. */
static char *bufbeg;		/* Beginning of user-visible stuff. */
static char *buflim;		/* Limit of user-visible stuff. */
static idx_t pagesize;		/* alignment of memory pages */
static idx_t good_readsize;	/* good size to pass to 'read' */
static off_t bufoffset;		/* Read offset.  */
static off_t after_last_match;	/* Pointer after last matching line that
                                   would have been output if we were
                                   outputting characters. */
static bool skip_nuls;		/* Skip '\0' in data.  */
static bool skip_empty_lines;	/* Skip empty lines in data.  */
static intmax_t totalnl;	/* Total newline count before lastnl. */

/* Minimum value for good_readsize.
   If it's too small, there are more syscalls;
   if too large, it wastes memory and likely cache.
   Use 96 KiB as it gave good results in a benchmark in 2018
   (see 2018-09-06 commit labeled "grep: triple initial buffer size: 32k->96k")
   even though the same benchmark in 2024 found no significant
   difference for values from 32 KiB to 1024 KiB on Ubuntu 24.04.1 LTS
   with an Intel Xeon W-1350.  */
enum { GOOD_READSIZE_MIN = 96 * 1024 };

/* Return VAL aligned to the next multiple of ALIGNMENT.  VAL can be
   an integer or a pointer.  Both args must be free of side effects.  */
#define ALIGN_TO(val, alignment) \
  ((uintptr_t) (val) % (alignment) == 0 \
   ? (val) \
   : (val) + ((alignment) - (uintptr_t) (val) % (alignment)))

/* Add two numbers that count input bytes or lines, and report an
   error if the addition overflows.  */
static intmax_t
add_count (intmax_t a, idx_t b)
{
  intmax_t sum;
  if (ckd_add (&sum, a, b))
    die (EXIT_TROUBLE, 0, _("input is too large to count"));
  return sum;
}

/* Return true if BUF (of size SIZE) is all zeros.  */
static bool
all_zeros (char const *buf, idx_t size)
{
  for (char const *p = buf; p < buf + size; p++)
    if (*p)
      return false;
  return true;
}

/* Reset the buffer for a new file, returning false if we should skip it.
   Initialize on the first time through. */
static bool
reset (int fd, struct stat const *st)
{
  bufbeg = buflim = ALIGN_TO (buffer + 1, pagesize);
  bufbeg[-1] = eolbyte;
  bufdesc = fd;
  bufoffset = fd == STDIN_FILENO ? lseek (fd, 0, SEEK_CUR) : 0;
  seek_failed = bufoffset < 0;

  /* Assume SEEK_DATA fails if SEEK_CUR does.  */
  seek_data_failed = seek_failed;

  if (seek_failed)
    {
      if (errno != ESPIPE)
        {
          suppressible_error (errno);
          return false;
        }
      bufoffset = 0;
    }
  return true;
}

/* Read new stuff into the buffer, saving the specified
   amount of old stuff.  When we're done, 'bufbeg' points
   to the beginning of the buffer contents, and 'buflim'
   points just after the end.  Return false if there's an error.  */
static bool
fillbuf (idx_t save, struct stat const *st)
{
  char *readbuf;

  /* After BUFLIM, we need room for a good-sized read plus a
     trailing uword.  */
  idx_t min_after_buflim = good_readsize + uword_size;

  if (min_after_buflim <= buffer + bufalloc - buflim)
    readbuf = buflim;
  else
    {
      char *newbuf;

      /* For data to be searched we need room for the saved bytes,
         plus at least a good-sized read.  */
      idx_t minsize = save + good_readsize;

      /* Add enough room so that the buffer is aligned and has room
         for byte sentinels fore and aft, and so that a uword can
         be read aft.  */
      ptrdiff_t incr_min = minsize - bufalloc + min_after_buflim;

      if (incr_min <= 0)
        newbuf = buffer;
      else
        {
          /* Try not to allocate more memory than the file size indicates,
             as that might cause unnecessary memory exhaustion if the file
             is large.  However, do not use the original file size as a
             heuristic if we've already read past the file end, as most
             likely the file is growing.  */
          ptrdiff_t alloc_max = -1;
          if (usable_st_size (st))
            {
              off_t to_be_read = st->st_size - bufoffset;
              ptrdiff_t a;
              if (0 <= to_be_read
                  && !ckd_add (&a, to_be_read, save + min_after_buflim))
                alloc_max = MAX (a, bufalloc + incr_min);
            }

          newbuf = xpalloc (nullptr, &bufalloc, incr_min, alloc_max, 1);
        }

      readbuf = ALIGN_TO (newbuf + 1 + save, pagesize);
      idx_t moved = save + 1;  /* Move the preceding byte sentinel too.  */
      memmove (readbuf - moved, buflim - moved, moved);
      if (0 < incr_min)
        {
          free (buffer);
          buffer = newbuf;
        }
    }

  bufbeg = readbuf - save;

  clear_asan_poison ();

  ptrdiff_t fillsize;
  bool cc = true;

  while (true)
    {
      fillsize = safe_read (bufdesc, readbuf, good_readsize);
      if (fillsize < 0)
        {
          fillsize = 0;
          cc = false;
        }
      bufoffset += fillsize;

      if (((fillsize == 0) | !skip_nuls) || !all_zeros (readbuf, fillsize))
        break;
      totalnl = add_count (totalnl, fillsize);

      if (SEEK_DATA != SEEK_SET && !seek_data_failed)
        {
          /* Solaris SEEK_DATA fails with errno == ENXIO in a hole at EOF.  */
          off_t data_start = lseek (bufdesc, bufoffset, SEEK_DATA);
          if (data_start < 0 && errno == ENXIO
              && usable_st_size (st) && bufoffset < st->st_size)
            data_start = lseek (bufdesc, 0, SEEK_END);

          if (data_start < 0)
            seek_data_failed = true;
          else
            {
              totalnl = add_count (totalnl, data_start - bufoffset);
              bufoffset = data_start;
            }
        }
    }

  buflim = readbuf + fillsize;

  /* Initialize the following word, because skip_easy_bytes and some
     matchers read (but do not use) those bytes.  This avoids false
     positive reports of these bytes being used uninitialized.  */
  memset (buflim, 0, uword_size);

  /* Mark the part of the buffer not filled by the read or set by
     the above memset call as ASAN-poisoned.  */
  asan_poison (buflim + uword_size, bufalloc - (buflim - buffer) - uword_size);

  return cc;
}

/* Flags controlling the style of output. */
static enum
{
  BINARY_BINARY_FILES,
  TEXT_BINARY_FILES,
  WITHOUT_MATCH_BINARY_FILES
} binary_files;		/* How to handle binary files.  */

/* Options for output as a list of matching/non-matching files */
static enum
{
  LISTFILES_NONE,
  LISTFILES_MATCHING,
  LISTFILES_NONMATCHING,
} list_files;

/* Whether to output filenames.  1 means yes, 0 means no, and -1 means
   'grep -r PATTERN FILE' was used and it is not known yet whether
   FILE is a directory (which means yes) or not (which means no).  */
static int out_file;

static int filename_mask;	/* If zero, output nulls after filenames.  */
static bool out_quiet;		/* Suppress all normal output. */
static bool out_invert;		/* Print nonmatching stuff. */
static bool out_line;		/* Print line numbers. */
static bool out_byte;		/* Print byte offsets. */
static intmax_t out_before;	/* Lines of leading context. */
static intmax_t out_after;	/* Lines of trailing context. */
static bool count_matches;	/* Count matching lines.  */
static intmax_t max_count;	/* Max number of selected
                                   lines from an input file.  */
static bool line_buffered;	/* Use line buffering.  */
static char *label;		/* Fake filename for stdin */


/* Internal variables to keep track of byte count, context, etc. */
static intmax_t totalcc;	/* Total character count before bufbeg. */
static char const *lastnl;	/* Pointer after last newline counted. */
static char *lastout;		/* Pointer after last character output;
                                   null if no character has been output
                                   or if it's conceptually before bufbeg. */
static intmax_t outleft;	/* Maximum number of selected lines.  */
static intmax_t pending;	/* Pending lines of output.
                                   Always kept 0 if out_quiet is true.  */
static bool done_on_match;	/* Stop scanning file on first match.  */
static bool exit_on_match;	/* Exit on first match.  */
static bool dev_null_output;	/* Stdout is known to be /dev/null.  */
static bool binary;		/* Use binary rather than text I/O.  */

static void
nlscan (char const *lim)
{
  idx_t newlines = 0;
  for (char const *beg = lastnl; beg < lim; beg++)
    {
      beg = memchr (beg, eolbyte, lim - beg);
      if (!beg)
        break;
      newlines++;
    }
  totalnl = add_count (totalnl, newlines);
  lastnl = lim;
}

/* Print the current filename.  */
static void
print_filename (void)
{
  pr_sgr_start_if (filename_color);
  fputs_errno (input_filename ());
  pr_sgr_end_if (filename_color);
}

/* Print a character separator.  */
static void
print_sep (char sep)
{
  pr_sgr_start_if (sep_color);
  putchar_errno (sep);
  pr_sgr_end_if (sep_color);
}

/* Print a line number or a byte offset.  */
static void
print_offset (intmax_t pos, const char *color)
{
  pr_sgr_start_if (color);
  printf_errno ("%*"PRIdMAX, offset_width, pos);
  pr_sgr_end_if (color);
}

/* Print a whole line head (filename, line, byte).  The output data
   starts at BEG and contains LEN bytes; it is followed by at least
   uword_size bytes, the first of which may be temporarily modified.
   The output data comes from what is perhaps a larger input line that
   goes until LIM, where LIM[-1] is an end-of-line byte.  Use SEP as
   the separator on output.

   Return true unless the line was suppressed due to an encoding error.  */

static bool
print_line_head (char *beg, idx_t len, char const *lim, char sep)
{
  if (binary_files != TEXT_BINARY_FILES)
    {
      char ch = beg[len];
      bool encoding_errors = buf_has_encoding_errors (beg, len);
      beg[len] = ch;
      if (encoding_errors)
        {
          encoding_error_output = true;
          return false;
        }
    }

  if (out_file)
    {
      print_filename ();
      if (filename_mask)
        print_sep (sep);
      else
        putchar_errno (0);
    }

  if (out_line)
    {
      if (lastnl < lim)
        {
          nlscan (beg);
          totalnl = add_count (totalnl, 1);
          lastnl = lim;
        }
      print_offset (totalnl, line_num_color);
      print_sep (sep);
    }

  if (out_byte)
    {
      intmax_t pos = add_count (totalcc, beg - bufbeg);
      print_offset (pos, byte_num_color);
      print_sep (sep);
    }

  if (align_tabs && (out_file | out_line | out_byte) && len != 0)
    putchar_errno ('\t');

  return true;
}

static char *
print_line_middle (char *beg, char *lim,
                   const char *line_color, const char *match_color)
{
  idx_t match_size;
  ptrdiff_t match_offset;
  char *cur;
  char *mid = nullptr;
  char *b;

  for (cur = beg;
       (cur < lim
        && 0 <= (match_offset = execute (compiled_pattern, beg, lim - beg,
                                         &match_size, cur)));
       cur = b + match_size)
    {
      b = beg + match_offset;

      /* Avoid matching the empty line at the end of the buffer. */
      if (b == lim)
        break;

      /* Avoid hanging on grep --color "" foo */
      if (match_size == 0)
        {
          /* Make minimal progress; there may be further non-empty matches.  */
          /* XXX - Could really advance by one whole multi-octet character.  */
          match_size = 1;
          if (!mid)
            mid = cur;
        }
      else
        {
          /* This function is called on a matching line only,
             but is it selected or rejected/context?  */
          if (only_matching)
            {
              char sep = out_invert ? SEP_CHAR_REJECTED : SEP_CHAR_SELECTED;
              if (! print_line_head (b, match_size, lim, sep))
                return nullptr;
            }
          else
            {
              pr_sgr_start (line_color);
              if (mid)
                {
                  cur = mid;
                  mid = nullptr;
                }
              fwrite_errno (cur, 1, b - cur);
            }

          pr_sgr_start_if (match_color);
          fwrite_errno (b, 1, match_size);
          pr_sgr_end_if (match_color);
          if (only_matching)
            putchar_errno (eolbyte);
        }
    }

  if (only_matching)
    cur = lim;
  else if (mid)
    cur = mid;

  return cur;
}

static char *
print_line_tail (char *beg, const char *lim, const char *line_color)
{
  idx_t eol_size;
  idx_t tail_size;

  eol_size   = (lim > beg && lim[-1] == eolbyte);
  eol_size  += (lim - eol_size > beg && lim[-(1 + eol_size)] == '\r');
  tail_size  =  lim - eol_size - beg;

  if (tail_size > 0)
    {
      pr_sgr_start (line_color);
      fwrite_errno (beg, 1, tail_size);
      beg += tail_size;
      pr_sgr_end (line_color);
    }

  return beg;
}

static void
prline (char *beg, char *lim, char sep)
{
  bool matching;
  const char *line_color;
  const char *match_color;

  if (!only_matching)
    if (! print_line_head (beg, lim - beg - 1, lim, sep))
      return;

  matching = (sep == SEP_CHAR_SELECTED) ^ out_invert;

  if (color_option)
    {
      line_color = (((sep == SEP_CHAR_SELECTED)
                     ^ (out_invert && (color_option < 0)))
                    ? selected_line_color  : context_line_color);
      match_color = (sep == SEP_CHAR_SELECTED
                     ? selected_match_color : context_match_color);
    }
  else
    line_color = match_color = nullptr; /* Shouldn't be used.  */

  if ((only_matching && matching)
      || (color_option && (*line_color || *match_color)))
    {
      /* We already know that non-matching lines have no match (to colorize). */
      if (matching && (only_matching || *match_color))
        {
          beg = print_line_middle (beg, lim, line_color, match_color);
          if (! beg)
            return;
        }

      if (!only_matching && *line_color)
        {
          /* This code is exercised at least when grep is invoked like this:
             echo k| GREP_COLORS='sl=01;32' src/grep k --color=always  */
          beg = print_line_tail (beg, lim, line_color);
        }
    }

  if (!only_matching && lim > beg)
    fwrite_errno (beg, 1, lim - beg);

  if (line_buffered)
    fflush_errno ();

  if (stdout_errno)
    die (EXIT_TROUBLE, stdout_errno, _("write error"));

  lastout = lim;
}

/* Print pending lines of trailing context prior to LIM.  */
static void
prpending (char const *lim)
{
  if (!lastout)
    lastout = bufbeg;
  for (; 0 < pending && lastout < lim; pending--)
    {
      char *nl = rawmemchr (lastout, eolbyte);
      prline (lastout, nl + 1, SEP_CHAR_REJECTED);
    }
}

/* Output the lines between BEG and LIM.  Deal with context.  */
static void
prtext (char *beg, char *lim)
{
  static bool used;	/* Avoid printing SEP_STR_GROUP before any output.  */
  char eol = eolbyte;

  if (!out_quiet && pending > 0)
    prpending (beg);

  char *p = beg;

  if (!out_quiet)
    {
      /* Deal with leading context.  */
      char const *bp = lastout ? lastout : bufbeg;
      intmax_t i;
      for (i = 0; i < out_before; ++i)
        if (p > bp)
          do
            --p;
          while (p[-1] != eol);

      /* Print the group separator unless the output is adjacent to
         the previous output in the file.  */
      if ((0 <= out_before || 0 <= out_after) && used
          && p != lastout && group_separator)
        {
          pr_sgr_start_if (sep_color);
          fputs_errno (group_separator);
          pr_sgr_end_if (sep_color);
          putchar_errno ('\n');
        }

      while (p < beg)
        {
          char *nl = rawmemchr (p, eol);
          nl++;
          prline (p, nl, SEP_CHAR_REJECTED);
          p = nl;
        }
    }

  intmax_t n;
  if (out_invert)
    {
      /* One or more lines are output.  */
      for (n = 0; p < lim && n < outleft; n++)
        {
          char *nl = rawmemchr (p, eol);
          nl++;
          if (!out_quiet)
            prline (p, nl, SEP_CHAR_SELECTED);
          p = nl;
        }
    }
  else
    {
      /* Just one line is output.  */
      if (!out_quiet)
        prline (beg, lim, SEP_CHAR_SELECTED);
      n = 1;
      p = lim;
    }

  after_last_match = bufoffset - (buflim - p);
  pending = out_quiet ? 0 : MAX (0, out_after);
  used = true;
  outleft -= n;
}

/* Replace all NUL bytes in buffer P (which ends at LIM) with EOL.
   This avoids running out of memory when binary input contains a long
   sequence of zeros, which would otherwise be considered to be part
   of a long line.  *LIM should be EOL.  */
static void
zap_nuls (char *p, char *lim, char eol)
{
  if (eol)
    while (true)
      {
        *lim = '\0';
        p += strlen (p);
        *lim = eol;
        if (p == lim)
          break;
        do
          *p++ = eol;
        while (!*p);
      }
}

/* Scan the specified portion of the buffer, matching lines (or
   between matching lines if OUT_INVERT is true).  Return a count of
   lines printed.  Replace all NUL bytes with NUL_ZAPPER as we go.  */
static intmax_t
grepbuf (char *beg, char const *lim)
{
  intmax_t outleft0 = outleft;
  char *endp;

  for (char *p = beg; p < lim; p = endp)
    {
      idx_t match_size;
      ptrdiff_t match_offset = execute (compiled_pattern, p, lim - p,
                                        &match_size, nullptr);
      if (match_offset < 0)
        {
          if (!out_invert)
            break;
          match_offset = lim - p;
          match_size = 0;
        }
      char *b = p + match_offset;
      endp = b + match_size;
      /* Avoid matching the empty line at the end of the buffer. */
      if (!out_invert && b == lim)
        break;
      if (!out_invert || p < b)
        {
          if (list_files != LISTFILES_NONE)
            return 1;
          char *prbeg = out_invert ? p : b;
          char *prend = out_invert ? b : endp;
          prtext (prbeg, prend);
          if (!outleft || done_on_match)
            {
              if (exit_on_match)
                {
                  stdout_errno = -1;
                  exit (EXIT_SUCCESS);
                }
              break;
            }
        }
    }

  return outleft0 - outleft;
}

/* Search a given (non-directory) file.  Return a count of lines printed.
   Set *INEOF to true if end-of-file reached.  */
static intmax_t
grep (int fd, struct stat const *st, bool *ineof)
{
  intmax_t nlines, i;
  idx_t residue, save;
  char eol = eolbyte;
  char nul_zapper = '\0';
  bool done_on_match_0 = done_on_match;
  bool out_quiet_0 = out_quiet;

  /* The value of NLINES when nulls were first deduced in the input;
     this is not necessarily the same as the number of matching lines
     before the first null.  -1 if no input nulls have been deduced.  */
  intmax_t nlines_first_null = -1;

  if (! reset (fd, st))
    return 0;

  totalcc = 0;
  lastout = 0;
  totalnl = 0;
  outleft = max_count;
  after_last_match = 0;
  pending = 0;
  skip_nuls = skip_empty_lines && !eol;
  encoding_error_output = false;

  nlines = 0;
  residue = 0;
  save = 0;

  if (! fillbuf (save, st))
    {
      suppressible_error (errno);
      return 0;
    }

  offset_width = 0;
  if (align_tabs)
    {
      /* Width is log of maximum number.  Line numbers are origin-1.  */
      intmax_t num = usable_st_size (st) ? st->st_size : INTMAX_MAX;
      num += out_line && num < INTMAX_MAX;
      do
        offset_width++;
      while ((num /= 10) != 0);
    }

  for (bool firsttime = true; ; firsttime = false)
    {
      if (nlines_first_null < 0 && eol && binary_files != TEXT_BINARY_FILES
          && (buf_has_nulls (bufbeg, buflim - bufbeg)
              || (firsttime && file_must_have_nulls (buflim - bufbeg, fd, st))))
        {
          if (binary_files == WITHOUT_MATCH_BINARY_FILES)
            return 0;
          if (!count_matches)
            {
              out_quiet = true;
              if (max_count == INTMAX_MAX)
                done_on_match = true;
            }
          nlines_first_null = nlines;
          nul_zapper = eol;
          skip_nuls = skip_empty_lines;
        }

      lastnl = bufbeg;
      if (lastout)
        lastout = bufbeg;

      char *beg = bufbeg + save;

      /* no more data to scan (eof) except for maybe a residue -> break */
      if (beg == buflim)
        {
          *ineof = true;
          break;
        }

      zap_nuls (beg, buflim, nul_zapper);

      /* Determine new residue (the length of an incomplete line at the end of
         the buffer, 0 means there is no incomplete last line).  */
      char *last_eol = memrchr (beg, eol, buflim - beg);
      beg -= residue;
      char *lim = last_eol ? last_eol + 1 : beg;
      residue = buflim - lim;

      if (beg < lim)
        {
          if (outleft)
            nlines += grepbuf (beg, lim);
          if (pending)
            prpending (lim);
          if ((!outleft && !pending)
              || (done_on_match && MAX (0, nlines_first_null) < nlines))
            goto finish_grep;
        }

      /* The last OUT_BEFORE lines at the end of the buffer will be needed as
         leading context if there is a matching line at the begin of the
         next data. Make beg point to their begin.  */
      i = 0;
      beg = lim;
      while (i < out_before && beg > bufbeg && beg != lastout)
        {
          ++i;
          do
            --beg;
          while (beg[-1] != eol);
        }

      /* Detect whether leading context is adjacent to previous output.  */
      if (beg != lastout)
        lastout = 0;

      /* Handle some details and read more data to scan.  */
      save = residue + lim - beg;
      if (out_byte)
        totalcc = add_count (totalcc, buflim - bufbeg - save);
      if (out_line)
        nlscan (beg);
      if (! fillbuf (save, st))
        {
          suppressible_error (errno);
          goto finish_grep;
        }
    }
  if (residue)
    {
      *buflim++ = eol;
      if (outleft)
        nlines += grepbuf (bufbeg + save - residue, buflim);
      if (pending)
        prpending (buflim);
    }

 finish_grep:
  done_on_match = done_on_match_0;
  out_quiet = out_quiet_0;
  if (binary_files == BINARY_BINARY_FILES && !out_quiet
      && (encoding_error_output
          || (0 <= nlines_first_null && nlines_first_null < nlines)))
    error (0, 0, _("%s: binary file matches"), input_filename ());
  return nlines;
}

static bool
grepdirent (FTS *fts, FTSENT *ent, bool command_line)
{
  bool follow;
  command_line &= ent->fts_level == FTS_ROOTLEVEL;

  if (ent->fts_info == FTS_DP)
    return true;

  if (!command_line
      && skipped_file (ent->fts_name, false,
                       (ent->fts_info == FTS_D || ent->fts_info == FTS_DC
                        || ent->fts_info == FTS_DNR)))
    {
      fts_set (fts, ent, FTS_SKIP);
      return true;
    }

  filename = ent->fts_path;
  if (omit_dot_slash && filename[1])
    filename += 2;
  follow = (fts->fts_options & FTS_LOGICAL
            || (fts->fts_options & FTS_COMFOLLOW && command_line));

  switch (ent->fts_info)
    {
    case FTS_D:
      if (directories == RECURSE_DIRECTORIES)
        return true;
      fts_set (fts, ent, FTS_SKIP);
      break;

    case FTS_DC:
      if (!suppress_errors)
        error (0, 0, _("%s: warning: recursive directory loop"), filename);
      return true;

    case FTS_DNR:
    case FTS_ERR:
    case FTS_NS:
      suppressible_error (ent->fts_errno);
      return true;

    case FTS_DEFAULT:
    case FTS_NSOK:
      if (skip_devices (command_line))
        {
          struct stat *st = ent->fts_statp;
          struct stat st1;
          if (! st->st_mode)
            {
              /* The file type is not already known.  Get the file status
                 before opening, since opening might have side effects
                 on a device.  */
              int flag = follow ? 0 : AT_SYMLINK_NOFOLLOW;
              if (fstatat (fts->fts_cwd_fd, ent->fts_accpath, &st1, flag) != 0)
                {
                  suppressible_error (errno);
                  return true;
                }
              st = &st1;
            }
          if (is_device_mode (st->st_mode))
            return true;
        }
      break;

    case FTS_F:
    case FTS_SLNONE:
      break;

    case FTS_SL:
    case FTS_W:
      return true;

    default:
      abort ();
    }

  return grepfile (fts->fts_cwd_fd, ent->fts_accpath, follow, command_line);
}

/* True if errno is ERR after 'open ("symlink", ... O_NOFOLLOW ...)'.
   POSIX specifies ELOOP, but it's EMLINK on FreeBSD and EFTYPE on NetBSD.  */
static bool
open_symlink_nofollow_error (int err)
{
  if (err == ELOOP || err == EMLINK)
    return true;
#ifdef EFTYPE
  if (err == EFTYPE)
    return true;
#endif
  return false;
}

static bool
grepfile (int dirdesc, char const *name, bool follow, bool command_line)
{
  int oflag = (O_RDONLY | O_NOCTTY
               | (IGNORE_DUPLICATE_BRANCH_WARNING
                  (binary ? O_BINARY : 0))
               | (follow ? 0 : O_NOFOLLOW)
               | (skip_devices (command_line) ? O_NONBLOCK : 0));
  int desc = openat_safer (dirdesc, name, oflag);
  if (desc < 0)
    {
      if (follow || ! open_symlink_nofollow_error (errno))
        suppressible_error (errno);
      return true;
    }
  return grepdesc (desc, command_line);
}

/* Read all data from FD, with status ST.  Return true if successful,
   false (setting errno) otherwise.  */
static bool
drain_input (int fd, struct stat const *st)
{
  ssize_t nbytes;
  if (S_ISFIFO (st->st_mode) && dev_null_output)
    {
#ifdef SPLICE_F_MOVE
      /* Should be faster, since it need not copy data to user space.  */
      nbytes = splice (fd, nullptr, STDOUT_FILENO, nullptr,
                       good_readsize, SPLICE_F_MOVE);
      if (0 <= nbytes || errno != EINVAL)
        {
          while (0 < nbytes)
            nbytes = splice (fd, nullptr, STDOUT_FILENO, nullptr,
                             good_readsize, SPLICE_F_MOVE);
          return nbytes == 0;
        }
#endif
    }
  while ((nbytes = safe_read (fd, buffer, bufalloc)))
    if (nbytes < 0)
      return false;
  return true;
}

/* Finish reading from FD, with status ST and where end-of-file has
   been seen if INEOF.  Typically this is a no-op, but when reading
   from standard input this may adjust the file offset or drain a
   pipe.  */

static void
finalize_input (int fd, struct stat const *st, bool ineof)
{
  if (fd == STDIN_FILENO
      && (outleft
          ? (!ineof
             && (seek_failed
                 || (lseek (fd, 0, SEEK_END) < 0
                     /* Linux proc file system has EINVAL (Bug#25180).  */
                     && errno != EINVAL))
             && ! drain_input (fd, st))
          : (bufoffset != after_last_match && !seek_failed
             && lseek (fd, after_last_match, SEEK_SET) < 0)))
    suppressible_error (errno);
}

static bool
grepdesc (int desc, bool command_line)
{
  intmax_t count;
  bool status = true;
  bool ineof = false;
  struct stat st;

  /* Get the file status, possibly for the second time.  This catches
     a race condition if the directory entry changes after the
     directory entry is read and before the file is opened.  For
     example, normally DESC is a directory only at the top level, but
     there is an exception if some other process substitutes a
     directory for a non-directory while 'grep' is running.  */
  if (fstat (desc, &st) != 0)
    {
      suppressible_error (errno);
      goto closeout;
    }

  if (desc != STDIN_FILENO && skip_devices (command_line)
      && is_device_mode (st.st_mode))
    goto closeout;

  if (desc != STDIN_FILENO && command_line
      && skipped_file (filename, true, S_ISDIR (st.st_mode) != 0))
    goto closeout;

  /* Don't output file names if invoked as 'grep -r PATTERN NONDIRECTORY'.  */
  if (out_file < 0)
    out_file = !!S_ISDIR (st.st_mode);

  if (desc != STDIN_FILENO
      && directories == RECURSE_DIRECTORIES && S_ISDIR (st.st_mode))
    {
      /* Traverse the directory starting with its full name, because
         unfortunately fts provides no way to traverse the directory
         starting from its file descriptor.  */

      FTS *fts;
      FTSENT *ent;
      int opts = fts_options & ~(command_line ? 0 : FTS_COMFOLLOW);
      char *fts_arg[2];

      /* Close DESC now, to conserve file descriptors if the race
         condition occurs many times in a deep recursion.  */
      if (close (desc) != 0)
        suppressible_error (errno);

      fts_arg[0] = (char *) filename;
      fts_arg[1] = nullptr;
      fts = fts_open (fts_arg, opts, nullptr);

      if (!fts)
        xalloc_die ();
      while ((ent = fts_read (fts)))
        status &= grepdirent (fts, ent, command_line);
      if (errno)
        suppressible_error (errno);
      if (fts_close (fts) != 0)
        suppressible_error (errno);
      return status;
    }
  if (desc != STDIN_FILENO
      && ((directories == SKIP_DIRECTORIES && S_ISDIR (st.st_mode))
          || ((devices == SKIP_DEVICES
               || (devices == READ_COMMAND_LINE_DEVICES && !command_line))
              && is_device_mode (st.st_mode))))
    goto closeout;

  /* If there is a regular file on stdout and the current file refers
     to the same i-node, we have to report the problem and skip it.
     Otherwise when matching lines from some other input reach the
     disk before we open this file, we can end up reading and matching
     those lines and appending them to the file from which we're reading.
     Then we'd have what appears to be an infinite loop that'd terminate
     only upon filling the output file system or reaching a quota.
     However, there is no risk of an infinite loop if grep is generating
     no output, i.e., with --silent, --quiet, -q.
     Similarly, with any of these:
       --max-count=N (-m) (for N >= 2)
       --files-with-matches (-l)
       --files-without-match (-L)
     there is no risk of trouble.
     For --max-count=1, grep stops after printing the first match,
     so there is no risk of malfunction.  But even --max-count=2, with
     input==output, while there is no risk of infloop, there is a race
     condition that could result in "alternate" output.  */
  if (!out_quiet && list_files == LISTFILES_NONE && 1 < max_count
      && S_ISREG (st.st_mode) && SAME_INODE (st, out_stat))
    {
      if (! suppress_errors)
        error (0, 0, _("%s: input file is also the output"), input_filename ());
      errseen = true;
      goto closeout;
    }

  count = grep (desc, &st, &ineof);
  if (count_matches)
    {
      if (out_file)
        {
          print_filename ();
          if (filename_mask)
            print_sep (SEP_CHAR_SELECTED);
          else
            putchar_errno (0);
        }
      printf_errno ("%" PRIdMAX "\n", count);
      if (line_buffered)
        fflush_errno ();
    }

  status = !count;

  if (list_files == LISTFILES_NONE)
    finalize_input (desc, &st, ineof);
  else if (list_files == (status ? LISTFILES_NONMATCHING : LISTFILES_MATCHING))
    {
      print_filename ();
      putchar_errno ('\n' & filename_mask);
      if (line_buffered)
        fflush_errno ();
    }

 closeout:
  if (desc != STDIN_FILENO && close (desc) != 0)
    suppressible_error (errno);
  return status;
}

static bool
grep_command_line_arg (char const *arg)
{
  if (STREQ (arg, "-"))
    {
      filename = label;
      if (binary)
        xset_binary_mode (STDIN_FILENO, O_BINARY);
      return grepdesc (STDIN_FILENO, true);
    }
  else
    {
      filename = arg;
      return grepfile (AT_FDCWD, arg, true, true);
    }
}

_Noreturn void usage (int);
void
usage (int status)
{
  if (status != 0)
    {
      fprintf (stderr, _("Usage: %s [OPTION]... PATTERNS [FILE]...\n"),
               getprogname ());
      fprintf (stderr, _("Try '%s --help' for more information.\n"),
               getprogname ());
    }
  else
    {
      printf (_("Usage: %s [OPTION]... PATTERNS [FILE]...\n"), getprogname ());
      printf (_("Search for PATTERNS in each FILE.\n"));
      printf (_("\
Example: %s -i 'hello world' menu.h main.c\n\
PATTERNS can contain multiple patterns separated by newlines.\n\
\n\
Pattern selection and interpretation:\n"), getprogname ());
      printf (_("\
  -E, --extended-regexp     PATTERNS are extended regular expressions\n\
  -F, --fixed-strings       PATTERNS are strings\n\
  -G, --basic-regexp        PATTERNS are basic regular expressions\n\
  -P, --perl-regexp         PATTERNS are Perl regular expressions\n"));
  /* -X is deliberately undocumented.  */
      printf (_("\
  -e, --regexp=PATTERNS     use PATTERNS for matching\n\
  -f, --file=FILE           take PATTERNS from FILE\n\
  -i, --ignore-case         ignore case distinctions in patterns and data\n\
      --no-ignore-case      do not ignore case distinctions (default)\n\
  -w, --word-regexp         match only whole words\n\
  -x, --line-regexp         match only whole lines\n\
  -z, --null-data           a data line ends in 0 byte, not newline\n"));
      printf (_("\
\n\
Miscellaneous:\n\
  -s, --no-messages         suppress error messages\n\
  -v, --invert-match        select non-matching lines\n\
  -V, --version             display version information and exit\n\
      --help                display this help text and exit\n"));
      printf (_("\
\n\
Output control:\n\
  -m, --max-count=NUM       stop after NUM selected lines\n\
  -b, --byte-offset         print the byte offset with output lines\n\
  -n, --line-number         print line number with output lines\n\
      --line-buffered       flush output on every line\n\
  -H, --with-filename       print file name with output lines\n\
  -h, --no-filename         suppress the file name prefix on output\n\
      --label=LABEL         use LABEL as the standard input file name prefix\n\
"));
      printf (_("\
  -o, --only-matching       show only nonempty parts of lines that match\n\
  -q, --quiet, --silent     suppress all normal output\n\
      --binary-files=TYPE   assume that binary files are TYPE;\n\
                            TYPE is 'binary', 'text', or 'without-match'\n\
  -a, --text                equivalent to --binary-files=text\n\
"));
      printf (_("\
  -I                        equivalent to --binary-files=without-match\n\
  -d, --directories=ACTION  how to handle directories;\n\
                            ACTION is 'read', 'recurse', or 'skip'\n\
  -D, --devices=ACTION      how to handle devices, FIFOs and sockets;\n\
                            ACTION is 'read' or 'skip'\n\
  -r, --recursive           like --directories=recurse\n\
  -R, --dereference-recursive  likewise, but follow all symlinks\n\
"));
      printf (_("\
      --include=GLOB        search only files that match GLOB (a file pattern)"
                "\n\
      --exclude=GLOB        skip files that match GLOB\n\
      --exclude-from=FILE   skip files that match any file pattern from FILE\n\
      --exclude-dir=GLOB    skip directories that match GLOB\n\
"));
      printf (_("\
  -L, --files-without-match  print only names of FILEs with no selected lines\n\
  -l, --files-with-matches  print only names of FILEs with selected lines\n\
  -c, --count               print only a count of selected lines per FILE\n\
  -T, --initial-tab         make tabs line up (if needed)\n\
  -Z, --null                print 0 byte after FILE name\n"));
      printf (_("\
\n\
Context control:\n\
  -B, --before-context=NUM  print NUM lines of leading context\n\
  -A, --after-context=NUM   print NUM lines of trailing context\n\
  -C, --context=NUM         print NUM lines of output context\n\
"));
      printf (_("\
  -NUM                      same as --context=NUM\n\
      --group-separator=SEP  print SEP on line between matches with context\n\
      --no-group-separator  do not print separator for matches with context\n\
      --color[=WHEN],\n\
      --colour[=WHEN]       use markers to highlight the matching strings;\n\
                            WHEN is 'always', 'never', or 'auto'\n\
  -U, --binary              do not strip CR characters at EOL (MSDOS/Windows)\n\
\n"));
      printf (_("\
When FILE is '-', read standard input.  If no FILE is given, read standard\n\
input, but with -r, recursively search the working directory instead.  With\n\
fewer than two FILEs, assume -h.  Exit status is 0 if any line is selected,\n\
1 otherwise; if any error occurs and -q is not given, the exit status is 2.\n"
                ));
      emit_bug_reporting_address ();
    }
  exit (status);
}

/* Pattern compilers and matchers.  */

static struct
{
  char name[12];
  int syntax; /* used if compile == GEAcompile */
  compile_fp_t compile;
  execute_fp_t execute;
} const matchers[] = {
  { "grep", RE_SYNTAX_GREP, GEAcompile, EGexecute },
  { "egrep", RE_SYNTAX_EGREP, GEAcompile, EGexecute },
  { "fgrep", 0, Fcompile, Fexecute, },
  { "awk", RE_SYNTAX_AWK, GEAcompile, EGexecute },
  { "gawk", RE_SYNTAX_GNU_AWK, GEAcompile, EGexecute },
  { "posixawk", RE_SYNTAX_POSIX_AWK, GEAcompile, EGexecute },
#if HAVE_LIBPCRE
  { "perl", 0, Pcompile, Pexecute, },
#endif
};
/* Keep these in sync with the 'matchers' table.  */
enum { E_MATCHER_INDEX = 1, F_MATCHER_INDEX = 2, G_MATCHER_INDEX = 0 };

/* Return the index of the matcher corresponding to M if available.
   MATCHER is the index of the previous matcher, or -1 if none.
   Exit in case of conflicts or if M is not available.  */
static int
setmatcher (char const *m, int matcher)
{
  for (int i = 0; i < sizeof matchers / sizeof *matchers; i++)
    if (STREQ (m, matchers[i].name))
      {
        if (0 <= matcher && matcher != i)
          die (EXIT_TROUBLE, 0, _("conflicting matchers specified"));
        return i;
      }

#if !HAVE_LIBPCRE
  if (STREQ (m, "perl"))
    die (EXIT_TROUBLE, 0,
         _("Perl matching not supported in a --disable-perl-regexp build"));
#endif
  die (EXIT_TROUBLE, 0, _("invalid matcher %s"), m);
}

/* Get the next non-digit option from ARGC and ARGV.
   Return -1 if there are no more options.
   Process any digit options that were encountered on the way,
   and store the resulting integer into *DEFAULT_CONTEXT.  */
static int
get_nondigit_option (int argc, char *const *argv, intmax_t *default_context)
{
  static int prev_digit_optind = -1;
  int this_digit_optind;
  bool was_digit;
  char buf[INT_BUFSIZE_BOUND (intmax_t) + 4];
  char *p = buf;
  int opt;

  was_digit = false;
  this_digit_optind = optind;
  while (true)
    {
      opt = getopt_long (argc, (char **) argv, short_options,
                         long_options, nullptr);
      if (! c_isdigit (opt))
        break;

      if (prev_digit_optind != this_digit_optind || !was_digit)
        {
          /* Reset to start another context length argument.  */
          p = buf;
        }
      else
        {
          /* Suppress trivial leading zeros, to avoid incorrect
             diagnostic on strings like 00000000000.  */
          p -= buf[0] == '0';
        }

      if (p == buf + sizeof buf - 4)
        {
          /* Too many digits.  Append "..." to make context_length_arg
             complain about "X...", where X contains the digits seen
             so far.  */
          strcpy (p, "...");
          p += 3;
          break;
        }
      *p++ = opt;

      was_digit = true;
      prev_digit_optind = this_digit_optind;
      this_digit_optind = optind;
    }
  if (p != buf)
    {
      *p = '\0';
      context_length_arg (buf, default_context);
    }

  return opt;
}

/* Parse GREP_COLORS.  The default would look like:
     GREP_COLORS='ms=01;31:mc=01;31:sl=:cx=:fn=35:ln=32:bn=32:se=36'
   with boolean capabilities (ne and rv) unset (i.e., omitted).
   No character escaping is needed or supported.  */
static void
parse_grep_colors (void)
{
  const char *p;
  char *q;
  char *name;
  char *val;

  p = getenv ("GREP_COLORS"); /* Plural! */
  if (!p || *p == '\0')
    return;

  /* Work off a writable copy.  */
  q = xstrdup (p);

  name = q;
  val = nullptr;
  /* From now on, be well-formed or you're gone.  */
  for (;;)
    if (*q == ':' || *q == '\0')
      {
        char c = *q;
        struct color_cap const *cap;

        *q++ = '\0'; /* Terminate name or val.  */
        /* Empty name without val (empty cap)
         * won't match and will be ignored.  */
        for (cap = color_dict; cap->name; cap++)
          if (STREQ (cap->name, name))
            break;
        /* If name unknown, go on for forward compatibility.  */
        if (cap->var && val)
          *(cap->var) = val;
        if (cap->fct)
          cap->fct ();
        if (c == '\0')
          return;
        name = q;
        val = nullptr;
      }
    else if (*q == '=')
      {
        if (q == name || val)
          return;
        *q++ = '\0'; /* Terminate name.  */
        val = q; /* Can be the empty string.  */
      }
    else if (!val)
      q++; /* Accumulate name.  */
    else if (*q == ';' || c_isdigit (*q))
      q++; /* Accumulate val.  Protect the terminal from being sent crap.  */
    else
      return;
}

/* Return true if PAT (of length PATLEN) contains an encoding error.  */
static bool
contains_encoding_error (char const *pat, idx_t patlen)
{
  mbstate_t mbs; mbszero (&mbs);
  ptrdiff_t charlen;

  for (idx_t i = 0; i < patlen; i += charlen)
    {
      charlen = mb_clen (pat + i, patlen - i, &mbs);
      if (charlen < 0)
        return true;
    }
  return false;
}

/* When ignoring case and (-E or -F or -G), then for each single-byte
   character I, ok_fold[I] is 1 if every case folded counterpart of I
   is also single-byte, and is -1 otherwise.  */
static signed char ok_fold[NCHAR];
static void
setup_ok_fold (void)
{
  for (int i = 0; i < NCHAR; i++)
    {
      wint_t wi = localeinfo.sbctowc[i];
      if (wi == WEOF)
        continue;

      int ok = 1;
      char32_t folded[CASE_FOLDED_BUFSIZE];
      for (int n = case_folded_counterparts (wi, folded); 0 <= --n; )
        {
          char buf[MB_LEN_MAX];
          mbstate_t s; mbszero (&s);
          if (c32rtomb (buf, folded[n], &s) != 1)
            {
              ok = -1;
              break;
            }
        }
      ok_fold[i] = ok;
    }
}

/* Return the number of bytes in the initial character of PAT, of size
   PATLEN, if Fcompile can handle that character.  Return -1 if
   Fcompile cannot handle it.  MBS is the multibyte conversion state.
   PATLEN must be nonzero.  */

static ptrdiff_t
fgrep_icase_charlen (char const *pat, idx_t patlen, mbstate_t *mbs)
{
  unsigned char pat0 = pat[0];

  /* If PAT starts with a single-byte character, Fcompile works if
     every case folded counterpart is also single-byte.  */
  if (localeinfo.sbctowc[pat0] != WEOF)
    return ok_fold[pat0];

  char32_t wc;
  size_t wn = mbrtoc32 (&wc, pat, patlen, mbs);

  /* If PAT starts with an encoding error, Fcompile does not work.  */
  if (MB_LEN_MAX < wn)
    return -1;

  /* PAT starts with a multibyte character.  Fcompile works if the
     character has no case folded counterparts and toupper translates
     none of its encoding's bytes.  */
  char32_t folded[CASE_FOLDED_BUFSIZE];
  if (case_folded_counterparts (wc, folded))
    return -1;
  for (idx_t i = wn; 0 < --i; )
    {
      unsigned char c = pat[i];
      if (toupper (c) != c)
        return -1;
    }
  return wn;
}

/* Return true if the -F patterns PAT, of size PATLEN, contain only
   single-byte characters that case-fold only to single-byte
   characters, or multibyte characters not subject to case folding,
   and so can be processed by Fcompile.  */

static bool
fgrep_icase_available (char const *pat, idx_t patlen)
{
  mbstate_t mbs; mbszero (&mbs);

  for (idx_t i = 0; i < patlen; )
    {
      int n = fgrep_icase_charlen (pat + i, patlen - i, &mbs);
      if (n < 0)
        return false;
      i += n;
    }

  return true;
}

/* Change the pattern *KEYS_P, of size *LEN_P, from fgrep to grep style.  */

void
fgrep_to_grep_pattern (char **keys_p, idx_t *len_p)
{
  idx_t len = *len_p;
  char *keys = *keys_p;
  mbstate_t mb_state; mbszero (&mb_state);
  char *new_keys = xnmalloc (len + 1, 2);
  char *p = new_keys;

  for (ptrdiff_t n; len; keys += n, len -= n)
    {
      n = mb_clen (keys, len, &mb_state);
      switch (n)
        {
        case -2:
          n = len;
          FALLTHROUGH;
        default:
          p = mempcpy (p, keys, n);
          break;

        case -1:
          memset (&mb_state, 0, sizeof mb_state);
          n = 1;
          FALLTHROUGH;
        case 1:
          switch (*keys)
            {
            case '$': case '*': case '.': case '[': case '\\': case '^':
              *p++ = '\\'; break;
            }
          *p++ = *keys;
          break;
        }
    }

  *p = '\n';
  free (*keys_p);
  *keys_p = new_keys;
  *len_p = p - new_keys;
}

/* If it is easy, convert the MATCHER-style patterns KEYS (of size
   *LEN_P) to -F style, update *LEN_P to a possibly-smaller value, and
   return F_MATCHER_INDEX.  If not, leave KEYS and *LEN_P alone and
   return MATCHER.  This function is conservative and sometimes misses
   conversions, e.g., it does not convert the -E pattern "(a|a|[aa])"
   to the -F pattern "a".  */

static int
try_fgrep_pattern (int matcher, char *keys, idx_t *len_p)
{
  int result = matcher;
  idx_t len = *len_p;
  char *new_keys = ximalloc (len + 1);
  char *p = new_keys;
  char const *q = keys;
  mbstate_t mb_state; mbszero (&mb_state);

  while (len != 0)
    {
      switch (*q)
        {
        case '$': case '*': case '.': case '[': case '^':
          goto fail;

        case '(': case '+': case '?': case '{': case '|':
          /* There is no "case ')'" here, as "grep -E ')'" acts like
             "grep -E '\)'".  */
          if (matcher != G_MATCHER_INDEX)
            goto fail;
          break;

        case '\\':
          if (1 < len)
            switch (q[1])
              {
              case '\n':
              case 'B': case 'S': case 'W': case'\'': case '<':
              case 'b': case 's': case 'w': case '`': case '>':
              case '1': case '2': case '3': case '4':
              case '5': case '6': case '7': case '8': case '9':
                goto fail;

              case '(': case '+': case '?': case '{': case '|':
                /* Pass '\)' to GEAcompile so it can complain.  Otherwise,
                   "grep '\)'" would act like "grep ')'" while "grep '.*\)'
                   would be an error.  */
              case ')':
                if (matcher == G_MATCHER_INDEX)
                  goto fail;
                FALLTHROUGH;
              default:
                q++, len--;
                break;
              }
          break;
        }

      ptrdiff_t clen = (match_icase
                        ? fgrep_icase_charlen (q, len, &mb_state)
                        : mb_clen (q, len, &mb_state));
      if (clen < 0)
        goto fail;
      p = mempcpy (p, q, clen);
      q += clen;
      len -= clen;
    }

  if (*len_p != p - new_keys)
    {
      *len_p = p - new_keys;
      char *keys_end = mempcpy (keys, new_keys, p - new_keys);
      *keys_end = '\n';
    }
  result = F_MATCHER_INDEX;

 fail:
  free (new_keys);
  return result;
}

int
main (int argc, char **argv)
{
  char *keys = nullptr;
  idx_t keycc = 0, keyalloc = 0;
  int matcher = -1;
  int opt;
  int prev_optind, last_recursive;
  intmax_t default_context;
  FILE *fp;
  exit_failure = EXIT_TROUBLE;
  initialize_main (&argc, &argv);

  /* Which command-line options have been specified for filename output.
     -1 for -h, 1 for -H, 0 for neither.  */
  int filename_option = 0;

  eolbyte = '\n';
  filename_mask = ~0;

  max_count = INTMAX_MAX;

  /* The value -1 means to use DEFAULT_CONTEXT. */
  out_after = out_before = -1;
  /* Default before/after context: changed by -C/-NUM options */
  default_context = -1;
  /* Changed by -o option */
  only_matching = false;

  /* Internationalization. */
#if defined HAVE_SETLOCALE
  setlocale (LC_ALL, "");
#endif
#if defined ENABLE_NLS
  bindtextdomain (PACKAGE, LOCALEDIR);
  bindtextdomain ("gnulib", GNULIB_LOCALEDIR);
  textdomain (PACKAGE);
#endif

  init_localeinfo (&localeinfo);

  atexit (clean_up_stdout);
  c_stack_action (nullptr);

  last_recursive = 0;

  pattern_table = hash_initialize (0, 0, hash_pattern, compare_patterns, 0);
  if (!pattern_table)
    xalloc_die ();

  while (prev_optind = optind,
         (opt = get_nondigit_option (argc, argv, &default_context)) != -1)
    switch (opt)
      {
      case 'A':
        context_length_arg (optarg, &out_after);
        break;

      case 'B':
        context_length_arg (optarg, &out_before);
        break;

      case 'C':
        /* Set output match context, but let any explicit leading or
           trailing amount specified with -A or -B stand. */
        context_length_arg (optarg, &default_context);
        break;

      case 'D':
        if (STREQ (optarg, "read"))
          devices = READ_DEVICES;
        else if (STREQ (optarg, "skip"))
          devices = SKIP_DEVICES;
        else
          die (EXIT_TROUBLE, 0, _("unknown devices method"));
        break;

      case 'E':
        matcher = setmatcher ("egrep", matcher);
        break;

      case 'F':
        matcher = setmatcher ("fgrep", matcher);
        break;

      case 'P':
        matcher = setmatcher ("perl", matcher);
        break;

      case 'G':
        matcher = setmatcher ("grep", matcher);
        break;

      case 'X': /* undocumented on purpose */
        matcher = setmatcher (optarg, matcher);
        break;

      case 'H':
        filename_option = 1;
        break;

      case 'I':
        binary_files = WITHOUT_MATCH_BINARY_FILES;
        break;

      case 'T':
        align_tabs = true;
        break;

      case 'U':
        if (O_BINARY)
          binary = true;
        break;

      case 'V':
        show_version = true;
        break;

      case 'a':
        binary_files = TEXT_BINARY_FILES;
        break;

      case 'b':
        out_byte = true;
        break;

      case 'c':
        count_matches = true;
        break;

      case 'd':
        directories = XARGMATCH ("--directories", optarg,
                                 directories_args, directories_types);
        if (directories == RECURSE_DIRECTORIES)
          last_recursive = prev_optind;
        break;

      case 'e':
        {
          idx_t cc = strlen (optarg);
          ptrdiff_t shortage = keycc - keyalloc + cc + 1;
          if (0 < shortage)
            pattern_array = keys = xpalloc (keys, &keyalloc, shortage, -1, 1);
          char *keyend = mempcpy (keys + keycc, optarg, cc);
          *keyend = '\n';
          keycc = update_patterns (keys, keycc, keycc + cc + 1, "");
        }
        break;

      case 'f':
        {
          if (STREQ (optarg, "-"))
            {
              if (binary)
                xset_binary_mode (STDIN_FILENO, O_BINARY);
              fp = stdin;
            }
          else
            {
              fp = fopen (optarg, binary ? "rb" : "r");
              if (!fp)
                die (EXIT_TROUBLE, errno, "%s", optarg);
            }
          idx_t newkeycc = keycc, cc;
          for (;; newkeycc += cc)
            {
              ptrdiff_t shortage = newkeycc - keyalloc + 2;
              if (0 < shortage)
                pattern_array = keys = xpalloc (keys, &keyalloc,
                                                shortage, -1, 1);
              cc = fread (keys + newkeycc, 1, keyalloc - (newkeycc + 1), fp);
              if (cc == 0)
                break;
            }
          int err = errno;
          if (!ferror (fp))
            {
              err = 0;
              if (fp == stdin)
                clearerr (fp);
              else if (fclose (fp) != 0)
                err = errno;
            }
          if (err)
            die (EXIT_TROUBLE, err, "%s", optarg);
          /* Append final newline if file ended in non-newline. */
          if (newkeycc != keycc && keys[newkeycc - 1] != '\n')
            keys[newkeycc++] = '\n';
          keycc = update_patterns (keys, keycc, newkeycc, optarg);
        }
        break;

      case 'h':
        filename_option = -1;
        break;

      case 'i':
      case 'y':			/* For old-timers . . . */
        match_icase = true;
        break;

      case NO_IGNORE_CASE_OPTION:
        match_icase = false;
        break;

      case 'L':
        /* Like -l, except list files that don't contain matches.
           Inspired by the same option in Hume's gre. */
        list_files = LISTFILES_NONMATCHING;
        break;

      case 'l':
        list_files = LISTFILES_MATCHING;
        break;

      case 'm':
        switch (xstrtoimax (optarg, 0, 10, &max_count, ""))
          {
          case LONGINT_OK:
          case LONGINT_OVERFLOW:
            break;

          default:
            die (EXIT_TROUBLE, 0, _("invalid max count"));
          }
        break;

      case 'n':
        out_line = true;
        break;

      case 'o':
        only_matching = true;
        break;

      case 'q':
        exit_on_match = true;
        break;

      case 'R':
        fts_options = basic_fts_options | FTS_LOGICAL;
        FALLTHROUGH;
      case 'r':
        directories = RECURSE_DIRECTORIES;
        last_recursive = prev_optind;
        break;

      case 's':
        suppress_errors = true;
        break;

      case 'v':
        out_invert = true;
        break;

      case 'w':
        wordinit ();
        match_words = true;
        break;

      case 'x':
        match_lines = true;
        break;

      case 'Z':
        filename_mask = 0;
        break;

      case 'z':
        eolbyte = '\0';
        break;

      case BINARY_FILES_OPTION:
        if (STREQ (optarg, "binary"))
          binary_files = BINARY_BINARY_FILES;
        else if (STREQ (optarg, "text"))
          binary_files = TEXT_BINARY_FILES;
        else if (STREQ (optarg, "without-match"))
          binary_files = WITHOUT_MATCH_BINARY_FILES;
        else
          die (EXIT_TROUBLE, 0, _("unknown binary-files type"));
        break;

      case COLOR_OPTION:
        if (optarg)
          {
            if (!c_strcasecmp (optarg, "always")
                || !c_strcasecmp (optarg, "yes")
                || !c_strcasecmp (optarg, "force"))
              color_option = 1;
            else if (!c_strcasecmp (optarg, "never")
                     || !c_strcasecmp (optarg, "no")
                     || !c_strcasecmp (optarg, "none"))
              color_option = 0;
            else if (!c_strcasecmp (optarg, "auto")
                     || !c_strcasecmp (optarg, "tty")
                     || !c_strcasecmp (optarg, "if-tty"))
              color_option = 2;
            else
              show_help = 1;
          }
        else
          color_option = 2;
        break;

      case EXCLUDE_OPTION:
      case INCLUDE_OPTION:
        for (int cmd = 0; cmd < 2; cmd++)
          {
            if (!excluded_patterns[cmd])
              excluded_patterns[cmd] = new_exclude ();
            add_exclude (excluded_patterns[cmd], optarg,
                         ((opt == INCLUDE_OPTION ? EXCLUDE_INCLUDE : 0)
                          | exclude_options (cmd)));
          }
        break;
      case EXCLUDE_FROM_OPTION:
        for (int cmd = 0; cmd < 2; cmd++)
          {
            if (!excluded_patterns[cmd])
              excluded_patterns[cmd] = new_exclude ();
            if (add_exclude_file (add_exclude, excluded_patterns[cmd],
                                  optarg, exclude_options (cmd), '\n')
                != 0)
              die (EXIT_TROUBLE, errno, "%s", optarg);
          }
        break;

      case EXCLUDE_DIRECTORY_OPTION:
        strip_trailing_slashes (optarg);
        for (int cmd = 0; cmd < 2; cmd++)
          {
            if (!excluded_directory_patterns[cmd])
              excluded_directory_patterns[cmd] = new_exclude ();
            add_exclude (excluded_directory_patterns[cmd], optarg,
                         exclude_options (cmd));
          }
        break;

      case GROUP_SEPARATOR_OPTION:
        group_separator = optarg;
        break;

      case LINE_BUFFERED_OPTION:
        line_buffered = true;
        break;

      case LABEL_OPTION:
        label = optarg;
        break;

      case 0:
        /* long options */
        break;

      default:
        usage (EXIT_TROUBLE);
        break;

      }

  if (show_version)
    {
      version_etc (stdout, getprogname (), PACKAGE_NAME, VERSION,
                   (char *) nullptr);
      puts (_("Written by Mike Haertel and others; see\n"
              "<https://git.savannah.gnu.org/cgit/grep.git/tree/AUTHORS>."));
#if HAVE_LIBPCRE
      Pprint_version ();
#endif
      return EXIT_SUCCESS;
    }

  if (show_help)
    usage (EXIT_SUCCESS);

  if (keys)
    {
      if (keycc == 0)
        {
          /* No keys were specified (e.g. -f /dev/null).  Match nothing.  */
          out_invert ^= true;
          match_lines = match_words = false;
          keys[keycc++] = '\n';
        }
    }
  else if (optind < argc)
    {
      /* If a command-line regular expression operand starts with '\-',
         skip the '\'.  This suppresses a stray-backslash warning if a
         script uses the non-POSIX "grep '\-x'" to avoid treating
         '-x' as an option.  */
      char const *pat = argv[optind++];
      bool skip_bs = (matcher != F_MATCHER_INDEX
                      && pat[0] == '\\' && pat[1] == '-');

      /* Make a copy so that it can be reallocated or freed later.  */
      pattern_array = keys = xstrdup (pat + skip_bs);
      idx_t patlen = strlen (keys);
      keys[patlen] = '\n';
      keycc = update_patterns (keys, 0, patlen + 1, "");
    }
  else
    usage (EXIT_TROUBLE);

  /* Strip trailing newline from keys.  */
  keycc--;

  hash_free (pattern_table);

  bool possibly_tty = false;
  struct stat tmp_stat;
  if (! exit_on_match && fstat (STDOUT_FILENO, &tmp_stat) == 0)
    {
      if (S_ISREG (tmp_stat.st_mode))
        out_stat = tmp_stat;
      else if (S_ISCHR (tmp_stat.st_mode))
        {
          struct stat null_stat;
          if (stat ("/dev/null", &null_stat) == 0
              && SAME_INODE (tmp_stat, null_stat))
            dev_null_output = true;
          else
            possibly_tty = true;
        }
    }

  /* POSIX says -c, -l and -q are mutually exclusive.  In this
     implementation, -q overrides -l and -L, which in turn override -c.  */
  if (exit_on_match | dev_null_output)
    list_files = LISTFILES_NONE;
  if ((exit_on_match | dev_null_output) || list_files != LISTFILES_NONE)
    {
      count_matches = false;
      if (max_count == INTMAX_MAX)
        done_on_match = true;
    }
  out_quiet = count_matches | done_on_match | exit_on_match;

  if (out_after < 0)
    out_after = default_context;
  if (out_before < 0)
    out_before = default_context;

  /* If it is easy to see that matching cannot succeed (e.g., 'grep -f
     /dev/null'), fail without reading the input.  */
  if ((max_count == 0
       || (keycc == 0 && out_invert && !match_lines && !match_words))
      && list_files != LISTFILES_NONMATCHING)
    return EXIT_FAILURE;

  if (color_option == 2)
    color_option = possibly_tty && should_colorize () && isatty (STDOUT_FILENO);
  init_colorize ();

  if (color_option)
    {
      /* Legacy.  */
      char *userval = getenv ("GREP_COLOR");
      if (userval && *userval)
        for (char *q = userval; *q == ';' || c_isdigit (*q); q++)
          if (!q[1])
            {
              selected_match_color = context_match_color = userval;
              break;
            }

      /* New GREP_COLORS has priority.  */
      parse_grep_colors ();

      /* Warn if GREP_COLOR has an effect, since it's deprecated.  */
      if (selected_match_color == userval || context_match_color == userval)
        error (0, 0, _("warning: GREP_COLOR='%s' is deprecated;"
                       " use GREP_COLORS='mt=%s'"),
               userval, userval);
    }

  initialize_unibyte_mask ();

  if (matcher < 0)
    matcher = G_MATCHER_INDEX;

  if (matcher == F_MATCHER_INDEX
      || matcher == E_MATCHER_INDEX || matcher == G_MATCHER_INDEX)
    {
      if (match_icase)
        setup_ok_fold ();

      /* In a single-byte locale, switch from -F to -G if it is a single
         pattern that matches words, where -G is typically faster.  In a
         multibyte locale, switch if the patterns have an encoding error
         (where -F does not work) or if -i and the patterns will not work
         for -iF.  */
      if (matcher == F_MATCHER_INDEX)
        {
          if (! localeinfo.multibyte
              ? n_patterns == 1 && match_words
              : (contains_encoding_error (keys, keycc)
                 || (match_icase && !fgrep_icase_available (keys, keycc))))
            {
              fgrep_to_grep_pattern (&pattern_array, &keycc);
              keys = pattern_array;
              matcher = G_MATCHER_INDEX;
            }
        }
      /* With two or more patterns, if -F works then switch from either -E
         or -G, as -F is probably faster then.  */
      else if (1 < n_patterns)
        matcher = try_fgrep_pattern (matcher, keys, &keycc);
    }

  execute = matchers[matcher].execute;
  compiled_pattern =
    matchers[matcher].compile (keys, keycc, matchers[matcher].syntax,
                               only_matching | color_option);
  /* We need one byte prior and one after.  */
  char eolbytes[3] = { 0, eolbyte, 0 };
  idx_t match_size;
  skip_empty_lines = (!execute (compiled_pattern, eolbytes + 1, 1,
                                &match_size, nullptr)
                      == out_invert);

  int num_operands = argc - optind;
  out_file = (filename_option == 0 && num_operands <= 1
              ? - (directories == RECURSE_DIRECTORIES)
              : 0 <= filename_option);

  if (binary)
    xset_binary_mode (STDOUT_FILENO, O_BINARY);

  /* Prefer sysconf for page size, as getpagesize typically returns int.  */
#ifdef _SC_PAGESIZE
  long psize = sysconf (_SC_PAGESIZE);
#else
  long psize = getpagesize ();
#endif
  if (! (0 < psize && psize <= (IDX_MAX - uword_size) / 2))
    abort ();
  pagesize = psize;
  good_readsize = ALIGN_TO (GOOD_READSIZE_MIN, pagesize);
  bufalloc = good_readsize + pagesize + uword_size;
  buffer = ximalloc (bufalloc);

  if (fts_options & FTS_LOGICAL && devices == READ_COMMAND_LINE_DEVICES)
    devices = READ_DEVICES;

  char *const *files;
  if (0 < num_operands)
    {
      files = argv + optind;
    }
  else if (directories == RECURSE_DIRECTORIES && 0 < last_recursive)
    {
      static char *const cwd_only[] = { (char *) ".", nullptr };
      files = cwd_only;
      omit_dot_slash = true;
    }
  else
    {
      static char *const stdin_only[] = { (char *) "-", nullptr };
      files = stdin_only;
    }

  bool status = true;
  do
    status &= grep_command_line_arg (*files++);
  while (*files);

  return errseen ? EXIT_TROUBLE : status;
}
