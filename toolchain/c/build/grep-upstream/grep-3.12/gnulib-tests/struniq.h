/* Define a file-local string uniquification function.
   Copyright (C) 2009-2025 Free Software Foundation, Inc.

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

/* Written by Bruno Haible <bruno@clisp.org>, 2009.  */


/* This file needs the following includes:

     #include <limits.h>
     #include <stdlib.h>
     #include <string.h>
     #include "flexmember.h"
     #include "glthread/lock.h"
     #include "thread-optim.h"

   and the following gnulib modules as dependencies:

     flexmember
     lock
     bool
     thread-optim
 */


/* Simple hash set of strings.  We don't want to drag in lots of hash table
   code here.  */

#define SIZE_BITS (sizeof (size_t) * CHAR_BIT)

/* A hash function for NUL-terminated char* strings using
   the method described by Bruno Haible.
   See https://www.haible.de/bruno/hashfunc.html.  */
static size_t _GL_ATTRIBUTE_PURE
string_hash (const void *x)
{
  const char *s = (const char *) x;
  size_t h = 0;

  for (; *s; s++)
    h = *s + ((h << 9) | (h >> (SIZE_BITS - 9)));

  return h;
}

/* A hash table of fixed size.  Multiple threads can access it read-only
   simultaneously, but only one thread can insert into it at the same time.  */

/* A node in a hash bucket collision list.  */
struct struniq_hash_node
  {
    struct struniq_hash_node * volatile next;
    char contents[FLEXIBLE_ARRAY_MEMBER];
  };

#define STRUNIQ_HASH_TABLE_SIZE 257
static struct struniq_hash_node * volatile struniq_hash_table[STRUNIQ_HASH_TABLE_SIZE]
  /* = { NULL, ..., NULL } */;

/* This lock protects the struniq_hash_table against multiple simultaneous
   insertions.  */
gl_lock_define_initialized(static, struniq_lock)

/* Store a copy of the given string in a string pool with indefinite extent.
   Return a pointer to this copy.  */
static const char *
struniq (const char *string)
{
  size_t hashcode = string_hash (string);
  size_t slot = hashcode % STRUNIQ_HASH_TABLE_SIZE;
  size_t size;
  struct struniq_hash_node *new_node;
  struct struniq_hash_node *p;
  for (p = struniq_hash_table[slot]; p != NULL; p = p->next)
    if (strcmp (p->contents, string) == 0)
      return p->contents;
  size = strlen (string) + 1;
  new_node =
    (struct struniq_hash_node *)
    malloc (FLEXSIZEOF (struct struniq_hash_node, contents, size));
  if (new_node == NULL)
    /* Out of memory.  Return a statically allocated string.  */
    return "C";
  memcpy (new_node->contents, string, size);
  {
    bool mt = gl_multithreaded ();
    /* Lock while inserting new_node.  */
    if (mt) gl_lock_lock (struniq_lock);
    /* Check whether another thread already added the string while we were
       waiting on the lock.  */
    for (p = struniq_hash_table[slot]; p != NULL; p = p->next)
      if (strcmp (p->contents, string) == 0)
        {
          free (new_node);
          new_node = p;
          goto done;
        }
    /* Really insert new_node into the hash table.  Fill new_node entirely
       first, because other threads may be iterating over the linked list.  */
    new_node->next = struniq_hash_table[slot];
    struniq_hash_table[slot] = new_node;
   done:
    /* Unlock after new_node is inserted.  */
    if (mt) gl_lock_unlock (struniq_lock);
  }
  return new_node->contents;
}
