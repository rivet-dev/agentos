/* kwset.c - search for any of a set of keywords.
   Copyright (C) 1989, 1998, 2000, 2005, 2007, 2009-2025 Free Software
   Foundation, Inc.

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

/* Written August 1989 by Mike Haertel.  */

/* For more on the Aho-Corasick and Boyer-Moore algorithms,
   as well as other algorithms that might help improve performance,
   see the grep manual's "Performance" chapter.  */

#include <config.h>

#include "kwset.h"

#include <stdckdint.h>
#include <stdint.h>
#include <sys/types.h>

#include "system.h"
#include "memchr2.h"
#include "obstack.h"
#include "xalloc.h"
#include "verify.h"

#define obstack_chunk_alloc xmalloc
#define obstack_chunk_free free

static unsigned char
U (char ch)
{
  return to_uchar (ch);
}

/* Balanced tree of edges and labels leaving a given trie node.  */
struct tree
{
  struct tree *llink;		/* Left link; MUST be first field.  */
  struct tree *rlink;		/* Right link (to larger labels).  */
  struct trie *trie;		/* Trie node pointed to by this edge.  */
  unsigned char label;		/* Label on this edge.  */
  char balance;			/* Difference in depths of subtrees.  */
};

/* Node of a trie representing a set of keywords.  */
struct trie
{
  /* If an accepting node, this is either 2*W + 1 where W is the word
     index, or is -1 if Aho-Corasick is in use and FAIL
     specifies where to look for more info.  If not an accepting node,
     this is zero.  */
  ptrdiff_t accepting;

  struct tree *links;		/* Tree of edges leaving this node.  */
  struct trie *parent;		/* Parent of this node.  */
  struct trie *next;		/* List of all trie nodes in level order.  */
  struct trie *fail;		/* Aho-Corasick failure function.  */
  idx_t depth;			/* Depth of this node from the root.  */
  idx_t shift;			/* Shift function for search failures.  */
  idx_t maxshift;		/* Max shift of self and descendants.  */
};

/* Structure returned opaquely to the caller, containing everything.  */
struct kwset
{
  struct obstack obstack;	/* Obstack for node allocation.  */
  idx_t words;			/* Number of words in the trie.  */
  struct trie *trie;		/* The trie itself.  */
  idx_t mind;			/* Minimum depth of an accepting node.  */
  unsigned char delta[NCHAR];	/* Delta table for rapid search.  */
  struct trie *next[NCHAR];	/* Table of children of the root.  */
  char *target;			/* Target string if there's only one.  */
  idx_t *shift;			/* Used in Boyer-Moore search for one
                                   string.  */
  char const *trans;		/* Character translation table.  */

  /* This helps to match a terminal byte, which is the first byte
     for Aho-Corasick, and the last byte for Boyer-More.  If all the
     patterns have the same terminal byte (after translation via TRANS
     if TRANS is nonnull), then this is that byte as an unsigned char.
     Otherwise this is -1 if there is disagreement among the strings
     about terminal bytes, and -2 if there are no terminal bytes and
     no disagreement because all the patterns are empty.  */
  int gc1;

  /* This helps to match a terminal byte.  If 0 <= GC1HELP, B is
     terminal when B == GC1 || B == GC1HELP (note that GC1 == GCHELP
     is common here).  This is typically faster than evaluating
     to_uchar (TRANS[B]) == GC1.  */
  int gc1help;

  /* If the string has two or more bytes, this is the penultimate byte,
     after translation via TRANS if TRANS is nonnull.  This variable
     is used only by Boyer-Moore.  */
  char gc2;

  /* kwsexec implementation.  */
  ptrdiff_t (*kwsexec) (kwset_t, char const *, idx_t, struct kwsmatch *, bool);
};

/* Use TRANS to transliterate C.  A null TRANS does no transliteration.  */
static inline char
tr (char const *trans, char c)
{
  return trans ? trans[U(c)] : c;
}

static ptrdiff_t acexec (kwset_t, char const *, idx_t,
                         struct kwsmatch *, bool);
static ptrdiff_t bmexec (kwset_t, char const *, idx_t,
                         struct kwsmatch *, bool);

/* Return a newly allocated keyword set.  A nonnull TRANS specifies a
   table of character translations to be applied to all pattern and
   search text.  */
kwset_t
kwsalloc (char const *trans)
{
  struct kwset *kwset = xmalloc (sizeof *kwset);

  obstack_init (&kwset->obstack);
  kwset->words = 0;
  kwset->trie = obstack_alloc (&kwset->obstack, sizeof *kwset->trie);
  kwset->trie->accepting = 0;
  kwset->trie->links = nullptr;
  kwset->trie->parent = nullptr;
  kwset->trie->next = nullptr;
  kwset->trie->fail = nullptr;
  kwset->trie->depth = 0;
  kwset->trie->shift = 0;
  kwset->mind = IDX_MAX;
  kwset->target = nullptr;
  kwset->trans = trans;
  kwset->kwsexec = acexec;

  return kwset;
}

/* This upper bound is valid for CHAR_BIT >= 4 and
   exact for CHAR_BIT in { 4..11, 13, 15, 17, 19 }.  */
enum { DEPTH_SIZE = CHAR_BIT + CHAR_BIT / 2 };

/* Add the given string to the contents of the keyword set.  */
void
kwsincr (kwset_t kwset, char const *text, idx_t len)
{
  assume (0 <= len);
  struct trie *trie = kwset->trie;
  char const *trans = kwset->trans;
  bool reverse = kwset->kwsexec == bmexec;

  if (reverse)
    text += len;

  /* Descend the trie (built of keywords) character-by-character,
     installing new nodes when necessary.  */
  while (len--)
    {
      unsigned char uc = reverse ? *--text : *text++;
      unsigned char label = trans ? trans[uc] : uc;

      /* Descend the tree of outgoing links for this trie node,
         looking for the current character and keeping track
         of the path followed.  */
      struct tree *cur = trie->links;
      struct tree *links[DEPTH_SIZE];
      enum { L, R } dirs[DEPTH_SIZE];
      links[0] = (struct tree *) &trie->links;
      dirs[0] = L;
      idx_t depth = 1;

      while (cur && label != cur->label)
        {
          links[depth] = cur;
          if (label < cur->label)
            dirs[depth++] = L, cur = cur->llink;
          else
            dirs[depth++] = R, cur = cur->rlink;
        }

      /* The current character doesn't have an outgoing link at
         this trie node, so build a new trie node and install
         a link in the current trie node's tree.  */
      if (!cur)
        {
          cur = obstack_alloc (&kwset->obstack, sizeof *cur);
          cur->llink = nullptr;
          cur->rlink = nullptr;
          cur->trie = obstack_alloc (&kwset->obstack, sizeof *cur->trie);
          cur->trie->accepting = 0;
          cur->trie->links = nullptr;
          cur->trie->parent = trie;
          cur->trie->next = nullptr;
          cur->trie->fail = nullptr;
          cur->trie->depth = trie->depth + 1;
          cur->trie->shift = 0;
          cur->label = label;
          cur->balance = 0;

          /* Install the new tree node in its parent.  */
          if (dirs[--depth] == L)
            links[depth]->llink = cur;
          else
            links[depth]->rlink = cur;

          /* Back up the tree fixing the balance flags.  */
          while (depth && !links[depth]->balance)
            {
              if (dirs[depth] == L)
                --links[depth]->balance;
              else
                ++links[depth]->balance;
              --depth;
            }

          /* Rebalance the tree by pointer rotations if necessary.  */
          if (depth && ((dirs[depth] == L && --links[depth]->balance)
                        || (dirs[depth] == R && ++links[depth]->balance)))
            {
              struct tree *t, *r, *l, *rl, *lr;

              switch (links[depth]->balance)
                {
                case (char) -2:
                  switch (dirs[depth + 1])
                    {
                    case L:
                      r = links[depth], t = r->llink, rl = t->rlink;
                      t->rlink = r, r->llink = rl;
                      t->balance = r->balance = 0;
                      break;
                    case R:
                      r = links[depth], l = r->llink, t = l->rlink;
                      rl = t->rlink, lr = t->llink;
                      t->llink = l, l->rlink = lr, t->rlink = r, r->llink = rl;
                      l->balance = t->balance != 1 ? 0 : -1;
                      r->balance = t->balance != (char) -1 ? 0 : 1;
                      t->balance = 0;
                      break;
                    default:
                      abort ();
                    }
                  break;
                case 2:
                  switch (dirs[depth + 1])
                    {
                    case R:
                      l = links[depth], t = l->rlink, lr = t->llink;
                      t->llink = l, l->rlink = lr;
                      t->balance = l->balance = 0;
                      break;
                    case L:
                      l = links[depth], r = l->rlink, t = r->llink;
                      lr = t->llink, rl = t->rlink;
                      t->llink = l, l->rlink = lr, t->rlink = r, r->llink = rl;
                      l->balance = t->balance != 1 ? 0 : -1;
                      r->balance = t->balance != (char) -1 ? 0 : 1;
                      t->balance = 0;
                      break;
                    default:
                      abort ();
                    }
                  break;
                default:
                  abort ();
                }

              if (dirs[depth - 1] == L)
                links[depth - 1]->llink = t;
              else
                links[depth - 1]->rlink = t;
            }
        }

      trie = cur->trie;
    }

  /* Mark the node finally reached as accepting, encoding the
     index number of this word in the keyword set so far.  */
  if (!trie->accepting)
    trie->accepting = 2 * kwset->words + 1;
  ++kwset->words;

  /* Keep track of the longest and shortest string of the keyword set.  */
  if (trie->depth < kwset->mind)
    kwset->mind = trie->depth;
}

idx_t
kwswords (kwset_t kwset)
{
  return kwset->words;
}

/* Enqueue the trie nodes referenced from the given tree in the
   given queue.  */
static void
enqueue (struct tree *tree, struct trie **last)
{
  if (!tree)
    return;
  enqueue (tree->llink, last);
  enqueue (tree->rlink, last);
  (*last) = (*last)->next = tree->trie;
}

/* Compute the Aho-Corasick failure function for the trie nodes referenced
   from the given tree, given the failure function for their parent as
   well as a last resort failure node.  */
static void
treefails (struct tree const *tree, struct trie const *fail,
           struct trie *recourse, bool reverse)
{
  struct tree *cur;

  if (!tree)
    return;

  treefails (tree->llink, fail, recourse, reverse);
  treefails (tree->rlink, fail, recourse, reverse);

  /* Find, in the chain of fails going back to the root, the first
     node that has a descendant on the current label.  */
  while (fail)
    {
      cur = fail->links;
      while (cur && tree->label != cur->label)
        if (tree->label < cur->label)
          cur = cur->llink;
        else
          cur = cur->rlink;
      if (cur)
        {
          tree->trie->fail = cur->trie;
          if (!reverse && cur->trie->accepting && !tree->trie->accepting)
            tree->trie->accepting = -1;
          return;
        }
      fail = fail->fail;
    }

  tree->trie->fail = recourse;
}

/* Set delta entries for the links of the given tree such that
   the preexisting delta value is larger than the current depth.  */
static void
treedelta (struct tree const *tree, idx_t depth, unsigned char delta[])
{
  if (!tree)
    return;
  treedelta (tree->llink, depth, delta);
  treedelta (tree->rlink, depth, delta);
  if (depth < delta[tree->label])
    delta[tree->label] = depth;
}

/* Return true if A has every label in B.  */
static bool _GL_ATTRIBUTE_PURE
hasevery (struct tree const *a, struct tree const *b)
{
  if (!b)
    return true;
  if (!hasevery (a, b->llink))
    return false;
  if (!hasevery (a, b->rlink))
    return false;
  while (a && b->label != a->label)
    if (b->label < a->label)
      a = a->llink;
    else
      a = a->rlink;
  return !!a;
}

/* Compute a vector, indexed by character code, of the trie nodes
   referenced from the given tree.  */
static void
treenext (struct tree const *tree, struct trie *next[])
{
  if (!tree)
    return;
  treenext (tree->llink, next);
  treenext (tree->rlink, next);
  next[tree->label] = tree->trie;
}

/* Prepare a built keyword set for use.  */
void
kwsprep (kwset_t kwset)
{
  char const *trans = kwset->trans;
  unsigned char deltabuf[NCHAR];
  unsigned char *delta = trans ? deltabuf : kwset->delta;
  struct trie *curr, *last;

  /* Use Boyer-Moore if just one pattern, Aho-Corasick otherwise.  */
  bool reverse = kwset->words == 1;

  if (reverse)
    {
      kwset_t new_kwset;

      /* Enqueue the immediate descendants in the level order queue.  */
      for (curr = last = kwset->trie; curr; curr = curr->next)
        enqueue (curr->links, &last);

      /* Looking for just one string.  Extract it from the trie.  */
      kwset->target = obstack_alloc (&kwset->obstack, kwset->mind);
      curr = kwset->trie;
      for (idx_t i = 0; i < kwset->mind; i++)
        {
          kwset->target[i] = curr->links->label;
          curr = curr->next;
        }

      new_kwset = kwsalloc (kwset->trans);
      new_kwset->kwsexec = bmexec;
      kwsincr (new_kwset, kwset->target, kwset->mind);
      obstack_free (&kwset->obstack, nullptr);
      *kwset = *new_kwset;
      free (new_kwset);
    }

  /* Initial values for the delta table; will be changed later.  The
     delta entry for a given character is the smallest depth of any
     node at which an outgoing edge is labeled by that character.  */
  memset (delta, MIN (kwset->mind, UCHAR_MAX), sizeof deltabuf);

  /* Traverse the nodes of the trie in level order, simultaneously
     computing the delta table, failure function, and shift function.  */
  for (curr = last = kwset->trie; curr; curr = curr->next)
    {
      /* Enqueue the immediate descendants in the level order queue.  */
      enqueue (curr->links, &last);

      /* Update the delta table for the descendants of this node.  */
      treedelta (curr->links, curr->depth, delta);

      /* Compute the failure function for the descendants of this node.  */
      treefails (curr->links, curr->fail, kwset->trie, reverse);

      if (reverse)
        {
          curr->shift = kwset->mind;
          curr->maxshift = kwset->mind;

          /* Update the shifts at each node in the current node's chain
             of fails back to the root.  */
          struct trie *fail;
          for (fail = curr->fail; fail; fail = fail->fail)
            {
              /* If the current node has some outgoing edge that the fail
                 doesn't, then the shift at the fail should be no larger
                 than the difference of their depths.  */
              if (!hasevery (fail->links, curr->links))
                if (curr->depth - fail->depth < fail->shift)
                  fail->shift = curr->depth - fail->depth;

              /* If the current node is accepting then the shift at the
                 fail and its descendants should be no larger than the
                 difference of their depths.  */
              if (curr->accepting && fail->maxshift > curr->depth - fail->depth)
                fail->maxshift = curr->depth - fail->depth;
            }
        }
    }

  if (reverse)
    {
      /* Traverse the trie in level order again, fixing up all nodes whose
         shift exceeds their inherited maxshift.  */
      for (curr = kwset->trie->next; curr; curr = curr->next)
        {
          if (curr->maxshift > curr->parent->maxshift)
            curr->maxshift = curr->parent->maxshift;
          if (curr->shift > curr->maxshift)
            curr->shift = curr->maxshift;
        }
    }

  /* Create a vector, indexed by character code, of the outgoing links
     from the root node.  Accumulate GC1 and GC1HELP.  */
  struct trie *nextbuf[NCHAR];
  struct trie **next = trans ? nextbuf : kwset->next;
  memset (next, 0, sizeof nextbuf);
  treenext (kwset->trie->links, next);
  int gc1 = -2;
  int gc1help = -1;
  for (int i = 0; i < NCHAR; i++)
    {
      int ti = i;
      if (trans)
        {
          ti = U(trans[i]);
          kwset->next[i] = next[ti];
        }
      if (kwset->next[i])
        {
          if (gc1 < -1)
            {
              gc1 = ti;
              gc1help = i;
            }
          else if (gc1 == ti)
            gc1help = gc1help == ti ? i : -1;
          else if (i == ti && gc1 == gc1help)
            gc1help = i;
          else
            gc1 = -1;
        }
    }
  kwset->gc1 = gc1;
  kwset->gc1help = gc1help;

  if (reverse)
    {
      /* Looking for just one string.  Extract it from the trie.  */
      kwset->target = obstack_alloc (&kwset->obstack, kwset->mind);
      curr = kwset->trie;
      for (idx_t i = kwset->mind; 0 < i; i--)
        {
          kwset->target[i - 1] = curr->links->label;
          curr = curr->next;
        }

      if (kwset->mind > 1)
        {
          /* Looking for the delta2 shift that might be made after a
             backwards match has failed.  Extract it from the trie.  */
          kwset->shift
            = obstack_alloc (&kwset->obstack,
                             sizeof *kwset->shift * (kwset->mind - 1));
          curr = kwset->trie->next;
          for (idx_t i = 0; i < kwset->mind - 1; i++)
            {
              kwset->shift[i] = curr->shift;
              curr = curr->next;
            }

          /* The penultimate byte.  */
          kwset->gc2 = tr (trans, kwset->target[kwset->mind - 2]);
        }
    }

  /* Fix things up for any translation table.  */
  if (trans)
    for (int i = 0; i < NCHAR; ++i)
      kwset->delta[i] = delta[U(trans[i])];
}

/* Delta2 portion of a Boyer-Moore search.  *TP is the string text
   pointer; it is updated in place.  EP is the end of the string text,
   and SP the end of the pattern.  LEN is the pattern length; it must
   be at least 2.  TRANS, if nonnull, is the input translation table.
   GC1 and GC2 are the last and second-from last bytes of the pattern,
   transliterated by TRANS; the caller precomputes them for
   efficiency.  If D1 is nonnull, it is a delta1 table for shifting *TP
   when failing.  KWSET->shift says how much to shift.  */
static inline bool
bm_delta2_search (char const **tpp, char const *ep, char const *sp,
                  idx_t len,
                  char const *trans, char gc1, char gc2,
                  unsigned char const *d1, kwset_t kwset)
{
  char const *tp = *tpp;
  idx_t d = len, skip = 0;

  while (true)
    {
      idx_t i = 2;
      if (tr (trans, tp[-2]) == gc2)
        {
          while (++i <= d)
            if (tr (trans, tp[-i]) != tr (trans, sp[-i]))
              break;
          if (i > d)
            {
              for (i = d + skip + 1; i <= len; ++i)
                if (tr (trans, tp[-i]) != tr (trans, sp[-i]))
                  break;
              if (i > len)
                {
                  *tpp = tp - len;
                  return true;
                }
            }
        }

      tp += d = kwset->shift[i - 2];
      if (tp > ep)
        break;
      if (tr (trans, tp[-1]) != gc1)
        {
          if (d1)
            tp += d1[U(tp[-1])];
          break;
        }
      skip = i - 1;
    }

  *tpp = tp;
  return false;
}

/* Return the address of the first byte in the buffer S (of size N)
   that matches the terminal byte specified by KWSET, or null if there
   is no match.  KWSET->gc1 should be nonnegative.  */
static char const *
memchr_kwset (char const *s, idx_t n, kwset_t kwset)
{
  char const *slim = s + n;
  if (kwset->gc1help < 0)
    {
      for (; s < slim; s++)
        if (kwset->next[U(*s)])
          return s;
    }
  else
    {
      int small_heuristic = 2;
      idx_t small_bytes = small_heuristic * sizeof (unsigned long int);
      while (s < slim)
        {
          if (kwset->next[U(*s)])
            return s;
          s++;
          if ((uintptr_t) s % small_bytes == 0)
            return memchr2 (s, kwset->gc1, kwset->gc1help, slim - s);
        }
    }
  return nullptr;
}

/* Fast Boyer-Moore search (inlinable version).  */
static inline ptrdiff_t _GL_ATTRIBUTE_PURE
bmexec_trans (kwset_t kwset, char const *text, idx_t size)
{
  assume (0 <= size);
  unsigned char const *d1;
  char const *ep, *sp, *tp;
  int d;
  idx_t len = kwset->mind;
  char const *trans = kwset->trans;

  if (len == 0)
    return 0;
  if (len > size)
    return -1;
  if (len == 1)
    {
      tp = memchr_kwset (text, size, kwset);
      return tp ? tp - text : -1;
    }

  d1 = kwset->delta;
  sp = kwset->target + len;
  tp = text + len;
  char gc1 = kwset->gc1;
  char gc2 = kwset->gc2;

  /* Significance of 12: 1 (initial offset) + 10 (skip loop) + 1 (md2).  */
  idx_t len12;
  if (!ckd_mul (&len12, len, 12) && len12 < size)
    /* 11 is not a bug, the initial offset happens only once.  */
    for (ep = text + size - 11 * len; tp <= ep; )
      {
        char const *tp0 = tp;
        d = d1[U(tp[-1])], tp += d;
        d = d1[U(tp[-1])], tp += d;
        if (d != 0)
          {
            d = d1[U(tp[-1])], tp += d;
            d = d1[U(tp[-1])], tp += d;
            d = d1[U(tp[-1])], tp += d;
            if (d != 0)
              {
                d = d1[U(tp[-1])], tp += d;
                d = d1[U(tp[-1])], tp += d;
                d = d1[U(tp[-1])], tp += d;
                if (d != 0)
                  {
                    d = d1[U(tp[-1])], tp += d;
                    d = d1[U(tp[-1])], tp += d;

                    /* As a heuristic, prefer memchr to seeking by
                       delta1 when the latter doesn't advance much.  */
                    int advance_heuristic = 16 * sizeof (long);
                    if (advance_heuristic <= tp - tp0)
                      continue;
                    tp--;
                    tp = memchr_kwset (tp, text + size - tp, kwset);
                    if (! tp)
                      return -1;
                    tp++;
                    if (ep <= tp)
                      break;
                  }
              }
          }
        if (bm_delta2_search (&tp, ep, sp, len, trans, gc1, gc2, d1, kwset))
          return tp - text;
      }

  /* Now only a few characters are left to search.  Carefully avoid
     ever producing an out-of-bounds pointer.  */
  ep = text + size;
  d = d1[U(tp[-1])];
  while (d <= ep - tp)
    {
      d = d1[U((tp += d)[-1])];
      if (d != 0)
        continue;
      if (bm_delta2_search (&tp, ep, sp, len, trans, gc1, gc2, nullptr, kwset))
        return tp - text;
    }

  return -1;
}

/* Fast Boyer-Moore search.  */
static ptrdiff_t
bmexec (kwset_t kwset, char const *text, idx_t size,
        struct kwsmatch *kwsmatch, bool longest)
{
  /* Help the compiler inline in two ways, depending on whether
     kwset->trans is null.  */
  ptrdiff_t ret = (IGNORE_DUPLICATE_BRANCH_WARNING
                   (kwset->trans
                    ? bmexec_trans (kwset, text, size)
                    : bmexec_trans (kwset, text, size)));
  kwsmatch->index = 0;
  kwsmatch->offset = ret;
  kwsmatch->size = kwset->mind;
  return ret;
}

/* Hairy multiple string search with the Aho-Corasick algorithm.
   (inlinable version)  */
static inline ptrdiff_t
acexec_trans (kwset_t kwset, char const *text, idx_t len,
              struct kwsmatch *kwsmatch, bool longest)
{
  struct trie const *trie, *accept;
  char const *tp, *left, *lim;
  struct tree const *tree;
  char const *trans;

  /* Initialize register copies and look for easy ways out.  */
  if (len < kwset->mind)
    return -1;
  trans = kwset->trans;
  trie = kwset->trie;
  lim = text + len;
  tp = text;

  if (!trie->accepting)
    {
      unsigned char c;
      int gc1 = kwset->gc1;

      while (true)
        {
          if (gc1 < 0)
            {
              while (! (trie = kwset->next[c = tr (trans, *tp++)]))
                if (tp >= lim)
                  return -1;
            }
          else
            {
              tp = memchr_kwset (tp, lim - tp, kwset);
              if (!tp)
                return -1;
              c = tr (trans, *tp++);
              trie = kwset->next[c];
            }

          while (true)
            {
              if (trie->accepting)
                goto match;
              if (tp >= lim)
                return -1;
              c = tr (trans, *tp++);

              for (tree = trie->links; c != tree->label; )
                {
                  tree = c < tree->label ? tree->llink : tree->rlink;
                  if (! tree)
                    {
                      trie = trie->fail;
                      if (!trie)
                        {
                          trie = kwset->next[c];
                          if (trie)
                            goto have_trie;
                          if (tp >= lim)
                            return -1;
                          goto next_c;
                        }
                      if (trie->accepting)
                        {
                          --tp;
                          goto match;
                        }
                      tree = trie->links;
                    }
                }
              trie = tree->trie;
            have_trie:;
            }
        next_c:;
        }
    }

 match:
  accept = trie;
  while (accept->accepting < 0)
    accept = accept->fail;
  left = tp - accept->depth;

  /* Try left-most longest match.  */
  if (longest)
    {
      while (tp < lim)
        {
          struct trie const *accept1;
          char const *left1;
          unsigned char c = tr (trans, *tp++);

          do
            {
              tree = trie->links;
              while (tree && c != tree->label)
                tree = c < tree->label ? tree->llink : tree->rlink;
            }
          while (!tree && (trie = trie->fail) && accept->depth <= trie->depth);

          if (!tree)
            break;
          trie = tree->trie;
          if (trie->accepting)
            {
              accept1 = trie;
              while (accept1->accepting < 0)
                accept1 = accept1->fail;
              left1 = tp - accept1->depth;
              if (left1 <= left)
                {
                  left = left1;
                  accept = accept1;
                }
            }
        }
    }

  kwsmatch->index = accept->accepting >> 1;
  kwsmatch->offset = left - text;
  kwsmatch->size = accept->depth;

  return left - text;
}

/* Hairy multiple string search with Aho-Corasick algorithm.  */
static ptrdiff_t
acexec (kwset_t kwset, char const *text, idx_t size,
        struct kwsmatch *kwsmatch, bool longest)
{
  assume (0 <= size);
  /* Help the compiler inline in two ways, depending on whether
     kwset->trans is null.  */
  return (IGNORE_DUPLICATE_BRANCH_WARNING
          (kwset->trans
           ? acexec_trans (kwset, text, size, kwsmatch, longest)
           : acexec_trans (kwset, text, size, kwsmatch, longest)));
}

/* Find the first instance of a KWSET member in TEXT, which has SIZE bytes.
   Return the offset (into TEXT) of the first byte of the matching substring,
   or -1 if no match is found.  Upon a match, store details in
   *KWSMATCH: index of matched keyword, start offset (same as the return
   value), and length.  If LONGEST, find the longest match; otherwise
   any match will do.  */
ptrdiff_t
kwsexec (kwset_t kwset, char const *text, idx_t size,
         struct kwsmatch *kwsmatch, bool longest)
{
  return kwset->kwsexec (kwset, text, size, kwsmatch, longest);
}

/* Free the components of the given keyword set.  */
void
kwsfree (kwset_t kwset)
{
  obstack_free (&kwset->obstack, nullptr);
  free (kwset);
}
