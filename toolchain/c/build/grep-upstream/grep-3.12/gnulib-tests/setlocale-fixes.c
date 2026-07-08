/* Make the global locale minimally POSIX compliant.
   Copyright (C) 2025 Free Software Foundation, Inc.

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

#include <config.h>

/* Specification.  */
#include "setlocale-fixes.h"


#if defined _WIN32 && !defined __CYGWIN__

# include <stddef.h>
# include <string.h>

/* The system does not store an LC_MESSAGES locale category.  Do it here.  */
static char lc_messages_name[64] = "C";

const char *
setlocale_messages (const char *name)
{
  if (name != NULL)
    {
      lc_messages_name[sizeof (lc_messages_name) - 1] = '\0';
      strncpy (lc_messages_name, name, sizeof (lc_messages_name) - 1);
    }
  return lc_messages_name;
}

const char *
setlocale_messages_null (void)
{
  /* This implementation is multithread-safe, assuming no other thread changes
     the LC_MESSAGES locale category.  */
  return lc_messages_name;
}

#endif


#if defined __ANDROID__

# include <stddef.h>
# include <stdlib.h>
# include <locale.h>
# include <string.h>

/* Use the system's setlocale() function, not the gnulib override, here.  */
# undef setlocale

/* Storage for the name of each category of the global locale.
   Regarding the size, cf. SETLOCALE_NULL_MAX.  */
static char lc_cat_name[12][256+1] =
  { "C", "C", "C", "C", "C", "C", "C", "C", "C", "C", "C", "C" };
/* Mapping from the valid categories to the indices 0..12-1.  */
#define cat_to_index(cat) ((cat) - ((cat) >= LC_ALL))
/* Mapping from the indicess 0..12-1 to the valid categories.  */
#define index_to_cat(i) ((i) + ((i) >= LC_ALL))

/* Storage for the name of the entire global locale.
   For mixed locales, use the syntax from glibc:
   LC_CTYPE=...;LC_NUMERIC=...;......;LC_IDENTIFICATION=...
   Regarding the size, cf. SETLOCALE_NULL_ALL_MAX.  */
static char lc_all_name[148+12*256+1];

/* Category names.  */
static const char cat_names[12][17+1] =
  {
    [cat_to_index (LC_CTYPE)]          = "LC_CTYPE",
    [cat_to_index (LC_NUMERIC)]        = "LC_NUMERIC",
    [cat_to_index (LC_TIME)]           = "LC_TIME",
    [cat_to_index (LC_COLLATE)]        = "LC_COLLATE",
    [cat_to_index (LC_MONETARY)]       = "LC_MONETARY",
    [cat_to_index (LC_MESSAGES)]       = "LC_MESSAGES",
    [cat_to_index (LC_PAPER)]          = "LC_PAPER",
    [cat_to_index (LC_NAME)]           = "LC_NAME",
    [cat_to_index (LC_ADDRESS)]        = "LC_ADDRESS",
    [cat_to_index (LC_TELEPHONE)]      = "LC_TELEPHONE",
    [cat_to_index (LC_MEASUREMENT)]    = "LC_MEASUREMENT",
    [cat_to_index (LC_IDENTIFICATION)] = "LC_IDENTIFICATION",
  };

/* Combines the contents of all lc_cat_name[], filling lc_all_name.  */
static void
fill_lc_all_name (void)
{
  bool all_same = true;
  for (size_t i = 1; i < 12; i++)
    if (strcmp (lc_cat_name[i], lc_cat_name[0]) != 0)
      {
        all_same = false;
        break;
      }
  if (all_same)
    /* Produce a simple locale name.  */
    strcpy (lc_all_name, lc_cat_name[0]);
  else
    {
      /* Produce a mixed locale name.  */
      size_t i;
      char *q;
      for (i = 0, q = lc_all_name; i < 12; i++)
        {
          const char *p = cat_names[i];
          size_t n = strlen (p);
          memcpy (q, p, n); q += n;
          *q++ = '=';
          p = lc_cat_name[i];
          n = strlen (p);
          memcpy (q, p, n); q += n;
          *q++ = ';';
        }
      *--q = '\0';
    }
}

/* Extracts the name of a single category from NAME and stores it in
   SINGLE_NAME.  CATEGORY must be a valid category name != LC_ALL.
   This is basically the opposite of fill_lc_all_name.  */
static int
extract_single_name (char single_name[256+1], int category, const char *name)
{
  if (strchr (name, ';') != NULL)
    {
      /* A mixed locale name.  */
      const char *cat_name = cat_names[cat_to_index (category)];
      size_t cat_name_len = strlen (cat_name);
      const char *p = name;
      for (;;)
        {
          const char *q = strchr (p, ';');
          if (strncmp (p, cat_name, cat_name_len) == 0
              && p[cat_name_len] == '=')
            {
              p += cat_name_len + 1;
              size_t n = (q == NULL ? strlen (p) : q - p);
              if (n >= 256+1)
                n = 256;
              memcpy (single_name, p, n);
              single_name[n] = '\0';
              return 0;
            }
          if (q == NULL)
            break;
          p = q + 1;
        }
      return -1;
    }
  else
    {
      /* A simple locale name.  */
      size_t n = strlen (name);
      if (n >= 256+1)
        n = 256;
      memcpy (single_name, name, n);
      single_name[n] = '\0';
      return 0;
    }
}

const char *
setlocale_fixed (int category, const char *name)
{
  if (category == LC_ALL)
    {
      if (name != NULL)
        {
          char single_name[256+1];
          int i;

          /* Test whether NAME is valid.  */
          for (i = 12-1; i >= 0; i--)
            if (extract_single_name (single_name, index_to_cat (i), name) < 0)
              return NULL;
          /* Now single_name contains the one for the index 0,
             i.e. for LC_CTYPE.  */
          if (setlocale (LC_CTYPE, single_name) == NULL)
            return NULL;

          /* Fill lc_cat_name[].  */
          for (i = 12-1; i >= 0; i--)
            {
              if (extract_single_name (single_name, index_to_cat (i), name) < 0)
                abort ();
              strcpy (lc_cat_name[i], single_name);
            }
          /* Update lc_all_name.  */
          fill_lc_all_name ();
        }

      return lc_all_name;
    }
  if (category >= LC_CTYPE && category <= LC_IDENTIFICATION)
    {
      if (name != NULL)
        {
          char single_name[256+1];

          /* Test whether NAME is valid.  */
          if (extract_single_name (single_name, category, name) < 0)
            return NULL;
          if (category == LC_CTYPE)
            if (setlocale (category, single_name) == NULL)
              return NULL;

          /* Fill lc_cat_name[].  */
          strcpy (lc_cat_name[cat_to_index (category)], single_name);
          /* Update lc_all_name.  */
          fill_lc_all_name ();
        }

      return lc_cat_name[cat_to_index (category)];
    }
  return NULL;
}

const char *
setlocale_fixed_null (int category)
{
  /* This implementation is multithread-safe, assuming no other thread changes
     any locale category.  */
  if (category == LC_ALL)
    return lc_all_name;
  if (category >= LC_CTYPE && category <= LC_IDENTIFICATION)
    return lc_cat_name[cat_to_index (category)];
  return NULL;
}

#endif
