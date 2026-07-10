/* dfa.c - deterministic extended regexp routines for GNU
   Copyright (C) 1988, 1998, 2000, 2002, 2004-2005, 2007-2025 Free Software
   Foundation, Inc.

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

/* Written June, 1988 by Mike Haertel
   Modified July, 1988 by Arthur David Olson to assist BMG speedups  */

#include <config.h>

#include "dfa.h"

#include "flexmember.h"
#include "idx.h"
#include "verify.h"

#include <assert.h>
#include <ctype.h>
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <limits.h>
#include <string.h>
#include <wchar.h>

#include "xalloc.h"
#include "localeinfo.h"

#include "gettext.h"
#define _(msgid) dgettext ("gnulib", msgid)

#if GAWK
/* Use ISO C 99 API.  */
# include <wctype.h>
# define char32_t wchar_t
# define mbrtoc32 mbrtowc
# define c32rtomb wcrtomb
# define c32tob wctob
# define c32isprint iswprint
# define c32isspace iswspace
# define mbszero(p) memset ((p), 0, sizeof (mbstate_t))
#else
/* Use ISO C 11 + gnulib API.  */
# include <uchar.h>
#endif

/* Pacify gcc -Wanalyzer-null-dereference in areas where GCC
   understandably cannot deduce that the input comes from a
   well-formed regular expression.  There's little point to the
   runtime overhead of 'assert' instead of 'assume_nonnull' when the
   MMU will check anyway.  */
#define assume_nonnull(x) assume ((x) != NULL)

static bool
str_eq (char const *a, char const *b)
{
  return strcmp (a, b) == 0;
}

static bool
c_isdigit (char c)
{
  return '0' <= c && c <= '9';
}

#ifndef FALLTHROUGH
# if 201710L < __STDC_VERSION__
#  define FALLTHROUGH [[__fallthrough__]]
# elif ((__GNUC__ >= 7 && !defined __clang__) \
        || (defined __apple_build_version__ \
            ? __apple_build_version__ >= 12000000 \
            : __clang_major__ >= 10))
#  define FALLTHROUGH __attribute__ ((__fallthrough__))
# else
#  define FALLTHROUGH ((void) 0)
# endif
#endif

#ifndef MIN
# define MIN(a,b) ((a) < (b) ? (a) : (b))
#endif

/* HPUX defines these as macros in sys/param.h.  */
#ifdef setbit
# undef setbit
#endif
#ifdef clrbit
# undef clrbit
#endif

/* For code that does not use Gnulib’s isblank module.  */
#if !defined isblank && !defined HAVE_ISBLANK && !defined GNULIB_ISBLANK
# define isblank dfa_isblank
static int
isblank (int c)
{
  return c == ' ' || c == '\t';
}
#endif

/* First integer value that is greater than any character code.  */
enum { NOTCHAR = 1 << CHAR_BIT };

#ifdef UINT_LEAST64_MAX

/* Number of bits used in a charclass word.  */
enum { CHARCLASS_WORD_BITS = 64 };

/* This represents part of a character class.  It must be unsigned and
   at least CHARCLASS_WORD_BITS wide.  Any excess bits are zero.  */
typedef uint_least64_t charclass_word;

/* Part of a charclass initializer that represents 64 bits' worth of a
   charclass, where LO and HI are the low and high-order 32 bits of
   the 64-bit quantity.  */
# define CHARCLASS_PAIR(lo, hi) (((charclass_word) (hi) << 32) + (lo))

#else
/* Fallbacks for pre-C99 hosts that lack 64-bit integers.  */
enum { CHARCLASS_WORD_BITS = 32 };
typedef unsigned long charclass_word;
# define CHARCLASS_PAIR(lo, hi) lo, hi
#endif

/* An initializer for a charclass whose 32-bit words are A through H.  */
#define CHARCLASS_INIT(a, b, c, d, e, f, g, h)		\
   {{							\
      CHARCLASS_PAIR (a, b), CHARCLASS_PAIR (c, d),	\
      CHARCLASS_PAIR (e, f), CHARCLASS_PAIR (g, h)	\
   }}

/* The maximum useful value of a charclass_word; all used bits are 1.  */
static charclass_word const CHARCLASS_WORD_MASK
  = ((charclass_word) 1 << (CHARCLASS_WORD_BITS - 1) << 1) - 1;

/* Number of words required to hold a bit for every character.  */
enum
{
  CHARCLASS_WORDS = (NOTCHAR + CHARCLASS_WORD_BITS - 1) / CHARCLASS_WORD_BITS
};

/* Sets of unsigned characters are stored as bit vectors in arrays of ints.  */
typedef struct { charclass_word w[CHARCLASS_WORDS]; } charclass;

/* Convert a possibly-signed character to an unsigned character.  This is
   a bit safer than casting to unsigned char, since it catches some type
   errors that the cast doesn't.  */
static unsigned char
to_uchar (char ch)
{
  return ch;
}

/* Contexts tell us whether a character is a newline or a word constituent.
   Word-constituent characters are those that satisfy iswalnum, plus '_'.
   Each character has a single CTX_* value; bitmasks of CTX_* values denote
   a particular character class.

   A state also stores a context value, which is a bitmask of CTX_* values.
   A state's context represents a set of characters that the state's
   predecessors must match.  For example, a state whose context does not
   include CTX_LETTER will never have transitions where the previous
   character is a word constituent.  A state whose context is CTX_ANY
   might have transitions from any character.  */

enum
  {
    CTX_NONE = 1,
    CTX_LETTER = 2,
    CTX_NEWLINE = 4,
    CTX_ANY = 7
  };

/* Sometimes characters can only be matched depending on the surrounding
   context.  Such context decisions depend on what the previous character
   was, and the value of the current (lookahead) character.  Context
   dependent constraints are encoded as 9-bit integers.  Each bit that
   is set indicates that the constraint succeeds in the corresponding
   context.

   bit 6-8  - valid contexts when next character is CTX_NEWLINE
   bit 3-5  - valid contexts when next character is CTX_LETTER
   bit 0-2  - valid contexts when next character is CTX_NONE

   succeeds_in_context determines whether a given constraint
   succeeds in a particular context.  Prev is a bitmask of possible
   context values for the previous character, curr is the (single-bit)
   context value for the lookahead character.  */
static int
newline_constraint (int constraint)
{
  return (constraint >> 6) & 7;
}
static int
letter_constraint (int constraint)
{
  return (constraint >> 3) & 7;
}
static int
other_constraint (int constraint)
{
  return constraint & 7;
}

static bool
succeeds_in_context (int constraint, int prev, int curr)
{
  return !! (((curr & CTX_NONE      ? other_constraint (constraint) : 0) \
              | (curr & CTX_LETTER  ? letter_constraint (constraint) : 0) \
              | (curr & CTX_NEWLINE ? newline_constraint (constraint) : 0)) \
             & prev);
}

/* The following describe what a constraint depends on.  */
static bool
prev_newline_dependent (int constraint)
{
  return ((constraint ^ constraint >> 2) & 0111) != 0;
}
static bool
prev_letter_dependent (int constraint)
{
  return ((constraint ^ constraint >> 1) & 0111) != 0;
}

/* Tokens that match the empty string subject to some constraint actually
   work by applying that constraint to determine what may follow them,
   taking into account what has gone before.  The following values are
   the constraints corresponding to the special tokens previously defined.  */
enum
  {
    NO_CONSTRAINT = 0777,
    BEGLINE_CONSTRAINT = 0444,
    ENDLINE_CONSTRAINT = 0700,
    BEGWORD_CONSTRAINT = 0050,
    ENDWORD_CONSTRAINT = 0202,
    LIMWORD_CONSTRAINT = 0252,
    NOTLIMWORD_CONSTRAINT = 0525
  };

/* The regexp is parsed into an array of tokens in postfix form.  Some tokens
   are operators and others are terminal symbols.  Most (but not all) of these
   codes are returned by the lexical analyzer.  */

typedef ptrdiff_t token;
static token const TOKEN_MAX = PTRDIFF_MAX;

/* States are indexed by state_num values.  These are normally
   nonnegative but -1 is used as a special value.  */
typedef ptrdiff_t state_num;

/* Predefined token values.  */
enum
{
  END = -1,                     /* END is a terminal symbol that matches the
                                   end of input; any value of END or less in
                                   the parse tree is such a symbol.  Accepting
                                   states of the DFA are those that would have
                                   a transition on END.  This is -1, not some
                                   more-negative value, to tweak the speed of
                                   comparisons to END.  */

  /* Ordinary character values are terminal symbols that match themselves.  */

  /* CSET must come last in the following list of special tokens.  Otherwise,
     the list order matters only for performance.  Related special tokens
     should have nearby values so that code like (t == ANYCHAR || t == MBCSET
     || CSET <= t) can be done with a single machine-level comparison.  */

  EMPTY = NOTCHAR,              /* EMPTY is a terminal symbol that matches
                                   the empty string.  */

  QMARK,                        /* QMARK is an operator of one argument that
                                   matches zero or one occurrences of its
                                   argument.  */

  STAR,                         /* STAR is an operator of one argument that
                                   matches the Kleene closure (zero or more
                                   occurrences) of its argument.  */

  PLUS,                         /* PLUS is an operator of one argument that
                                   matches the positive closure (one or more
                                   occurrences) of its argument.  */

  REPMN,                        /* REPMN is a lexical token corresponding
                                   to the {m,n} construct.  REPMN never
                                   appears in the compiled token vector.  */

  CAT,                          /* CAT is an operator of two arguments that
                                   matches the concatenation of its
                                   arguments.  CAT is never returned by the
                                   lexical analyzer.  */

  OR,                           /* OR is an operator of two arguments that
                                   matches either of its arguments.  */

  LPAREN,                       /* LPAREN never appears in the parse tree,
                                   it is only a lexeme.  */

  RPAREN,                       /* RPAREN never appears in the parse tree.  */

  WCHAR,                        /* Only returned by lex.  wctok contains the
                                   32-bit wide character representation.  */

  ANYCHAR,                      /* ANYCHAR is a terminal symbol that matches
                                   a valid multibyte (or single byte) character.
                                   It is used only if MB_CUR_MAX > 1.  */

  BEG,                          /* BEG is an initial symbol that matches the
                                   beginning of input.  */

  BEGLINE,                      /* BEGLINE is a terminal symbol that matches
                                   the empty string at the beginning of a
                                   line.  */

  ENDLINE,                      /* ENDLINE is a terminal symbol that matches
                                   the empty string at the end of a line.  */

  BEGWORD,                      /* BEGWORD is a terminal symbol that matches
                                   the empty string at the beginning of a
                                   word.  */

  ENDWORD,                      /* ENDWORD is a terminal symbol that matches
                                   the empty string at the end of a word.  */

  LIMWORD,                      /* LIMWORD is a terminal symbol that matches
                                   the empty string at the beginning or the
                                   end of a word.  */

  NOTLIMWORD,                   /* NOTLIMWORD is a terminal symbol that
                                   matches the empty string not at
                                   the beginning or end of a word.  */

  BACKREF,                      /* BACKREF is generated by \<digit>
                                   or by any other construct that
                                   is not completely handled.  If the scanner
                                   detects a transition on backref, it returns
                                   a kind of "semi-success" indicating that
                                   the match will have to be verified with
                                   a backtracking matcher.  */

  MBCSET,                       /* MBCSET is similar to CSET, but for
                                   multibyte characters.  */

  CSET                          /* CSET and (and any value greater) is a
                                   terminal symbol that matches any of a
                                   class of characters.  */
};


/* States of the recognizer correspond to sets of positions in the parse
   tree, together with the constraints under which they may be matched.
   So a position is encoded as an index into the parse tree together with
   a constraint.  */
typedef struct
{
  idx_t index;			/* Index into the parse array.  */
  unsigned int constraint;      /* Constraint for matching this position.  */
} position;

/* Sets of positions are stored as arrays.  */
typedef struct
{
  position *elems;              /* Elements of this position set.  */
  idx_t nelem;			/* Number of elements in this set.  */
  idx_t alloc;			/* Number of elements allocated in ELEMS.  */
} position_set;

/* A state of the dfa consists of a set of positions, some flags,
   and the token value of the lowest-numbered position of the state that
   contains an END token.  */
typedef struct
{
  size_t hash;                  /* Hash of the positions of this state.  */
  position_set elems;           /* Positions this state could match.  */
  unsigned char context;        /* Context from previous state.  */
  unsigned short constraint;    /* Constraint for this state to accept.  */
  position_set mbps;            /* Positions which can match multibyte
                                   characters or the follows, e.g., period.
                                   Used only if MB_CUR_MAX > 1.  */
  state_num mb_trindex;         /* Index of this state in MB_TRANS, or
                                   negative if the state does not have
                                   ANYCHAR.  */
} dfa_state;

/* Maximum for any transition table count.  This should be at least 3,
   for the initial state setup.  */
enum { MAX_TRCOUNT = 1024 };

/* A bracket operator.
   e.g., [a-c], [[:alpha:]], etc.  */
struct mb_char_classes
{
  ptrdiff_t cset;
  bool invert;
  char32_t *chars;              /* Normal characters.  */
  idx_t nchars;
  idx_t nchars_alloc;
};

struct regex_syntax
{
  /* Syntax bits controlling the behavior of the lexical analyzer.  */
  reg_syntax_t syntax_bits;
  int dfaopts;
  bool syntax_bits_set;

  /* Flag for case-folding letters into sets.  */
  bool case_fold;

  /* End-of-line byte in data.  */
  unsigned char eolbyte;

  /* Cache of char-context values.  */
  char sbit[NOTCHAR];

  /* If never_trail[B], the byte B cannot be a non-initial byte in a
     multibyte character.  */
  bool never_trail[NOTCHAR];

  /* Set of characters considered letters.  */
  charclass letters;

  /* Set of characters that are newline.  */
  charclass newline;
};

/* Lexical analyzer.  All the dross that deals with the obnoxious
   GNU Regex syntax bits is located here.  The poor, suffering
   reader is referred to the GNU Regex documentation for the
   meaning of the @#%!@#%^!@ syntax bits.  */
struct lexer_state
{
  char const *ptr;	/* Pointer to next input character.  */
  idx_t left;		/* Number of characters remaining.  */
  token lasttok;	/* Previous token returned; initially END.  */
  idx_t parens;		/* Count of outstanding left parens.  */
  int minrep, maxrep;	/* Repeat counts for {m,n}.  */

  /* 32-bit wide character representation of the current multibyte character,
     or WEOF if there was an encoding error.  Used only if
     MB_CUR_MAX > 1.  */
  wint_t wctok;

  /* The most recently analyzed multibyte bracket expression.  */
  struct mb_char_classes brack;

  /* We're separated from beginning or (, | only by zero-width characters.  */
  bool laststart;
};

/* Recursive descent parser for regular expressions.  */

struct parser_state
{
  token tok;               /* Lookahead token.  */
  idx_t depth;		   /* Current depth of a hypothetical stack
                              holding deferred productions.  This is
                              used to determine the depth that will be
                              required of the real stack later on in
                              dfaanalyze.  */
};

/* A compiled regular expression.  */
struct dfa
{
  /* Fields filled by the scanner.  */
  charclass *charclasses;       /* Array of character sets for CSET tokens.  */
  idx_t cindex;			/* Index for adding new charclasses.  */
  idx_t calloc;			/* Number of charclasses allocated.  */
  ptrdiff_t canychar;           /* Index of anychar class, or -1.  */

  /* Scanner state */
  struct lexer_state lex;

  /* Parser state */
  struct parser_state parse;

  /* Fields filled by the parser.  */
  token *tokens;                /* Postfix parse array.  */
  idx_t tindex;			/* Index for adding new tokens.  */
  idx_t talloc;			/* Number of tokens currently allocated.  */
  idx_t depth;			/* Depth required of an evaluation stack
                                   used for depth-first traversal of the
                                   parse tree.  */
  idx_t nleaves;		/* Number of non-EMPTY leaves
                                   in the parse tree.  */
  idx_t nregexps;		/* Count of parallel regexps being built
                                   with dfaparse.  */
  bool fast;			/* The DFA is fast.  */
  bool epsilon;			/* Does a token match only the empty string?  */
  token utf8_anychar_classes[9]; /* To lower ANYCHAR in UTF-8 locales.  */
  mbstate_t mbs;		/* Multibyte conversion state.  */

  /* The following are valid only if MB_CUR_MAX > 1.  */

  /* The value of multibyte_prop[i] is defined by following rule.
     if tokens[i] < NOTCHAR
     bit 0 : tokens[i] is the first byte of a character, including
     single-byte characters.
     bit 1 : tokens[i] is the last byte of a character, including
     single-byte characters.

     e.g.
     tokens
     = 'single_byte_a', 'multi_byte_A', single_byte_b'
     = 'sb_a', 'mb_A(1st byte)', 'mb_A(2nd byte)', 'mb_A(3rd byte)', 'sb_b'
     multibyte_prop
     = 3     , 1               ,  0              ,  2              , 3
   */
  char *multibyte_prop;

  /* Fields filled by the superset.  */
  struct dfa *superset;             /* Hint of the dfa.  */

  /* Fields filled by the state builder.  */
  dfa_state *states;            /* States of the dfa.  */
  state_num sindex;             /* Index for adding new states.  */
  idx_t salloc;			/* Number of states currently allocated.  */

  /* Fields filled by the parse tree->NFA conversion.  */
  position_set *follows;        /* Array of follow sets, indexed by position
                                   index.  The follow of a position is the set
                                   of positions containing characters that
                                   could conceivably follow a character
                                   matching the given position in a string
                                   matching the regexp.  Allocated to the
                                   maximum possible position index.  */
  bool searchflag;		/* We are supposed to build a searching
                                   as opposed to an exact matcher.  A searching
                                   matcher finds the first and shortest string
                                   matching a regexp anywhere in the buffer,
                                   whereas an exact matcher finds the longest
                                   string matching, but anchored to the
                                   beginning of the buffer.  */

  /* Fields filled by dfaanalyze.  */
  int *constraints;             /* Array of union of accepting constraints
                                   in the follow of a position.  */
  int *separates;               /* Array of contexts on follow of a
                                   position.  */

  /* Fields filled by dfaexec.  */
  state_num tralloc;            /* Number of transition tables that have
                                   slots so far, not counting trans[-1] and
                                   trans[-2].  */
  int trcount;                  /* Number of transition tables that have
                                   been built, other than for initial
                                   states.  */
  int min_trcount;              /* Number of initial states.  Equivalently,
                                   the minimum state number for which trcount
                                   counts transitions.  */
  state_num **trans;            /* Transition tables for states that can
                                   never accept.  If the transitions for a
                                   state have not yet been computed, or the
                                   state could possibly accept, its entry in
                                   this table is NULL.  This points to two
                                   past the start of the allocated array,
                                   and trans[-1] and trans[-2] are always
                                   NULL.  */
  state_num **fails;            /* Transition tables after failing to accept
                                   on a state that potentially could do so.
                                   If trans[i] is non-null, fails[i] must
                                   be null.  */
  char *success;                /* Table of acceptance conditions used in
                                   dfaexec and computed in build_state.  */
  state_num *newlines;          /* Transitions on newlines.  The entry for a
                                   newline in any transition table is always
                                   -1 so we can count lines without wasting
                                   too many cycles.  The transition for a
                                   newline is stored separately and handled
                                   as a special case.  Newline is also used
                                   as a sentinel at the end of the buffer.  */
  state_num initstate_notbol;   /* Initial state for CTX_LETTER and CTX_NONE
                                   context in multibyte locales, in which we
                                   do not distinguish between their contexts,
                                   as not supported word.  */
  position_set mb_follows;      /* Follow set added by ANYCHAR on demand.  */
  state_num **mb_trans;         /* Transition tables for states with
                                   ANYCHAR.  */
  state_num mb_trcount;         /* Number of transition tables for states with
                                   ANYCHAR that have actually been built.  */

  /* Syntax configuration.  This is near the end so that dfacopysyntax
     can memset up to here.  */
  struct regex_syntax syntax;

  /* Information derived from the locale.  This is at the end so that
     a quick memset need not clear it specially.  */

  /* dfaexec implementation.  */
  char *(*dfaexec) (struct dfa *, char const *, char *,
                    bool, idx_t *, bool *);

  /* Other cached information derived from the locale.  */
  struct localeinfo localeinfo;
};

/* User access to dfa internals.  */

/* S could possibly be an accepting state of R.  */
static bool
accepting (state_num s, struct dfa const *r)
{
  return r->states[s].constraint != 0;
}

/* STATE accepts in the specified context.  */
static bool
accepts_in_context (int prev, int curr, state_num state, struct dfa const *dfa)
{
  return succeeds_in_context (dfa->states[state].constraint, prev, curr);
}

static void regexp (struct dfa *dfa);

/* Store into *PWC the result of converting the leading bytes of the
   multibyte buffer S of length N bytes, using D->localeinfo.sbctowc
   and updating the conversion state in *D.  On conversion error,
   convert just a single byte, to WEOF.  Return the number of bytes
   converted.

   This differs from mbrtoc32 (PWC, S, N, &D->mbs) as follows:

   * PWC points to wint_t, not to char32_t.
   * The last arg is a dfa *D instead of merely a multibyte conversion
     state D->mbs.
   * N is idx_t not size_t, and must be at least 1.
   * S[N - 1] must be a sentinel byte.
   * Shift encodings are not supported.
   * The return value is always in the range 1..N.
   * D->mbs is always valid afterwards.
   * *PWC is always set to something.  */
static int
mbs_to_wchar (wint_t *pwc, char const *s, idx_t n, struct dfa *d)
{
  unsigned char uc = s[0];
  wint_t wc = d->localeinfo.sbctowc[uc];

  if (wc == WEOF)
    {
      char32_t wch;
      size_t nbytes = mbrtoc32 (&wch, s, n, &d->mbs);
      if (0 < nbytes && nbytes < (size_t) -2)
        {
          *pwc = wch;
          /* nbytes cannot be == (size) -3 here, since we rely on the
             'mbrtoc32-regular' module.  */
          return nbytes;
        }
      mbszero (&d->mbs);
    }

  *pwc = wc;
  return 1;
}

#ifdef DEBUG

static void
prtok (token t)
{
  if (t <= END)
    fprintf (stderr, "END");
  else if (0 <= t && t < NOTCHAR)
    {
      unsigned int ch = t;
      fprintf (stderr, "0x%02x", ch);
    }
  else
    {
      char const *s;
      switch (t)
        {
        case BEG:
          s = "BEG";
          break;
        case EMPTY:
          s = "EMPTY";
          break;
        case BACKREF:
          s = "BACKREF";
          break;
        case BEGLINE:
          s = "BEGLINE";
          break;
        case ENDLINE:
          s = "ENDLINE";
          break;
        case BEGWORD:
          s = "BEGWORD";
          break;
        case ENDWORD:
          s = "ENDWORD";
          break;
        case LIMWORD:
          s = "LIMWORD";
          break;
        case NOTLIMWORD:
          s = "NOTLIMWORD";
          break;
        case QMARK:
          s = "QMARK";
          break;
        case STAR:
          s = "STAR";
          break;
        case PLUS:
          s = "PLUS";
          break;
        case CAT:
          s = "CAT";
          break;
        case OR:
          s = "OR";
          break;
        case LPAREN:
          s = "LPAREN";
          break;
        case RPAREN:
          s = "RPAREN";
          break;
        case ANYCHAR:
          s = "ANYCHAR";
          break;
        case MBCSET:
          s = "MBCSET";
          break;
        default:
          s = "CSET";
          break;
        }
      fprintf (stderr, "%s", s);
    }
}
#endif /* DEBUG */

/* Stuff pertaining to charclasses.  */

static bool
tstbit (unsigned int b, charclass const *c)
{
  return c->w[b / CHARCLASS_WORD_BITS] >> b % CHARCLASS_WORD_BITS & 1;
}

static void
setbit (unsigned int b, charclass *c)
{
  charclass_word one = 1;
  c->w[b / CHARCLASS_WORD_BITS] |= one << b % CHARCLASS_WORD_BITS;
}

static void
clrbit (unsigned int b, charclass *c)
{
  charclass_word one = 1;
  c->w[b / CHARCLASS_WORD_BITS] &= ~(one << b % CHARCLASS_WORD_BITS);
}

static void
zeroset (charclass *s)
{
  memset (s, 0, sizeof *s);
}

static void
fillset (charclass *s)
{
  for (int i = 0; i < CHARCLASS_WORDS; i++)
    s->w[i] = CHARCLASS_WORD_MASK;
}

static void
notset (charclass *s)
{
  for (int i = 0; i < CHARCLASS_WORDS; ++i)
    s->w[i] = CHARCLASS_WORD_MASK & ~s->w[i];
}

static bool
equal (charclass const *s1, charclass const *s2)
{
  charclass_word w = 0;
  for (int i = 0; i < CHARCLASS_WORDS; i++)
    w |= s1->w[i] ^ s2->w[i];
  return w == 0;
}

static bool
emptyset (charclass const *s)
{
  charclass_word w = 0;
  for (int i = 0; i < CHARCLASS_WORDS; i++)
    w |= s->w[i];
  return w == 0;
}

/* Ensure that the array addressed by PA holds at least I + 1 items.
   Either return PA, or reallocate the array and return its new address.
   Although PA may be null, the returned value is never null.

   The array holds *NITEMS items, where 0 <= I <= *NITEMS; *NITEMS
   is updated on reallocation.  If PA is null, *NITEMS must be zero.
   Do not allocate more than NITEMS_MAX items total; -1 means no limit.
   ITEM_SIZE is the size of one item; it must be positive.
   Avoid O(N**2) behavior on arrays growing linearly.  */
static void *
maybe_realloc (void *pa, idx_t i, idx_t *nitems,
               ptrdiff_t nitems_max, idx_t item_size)
{
  if (i < *nitems)
    return pa;
  return xpalloc (pa, nitems, 1, nitems_max, item_size);
}

/* In DFA D, find the index of charclass S, or allocate a new one.  */
static idx_t
charclass_index (struct dfa *d, charclass const *s)
{
  idx_t i;

  for (i = 0; i < d->cindex; ++i)
    if (equal (s, &d->charclasses[i]))
      return i;
  d->charclasses = maybe_realloc (d->charclasses, d->cindex, &d->calloc,
                                  TOKEN_MAX - CSET, sizeof *d->charclasses);
  ++d->cindex;
  d->charclasses[i] = *s;
  return i;
}

static bool
unibyte_word_constituent (struct dfa const *dfa, unsigned char c)
{
  return dfa->localeinfo.sbctowc[c] != WEOF && (isalnum (c) || (c) == '_');
}

static int
char_context (struct dfa const *dfa, unsigned char c)
{
  if (c == dfa->syntax.eolbyte && !(dfa->syntax.dfaopts & DFA_ANCHOR))
    return CTX_NEWLINE;
  if (unibyte_word_constituent (dfa, c))
    return CTX_LETTER;
  return CTX_NONE;
}

/* Set a bit in the charclass for the given char32_t.  Do nothing if WC
   is represented by a multi-byte sequence.  Even for MB_CUR_MAX == 1,
   this may happen when folding case in weird Turkish locales where
   dotless i/dotted I are not included in the chosen character set.
   Return whether a bit was set in the charclass.  */
static bool
setbit_wc (char32_t wc, charclass *c)
{
  int b = c32tob (wc);
  if (b < 0)
    return false;

  setbit (b, c);
  return true;
}

/* Set a bit for B and its case variants in the charclass C.
   MB_CUR_MAX must be 1.  */
static void
setbit_case_fold_c (int b, charclass *c)
{
  int ub = toupper (b);
  for (int i = 0; i < NOTCHAR; i++)
    if (toupper (i) == ub)
      setbit (i, c);
}

/* Fetch the next lexical input character from the pattern.  There
   must at least one byte of pattern input.  Set DFA->lex.wctok to the
   value of the character or to WEOF depending on whether the input is
   a valid multibyte character (possibly of length 1).  Then return
   the next input byte value, except return EOF if the input is a
   multibyte character of length greater than 1.  */
static int
fetch_wc (struct dfa *dfa)
{
  int nbytes = mbs_to_wchar (&dfa->lex.wctok, dfa->lex.ptr, dfa->lex.left,
                             dfa);
  int c = nbytes == 1 ? to_uchar (dfa->lex.ptr[0]) : EOF;
  dfa->lex.ptr += nbytes;
  dfa->lex.left -= nbytes;
  return c;
}

/* If there is no more input, report an error about unbalanced brackets.
   Otherwise, behave as with fetch_wc (DFA).  */
static int
bracket_fetch_wc (struct dfa *dfa)
{
  if (! dfa->lex.left)
    dfaerror (_("unbalanced ["));
  return fetch_wc (dfa);
}

typedef int predicate (int);

/* The following list maps the names of the Posix named character classes
   to predicate functions that determine whether a given character is in
   the class.  The leading [ has already been eaten by the lexical
   analyzer.  */
struct dfa_ctype
{
  const char *name;
  predicate *func;
  bool single_byte_only;
};

static const struct dfa_ctype prednames[] = {
  {"alpha", isalpha, false},
  {"upper", isupper, false},
  {"lower", islower, false},
  {"digit", isdigit, true},
  {"xdigit", isxdigit, false},
  {"space", isspace, false},
  {"punct", ispunct, false},
  {"alnum", isalnum, false},
  {"print", isprint, false},
  {"graph", isgraph, false},
  {"cntrl", iscntrl, false},
  {"blank", isblank, false},
  {NULL, NULL, false}
};

static const struct dfa_ctype *_GL_ATTRIBUTE_PURE
find_pred (const char *str)
{
  for (int i = 0; prednames[i].name; i++)
    if (str_eq (str, prednames[i].name))
      return &prednames[i];
  return NULL;
}

/* Parse a bracket expression, which possibly includes multibyte
   characters.  */
static token
parse_bracket_exp (struct dfa *dfa)
{
  /* This is a bracket expression that dfaexec is known to
     process correctly.  */
  bool known_bracket_exp = true;

  /* Used to warn about [:space:].
     Bit 0 = first character is a colon.
     Bit 1 = last character is a colon.
     Bit 2 = includes any other character but a colon.
     Bit 3 = includes ranges, char/equiv classes or collation elements.  */
  int colon_warning_state;

  dfa->lex.brack.nchars = 0;
  charclass ccl;
  zeroset (&ccl);
  int c = bracket_fetch_wc (dfa);
  bool invert = c == '^';
  if (invert)
    {
      c = bracket_fetch_wc (dfa);
      known_bracket_exp = dfa->localeinfo.simple;
    }
  wint_t wc = dfa->lex.wctok;
  int c1;
  wint_t wc1;
  colon_warning_state = (c == ':');
  do
    {
      c1 = NOTCHAR;	/* Mark c1 as not initialized.  */
      colon_warning_state &= ~2;

      /* Note that if we're looking at some other [:...:] construct,
         we just treat it as a bunch of ordinary characters.  We can do
         this because we assume regex has checked for syntax errors before
         dfa is ever called.  */
      if (c == '[')
        {
          c1 = bracket_fetch_wc (dfa);
          wc1 = dfa->lex.wctok;

          if ((c1 == ':' && (dfa->syntax.syntax_bits & RE_CHAR_CLASSES))
              || c1 == '.' || c1 == '=')
            {
              enum { MAX_BRACKET_STRING_LEN = 32 };
              char str[MAX_BRACKET_STRING_LEN + 1];
              int len = 0;
              for (;;)
                {
                  c = bracket_fetch_wc (dfa);
                  if (dfa->lex.left == 0
                      || (c == c1 && dfa->lex.ptr[0] == ']'))
                    break;
                  if (len < MAX_BRACKET_STRING_LEN)
                    str[len++] = c;
                  else
                    /* This is in any case an invalid class name.  */
                    str[0] = '\0';
                }
              str[len] = '\0';

              /* Fetch bracket.  */
              c = bracket_fetch_wc (dfa);
              wc = dfa->lex.wctok;
              if (c1 == ':')
                /* Build character class.  POSIX allows character
                   classes to match multicharacter collating elements,
                   but the regex code does not support that, so do not
                   worry about that possibility.  */
                {
                  char const *class
                    = (dfa->syntax.case_fold && (str_eq (str, "upper")
                                                 || str_eq (str, "lower"))
                       ? "alpha" : str);
                  const struct dfa_ctype *pred = find_pred (class);
                  if (!pred)
                    dfaerror (_("invalid character class"));

                  if (dfa->localeinfo.multibyte && !pred->single_byte_only)
                    known_bracket_exp = false;
                  else
                    for (int c2 = 0; c2 < NOTCHAR; ++c2)
                      if (pred->func (c2))
                        setbit (c2, &ccl);
                }
              else
                known_bracket_exp = false;

              colon_warning_state |= 8;

              /* Fetch new lookahead character.  */
              c1 = bracket_fetch_wc (dfa);
              wc1 = dfa->lex.wctok;
              continue;
            }

          /* We treat '[' as a normal character here.  c/c1/wc/wc1
             are already set up.  */
        }

      if (c == '\\'
          && (dfa->syntax.syntax_bits & RE_BACKSLASH_ESCAPE_IN_LISTS))
        {
          c = bracket_fetch_wc (dfa);
          wc = dfa->lex.wctok;
        }

      if (c1 == NOTCHAR)
        {
          c1 = bracket_fetch_wc (dfa);
          wc1 = dfa->lex.wctok;
        }

      if (c1 == '-')
        /* build range characters.  */
        {
          int c2 = bracket_fetch_wc (dfa);
          wint_t wc2 = dfa->lex.wctok;

          /* A bracket expression like [a-[.aa.]] matches an unknown set.
             Treat it like [-a[.aa.]] while parsing it, and
             remember that the set is unknown.  */
          if (c2 == '[' && dfa->lex.ptr[0] == '.')
            {
              known_bracket_exp = false;
              c2 = ']';
            }

          if (c2 == ']')
            {
              /* In the case [x-], the - is an ordinary hyphen,
                 which is left in c1, the lookahead character.  */
              dfa->lex.ptr--;
              dfa->lex.left++;
            }
          else
            {
              if (c2 == '\\' && (dfa->syntax.syntax_bits
                                 & RE_BACKSLASH_ESCAPE_IN_LISTS))
                {
                  c2 = bracket_fetch_wc (dfa);
                  wc2 = dfa->lex.wctok;
                }

              colon_warning_state |= 8;
              c1 = bracket_fetch_wc (dfa);
              wc1 = dfa->lex.wctok;

              /* Treat [x-y] as a range if x != y.  */
              if (wc != wc2 || wc == WEOF)
                {
                  if (dfa->localeinfo.simple
                      || (c_isdigit (c) & c_isdigit (c2)))
                    {
                      for (int ci = c; ci <= c2; ci++)
                        if (dfa->syntax.case_fold && isalpha (ci))
                          setbit_case_fold_c (ci, &ccl);
                        else
                          setbit (ci, &ccl);
                    }
                  else
                    known_bracket_exp = false;

                  continue;
                }
            }
        }

      colon_warning_state |= (c == ':') ? 2 : 4;

      if (!dfa->localeinfo.multibyte)
        {
          if (dfa->syntax.case_fold && isalpha (c))
            setbit_case_fold_c (c, &ccl);
          else
            setbit (c, &ccl);
          continue;
        }

      if (wc == WEOF)
        known_bracket_exp = false;
      else
        {
          char32_t folded[CASE_FOLDED_BUFSIZE + 1];
          int n = (dfa->syntax.case_fold
                   ? case_folded_counterparts (wc, folded + 1) + 1
                   : 1);
          folded[0] = wc;
          for (int i = 0; i < n; i++)
            if (!setbit_wc (folded[i], &ccl))
              {
                dfa->lex.brack.chars
                  = maybe_realloc (dfa->lex.brack.chars, dfa->lex.brack.nchars,
                                   &dfa->lex.brack.nchars_alloc, -1,
                                   sizeof *dfa->lex.brack.chars);
                dfa->lex.brack.chars[dfa->lex.brack.nchars++] = folded[i];
              }
        }
    }
  while ((wc = wc1, (c = c1) != ']'));

  if (colon_warning_state == 7)
    {
      char const *msg
        = _("character class syntax is [[:space:]], not [:space:]");
      if (dfa->syntax.dfaopts & DFA_CONFUSING_BRACKETS_ERROR)
        dfaerror (msg);
      else
        dfawarn (msg);
    }

  if (! known_bracket_exp)
    return BACKREF;

  if (dfa->localeinfo.multibyte && (invert || dfa->lex.brack.nchars != 0))
    {
      dfa->lex.brack.invert = invert;
      dfa->lex.brack.cset = emptyset (&ccl) ? -1 : charclass_index (dfa, &ccl);
      return MBCSET;
    }

  if (invert)
    {
      notset (&ccl);
      if (dfa->syntax.syntax_bits & RE_HAT_LISTS_NOT_NEWLINE)
        clrbit ('\n', &ccl);
    }

  return CSET + charclass_index (dfa, &ccl);
}

struct lexptr
{
  char const *ptr;
  idx_t left;
};

static void
push_lex_state (struct dfa *dfa, struct lexptr *ls, char const *s)
{
  ls->ptr = dfa->lex.ptr;
  ls->left = dfa->lex.left;
  dfa->lex.ptr = s;
  dfa->lex.left = strlen (s);
}

static void
pop_lex_state (struct dfa *dfa, struct lexptr const *ls)
{
  dfa->lex.ptr = ls->ptr;
  dfa->lex.left = ls->left;
}

static token
lex (struct dfa *dfa)
{
  bool backslash = false;

  /* Basic plan: We fetch a character.  If it's a backslash,
     we set the backslash flag and go through the loop again.
     On the plus side, this avoids having a duplicate of the
     main switch inside the backslash case.  On the minus side,
     it means that just about every case tests the backslash flag.  */
  for (int i = 0; ; i++)
    {
      /* This loop should consume at most a backslash and some other
         character.  */
      assume (i < 2);

      if (! dfa->lex.left)
        return dfa->lex.lasttok = END;
      int c = fetch_wc (dfa);

      switch (c)
        {
        case '\\':
          if (backslash)
            goto normal_char;
          if (dfa->lex.left == 0)
            dfaerror (_("unfinished \\ escape"));
          backslash = true;
          break;

        case '^':
          if (backslash)
            goto normal_char;
          if (dfa->syntax.syntax_bits & RE_CONTEXT_INDEP_ANCHORS
              || dfa->lex.lasttok == END || dfa->lex.lasttok == LPAREN
              || dfa->lex.lasttok == OR)
            return dfa->lex.lasttok = BEGLINE;
          goto normal_char;

        case '$':
          if (backslash)
            goto normal_char;
          if (dfa->syntax.syntax_bits & RE_CONTEXT_INDEP_ANCHORS
              || dfa->lex.left == 0
              || ((dfa->lex.left
                   > !(dfa->syntax.syntax_bits & RE_NO_BK_PARENS))
                  && (dfa->lex.ptr[!(dfa->syntax.syntax_bits & RE_NO_BK_PARENS)
                                   & (dfa->lex.ptr[0] == '\\')]
                      == ')'))
              || ((dfa->lex.left
                   > !(dfa->syntax.syntax_bits & RE_NO_BK_VBAR))
                  && (dfa->lex.ptr[!(dfa->syntax.syntax_bits & RE_NO_BK_VBAR)
                                   & (dfa->lex.ptr[0] == '\\')]
                      == '|'))
              || ((dfa->syntax.syntax_bits & RE_NEWLINE_ALT)
                  && dfa->lex.left > 0 && dfa->lex.ptr[0] == '\n'))
            return dfa->lex.lasttok = ENDLINE;
          goto normal_char;

        case '1':
        case '2':
        case '3':
        case '4':
        case '5':
        case '6':
        case '7':
        case '8':
        case '9':
          if (!backslash)
            goto normal_char;
          if (dfa->syntax.syntax_bits & RE_NO_BK_REFS)
            goto stray_backslash;

          dfa->lex.laststart = false;
          return dfa->lex.lasttok = BACKREF;

        case '`':
          if (!backslash)
            goto normal_char;
          if (dfa->syntax.syntax_bits & RE_NO_GNU_OPS)
            goto stray_backslash;

          /* FIXME: should be beginning of string */
          return dfa->lex.lasttok = BEGLINE;

        case '\'':
          if (!backslash)
            goto normal_char;
          if (dfa->syntax.syntax_bits & RE_NO_GNU_OPS)
            goto stray_backslash;

          /* FIXME: should be end of string */
          return dfa->lex.lasttok = ENDLINE;

        case '<':
          if (!backslash)
            goto normal_char;
          if (dfa->syntax.syntax_bits & RE_NO_GNU_OPS)
            goto stray_backslash;

          return dfa->lex.lasttok = BEGWORD;

        case '>':
          if (!backslash)
            goto normal_char;
          if (dfa->syntax.syntax_bits & RE_NO_GNU_OPS)
            goto stray_backslash;

          return dfa->lex.lasttok = ENDWORD;

        case 'b':
          if (!backslash)
            goto normal_char;
          if (dfa->syntax.syntax_bits & RE_NO_GNU_OPS)
            goto stray_backslash;

          return dfa->lex.lasttok = LIMWORD;

        case 'B':
          if (!backslash)
            goto normal_char;
          if (dfa->syntax.syntax_bits & RE_NO_GNU_OPS)
            goto stray_backslash;

          return dfa->lex.lasttok = NOTLIMWORD;

        case '?':
          if (dfa->syntax.syntax_bits & RE_LIMITED_OPS)
            goto default_case;
          if (backslash != ((dfa->syntax.syntax_bits & RE_BK_PLUS_QM) != 0))
            goto normal_char;
          if (dfa->lex.laststart)
            {
              if (!(dfa->syntax.syntax_bits & RE_CONTEXT_INDEP_OPS))
                goto default_case;
              if (dfa->syntax.dfaopts & DFA_PLUS_WARN)
                dfawarn (_("? at start of expression"));
            }
          return dfa->lex.lasttok = QMARK;

        case '*':
          if (backslash)
            goto normal_char;
          if (dfa->lex.laststart)
            {
              if (!(dfa->syntax.syntax_bits & RE_CONTEXT_INDEP_OPS))
                goto default_case;
              if (dfa->syntax.dfaopts & DFA_STAR_WARN)
                dfawarn (_("* at start of expression"));
            }
          return dfa->lex.lasttok = STAR;

        case '+':
          if (dfa->syntax.syntax_bits & RE_LIMITED_OPS)
            goto default_case;
          if (backslash != ((dfa->syntax.syntax_bits & RE_BK_PLUS_QM) != 0))
            goto normal_char;
          if (dfa->lex.laststart)
            {
              if (!(dfa->syntax.syntax_bits & RE_CONTEXT_INDEP_OPS))
                goto default_case;
              if (dfa->syntax.dfaopts & DFA_PLUS_WARN)
                dfawarn (_("+ at start of expression"));
            }
          return dfa->lex.lasttok = PLUS;

        case '{':
          if (!(dfa->syntax.syntax_bits & RE_INTERVALS))
            goto default_case;
          if (backslash != ((dfa->syntax.syntax_bits & RE_NO_BK_BRACES) == 0))
            goto normal_char;

          /* Cases:
             {M} - exact count
             {M,} - minimum count, maximum is infinity
             {,N} - 0 through N
             {,} - 0 to infinity (same as '*')
             {M,N} - M through N */
          {
            char const *p = dfa->lex.ptr;
            char const *lim = p + dfa->lex.left;
            dfa->lex.minrep = dfa->lex.maxrep = -1;
            for (; p != lim && c_isdigit (*p); p++)
              dfa->lex.minrep = (dfa->lex.minrep < 0
                                 ? *p - '0'
                                 : MIN (RE_DUP_MAX + 1,
                                        dfa->lex.minrep * 10 + *p - '0'));
            if (p != lim)
              {
                if (*p != ',')
                  dfa->lex.maxrep = dfa->lex.minrep;
                else
                  {
                    if (dfa->lex.minrep < 0)
                      dfa->lex.minrep = 0;
                    while (++p != lim && c_isdigit (*p))
                      dfa->lex.maxrep
                        = (dfa->lex.maxrep < 0
                           ? *p - '0'
                           : MIN (RE_DUP_MAX + 1,
                                  dfa->lex.maxrep * 10 + *p - '0'));
                  }
              }
            bool invalid_content
              = ! ((! backslash || (p != lim && *p++ == '\\'))
                   && p != lim && *p++ == '}'
                   && 0 <= dfa->lex.minrep
                   && (dfa->lex.maxrep < 0
                       || dfa->lex.minrep <= dfa->lex.maxrep));
            if (invalid_content
                && (dfa->syntax.syntax_bits & RE_INVALID_INTERVAL_ORD))
              goto normal_char;
            if (dfa->lex.laststart)
              {
                if (!(dfa->syntax.syntax_bits & RE_CONTEXT_INDEP_OPS))
                  goto default_case;
                if (dfa->syntax.dfaopts & DFA_PLUS_WARN)
                  dfawarn (_("{...} at start of expression"));
              }
            if (invalid_content)
              dfaerror (_("invalid content of \\{\\}"));
            if (RE_DUP_MAX < dfa->lex.maxrep)
              dfaerror (_("regular expression too big"));
            dfa->lex.ptr = p;
            dfa->lex.left = lim - p;
          }
          dfa->lex.laststart = false;
          return dfa->lex.lasttok = REPMN;

        case '|':
          if (dfa->syntax.syntax_bits & RE_LIMITED_OPS)
            goto default_case;
          if (backslash != ((dfa->syntax.syntax_bits & RE_NO_BK_VBAR) == 0))
            goto normal_char;
          dfa->lex.laststart = true;
          return dfa->lex.lasttok = OR;

        case '\n':
          if (!(dfa->syntax.syntax_bits & RE_NEWLINE_ALT))
            goto default_case;
          if (backslash)
            goto normal_char;
          dfa->lex.laststart = true;
          return dfa->lex.lasttok = OR;

        case '(':
          if (backslash != ((dfa->syntax.syntax_bits & RE_NO_BK_PARENS) == 0))
            goto normal_char;
          dfa->lex.parens++;
          dfa->lex.laststart = true;
          return dfa->lex.lasttok = LPAREN;

        case ')':
          if (backslash != ((dfa->syntax.syntax_bits & RE_NO_BK_PARENS) == 0))
            goto normal_char;
          if (dfa->lex.parens == 0
              && dfa->syntax.syntax_bits & RE_UNMATCHED_RIGHT_PAREN_ORD)
            goto normal_char;
          dfa->lex.parens--;
          dfa->lex.laststart = false;
          return dfa->lex.lasttok = RPAREN;

        case '.':
          if (backslash)
            goto normal_char;
          if (dfa->canychar < 0)
            {
              charclass ccl;
              fillset (&ccl);
              if (!(dfa->syntax.syntax_bits & RE_DOT_NEWLINE))
                clrbit ('\n', &ccl);
              if (dfa->syntax.syntax_bits & RE_DOT_NOT_NULL)
                clrbit ('\0', &ccl);
              if (dfa->localeinfo.multibyte)
                for (int c2 = 0; c2 < NOTCHAR; c2++)
                  if (dfa->localeinfo.sbctowc[c2] == WEOF)
                    clrbit (c2, &ccl);
              dfa->canychar = charclass_index (dfa, &ccl);
            }
          dfa->lex.laststart = false;
          return dfa->lex.lasttok = (dfa->localeinfo.multibyte
                                     ? ANYCHAR
                                     : CSET + dfa->canychar);

        case 's':
        case 'S':
          if (!backslash)
            goto normal_char;
          if (dfa->syntax.syntax_bits & RE_NO_GNU_OPS)
            goto stray_backslash;

          if (!dfa->localeinfo.multibyte)
            {
              charclass ccl;
              zeroset (&ccl);
              for (int c2 = 0; c2 < NOTCHAR; ++c2)
                if (isspace (c2))
                  setbit (c2, &ccl);
              if (c == 'S')
                notset (&ccl);
              dfa->lex.laststart = false;
              return dfa->lex.lasttok = CSET + charclass_index (dfa, &ccl);
            }

          /* FIXME: see if optimizing this, as is done with ANYCHAR and
             add_utf8_anychar, makes sense.  */

          /* \s and \S are documented to be equivalent to [[:space:]] and
             [^[:space:]] respectively, so tell the lexer to process those
             strings, each minus its "already processed" '['.  */
          {
            struct lexptr ls;
            push_lex_state (dfa, &ls, &"^[:space:]]"[c == 's']);
            dfa->lex.lasttok = parse_bracket_exp (dfa);
            pop_lex_state (dfa, &ls);
          }

          dfa->lex.laststart = false;
          return dfa->lex.lasttok;

        case 'w':
        case 'W':
          if (!backslash)
            goto normal_char;
          if (dfa->syntax.syntax_bits & RE_NO_GNU_OPS)
            goto stray_backslash;

          if (!dfa->localeinfo.multibyte)
            {
              charclass ccl;
              zeroset (&ccl);
              for (int c2 = 0; c2 < NOTCHAR; ++c2)
                if (dfa->syntax.sbit[c2] == CTX_LETTER)
                  setbit (c2, &ccl);
              if (c == 'W')
                notset (&ccl);
              dfa->lex.laststart = false;
              return dfa->lex.lasttok = CSET + charclass_index (dfa, &ccl);
            }

          /* FIXME: see if optimizing this, as is done with ANYCHAR and
             add_utf8_anychar, makes sense.  */

          /* \w and \W are documented to be equivalent to [_[:alnum:]] and
             [^_[:alnum:]] respectively, so tell the lexer to process those
             strings, each minus its "already processed" '['.  */
          {
            struct lexptr ls;
            push_lex_state (dfa, &ls, &"^_[:alnum:]]"[c == 'w']);
            dfa->lex.lasttok = parse_bracket_exp (dfa);
            pop_lex_state (dfa, &ls);
          }

          dfa->lex.laststart = false;
          return dfa->lex.lasttok;

        case '[':
          if (backslash)
            goto normal_char;
          dfa->lex.laststart = false;
          return dfa->lex.lasttok = parse_bracket_exp (dfa);

        default:
        default_case:
          if (!backslash)
            goto normal_char;
        stray_backslash:
          if (dfa->syntax.dfaopts & DFA_STRAY_BACKSLASH_WARN)
            {
              char const *msg;
              char msgbuf[100];
              if (!c32isprint (dfa->lex.wctok))
                msg = _("stray \\ before unprintable character");
              else if (c32isspace (dfa->lex.wctok))
                msg = _("stray \\ before white space");
              else
                {
                  char buf[MB_LEN_MAX + 1];
                  mbstate_t s;
                  mbszero (&s);
                  size_t stored_bytes = c32rtomb (buf, dfa->lex.wctok, &s);
                  if (stored_bytes < (size_t) -1)
                    {
                      buf[stored_bytes] = '\0';
                      int n = snprintf (msgbuf, sizeof msgbuf,
                                        _("stray \\ before %s"), buf);
                      msg = 0 <= n && n < sizeof msgbuf ? msgbuf : _("stray \\");
                    }
                  else
                    msg = _("stray \\");
                }
              dfawarn (msg);
            }
          FALLTHROUGH;
        case ']': case '}':
        normal_char:
          dfa->lex.laststart = false;
          /* For multibyte character sets, folding is done in atom.  Always
             return WCHAR.  */
          if (dfa->localeinfo.multibyte)
            return dfa->lex.lasttok = WCHAR;

          if (dfa->syntax.case_fold && isalpha (c))
            {
              charclass ccl;
              zeroset (&ccl);
              setbit_case_fold_c (c, &ccl);
              return dfa->lex.lasttok = CSET + charclass_index (dfa, &ccl);
            }

          return dfa->lex.lasttok = c;
        }
    }
}

static void
addtok_mb (struct dfa *dfa, token t, char mbprop)
{
  if (dfa->talloc == dfa->tindex)
    {
      dfa->tokens = xpalloc (dfa->tokens, &dfa->talloc, 1, -1,
                             sizeof *dfa->tokens);
      if (dfa->localeinfo.multibyte)
        dfa->multibyte_prop = xreallocarray (dfa->multibyte_prop, dfa->talloc,
                                             sizeof *dfa->multibyte_prop);
    }
  if (dfa->localeinfo.multibyte)
    dfa->multibyte_prop[dfa->tindex] = mbprop;
  dfa->tokens[dfa->tindex++] = t;

  switch (t)
    {
    case QMARK:
    case STAR:
    case PLUS:
      break;

    case CAT:
    case OR:
      dfa->parse.depth--;
      break;

    case EMPTY:
      dfa->epsilon = true;
      goto increment_depth;

    case BACKREF:
      dfa->fast = false;
      goto increment_nleaves;

    case BEGLINE:
    case ENDLINE:
    case BEGWORD:
    case ENDWORD:
    case LIMWORD:
    case NOTLIMWORD:
      dfa->epsilon = true;
      FALLTHROUGH;
    default:
    increment_nleaves:
      dfa->nleaves++;
    increment_depth:
      dfa->parse.depth++;
      if (dfa->depth < dfa->parse.depth)
        dfa->depth = dfa->parse.depth;
      break;
    }
}

static void addtok_wc (struct dfa *dfa, wint_t wc);

/* Add the given token to the parse tree, maintaining the depth count and
   updating the maximum depth if necessary.  */
static void
addtok (struct dfa *dfa, token t)
{
  if (dfa->localeinfo.multibyte && t == MBCSET)
    {
      bool need_or = false;

      /* Extract wide characters into alternations for better performance.
         This does not require UTF-8.  */
      for (idx_t i = 0; i < dfa->lex.brack.nchars; i++)
        {
          addtok_wc (dfa, dfa->lex.brack.chars[i]);
          if (need_or)
            addtok (dfa, OR);
          need_or = true;
        }
      dfa->lex.brack.nchars = 0;

      /* Wide characters have been handled above, so it is possible
         that the set is empty now.  Do nothing in that case.  */
      if (dfa->lex.brack.cset != -1)
        {
          addtok (dfa, CSET + dfa->lex.brack.cset);
          if (need_or)
            addtok (dfa, OR);
        }
    }
  else
    {
      addtok_mb (dfa, t, 3);
    }
}

/* We treat a multibyte character as a single atom, so that DFA
   can treat a multibyte character as a single expression.

   e.g., we construct the following tree from "<mb1><mb2>".
   <mb1(1st-byte)><mb1(2nd-byte)><CAT><mb1(3rd-byte)><CAT>
   <mb2(1st-byte)><mb2(2nd-byte)><CAT><mb2(3rd-byte)><CAT><CAT> */
static void
addtok_wc (struct dfa *dfa, wint_t wc)
{
  unsigned char buf[MB_LEN_MAX];
  mbstate_t s;
  mbszero (&s);
  size_t stored_bytes = c32rtomb ((char *) buf, wc, &s);
  int buflen;

  if (stored_bytes != (size_t) -1)
    buflen = stored_bytes;
  else
    {
      /* This is merely stop-gap.  buf[0] is undefined, yet skipping
         the addtok_mb call altogether can corrupt the heap.  */
      buflen = 1;
      buf[0] = 0;
    }

  addtok_mb (dfa, buf[0], buflen == 1 ? 3 : 1);
  for (int i = 1; i < buflen; i++)
    {
      addtok_mb (dfa, buf[i], i == buflen - 1 ? 2 : 0);
      addtok (dfa, CAT);
    }
}

static void
add_utf8_anychar (struct dfa *dfa)
{
  /* Since the Unicode Standard Version 4.0.0 (2003), a well-formed
     UTF-8 byte sequence has been defined as follows:

     ([\x00-\x7f]
     |[\xc2-\xdf][\x80-\xbf]
     |[\xe0][\xa0-\xbf][\x80-\xbf]
     |[\xe1-\xec\xee-\xef][\x80-\xbf][\x80-\xbf]
     |[\xed][\x80-\x9f][\x80-\xbf]
     |[\xf0][\x90-\xbf][\x80-\xbf][\x80-\xbf])
     |[\xf1-\xf3][\x80-\xbf][\x80-\xbf][\x80-\xbf]
     |[\xf4][\x80-\x8f][\x80-\xbf][\x80-\xbf])

     which I'll write more concisely "A|BC|DEC|FCC|GHC|IJCC|KCCC|LMCC",
     where A = [\x00-\x7f], B = [\xc2-\xdf], C = [\x80-\xbf],
     D = [\xe0], E = [\xa0-\xbf], F = [\xe1-\xec\xee-\xef], G = [\xed],
     H = [\x80-\x9f], I = [\xf0],
     J = [\x90-\xbf], K = [\xf1-\xf3], L = [\xf4], M = [\x80-\x8f].

     This can be refactored to "A|(B|DE|GH|(F|IJ|LM|KC)C)C".  */

  /* Mnemonics for classes containing two or more bytes.  */
  enum { A, B, C, E, F, H, J, K, M };

  /* Mnemonics for single-byte tokens.  */
  enum { D_token = 0xe0, G_token = 0xed, I_token = 0xf0, L_token = 0xf4 };

  static charclass const utf8_classes[] = {
    /* A. 00-7f: 1-byte sequence.  */
    CHARCLASS_INIT (0xffffffff, 0xffffffff, 0xffffffff, 0xffffffff, 0, 0, 0, 0),

    /* B. c2-df: 1st byte of a 2-byte sequence.  */
    CHARCLASS_INIT (0, 0, 0, 0, 0, 0, 0xfffffffc, 0),

    /* C. 80-bf: non-leading bytes.  */
    CHARCLASS_INIT (0, 0, 0, 0, 0xffffffff, 0xffffffff, 0, 0),

    /* D. e0 (just a token).  */

    /* E. a0-bf: 2nd byte of a "DEC" sequence.  */
    CHARCLASS_INIT (0, 0, 0, 0, 0, 0xffffffff, 0, 0),

    /* F. e1-ec + ee-ef: 1st byte of an "FCC" sequence.  */
    CHARCLASS_INIT (0, 0, 0, 0, 0, 0, 0, 0xdffe),

    /* G. ed (just a token).  */

    /* H. 80-9f: 2nd byte of a "GHC" sequence.  */
    CHARCLASS_INIT (0, 0, 0, 0, 0xffffffff, 0, 0, 0),

    /* I. f0 (just a token).  */

    /* J. 90-bf: 2nd byte of an "IJCC" sequence.  */
    CHARCLASS_INIT (0, 0, 0, 0, 0xffff0000, 0xffffffff, 0, 0),

    /* K. f1-f3: 1st byte of a "KCCC" sequence.  */
    CHARCLASS_INIT (0, 0, 0, 0, 0, 0, 0, 0xe0000),

    /* L. f4 (just a token).  */

    /* M. 80-8f: 2nd byte of a "LMCC" sequence.  */
    CHARCLASS_INIT (0, 0, 0, 0, 0xffff, 0, 0, 0),
  };

  /* Define the character classes that are needed below.  */
  if (dfa->utf8_anychar_classes[0] == 0)
    {
      charclass c = utf8_classes[0];
      if (! (dfa->syntax.syntax_bits & RE_DOT_NEWLINE))
        clrbit ('\n', &c);
      if (dfa->syntax.syntax_bits & RE_DOT_NOT_NULL)
        clrbit ('\0', &c);
      dfa->utf8_anychar_classes[0] = CSET + charclass_index (dfa, &c);

      for (int i = 1; i < sizeof utf8_classes / sizeof *utf8_classes; i++)
        dfa->utf8_anychar_classes[i]
          = CSET + charclass_index (dfa, &utf8_classes[i]);
    }

  /* Implement the "A|(B|DE|GH|(F|IJ|LM|KC)C)C" pattern mentioned above.
     The token buffer is in reverse Polish order, so we get
     "A B D E CAT OR G H CAT OR F I J CAT OR L M CAT OR K
      C CAT OR C CAT OR C CAT OR".  */
  addtok (dfa, dfa->utf8_anychar_classes[A]);
  addtok (dfa, dfa->utf8_anychar_classes[B]);
  addtok (dfa, D_token);
  addtok (dfa, dfa->utf8_anychar_classes[E]);
  addtok (dfa, CAT);
  addtok (dfa, OR);
  addtok (dfa, G_token);
  addtok (dfa, dfa->utf8_anychar_classes[H]);
  addtok (dfa, CAT);
  addtok (dfa, OR);
  addtok (dfa, dfa->utf8_anychar_classes[F]);
  addtok (dfa, I_token);
  addtok (dfa, dfa->utf8_anychar_classes[J]);
  addtok (dfa, CAT);
  addtok (dfa, OR);
  addtok (dfa, L_token);
  addtok (dfa, dfa->utf8_anychar_classes[M]);
  addtok (dfa, CAT);
  addtok (dfa, OR);
  addtok (dfa, dfa->utf8_anychar_classes[K]);
  for (int i = 0; i < 3; i++)
    {
      addtok (dfa, dfa->utf8_anychar_classes[C]);
      addtok (dfa, CAT);
      addtok (dfa, OR);
    }
}

/* The grammar understood by the parser is as follows.

   regexp:
     regexp OR branch
     branch

   branch:
     branch closure
     closure

   closure:
     closure QMARK
     closure STAR
     closure PLUS
     closure REPMN
     atom

   atom:
     <normal character>
     <multibyte character>
     ANYCHAR
     MBCSET
     CSET
     BACKREF
     BEGLINE
     ENDLINE
     BEGWORD
     ENDWORD
     LIMWORD
     NOTLIMWORD
     LPAREN regexp RPAREN
     <empty>

   The parser builds a parse tree in postfix form in an array of tokens.  */

static void
atom (struct dfa *dfa)
{
  if ((0 <= dfa->parse.tok && dfa->parse.tok < NOTCHAR)
      || dfa->parse.tok >= CSET
      || dfa->parse.tok == BEG || dfa->parse.tok == BACKREF
      || dfa->parse.tok == BEGLINE || dfa->parse.tok == ENDLINE
      || dfa->parse.tok == BEGWORD || dfa->parse.tok == ENDWORD
      || dfa->parse.tok == LIMWORD || dfa->parse.tok == NOTLIMWORD
      || dfa->parse.tok == ANYCHAR || dfa->parse.tok == MBCSET)
    {
      if (dfa->parse.tok == ANYCHAR && dfa->localeinfo.using_utf8)
        {
          /* For UTF-8 expand the period to a series of CSETs that define a
             valid UTF-8 character.  This avoids using the slow multibyte
             path.  I'm pretty sure it would be both profitable and correct to
             do it for any encoding; however, the optimization must be done
             manually as it is done above in add_utf8_anychar.  So, let's
             start with UTF-8: it is the most used, and the structure of the
             encoding makes the correctness more obvious.  */
          add_utf8_anychar (dfa);
        }
      else
        addtok (dfa, dfa->parse.tok);
      dfa->parse.tok = lex (dfa);
    }
  else if (dfa->parse.tok == WCHAR)
    {
      if (dfa->lex.wctok == WEOF)
        addtok (dfa, BACKREF);
      else
        {
          addtok_wc (dfa, dfa->lex.wctok);

          if (dfa->syntax.case_fold)
            {
              char32_t folded[CASE_FOLDED_BUFSIZE];
              int n = case_folded_counterparts (dfa->lex.wctok, folded);
              for (int i = 0; i < n; i++)
                {
                  addtok_wc (dfa, folded[i]);
                  addtok (dfa, OR);
                }
            }
        }

      dfa->parse.tok = lex (dfa);
    }
  else if (dfa->parse.tok == LPAREN)
    {
      dfa->parse.tok = lex (dfa);
      regexp (dfa);
      if (dfa->parse.tok != RPAREN)
        dfaerror (_("unbalanced ("));
      dfa->parse.tok = lex (dfa);
    }
  else
    addtok (dfa, EMPTY);
}

/* Return the number of tokens in the given subexpression.  */
static idx_t _GL_ATTRIBUTE_PURE
nsubtoks (struct dfa const *dfa, idx_t tindex)
{
  switch (dfa->tokens[tindex - 1])
    {
    default:
      return 1;
    case QMARK:
    case STAR:
    case PLUS:
      return 1 + nsubtoks (dfa, tindex - 1);
    case CAT:
    case OR:
      {
        idx_t ntoks1 = nsubtoks (dfa, tindex - 1);
        return 1 + ntoks1 + nsubtoks (dfa, tindex - 1 - ntoks1);
      }
    }
}

/* Copy the given subexpression to the top of the tree.  */
static void
copytoks (struct dfa *dfa, idx_t tindex, idx_t ntokens)
{
  if (dfa->localeinfo.multibyte)
    for (idx_t i = 0; i < ntokens; i++)
      addtok_mb (dfa, dfa->tokens[tindex + i],
                 dfa->multibyte_prop[tindex + i]);
  else
    for (idx_t i = 0; i < ntokens; i++)
      addtok_mb (dfa, dfa->tokens[tindex + i], 3);
}

static void
closure (struct dfa *dfa)
{
  atom (dfa);
  while (dfa->parse.tok == QMARK || dfa->parse.tok == STAR
         || dfa->parse.tok == PLUS || dfa->parse.tok == REPMN)
    if (dfa->parse.tok == REPMN && (dfa->lex.minrep || dfa->lex.maxrep))
      {
        idx_t ntokens = nsubtoks (dfa, dfa->tindex);
        idx_t tindex = dfa->tindex - ntokens;
        if (dfa->lex.maxrep < 0)
          addtok (dfa, PLUS);
        if (dfa->lex.minrep == 0)
          addtok (dfa, QMARK);
        int i;
        for (i = 1; i < dfa->lex.minrep; i++)
          {
            copytoks (dfa, tindex, ntokens);
            addtok (dfa, CAT);
          }
        for (; i < dfa->lex.maxrep; i++)
          {
            copytoks (dfa, tindex, ntokens);
            addtok (dfa, QMARK);
            addtok (dfa, CAT);
          }
        dfa->parse.tok = lex (dfa);
      }
    else if (dfa->parse.tok == REPMN)
      {
        dfa->tindex -= nsubtoks (dfa, dfa->tindex);
        dfa->parse.tok = lex (dfa);
        closure (dfa);
      }
    else
      {
        addtok (dfa, dfa->parse.tok);
        dfa->parse.tok = lex (dfa);
      }
}

static void
branch (struct dfa* dfa)
{
  closure (dfa);
  while (dfa->parse.tok != RPAREN && dfa->parse.tok != OR
         && dfa->parse.tok >= 0)
    {
      closure (dfa);
      addtok (dfa, CAT);
    }
}

static void
regexp (struct dfa *dfa)
{
  branch (dfa);
  while (dfa->parse.tok == OR)
    {
      dfa->parse.tok = lex (dfa);
      branch (dfa);
      addtok (dfa, OR);
    }
}

/* Parse a string S of length LEN into D.  S can include NUL characters.
   This is the main entry point for the parser.  */
void
dfaparse (char const *s, idx_t len, struct dfa *d)
{
  d->lex.ptr = s;
  d->lex.left = len;
  d->lex.lasttok = END;
  d->lex.laststart = true;

  if (!d->syntax.syntax_bits_set)
    dfaerror (_("no syntax specified"));

  if (!d->nregexps)
    addtok (d, BEG);

  d->parse.tok = lex (d);
  d->parse.depth = d->depth;

  regexp (d);

  if (d->parse.tok != END)
    dfaerror (_("unbalanced )"));

  addtok (d, END - d->nregexps);
  addtok (d, CAT);

  if (d->nregexps)
    addtok (d, OR);

  ++d->nregexps;
}

/* Some primitives for operating on sets of positions.  */

/* Copy one set to another.  */
static void
copy (position_set const *src, position_set *dst)
{
  if (dst->alloc < src->nelem)
    {
      free (dst->elems);
      dst->elems = xpalloc (NULL, &dst->alloc, src->nelem - dst->alloc, -1,
                            sizeof *dst->elems);
    }
  dst->nelem = src->nelem;
  if (src->nelem != 0)
    memcpy (dst->elems, src->elems, src->nelem * sizeof *dst->elems);
}

static void
alloc_position_set (position_set *s, idx_t size)
{
  s->elems = xnmalloc (size, sizeof *s->elems);
  s->alloc = size;
  s->nelem = 0;
}

/* Insert position P in set S.  S is maintained in sorted order on
   decreasing index.  If there is already an entry in S with P.index
   then merge (logically-OR) P's constraints into the one in S.
   S->elems must point to an array large enough to hold the resulting set.  */
static void
insert (position p, position_set *s)
{
  idx_t count = s->nelem;
  idx_t lo = 0, hi = count;
  while (lo < hi)
    {
      idx_t mid = (lo + hi) >> 1;
      if (s->elems[mid].index < p.index)
        lo = mid + 1;
      else if (s->elems[mid].index == p.index)
        {
          s->elems[mid].constraint |= p.constraint;
          return;
        }
      else
        hi = mid;
    }

  s->elems = maybe_realloc (s->elems, count, &s->alloc, -1, sizeof *s->elems);
  for (idx_t i = count; i > lo; i--)
    s->elems[i] = s->elems[i - 1];
  s->elems[lo] = p;
  ++s->nelem;
}

static void
append (position p, position_set *s)
{
  idx_t count = s->nelem;
  s->elems = maybe_realloc (s->elems, count, &s->alloc, -1, sizeof *s->elems);
  s->elems[s->nelem++] = p;
}

/* Merge S1 and S2 (with the additional constraint C2) into M.  The
   result is as if the positions of S1, and of S2 with the additional
   constraint C2, were inserted into an initially empty set.  */
static void
merge_constrained (position_set const *s1, position_set const *s2,
                   unsigned int c2, position_set *m)
{
  idx_t i = 0, j = 0;

  if (m->alloc - s1->nelem < s2->nelem)
    {
      free (m->elems);
      m->alloc = s1->nelem;
      m->elems = xpalloc (NULL, &m->alloc, s2->nelem, -1, sizeof *m->elems);
    }
  m->nelem = 0;
  while (i < s1->nelem || j < s2->nelem)
    if (! (j < s2->nelem)
        || (i < s1->nelem && s1->elems[i].index <= s2->elems[j].index))
      {
        unsigned int c = ((i < s1->nelem && j < s2->nelem
                           && s1->elems[i].index == s2->elems[j].index)
                          ? s2->elems[j++].constraint & c2
                          : 0);
        m->elems[m->nelem].index = s1->elems[i].index;
        m->elems[m->nelem++].constraint = s1->elems[i++].constraint | c;
      }
    else
      {
        if (s2->elems[j].constraint & c2)
          {
            m->elems[m->nelem].index = s2->elems[j].index;
            m->elems[m->nelem++].constraint = s2->elems[j].constraint & c2;
          }
        j++;
      }
}

/* Merge two sets of positions into a third.  The result is exactly as if
   the positions of both sets were inserted into an initially empty set.  */
static void
merge (position_set const *s1, position_set const *s2, position_set *m)
{
  merge_constrained (s1, s2, -1, m);
}

/* Merge into DST all the elements of SRC, possibly destroying
   the contents of the temporary M.  */
static void
merge2 (position_set *dst, position_set const *src, position_set *m)
{
  if (src->nelem < 4)
    {
      for (idx_t i = 0; i < src->nelem; i++)
        insert (src->elems[i], dst);
    }
   else
    {
      merge (src, dst, m);
      copy (m, dst);
    }
}

/* Delete a position from a set.  Return the nonzero constraint of the
   deleted position, or zero if there was no such position.  */
static unsigned int
delete (idx_t del, position_set *s)
{
  idx_t count = s->nelem;
  idx_t lo = 0, hi = count;
  while (lo < hi)
    {
      idx_t mid = (lo + hi) >> 1;
      if (s->elems[mid].index < del)
        lo = mid + 1;
      else if (s->elems[mid].index == del)
        {
          unsigned int c = s->elems[mid].constraint;
          idx_t i;
          for (i = mid; i + 1 < count; i++)
            s->elems[i] = s->elems[i + 1];
          s->nelem = i;
          return c;
        }
      else
        hi = mid;
    }
  return 0;
}

/* Replace a position with the followed set.  */
static void
replace (position_set *dst, idx_t del, position_set *add,
         unsigned int constraint, position_set *tmp)
{
  unsigned int c = delete (del, dst) & constraint;

  if (c)
    {
      copy (dst, tmp);
      merge_constrained (tmp, add, c, dst);
    }
}

/* Find the index of the state corresponding to the given position set with
   the given preceding context, or create a new state if there is no such
   state.  Context tells whether we got here on a newline or letter.  */
static state_num
state_index (struct dfa *d, position_set const *s, int context)
{
  size_t hash = 0;
  int constraint = 0;
  state_num i;

  for (i = 0; i < s->nelem; ++i)
    {
      idx_t ind = s->elems[i].index;
      hash ^= ind + s->elems[i].constraint;
    }

  /* Try to find a state that exactly matches the proposed one.  */
  for (i = 0; i < d->sindex; ++i)
    {
      if (hash != d->states[i].hash || s->nelem != d->states[i].elems.nelem
          || context != d->states[i].context)
        continue;
      state_num j;
      for (j = 0; j < s->nelem; ++j)
        if (s->elems[j].constraint != d->states[i].elems.elems[j].constraint
            || s->elems[j].index != d->states[i].elems.elems[j].index)
          break;
      if (j == s->nelem)
        return i;
    }

#ifdef DEBUG
  fprintf (stderr, "new state %td\n nextpos:", i);
  for (state_num j = 0; j < s->nelem; j++)
    {
      fprintf (stderr, " %td:", s->elems[j].index);
      prtok (d->tokens[s->elems[j].index]);
    }
  fprintf (stderr, "\n context:");
  if (context ^ CTX_ANY)
    {
      if (context & CTX_NONE)
        fprintf (stderr, " CTX_NONE");
      if (context & CTX_LETTER)
        fprintf (stderr, " CTX_LETTER");
      if (context & CTX_NEWLINE)
        fprintf (stderr, " CTX_NEWLINE");
    }
  else
    fprintf (stderr, " CTX_ANY");
  fprintf (stderr, "\n");
#endif

  for (state_num j = 0; j < s->nelem; j++)
    {
      int c = d->constraints[s->elems[j].index];

      if (c != 0)
        {
          if (succeeds_in_context (c, context, CTX_ANY))
            constraint |= c;
        }
      else if (d->tokens[s->elems[j].index] == BACKREF)
        constraint = NO_CONSTRAINT;
    }


  /* Create a new state.  */
  d->states = maybe_realloc (d->states, d->sindex, &d->salloc, -1,
                             sizeof *d->states);
  d->states[i].hash = hash;
  alloc_position_set (&d->states[i].elems, s->nelem);
  copy (s, &d->states[i].elems);
  d->states[i].context = context;
  d->states[i].constraint = constraint;
  d->states[i].mbps.nelem = 0;
  d->states[i].mbps.elems = NULL;
  d->states[i].mb_trindex = -1;

  ++d->sindex;

  return i;
}

/* Find the epsilon closure of D's set of positions.  If any position of the set
   contains a symbol that matches the empty string in some context, replace
   that position with the elements of its follow labeled with an appropriate
   constraint.  Repeat exhaustively until no funny positions are left.
   S->elems must be large enough to hold the result.  BACKWARD is D's
   backward set; use and update it too.  */
static void
epsclosure (struct dfa const *d, position_set *backward)
{
  position_set tmp;
  alloc_position_set (&tmp, d->nleaves);
  for (idx_t i = 0; i < d->tindex; i++)
    if (0 < d->follows[i].nelem)
      {
        unsigned int constraint;
        switch (d->tokens[i])
          {
          default:
            continue;

          case BEGLINE:
            constraint = BEGLINE_CONSTRAINT;
            break;
          case ENDLINE:
            constraint = ENDLINE_CONSTRAINT;
            break;
          case BEGWORD:
            constraint = BEGWORD_CONSTRAINT;
            break;
          case ENDWORD:
            constraint = ENDWORD_CONSTRAINT;
            break;
          case LIMWORD:
            constraint = LIMWORD_CONSTRAINT;
            break;
          case NOTLIMWORD:
            constraint = NOTLIMWORD_CONSTRAINT;
            break;
          case EMPTY:
            constraint = NO_CONSTRAINT;
            break;
          }

        delete (i, &d->follows[i]);

        for (idx_t j = 0; j < backward[i].nelem; j++)
          replace (&d->follows[backward[i].elems[j].index], i, &d->follows[i],
                   constraint, &tmp);
        for (idx_t j = 0; j < d->follows[i].nelem; j++)
          replace (&backward[d->follows[i].elems[j].index], i, &backward[i],
                   NO_CONSTRAINT, &tmp);
      }
  free (tmp.elems);
}

/* Returns the set of contexts for which there is at least one
   character included in C.  */

static int
charclass_context (struct dfa const *dfa, charclass const *c)
{
  int context = 0;

  for (int j = 0; j < CHARCLASS_WORDS; j++)
    {
      if (c->w[j] & dfa->syntax.newline.w[j])
        context |= CTX_NEWLINE;
      if (c->w[j] & dfa->syntax.letters.w[j])
        context |= CTX_LETTER;
      if (c->w[j] & ~(dfa->syntax.letters.w[j] | dfa->syntax.newline.w[j]))
        context |= CTX_NONE;
    }

  return context;
}

/* Returns the contexts on which the position set S depends.  Each context
   in the set of returned contexts (let's call it SC) may have a different
   follow set than other contexts in SC, and also different from the
   follow set of the complement set (sc ^ CTX_ANY).  However, all contexts
   in the complement set will have the same follow set.  */

static int _GL_ATTRIBUTE_PURE
state_separate_contexts (struct dfa *d, position_set const *s)
{
  int separate_contexts = 0;

  for (idx_t j = 0; j < s->nelem; j++)
    separate_contexts |= d->separates[s->elems[j].index];

  return separate_contexts;
}

enum
{
  /* Single token is repeated.  It is distinguished from non-repeated.  */
  OPT_REPEAT = (1 << 0),

  /* Multiple tokens are repeated.  This flag is on at head of tokens.  The
     node is not merged.  */
  OPT_LPAREN = (1 << 1),

  /* Multiple branches are joined.  The node is not merged.  */
  OPT_RPAREN = (1 << 2),

  /* The node is walked.  If the node is found in walking again, OPT_RPAREN
     flag is turned on. */
  OPT_WALKED = (1 << 3),

  /* The node is queued.  The node is not queued again.  */
  OPT_QUEUED = (1 << 4)
};

static void
merge_nfa_state (struct dfa *d, idx_t tindex, char *flags,
                 position_set *merged)
{
  position_set *follows = d->follows;
  idx_t nelem = 0;

  for (idx_t i = 0; i < follows[tindex].nelem; i++)
    {
      idx_t sindex = follows[tindex].elems[i].index;

      /* Skip the node as pruned in future.  */
      unsigned int iconstraint = follows[tindex].elems[i].constraint;
      if (iconstraint == 0)
        continue;

      if (d->tokens[follows[tindex].elems[i].index] <= END)
        {
          d->constraints[tindex] |= follows[tindex].elems[i].constraint;
          continue;
        }

      if (sindex != tindex && !(flags[sindex] & (OPT_LPAREN | OPT_RPAREN)))
        {
          idx_t j;

          for (j = 0; j < nelem; j++)
            {
              idx_t dindex = follows[tindex].elems[j].index;

              if (dindex == tindex)
                continue;

              if (follows[tindex].elems[j].constraint != iconstraint)
                continue;

              if (flags[dindex] & (OPT_LPAREN | OPT_RPAREN))
                continue;

              if (d->tokens[sindex] != d->tokens[dindex])
                continue;

              if ((flags[sindex] ^ flags[dindex]) & OPT_REPEAT)
                continue;

              if (flags[sindex] & OPT_REPEAT)
                delete (sindex, &follows[sindex]);

              merge2 (&follows[dindex], &follows[sindex], merged);

              break;
            }

          if (j < nelem)
            continue;
        }

      follows[tindex].elems[nelem++] = follows[tindex].elems[i];
      flags[sindex] |= OPT_QUEUED;
    }

  follows[tindex].nelem = nelem;
}

static int
compare (const void *a, const void *b)
{
  position const *p = a, *q = b;
  return (p->index > q->index) - (p->index < q->index);
}

static void
reorder_tokens (struct dfa *d)
{
  idx_t nleaves = 0;
  ptrdiff_t *map = xnmalloc (d->tindex, sizeof *map);
  map[0] = nleaves++;
  for (idx_t i = 1; i < d->tindex; i++)
    map[i] = -1;

  token *tokens = xnmalloc (d->nleaves, sizeof *tokens);
  position_set *follows = xnmalloc (d->nleaves, sizeof *follows);
  int *constraints = xnmalloc (d->nleaves, sizeof *constraints);
  char *multibyte_prop = (d->localeinfo.multibyte
                          ? xnmalloc (d->nleaves, sizeof *multibyte_prop)
                          : NULL);

  for (idx_t i = 0; i < d->tindex; i++)
    {
      if (map[i] < 0)
        {
          free (d->follows[i].elems);
          d->follows[i].elems = NULL;
          d->follows[i].nelem = 0;
          continue;
        }

      tokens[map[i]] = d->tokens[i];
      follows[map[i]] = d->follows[i];
      constraints[map[i]] = d->constraints[i];

      if (multibyte_prop != NULL)
        multibyte_prop[map[i]] = d->multibyte_prop[i];

      for (idx_t j = 0; j < d->follows[i].nelem; j++)
        {
          if (map[d->follows[i].elems[j].index] == -1)
            map[d->follows[i].elems[j].index] = nleaves++;

          d->follows[i].elems[j].index = map[d->follows[i].elems[j].index];
        }

      qsort (d->follows[i].elems, d->follows[i].nelem,
             sizeof *d->follows[i].elems, compare);
    }

  for (idx_t i = 0; i < nleaves; i++)
    {
      d->tokens[i] = tokens[i];
      d->follows[i] = follows[i];
      d->constraints[i] = constraints[i];

      if (multibyte_prop != NULL)
        d->multibyte_prop[i] = multibyte_prop[i];
    }

  d->tindex = d->nleaves = nleaves;

  free (tokens);
  free (follows);
  free (constraints);
  free (multibyte_prop);
  free (map);
}

static void
dfaoptimize (struct dfa *d)
{
  char *flags = xizalloc (d->tindex);

  for (idx_t i = 0; i < d->tindex; i++)
    {
      for (idx_t j = 0; j < d->follows[i].nelem; j++)
        {
          if (d->follows[i].elems[j].index == i)
            flags[d->follows[i].elems[j].index] |= OPT_REPEAT;
          else if (d->follows[i].elems[j].index < i)
            flags[d->follows[i].elems[j].index] |= OPT_LPAREN;
          else if (flags[d->follows[i].elems[j].index] &= OPT_WALKED)
            flags[d->follows[i].elems[j].index] |= OPT_RPAREN;
          else
            flags[d->follows[i].elems[j].index] |= OPT_WALKED;
        }
    }

  flags[0] |= OPT_QUEUED;

  position_set merged0;
  position_set *merged = &merged0;
  alloc_position_set (merged, d->nleaves);

  d->constraints = xicalloc (d->tindex, sizeof *d->constraints);

  for (idx_t i = 0; i < d->tindex; i++)
    if (flags[i] & OPT_QUEUED)
      merge_nfa_state (d, i, flags, merged);

  reorder_tokens (d);

  free (merged->elems);
  free (flags);
}

/* Perform bottom-up analysis on the parse tree, computing various functions.
   Note that at this point, we're pretending constructs like \< are real
   characters rather than constraints on what can follow them.

   Nullable:  A node is nullable if it is at the root of a regexp that can
   match the empty string.
   *  EMPTY leaves are nullable.
   * No other leaf is nullable.
   * A QMARK or STAR node is nullable.
   * A PLUS node is nullable if its argument is nullable.
   * A CAT node is nullable if both its arguments are nullable.
   * An OR node is nullable if either argument is nullable.

   Firstpos:  The firstpos of a node is the set of positions (nonempty leaves)
   that could correspond to the first character of a string matching the
   regexp rooted at the given node.
   * EMPTY leaves have empty firstpos.
   * The firstpos of a nonempty leaf is that leaf itself.
   * The firstpos of a QMARK, STAR, or PLUS node is the firstpos of its
     argument.
   * The firstpos of a CAT node is the firstpos of the left argument, union
     the firstpos of the right if the left argument is nullable.
   * The firstpos of an OR node is the union of firstpos of each argument.

   Lastpos:  The lastpos of a node is the set of positions that could
   correspond to the last character of a string matching the regexp at
   the given node.
   * EMPTY leaves have empty lastpos.
   * The lastpos of a nonempty leaf is that leaf itself.
   * The lastpos of a QMARK, STAR, or PLUS node is the lastpos of its
     argument.
   * The lastpos of a CAT node is the lastpos of its right argument, union
     the lastpos of the left if the right argument is nullable.
   * The lastpos of an OR node is the union of the lastpos of each argument.

   Follow:  The follow of a position is the set of positions that could
   correspond to the character following a character matching the node in
   a string matching the regexp.  At this point we consider special symbols
   that match the empty string in some context to be just normal characters.
   Later, if we find that a special symbol is in a follow set, we will
   replace it with the elements of its follow, labeled with an appropriate
   constraint.
   * Every node in the firstpos of the argument of a STAR or PLUS node is in
     the follow of every node in the lastpos.
   * Every node in the firstpos of the second argument of a CAT node is in
     the follow of every node in the lastpos of the first argument.

   Because of the postfix representation of the parse tree, the depth-first
   analysis is conveniently done by a linear scan with the aid of a stack.
   Sets are stored as arrays of the elements, obeying a stack-like allocation
   scheme; the number of elements in each set deeper in the stack can be
   used to determine the address of a particular set's array.  */
static void
dfaanalyze (struct dfa *d, bool searchflag)
{
  /* Array allocated to hold position sets.  */
  position *posalloc = xnmalloc (d->nleaves, 2 * sizeof *posalloc);
  /* Firstpos and lastpos elements.  */
  position *firstpos = posalloc;
  position *lastpos = firstpos + d->nleaves;
  position pos;
  position_set tmp;

  /* Stack for element counts and nullable flags.  */
  struct
  {
    /* Whether the entry is nullable.  */
    bool nullable;

    /* Counts of firstpos and lastpos sets.  */
    idx_t nfirstpos;
    idx_t nlastpos;
  } *stkalloc = xnmalloc (d->depth, sizeof *stkalloc), *stk = stkalloc;

  position_set merged;          /* Result of merging sets.  */

  addtok (d, CAT);
  idx_t tindex = d->tindex;

#ifdef DEBUG
  fprintf (stderr, "dfaanalyze:\n");
  for (idx_t i = 0; i < tindex; i++)
    {
      fprintf (stderr, " %td:", i);
      prtok (d->tokens[i]);
    }
  putc ('\n', stderr);
#endif

  d->searchflag = searchflag;
  alloc_position_set (&merged, d->nleaves);
  d->follows = xicalloc (tindex, sizeof *d->follows);
  position_set *backward
    = d->epsilon ? xicalloc (tindex, sizeof *backward) : NULL;

  for (idx_t i = 0; i < tindex; i++)
    {
      switch (d->tokens[i])
        {
        case EMPTY:
          /* The empty set is nullable.  */
          stk->nullable = true;

          /* The firstpos and lastpos of the empty leaf are both empty.  */
          stk->nfirstpos = stk->nlastpos = 0;
          stk++;
          break;

        case STAR:
        case PLUS:
          /* Every element in the lastpos of the argument is in the backward
             set of every element in the firstpos.  */
          if (d->epsilon)
            {
              tmp.elems = lastpos - stk[-1].nlastpos;
              tmp.nelem = stk[-1].nlastpos;
              for (position *p = firstpos - stk[-1].nfirstpos;
                   p < firstpos; p++)
                merge2 (&backward[p->index], &tmp, &merged);
            }

          /* Every element in the firstpos of the argument is in the follow
             of every element in the lastpos.  */
          {
            tmp.elems = firstpos - stk[-1].nfirstpos;
            tmp.nelem = stk[-1].nfirstpos;
            for (position *p = lastpos - stk[-1].nlastpos; p < lastpos; p++)
              merge2 (&d->follows[p->index], &tmp, &merged);
          }
          FALLTHROUGH;
        case QMARK:
          /* A QMARK or STAR node is automatically nullable.  */
          if (d->tokens[i] != PLUS)
            stk[-1].nullable = true;
          break;

        case CAT:
          /* Every element in the lastpos of the first argument is in
             the backward set of every element in the firstpos of the
             second argument.  */
          if (backward)
            {
              tmp.nelem = stk[-2].nlastpos;
              tmp.elems = lastpos - stk[-1].nlastpos - stk[-2].nlastpos;
              for (position *p = firstpos - stk[-1].nfirstpos;
                   p < firstpos; p++)
                merge2 (&backward[p->index], &tmp, &merged);
            }

          /* Every element in the firstpos of the second argument is in the
             follow of every element in the lastpos of the first argument.  */
          {
            tmp.nelem = stk[-1].nfirstpos;
            tmp.elems = firstpos - stk[-1].nfirstpos;
            for (position *plim = lastpos - stk[-1].nlastpos,
                   *p = plim - stk[-2].nlastpos;
                 p < plim; p++)
              merge2 (&d->follows[p->index], &tmp, &merged);
          }

          /* The firstpos of a CAT node is the firstpos of the first argument,
             union that of the second argument if the first is nullable.  */
          if (stk[-2].nullable)
            stk[-2].nfirstpos += stk[-1].nfirstpos;
          else
            firstpos -= stk[-1].nfirstpos;

          /* The lastpos of a CAT node is the lastpos of the second argument,
             union that of the first argument if the second is nullable.  */
          if (stk[-1].nullable)
            stk[-2].nlastpos += stk[-1].nlastpos;
          else
            {
              position *p = lastpos - stk[-1].nlastpos - stk[-2].nlastpos;
              for (idx_t j = 0; j < stk[-1].nlastpos; j++)
                p[j] = p[j + stk[-2].nlastpos];
              lastpos -= stk[-2].nlastpos;
              stk[-2].nlastpos = stk[-1].nlastpos;
            }

          /* A CAT node is nullable if both arguments are nullable.  */
          stk[-2].nullable &= stk[-1].nullable;
          stk--;
          break;

        case OR:
          /* The firstpos is the union of the firstpos of each argument.  */
          stk[-2].nfirstpos += stk[-1].nfirstpos;

          /* The lastpos is the union of the lastpos of each argument.  */
          stk[-2].nlastpos += stk[-1].nlastpos;

          /* An OR node is nullable if either argument is nullable.  */
          stk[-2].nullable |= stk[-1].nullable;
          stk--;
          break;

        default:
          /* Anything else is a nonempty position.  (Note that special
             constructs like \< are treated as nonempty strings here;
             an "epsilon closure" effectively makes them nullable later.
             Backreferences have to get a real position so we can detect
             transitions on them later.  But they are nullable.  */
          stk->nullable = d->tokens[i] == BACKREF;

          /* This position is in its own firstpos and lastpos.  */
          stk->nfirstpos = stk->nlastpos = 1;
          stk++;

          firstpos->index = lastpos->index = i;
          firstpos->constraint = lastpos->constraint = NO_CONSTRAINT;
          firstpos++, lastpos++;

          break;
        }
#ifdef DEBUG
      /* ... balance the above nonsyntactic #ifdef goo...  */
      fprintf (stderr, "node %td:", i);
      prtok (d->tokens[i]);
      putc ('\n', stderr);
      fprintf (stderr,
               stk[-1].nullable ? " nullable: yes\n" : " nullable: no\n");
      fprintf (stderr, " firstpos:");
      for (idx_t j = 0; j < stk[-1].nfirstpos; j++)
        {
          fprintf (stderr, " %td:", firstpos[j - stk[-1].nfirstpos].index);
          prtok (d->tokens[firstpos[j - stk[-1].nfirstpos].index]);
        }
      fprintf (stderr, "\n lastpos:");
      for (idx_t j = 0; j < stk[-1].nlastpos; j++)
        {
          fprintf (stderr, " %td:", lastpos[j - stk[-1].nlastpos].index);
          prtok (d->tokens[lastpos[j - stk[-1].nlastpos].index]);
        }
      putc ('\n', stderr);
#endif
    }

  if (backward)
    {
      /* For each follow set that is the follow set of a real position,
         replace it with its epsilon closure.  */
      epsclosure (d, backward);

      for (idx_t i = 0; i < tindex; i++)
        free (backward[i].elems);
      free (backward);
    }

  dfaoptimize (d);

#ifdef DEBUG
  for (idx_t i = 0; i < tindex; i++)
    if (d->tokens[i] == BEG || d->tokens[i] < NOTCHAR
        || d->tokens[i] == BACKREF || d->tokens[i] == ANYCHAR
        || d->tokens[i] == MBCSET || d->tokens[i] >= CSET)
      {
        fprintf (stderr, "follows(%td:", i);
        prtok (d->tokens[i]);
        fprintf (stderr, "):");
        for (idx_t j = 0; j < d->follows[i].nelem; j++)
          {
            fprintf (stderr, " %td:", d->follows[i].elems[j].index);
            prtok (d->tokens[d->follows[i].elems[j].index]);
          }
        putc ('\n', stderr);
      }
#endif

  pos.index = 0;
  pos.constraint = NO_CONSTRAINT;

  alloc_position_set (&tmp, 1);

  append (pos, &tmp);

  d->separates = xicalloc (tindex, sizeof *d->separates);

  for (idx_t i = 0; i < tindex; i++)
    {
      if (prev_newline_dependent (d->constraints[i]))
        d->separates[i] |= CTX_NEWLINE;
      if (prev_letter_dependent (d->constraints[i]))
        d->separates[i] |= CTX_LETTER;

      for (idx_t j = 0; j < d->follows[i].nelem; j++)
        {
          if (prev_newline_dependent (d->follows[i].elems[j].constraint))
            d->separates[i] |= CTX_NEWLINE;
          if (prev_letter_dependent (d->follows[i].elems[j].constraint))
            d->separates[i] |= CTX_LETTER;
        }
    }

  /* Context wanted by some position.  */
  int separate_contexts = state_separate_contexts (d, &tmp);

  /* Build the initial state.  */
  if (separate_contexts & CTX_NEWLINE)
    state_index (d, &tmp, CTX_NEWLINE);
  d->initstate_notbol = d->min_trcount
    = state_index (d, &tmp, separate_contexts ^ CTX_ANY);
  if (separate_contexts & CTX_LETTER)
    d->min_trcount = state_index (d, &tmp, CTX_LETTER);
  d->min_trcount++;
  d->trcount = 0;

  free (posalloc);
  free (stkalloc);
  free (merged.elems);
  free (tmp.elems);
}

/* Make sure D's state arrays are large enough to hold NEW_STATE.  */
static void
realloc_trans_if_necessary (struct dfa *d)
{
  state_num oldalloc = d->tralloc;
  if (oldalloc < d->sindex)
    {
      state_num **realtrans = d->trans ? d->trans - 2 : NULL;
      idx_t newalloc1 = realtrans ? d->tralloc + 2 : 0;
      realtrans = xpalloc (realtrans, &newalloc1, d->sindex - oldalloc,
                           -1, sizeof *realtrans);
      realtrans[0] = realtrans[1] = NULL;
      d->trans = realtrans + 2;
      idx_t newalloc = d->tralloc = newalloc1 - 2;
      d->fails = xreallocarray (d->fails, newalloc, sizeof *d->fails);
      d->success = xreallocarray (d->success, newalloc, sizeof *d->success);
      d->newlines = xreallocarray (d->newlines, newalloc, sizeof *d->newlines);
      if (d->localeinfo.multibyte)
        {
          realtrans = d->mb_trans ? d->mb_trans - 2 : NULL;
          realtrans = xreallocarray (realtrans, newalloc1, sizeof *realtrans);
          if (oldalloc == 0)
            realtrans[0] = realtrans[1] = NULL;
          d->mb_trans = realtrans + 2;
        }
      for (; oldalloc < newalloc; oldalloc++)
        {
          d->trans[oldalloc] = NULL;
          d->fails[oldalloc] = NULL;
          if (d->localeinfo.multibyte)
            d->mb_trans[oldalloc] = NULL;
        }
    }
}

/*
   Calculate the transition table for a new state derived from state s
   for a compiled dfa d after input character uc, and return the new
   state number.

   Do not worry about all possible input characters; calculate just the group
   of positions that match uc.  Label it with the set of characters that
   every position in the group matches (taking into account, if necessary,
   preceding context information of s).  Then find the union
   of these positions' follows, i.e., the set of positions of the
   new state.  For each character in the group's label, set the transition
   on this character to be to a state corresponding to the set's positions,
   and its associated backward context information, if necessary.

   When building a searching matcher, include the positions of state
   0 in every state.

   The group is constructed by building an equivalence-class
   partition of the positions of s.

   For each position, find the set of characters C that it matches.  Eliminate
   any characters from C that fail on grounds of backward context.

   Check whether the group's label L has nonempty
   intersection with C.  If L - C is nonempty, create a new group labeled
   L - C and having the same positions as the current group, and set L to
   the intersection of L and C.  Insert the position in the group, set
   C = C - L, and resume scanning.

   If after comparing with every group there are characters remaining in C,
   create a new group labeled with the characters of C and insert this
   position in that group.  */

static state_num
build_state (state_num s, struct dfa *d, unsigned char uc)
{
  position_set follows;         /* Union of the follows for each
                                   position of the current state.  */
  position_set group;           /* Positions that match the input char.  */
  position_set tmp;             /* Temporary space for merging sets.  */
  state_num state;              /* New state.  */
  state_num state_newline;      /* New state on a newline transition.  */
  state_num state_letter;       /* New state on a letter transition.  */

#ifdef DEBUG
  fprintf (stderr, "build state %td\n", s);
#endif

  /* A pointer to the new transition table, and the table itself.  */
  state_num **ptrans = (accepting (s, d) ? d->fails : d->trans) + s;
  state_num *trans = *ptrans;

  if (!trans)
    {
      /* MAX_TRCOUNT is an arbitrary upper limit on the number of
         transition tables that can exist at once, other than for
         initial states.  Often-used transition tables are quickly
         rebuilt, whereas rarely-used ones are cleared away.  */
      if (MAX_TRCOUNT <= d->trcount)
        {
          for (state_num i = d->min_trcount; i < d->tralloc; i++)
            {
              free (d->trans[i]);
              free (d->fails[i]);
              d->trans[i] = d->fails[i] = NULL;
            }
          d->trcount = 0;
        }

      d->trcount++;
      *ptrans = trans = xmalloc (NOTCHAR * sizeof *trans);

      /* Fill transition table with a default value which means that the
         transited state has not been calculated yet.  */
      for (int i = 0; i < NOTCHAR; i++)
        trans[i] = -2;
    }

  /* Set up the success bits for this state.  */
  d->success[s] = 0;
  if (accepts_in_context (d->states[s].context, CTX_NEWLINE, s, d))
    d->success[s] |= CTX_NEWLINE;
  if (accepts_in_context (d->states[s].context, CTX_LETTER, s, d))
    d->success[s] |= CTX_LETTER;
  if (accepts_in_context (d->states[s].context, CTX_NONE, s, d))
    d->success[s] |= CTX_NONE;

  alloc_position_set (&follows, d->nleaves);

  /* Find the union of the follows of the positions of the group.
     This is a hideously inefficient loop.  Fix it someday.  */
  for (idx_t j = 0; j < d->states[s].elems.nelem; j++)
    for (idx_t k = 0;
         k < d->follows[d->states[s].elems.elems[j].index].nelem; ++k)
      insert (d->follows[d->states[s].elems.elems[j].index].elems[k],
              &follows);

  /* Positions that match the input char.  */
  alloc_position_set (&group, d->nleaves);

  /* The group's label.  */
  charclass label;
  fillset (&label);

  for (idx_t i = 0; i < follows.nelem; i++)
    {
      charclass matches;            /* Set of matching characters.  */
      position pos = follows.elems[i];
      bool matched = false;
      if (d->tokens[pos.index] >= 0 && d->tokens[pos.index] < NOTCHAR)
        {
          zeroset (&matches);
          setbit (d->tokens[pos.index], &matches);
          if (d->tokens[pos.index] == uc)
            matched = true;
        }
      else if (d->tokens[pos.index] >= CSET)
        {
          matches = d->charclasses[d->tokens[pos.index] - CSET];
          if (tstbit (uc, &matches))
            matched = true;
        }
      else if (d->tokens[pos.index] == ANYCHAR)
        {
          matches = d->charclasses[d->canychar];
          if (tstbit (uc, &matches))
            matched = true;

          /* ANYCHAR must match with a single character, so we must put
             it to D->states[s].mbps which contains the positions which
             can match with a single character not a byte.  If all
             positions which has ANYCHAR does not depend on context of
             next character, we put the follows instead of it to
             D->states[s].mbps to optimize.  */
          if (succeeds_in_context (pos.constraint, d->states[s].context,
                                   CTX_NONE))
            {
              if (d->states[s].mbps.nelem == 0)
                alloc_position_set (&d->states[s].mbps, 1);
              insert (pos, &d->states[s].mbps);
            }
        }
      else
        continue;

      /* Some characters may need to be eliminated from matches because
         they fail in the current context.  */
      if (pos.constraint != NO_CONSTRAINT)
        {
          if (!succeeds_in_context (pos.constraint,
                                    d->states[s].context, CTX_NEWLINE))
            for (int j = 0; j < CHARCLASS_WORDS; j++)
              matches.w[j] &= ~d->syntax.newline.w[j];
          if (!succeeds_in_context (pos.constraint,
                                    d->states[s].context, CTX_LETTER))
            for (int j = 0; j < CHARCLASS_WORDS; ++j)
              matches.w[j] &= ~d->syntax.letters.w[j];
          if (!succeeds_in_context (pos.constraint,
                                    d->states[s].context, CTX_NONE))
            for (int j = 0; j < CHARCLASS_WORDS; ++j)
              matches.w[j] &= d->syntax.letters.w[j] | d->syntax.newline.w[j];

          /* If there are no characters left, there's no point in going on.  */
          if (emptyset (&matches))
            continue;

          /* If we have reset the bit that made us declare "matched", reset
             that indicator, too.  This is required to avoid an infinite loop
             with this command: echo cx | LC_ALL=C grep -E 'c\b[x ]'  */
          if (!tstbit (uc, &matches))
            matched = false;
        }

#ifdef DEBUG
      fprintf (stderr, " nextpos %td:", pos.index);
      prtok (d->tokens[pos.index]);
      fprintf (stderr, " of");
      for (unsigned j = 0; j < NOTCHAR; j++)
        if (tstbit (j, &matches))
          fprintf (stderr, " 0x%02x", j);
      fprintf (stderr, "\n");
#endif

      if (matched)
        {
          for (int k = 0; k < CHARCLASS_WORDS; ++k)
            label.w[k] &= matches.w[k];
          append (pos, &group);
        }
      else
        {
          for (int k = 0; k < CHARCLASS_WORDS; ++k)
            label.w[k] &= ~matches.w[k];
        }
    }

  alloc_position_set (&tmp, d->nleaves);

  if (group.nelem > 0)
    {
      /* If we are building a searching matcher, throw in the positions
         of state 0 as well, if possible.  */
      if (d->searchflag)
        {
          /* If a token in follows.elems is not 1st byte of a multibyte
             character, or the states of follows must accept the bytes
             which are not 1st byte of the multibyte character.
             Then, if a state of follows encounters a byte, it must not be
             a 1st byte of a multibyte character nor a single byte character.
             In this case, do not add state[0].follows to next state, because
             state[0] must accept 1st-byte.

             For example, suppose <sb a> is a certain single byte character,
             <mb A> is a certain multibyte character, and the codepoint of
             <sb a> equals the 2nd byte of the codepoint of <mb A>.  When
             state[0] accepts <sb a>, state[i] transits to state[i+1] by
             accepting the 1st byte of <mb A>, and state[i+1] accepts the
             2nd byte of <mb A>, if state[i+1] encounters the codepoint of
             <sb a>, it must not be <sb a> but the 2nd byte of <mb A>, so do
             not add state[0].  */

          bool mergeit = !d->localeinfo.multibyte;
          if (!mergeit)
            {
              mergeit = true;
              for (idx_t j = 0; mergeit && j < group.nelem; j++)
                mergeit &= d->multibyte_prop[group.elems[j].index];
            }
          if (mergeit)
            merge2 (&group, &d->states[0].elems, &tmp);
        }

      /* Find out if the new state will want any context information,
         by calculating possible contexts that the group can match,
         and separate contexts that the new state wants to know.  */
      int possible_contexts = charclass_context (d, &label);
      int separate_contexts = state_separate_contexts (d, &group);

      /* Find the state(s) corresponding to the union of the follows.  */
      if (possible_contexts & ~separate_contexts)
        state = state_index (d, &group, separate_contexts ^ CTX_ANY);
      else
        state = -1;
      if (separate_contexts & possible_contexts & CTX_NEWLINE)
        state_newline = state_index (d, &group, CTX_NEWLINE);
      else
        state_newline = state;
      if (separate_contexts & possible_contexts & CTX_LETTER)
        state_letter = state_index (d, &group, CTX_LETTER);
      else
        state_letter = state;

      /* Reallocate now, to reallocate any newline transition properly.  */
      realloc_trans_if_necessary (d);
    }

  /* If we are a searching matcher, the default transition is to a state
     containing the positions of state 0, otherwise the default transition
     is to fail miserably.  */
  else if (d->searchflag)
    {
      state_newline = 0;
      state_letter = d->min_trcount - 1;
      state = d->initstate_notbol;
    }
  else
    {
      state_newline = -1;
      state_letter = -1;
      state = -1;
    }

  /* Set the transitions for each character in the label.  */
  for (int i = 0; i < NOTCHAR; i++)
    if (tstbit (i, &label))
      switch (d->syntax.sbit[i])
        {
        case CTX_NEWLINE:
          trans[i] = state_newline;
          break;
        case CTX_LETTER:
          trans[i] = state_letter;
          break;
        default:
          trans[i] = state;
          break;
        }

#ifdef DEBUG
  fprintf (stderr, "trans table %td", s);
  for (int i = 0; i < NOTCHAR; ++i)
    {
      if (!(i & 0xf))
        fprintf (stderr, "\n");
      fprintf (stderr, " %2td", trans[i]);
    }
  fprintf (stderr, "\n");
#endif

  free (group.elems);
  free (follows.elems);
  free (tmp.elems);

  /* Keep the newline transition in a special place so we can use it as
     a sentinel.  */
  if (tstbit (d->syntax.eolbyte, &label))
    {
      d->newlines[s] = trans[d->syntax.eolbyte];
      trans[d->syntax.eolbyte] = -1;
    }

  return trans[uc];
}

/* Multibyte character handling sub-routines for dfaexec.  */

/* Consume a single byte and transit state from 's' to '*next_state'.
   This is almost the same as the state transition routine in dfaexec.
   But the state transition is done just once; otherwise, matching succeeds or
   we reach the end of the buffer.  */
static state_num
transit_state_singlebyte (struct dfa *d, state_num s, unsigned char const **pp)
{
  state_num *t;

  if (d->trans[s])
    t = d->trans[s];
  else if (d->fails[s])
    t = d->fails[s];
  else
    {
      build_state (s, d, **pp);
      if (d->trans[s])
        t = d->trans[s];
      else
        {
          t = d->fails[s];
          assert (t);
        }
    }

  if (t[**pp] == -2)
    build_state (s, d, **pp);

  return t[*(*pp)++];
}

/* Transit state from s, then return new state and update the pointer of
   the buffer.  This function is for a period operator which can match a
   multi-byte character.  */
static state_num
transit_state (struct dfa *d, state_num s, unsigned char const **pp,
               unsigned char const *end)
{
  wint_t wc;

  int mbclen = mbs_to_wchar (&wc, (char const *) *pp, end - *pp, d);

  /* This state has some operators which can match a multibyte character.  */
  d->mb_follows.nelem = 0;

  /* Calculate the state which can be reached from the state 's' by
     consuming 'mbclen' single bytes from the buffer.  */
  state_num s1 = s;
  int mbci;
  for (mbci = 0; mbci < mbclen && (mbci == 0 || d->min_trcount <= s); mbci++)
    s = transit_state_singlebyte (d, s, pp);
  *pp += mbclen - mbci;

  if (wc == WEOF)
    {
      /* It is an invalid character, so ANYCHAR is not accepted.  */
      return s;
    }

  /* If all positions which have ANYCHAR do not depend on the context
     of the next character, calculate the next state with
     pre-calculated follows and cache the result.  */
  if (d->states[s1].mb_trindex < 0)
    {
      if (MAX_TRCOUNT <= d->mb_trcount)
        {
          state_num s3;
          for (s3 = -1; s3 < d->tralloc; s3++)
            {
              free (d->mb_trans[s3]);
              d->mb_trans[s3] = NULL;
            }

          for (state_num i = 0; i < d->sindex; i++)
            d->states[i].mb_trindex = -1;
          d->mb_trcount = 0;
        }
      d->states[s1].mb_trindex = d->mb_trcount++;
    }

  if (! d->mb_trans[s])
    {
      enum { TRANSPTR_SIZE = sizeof *d->mb_trans[s] };
      enum { TRANSALLOC_SIZE = MAX_TRCOUNT * TRANSPTR_SIZE };
      d->mb_trans[s] = xmalloc (TRANSALLOC_SIZE);
      for (int i = 0; i < MAX_TRCOUNT; i++)
        d->mb_trans[s][i] = -1;
    }
  else if (d->mb_trans[s][d->states[s1].mb_trindex] >= 0)
    return d->mb_trans[s][d->states[s1].mb_trindex];

  if (s == -1)
    copy (&d->states[s1].mbps, &d->mb_follows);
  else
    merge (&d->states[s1].mbps, &d->states[s].elems, &d->mb_follows);

  int separate_contexts = state_separate_contexts (d, &d->mb_follows);
  state_num s2 = state_index (d, &d->mb_follows, separate_contexts ^ CTX_ANY);
  realloc_trans_if_necessary (d);

  d->mb_trans[s][d->states[s1].mb_trindex] = s2;

  return s2;
}

/* The initial state may encounter a byte which is not a single byte character
   nor the first byte of a multibyte character.  But it is incorrect for the
   initial state to accept such a byte.  For example, in Shift JIS the regular
   expression "\\" accepts the codepoint 0x5c, but should not accept the second
   byte of the codepoint 0x815c.  Then the initial state must skip the bytes
   that are not a single byte character nor the first byte of a multibyte
   character.

   Given DFA state d, use mbs_to_wchar to advance MBP until it reaches
   or exceeds P, and return the advanced MBP.  If WCP is non-NULL and
   the result is greater than P, set *WCP to the final wide character
   processed, or to WEOF if no wide character is processed.  Otherwise,
   if WCP is non-NULL, *WCP may or may not be updated.

   Both P and MBP must be no larger than END.  */
static unsigned char const *
skip_remains_mb (struct dfa *d, unsigned char const *p,
                 unsigned char const *mbp, char const *end)
{
  if (d->syntax.never_trail[*p])
    return p;
  while (mbp < p)
    {
      wint_t wc;
      mbp += mbs_to_wchar (&wc, (char const *) mbp,
                           end - (char const *) mbp, d);
    }
  return mbp;
}

/* Search through a buffer looking for a match to the struct dfa *D.
   Find the first occurrence of a string matching the regexp in the
   buffer, and the shortest possible version thereof.  Return a pointer to
   the first character after the match, or NULL if none is found.  BEGIN
   points to the beginning of the buffer, and END points to the first byte
   after its end.  Note however that we store a sentinel byte (usually
   newline) in *END, so the actual buffer must be one byte longer.
   When ALLOW_NL, newlines may appear in the matching string.
   If COUNT is non-NULL, increment *COUNT once for each newline processed.
   If MULTIBYTE, the input consists of multibyte characters and/or
   encoding-error bytes.  Otherwise, it consists of single-byte characters.
   Here is the list of features that make this DFA matcher punt:
    - [M-N] range in non-simple locale: regex is up to 25% faster on [a-z]
    - [^...] in non-simple locale
    - [[=foo=]] or [[.foo.]]
    - [[:alpha:]] etc. in multibyte locale (except [[:digit:]] works OK)
    - back-reference: (.)\1
    - word-delimiter in multibyte locale: \<, \>, \b, \B
   See struct localeinfo.simple for the definition of "simple locale".  */

static inline char *
dfaexec_main (struct dfa *d, char const *begin, char *end, bool allow_nl,
              idx_t *count, bool multibyte)
{
  if (MAX_TRCOUNT <= d->sindex)
    {
      for (state_num s = d->min_trcount; s < d->sindex; s++)
        {
          free (d->states[s].elems.elems);
          free (d->states[s].mbps.elems);
        }
      d->sindex = d->min_trcount;

      if (d->trans)
        {
          for (state_num s = 0; s < d->tralloc; s++)
            {
              free (d->trans[s]);
              free (d->fails[s]);
              d->trans[s] = d->fails[s] = NULL;
            }
          d->trcount = 0;
        }

      if (d->localeinfo.multibyte && d->mb_trans)
        {
          for (state_num s = -1; s < d->tralloc; s++)
            {
              free (d->mb_trans[s]);
              d->mb_trans[s] = NULL;
            }
          for (state_num s = 0; s < d->min_trcount; s++)
            d->states[s].mb_trindex = -1;
          d->mb_trcount = 0;
        }
    }

  if (!d->tralloc)
    realloc_trans_if_necessary (d);

  /* Current state.  */
  state_num s = 0, s1 = 0;

  /* Current input character.  */
  unsigned char const *p = (unsigned char const *) begin;
  unsigned char const *mbp = p;

  /* Copy of d->trans so it can be optimized into a register.  */
  state_num **trans = d->trans;
  unsigned char eol = d->syntax.eolbyte;  /* Likewise for eolbyte.  */
  unsigned char saved_end = *(unsigned char *) end;
  *end = eol;

  if (multibyte)
    {
      mbszero (&d->mbs);
      if (d->mb_follows.alloc == 0)
        alloc_position_set (&d->mb_follows, d->nleaves);
    }

  idx_t nlcount = 0;
  for (;;)
    {
      state_num *t;
      while ((t = trans[s]) != NULL)
        {
          if (s < d->min_trcount)
            {
              if (!multibyte || d->states[s].mbps.nelem == 0)
                {
                  while (t[*p] == s)
                    p++;
                }
              if (multibyte)
                p = mbp = skip_remains_mb (d, p, mbp, end);
            }

          if (multibyte)
            {
              s1 = s;

              if (d->states[s].mbps.nelem == 0
                  || d->localeinfo.sbctowc[*p] != WEOF || (char *) p >= end)
                {
                  /* If an input character does not match ANYCHAR, do it
                     like a single-byte character.  */
                  s = t[*p++];
                }
              else
                {
                  s = transit_state (d, s, &p, (unsigned char *) end);
                  mbp = p;
                  trans = d->trans;
                }
            }
          else
            {
              s1 = t[*p++];
              t = trans[s1];
              if (! t)
                {
                  state_num tmp = s;
                  s = s1;
                  s1 = tmp;     /* swap */
                  break;
                }
              if (s < d->min_trcount)
                {
                  while (t[*p] == s1)
                    p++;
                }
              s = t[*p++];
            }
        }

      if (s < 0)
        {
          if (s == -2)
            {
              s = build_state (s1, d, p[-1]);
              trans = d->trans;
            }
          else if ((char *) p <= end && p[-1] == eol && 0 <= d->newlines[s1])
            {
              /* The previous character was a newline.  Count it, and skip
                 checking of multibyte character boundary until here.  */
              nlcount++;
              mbp = p;

              s = (allow_nl ? d->newlines[s1]
                   : d->syntax.sbit[eol] == CTX_NEWLINE ? 0
                   : d->syntax.sbit[eol] == CTX_LETTER ? d->min_trcount - 1
                   : d->initstate_notbol);
            }
          else
            {
              p = NULL;
              goto done;
            }
        }
      else if (d->fails[s])
        {
          if ((d->success[s] & d->syntax.sbit[*p])
              || ((char *) p == end
                  && accepts_in_context (d->states[s].context, CTX_NEWLINE, s,
                                         d)))
            goto done;

          if (multibyte && s < d->min_trcount)
            p = mbp = skip_remains_mb (d, p, mbp, end);

          s1 = s;
          if (!multibyte || d->states[s].mbps.nelem == 0
              || d->localeinfo.sbctowc[*p] != WEOF || (char *) p >= end)
            {
              /* If a input character does not match ANYCHAR, do it
                 like a single-byte character.  */
              s = d->fails[s][*p++];
            }
          else
            {
              s = transit_state (d, s, &p, (unsigned char *) end);
              mbp = p;
              trans = d->trans;
            }
        }
      else
        {
          build_state (s, d, p[0]);
          trans = d->trans;
        }
    }

 done:
  if (count)
    *count += nlcount;
  *end = saved_end;
  return (char *) p;
}

/* Specialized versions of dfaexec for multibyte and single-byte cases.
   This is for performance, as dfaexec_main is an inline function.  */

static char *
dfaexec_mb (struct dfa *d, char const *begin, char *end,
            bool allow_nl, idx_t *count, bool *backref)
{
  return dfaexec_main (d, begin, end, allow_nl, count, true);
}

static char *
dfaexec_sb (struct dfa *d, char const *begin, char *end,
            bool allow_nl, idx_t *count, bool *backref)
{
  return dfaexec_main (d, begin, end, allow_nl, count, false);
}

/* Always set *BACKREF and return BEGIN.  Use this wrapper for
   any regexp that uses a construct not supported by this code.  */
static char *
dfaexec_noop (struct dfa *d, char const *begin, char *end,
              bool allow_nl, idx_t *count, bool *backref)
{
  *backref = true;
  return (char *) begin;
}

/* Like dfaexec_main (D, BEGIN, END, ALLOW_NL, COUNT, D->localeinfo.multibyte),
   but faster and set *BACKREF if the DFA code does not support this
   regexp usage.  */

char *
dfaexec (struct dfa *d, char const *begin, char *end,
         bool allow_nl, idx_t *count, bool *backref)
{
  return d->dfaexec (d, begin, end, allow_nl, count, backref);
}

struct dfa *
dfasuperset (struct dfa const *d)
{
  return d->superset;
}

bool
dfaisfast (struct dfa const *d)
{
  return d->fast;
}

static void
free_mbdata (struct dfa *d)
{
  free (d->multibyte_prop);
  free (d->lex.brack.chars);
  free (d->mb_follows.elems);

  if (d->mb_trans)
    {
      state_num s;
      for (s = -1; s < d->tralloc; s++)
        free (d->mb_trans[s]);
      free (d->mb_trans - 2);
    }
}

/* Return true if every construct in D is supported by this DFA matcher.  */
bool
dfasupported (struct dfa const *d)
{
  for (idx_t i = 0; i < d->tindex; i++)
    {
      switch (d->tokens[i])
        {
        case BEGWORD:
        case ENDWORD:
        case LIMWORD:
        case NOTLIMWORD:
          if (!d->localeinfo.multibyte)
            continue;
          FALLTHROUGH;
        case BACKREF:
        case MBCSET:
          return false;
        }
    }
  return true;
}

/* Disable use of the superset DFA if it is not likely to help
   performance.  */
static void
maybe_disable_superset_dfa (struct dfa *d)
{
  if (!d->localeinfo.using_utf8)
    return;

  bool have_backref = false;
  for (idx_t i = 0; i < d->tindex; i++)
    {
      switch (d->tokens[i])
        {
        case ANYCHAR:
          /* Lowered.  */
          assume (false);
        case BACKREF:
          have_backref = true;
          break;
        case MBCSET:
          /* Requires multi-byte algorithm.  */
          return;
        default:
          break;
        }
    }

  if (!have_backref && d->superset)
    {
      /* The superset DFA is not likely to be much faster, so remove it.  */
      dfafree (d->superset);
      free (d->superset);
      d->superset = NULL;
    }

  free_mbdata (d);
  d->localeinfo.multibyte = false;
  d->dfaexec = dfaexec_sb;
  d->fast = true;
}

static void
dfassbuild (struct dfa *d)
{
  struct dfa *sup = dfaalloc ();

  *sup = *d;
  sup->localeinfo.multibyte = false;
  sup->dfaexec = dfaexec_sb;
  sup->multibyte_prop = NULL;
  sup->superset = NULL;
  sup->states = NULL;
  sup->sindex = 0;
  sup->constraints = NULL;
  sup->separates = NULL;
  sup->follows = NULL;
  sup->tralloc = 0;
  sup->trans = NULL;
  sup->fails = NULL;
  sup->success = NULL;
  sup->newlines = NULL;

  sup->charclasses = xnmalloc (sup->calloc, sizeof *sup->charclasses);
  if (d->cindex)
    {
      memcpy (sup->charclasses, d->charclasses,
              d->cindex * sizeof *sup->charclasses);
    }

  sup->tokens = xnmalloc (d->tindex, 2 * sizeof *sup->tokens);
  sup->talloc = d->tindex * 2;

  bool have_achar = false;
  bool have_nchar = false;
  idx_t j;
  for (idx_t i = j = 0; i < d->tindex; i++)
    {
      switch (d->tokens[i])
        {
        case ANYCHAR:
        case MBCSET:
        case BACKREF:
          {
            charclass ccl;
            fillset (&ccl);
            sup->tokens[j++] = CSET + charclass_index (sup, &ccl);
            sup->tokens[j++] = STAR;
            if (d->tokens[i + 1] == QMARK || d->tokens[i + 1] == STAR
                || d->tokens[i + 1] == PLUS)
              i++;
            have_achar = true;
          }
          break;
        case BEGWORD:
        case ENDWORD:
        case LIMWORD:
        case NOTLIMWORD:
          if (d->localeinfo.multibyte)
            {
              /* These constraints aren't supported in a multibyte locale.
                 Ignore them in the superset DFA.  */
              sup->tokens[j++] = EMPTY;
              break;
            }
          FALLTHROUGH;
        default:
          sup->tokens[j++] = d->tokens[i];
          if ((0 <= d->tokens[i] && d->tokens[i] < NOTCHAR)
              || d->tokens[i] >= CSET)
            have_nchar = true;
          break;
        }
    }
  sup->tindex = j;

  if (have_nchar && (have_achar || d->localeinfo.multibyte))
    d->superset = sup;
  else
    {
      dfafree (sup);
      free (sup);
    }
}

/* Parse a string S of length LEN into D (but skip this step if S is null).
   Then analyze D and build a matcher for it.
   SEARCHFLAG says whether to build a searching or an exact matcher.  */
void
dfacomp (char const *s, idx_t len, struct dfa *d, bool searchflag)
{
  if (s != NULL)
    dfaparse (s, len, d);

  dfassbuild (d);

  if (dfasupported (d))
    {
      maybe_disable_superset_dfa (d);
      dfaanalyze (d, searchflag);
    }
  else
    {
      d->dfaexec = dfaexec_noop;
    }

  if (d->superset)
    {
      d->fast = true;
      dfaanalyze (d->superset, searchflag);
    }
}

/* Free the storage held by the components of a dfa.  */
void
dfafree (struct dfa *d)
{
  free (d->charclasses);
  free (d->tokens);

  if (d->localeinfo.multibyte)
    free_mbdata (d);

  free (d->constraints);
  free (d->separates);

  for (idx_t i = 0; i < d->sindex; i++)
    {
      free (d->states[i].elems.elems);
      free (d->states[i].mbps.elems);
    }
  free (d->states);

  if (d->follows)
    {
      for (idx_t i = 0; i < d->tindex; i++)
        free (d->follows[i].elems);
      free (d->follows);
    }

  if (d->trans)
    {
      for (idx_t i = 0; i < d->tralloc; i++)
        {
          free (d->trans[i]);
          free (d->fails[i]);
        }

      free (d->trans - 2);
      free (d->fails);
      free (d->newlines);
      free (d->success);
    }

  if (d->superset)
    {
      dfafree (d->superset);
      free (d->superset);
    }
}

/* Having found the postfix representation of the regular expression,
   try to find a long sequence of characters that must appear in any line
   containing the r.e.
   Finding a "longest" sequence is beyond the scope here;
   we take an easy way out and hope for the best.
   (Take "(ab|a)b"--please.)

   We do a bottom-up calculation of sequences of characters that must appear
   in matches of r.e.'s represented by trees rooted at the nodes of the postfix
   representation:
        sequences that must appear at the left of the match ("left")
        sequences that must appear at the right of the match ("right")
        lists of sequences that must appear somewhere in the match ("in")
        sequences that must constitute the match ("is")

   When we get to the root of the tree, we use one of the longest of its
   calculated "in" sequences as our answer.

   The sequences calculated for the various types of node (in pseudo ANSI c)
   are shown below.  "p" is the operand of unary operators (and the left-hand
   operand of binary operators); "q" is the right-hand operand of binary
   operators.

   "ZERO" means "a zero-length sequence" below.

        Type	left		right		is		in
        ----	----		-----		--		--
        char c	# c		# c		# c		# c

        ANYCHAR	ZERO		ZERO		ZERO		ZERO

        MBCSET	ZERO		ZERO		ZERO		ZERO

        CSET	ZERO		ZERO		ZERO		ZERO

        STAR	ZERO		ZERO		ZERO		ZERO

        QMARK	ZERO		ZERO		ZERO		ZERO

        PLUS	p->left		p->right	ZERO		p->in

        CAT	(p->is==ZERO)?	(q->is==ZERO)?	(p->is!=ZERO &&	p->in plus
                p->left :	q->right :	q->is!=ZERO) ?	q->in plus
                p->is##q->left	p->right##q->is	p->is##q->is : p->right##q->left
                                                ZERO

        OR	longest common	longest common	(do p->is and substrings common
                leading		trailing	to q->is have same p->in and
                (sub)sequence	(sub)sequence	q->in length and content) ?
                of p->left	of p->right
                and q->left	and q->right	p->is : NULL

   If there's anything else we recognize in the tree, all four sequences get set
   to zero-length sequences.  If there's something we don't recognize in the
   tree, we just return a zero-length sequence.

   Break ties in favor of infrequent letters (choosing 'zzz' in preference to
   'aaa')?

   And ... is it here or someplace that we might ponder "optimizations" such as
        egrep 'psi|epsilon'	->	egrep 'psi'
        egrep 'pepsi|epsilon'	->	egrep 'epsi'
                                        (Yes, we now find "epsi" as a "string
                                        that must occur", but we might also
                                        simplify the *entire* r.e. being sought)
        grep '[c]'		->	grep 'c'
        grep '(ab|a)b'		->	grep 'ab'
        grep 'ab*'		->	grep 'a'
        grep 'a*b'		->	grep 'b'

   There are several issues:

   Is optimization easy (enough)?

   Does optimization actually accomplish anything,
   or is the automaton you get from "psi|epsilon" (for example)
   the same as the one you get from "psi" (for example)?

   Are optimizable r.e.'s likely to be used in real-life situations
   (something like 'ab*' is probably unlikely; something like is
   'psi|epsilon' is likelier)?  */

static char *
icatalloc (char *old, char const *new)
{
  idx_t newsize = strlen (new);
  if (newsize == 0)
    return old;
  idx_t oldsize = strlen (old);
  char *result = xirealloc (old, oldsize + newsize + 1);
  memcpy (result + oldsize, new, newsize + 1);
  return result;
}

static void
freelist (char **cpp)
{
  while (*cpp)
    free (*cpp++);
}

static char **
enlistnew (char **cpp, char *new)
{
  /* Is there already something in the list that's new (or longer)?  */
  idx_t i;
  for (i = 0; cpp[i] != NULL; i++)
    if (strstr (cpp[i], new) != NULL)
      {
        free (new);
        return cpp;
      }
  /* Eliminate any obsoleted strings.  */
  for (idx_t j = 0; cpp[j] != NULL; )
    if (strstr (new, cpp[j]) == NULL)
      ++j;
    else
      {
        free (cpp[j]);
        if (--i == j)
          break;
        cpp[j] = cpp[i];
        cpp[i] = NULL;
      }
  /* Add the new string.  */
  cpp = xreallocarray (cpp, i + 2, sizeof *cpp);
  cpp[i] = new;
  cpp[i + 1] = NULL;
  return cpp;
}

static char **
enlist (char **cpp, char const *str, idx_t len)
{
  return enlistnew (cpp, ximemdup0 (str, len));
}

/* Given pointers to two strings, return a pointer to an allocated
   list of their distinct common substrings.  */
static char **
comsubs (char *left, char const *right)
{
  char **cpp = xzalloc (sizeof *cpp);

  for (char *lcp = left; *lcp != '\0'; lcp++)
    {
      idx_t len = 0;
      char *rcp = strchr (right, *lcp);
      while (rcp != NULL)
        {
          idx_t i;
          for (i = 1; lcp[i] != '\0' && lcp[i] == rcp[i]; ++i)
            continue;
          if (i > len)
            len = i;
          rcp = strchr (rcp + 1, *lcp);
        }
      if (len != 0)
        cpp = enlist (cpp, lcp, len);
    }
  return cpp;
}

static char **
addlists (char **old, char **new)
{
  for (; *new; new++)
    old = enlistnew (old, xstrdup (*new));
  return old;
}

/* Given two lists of substrings, return a new list giving substrings
   common to both.  */
static char **
inboth (char **left, char **right)
{
  char **both = xzalloc (sizeof *both);

  for (idx_t lnum = 0; left[lnum] != NULL; lnum++)
    {
      for (idx_t rnum = 0; right[rnum] != NULL; rnum++)
        {
          char **temp = comsubs (left[lnum], right[rnum]);
          both = addlists (both, temp);
          freelist (temp);
          free (temp);
        }
    }
  return both;
}

typedef struct must must;

struct must
{
  char **in;
  char *left;
  char *right;
  char *is;
  bool begline;
  bool endline;
  must *prev;
};

static must *
allocmust (must *mp, idx_t size)
{
  must *new_mp = xmalloc (sizeof *new_mp);
  new_mp->in = xzalloc (sizeof *new_mp->in);
  new_mp->left = xizalloc (size);
  new_mp->right = xizalloc (size);
  new_mp->is = xizalloc (size);
  new_mp->begline = false;
  new_mp->endline = false;
  new_mp->prev = mp;
  return new_mp;
}

static void
resetmust (must *mp)
{
  freelist (mp->in);
  mp->in[0] = NULL;
  mp->left[0] = mp->right[0] = mp->is[0] = '\0';
  mp->begline = false;
  mp->endline = false;
}

static void
freemust (must *mp)
{
  freelist (mp->in);
  free (mp->in);
  free (mp->left);
  free (mp->right);
  free (mp->is);
  free (mp);
}

struct dfamust *
dfamust (struct dfa const *d)
{
  must *mp = NULL;
  char const *result = "";
  bool exact = false;
  bool begline = false;
  bool endline = false;
  bool need_begline = false;
  bool need_endline = false;
  bool case_fold_unibyte = d->syntax.case_fold & !d->localeinfo.multibyte;

  for (idx_t ri = 1; ri + 1 < d->tindex; ri++)
    {
      token t = d->tokens[ri];
      switch (t)
        {
        case BEGLINE:
          mp = allocmust (mp, 2);
          mp->begline = true;
          need_begline = true;
          break;
        case ENDLINE:
          mp = allocmust (mp, 2);
          mp->endline = true;
          need_endline = true;
          break;
        case LPAREN:
        case RPAREN:
          assert (!"neither LPAREN nor RPAREN may appear here");

        case EMPTY:
        case BEGWORD:
        case ENDWORD:
        case LIMWORD:
        case NOTLIMWORD:
        case BACKREF:
        case ANYCHAR:
        case MBCSET:
          mp = allocmust (mp, 2);
          break;

        case STAR:
        case QMARK:
          assume_nonnull (mp);
          resetmust (mp);
          break;

        case OR:
          {
            char **new;
            must *rmp = mp;
            assume_nonnull (rmp);
            must *lmp = mp = mp->prev;
            assume_nonnull (lmp);
            idx_t j, ln, rn, n;

            /* Guaranteed to be.  Unlikely, but ...  */
            if (str_eq (lmp->is, rmp->is))
              {
                lmp->begline &= rmp->begline;
                lmp->endline &= rmp->endline;
              }
            else
              {
                lmp->is[0] = '\0';
                lmp->begline = false;
                lmp->endline = false;
              }
            /* Left side--easy */
            idx_t i = 0;
            while (lmp->left[i] != '\0' && lmp->left[i] == rmp->left[i])
              ++i;
            lmp->left[i] = '\0';
            /* Right side */
            ln = strlen (lmp->right);
            rn = strlen (rmp->right);
            n = ln;
            if (n > rn)
              n = rn;
            for (i = 0; i < n; ++i)
              if (lmp->right[ln - i - 1] != rmp->right[rn - i - 1])
                break;
            for (j = 0; j < i; ++j)
              lmp->right[j] = lmp->right[(ln - i) + j];
            lmp->right[j] = '\0';
            new = inboth (lmp->in, rmp->in);
            freelist (lmp->in);
            free (lmp->in);
            lmp->in = new;
            freemust (rmp);
          }
          break;

        case PLUS:
          assume_nonnull (mp);
          mp->is[0] = '\0';
          break;

        case END:
          assume_nonnull (mp);
          assert (!mp->prev);
          for (idx_t i = 0; mp->in[i] != NULL; i++)
            if (strlen (mp->in[i]) > strlen (result))
              result = mp->in[i];
          if (str_eq (result, mp->is))
            {
              if ((!need_begline || mp->begline) && (!need_endline
                                                     || mp->endline))
                exact = true;
              begline = mp->begline;
              endline = mp->endline;
            }
          goto done;

        case CAT:
          {
            must *rmp = mp;
            assume_nonnull (rmp);
            must *lmp = mp = mp->prev;
            assume_nonnull (lmp);

            /* In.  Everything in left, plus everything in
               right, plus concatenation of
               left's right and right's left.  */
            lmp->in = addlists (lmp->in, rmp->in);
            if (lmp->right[0] != '\0' && rmp->left[0] != '\0')
              {
                idx_t lrlen = strlen (lmp->right);
                idx_t rllen = strlen (rmp->left);
                char *tp = ximalloc (lrlen + rllen + 1);
                memcpy (tp + lrlen, rmp->left, rllen + 1);
                memcpy (tp, lmp->right, lrlen);
                lmp->in = enlistnew (lmp->in, tp);
              }
            /* Left-hand */
            if (lmp->is[0] != '\0')
              lmp->left = icatalloc (lmp->left, rmp->left);
            /* Right-hand */
            if (rmp->is[0] == '\0')
              lmp->right[0] = '\0';
            lmp->right = icatalloc (lmp->right, rmp->right);
            /* Guaranteed to be */
            if ((lmp->is[0] != '\0' || lmp->begline)
                && (rmp->is[0] != '\0' || rmp->endline))
              {
                lmp->is = icatalloc (lmp->is, rmp->is);
                lmp->endline = rmp->endline;
              }
            else
              {
                lmp->is[0] = '\0';
                lmp->begline = false;
                lmp->endline = false;
              }
            freemust (rmp);
          }
          break;

        case '\0':
          /* Not on *my* shift.  */
          goto done;

        default:
          if (CSET <= t)
            {
              /* If T is a singleton, or if case-folding in a unibyte
                 locale and T's members all case-fold to the same char,
                 convert T to one of its members.  Otherwise, do
                 nothing further with T.  */
              charclass *ccl = &d->charclasses[t - CSET];
              int j;
              for (j = 0; j < NOTCHAR; j++)
                if (tstbit (j, ccl))
                  break;
              if (! (j < NOTCHAR))
                {
                  mp = allocmust (mp, 2);
                  break;
                }
              t = j;
              while (++j < NOTCHAR)
                if (tstbit (j, ccl)
                    && ! (case_fold_unibyte
                          && toupper (j) == toupper (t)))
                  break;
              if (j < NOTCHAR)
                {
                  mp = allocmust (mp, 2);
                  break;
                }
            }

          idx_t rj = ri + 2;
          if (d->tokens[ri + 1] == CAT)
            {
              for (; rj < d->tindex - 1; rj += 2)
                {
                  if ((rj != ri && (d->tokens[rj] <= 0
                                    || NOTCHAR <= d->tokens[rj]))
                      || d->tokens[rj + 1] != CAT)
                    break;
                }
            }
          mp = allocmust (mp, ((rj - ri) >> 1) + 1);
          mp->is[0] = mp->left[0] = mp->right[0]
            = case_fold_unibyte ? toupper (t) : t;

          idx_t i;
          for (i = 1; ri + 2 < rj; i++)
            {
              ri += 2;
              t = d->tokens[ri];
              mp->is[i] = mp->left[i] = mp->right[i]
                = case_fold_unibyte ? toupper (t) : t;
            }
          mp->is[i] = mp->left[i] = mp->right[i] = '\0';
          mp->in = enlist (mp->in, mp->is, i);
          break;
        }
    }
 done:;

  struct dfamust *dm = NULL;
  if (*result)
    {
      dm = xmalloc (FLEXSIZEOF (struct dfamust, must, strlen (result) + 1));
      dm->exact = exact;
      dm->begline = begline;
      dm->endline = endline;
      strcpy (dm->must, result);
    }

  while (mp)
    {
      must *prev = mp->prev;
      freemust (mp);
      mp = prev;
    }

  return dm;
}

void
dfamustfree (struct dfamust *dm)
{
  free (dm);
}

struct dfa *
dfaalloc (void)
{
  return xmalloc (sizeof (struct dfa));
}

/* Initialize DFA.  */
void
dfasyntax (struct dfa *dfa, struct localeinfo const *linfo,
           reg_syntax_t bits, int dfaopts)
{
  memset (dfa, 0, offsetof (struct dfa, dfaexec));
  dfa->dfaexec = linfo->multibyte ? dfaexec_mb : dfaexec_sb;
  dfa->localeinfo = *linfo;

  dfa->fast = !dfa->localeinfo.multibyte;

  dfa->canychar = -1;
  dfa->syntax.syntax_bits_set = true;
  dfa->syntax.case_fold = (bits & RE_ICASE) != 0;
  dfa->syntax.eolbyte = dfaopts & DFA_EOL_NUL ? '\0' : '\n';
  dfa->syntax.syntax_bits = bits;
  dfa->syntax.dfaopts = dfaopts;

  for (int i = CHAR_MIN; i <= CHAR_MAX; ++i)
    {
      unsigned char uc = i;

      dfa->syntax.sbit[uc] = char_context (dfa, uc);
      switch (dfa->syntax.sbit[uc])
        {
        case CTX_LETTER:
          setbit (uc, &dfa->syntax.letters);
          break;
        case CTX_NEWLINE:
          setbit (uc, &dfa->syntax.newline);
          break;
        }

      /* POSIX requires that the five bytes in "\n\r./" (including the
         terminating NUL) cannot occur inside a multibyte character.  */
      dfa->syntax.never_trail[uc] = (dfa->localeinfo.using_utf8
                                     ? (uc & 0xc0) != 0x80
                                     : strchr ("\n\r./", uc) != NULL);
    }
}

/* Initialize TO by copying FROM's syntax settings.  */
void
dfacopysyntax (struct dfa *to, struct dfa const *from)
{
  memset (to, 0, offsetof (struct dfa, syntax));
  to->canychar = -1;
  to->fast = from->fast;
  to->syntax = from->syntax;
  to->dfaexec = from->dfaexec;
  to->localeinfo = from->localeinfo;
}

/* vim:set shiftwidth=2: */
