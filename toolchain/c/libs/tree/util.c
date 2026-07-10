/* $Copyright: $
 * Copyright (c) 1996 - 2026 by Steve Baker (steve.baker.llc@gmail.com)
 *
 * This program is free software; you can redistribute it and/or modify
 * it under the terms of the GNU General Public License as published by
 * the Free Software Foundation; either version 2 of the License, or
 * (at your option) any later version.
 *
 * This program is distributed in the hope that it will be useful,
 * but WITHOUT ANY WARRANTY; without even the implied warranty of
 * MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE.  See the
 * GNU General Public License for more details.
 *
 * You should have received a copy of the GNU General Public License
 * along with this program; if not, write to the Free Software
 * Foundation, Inc., 59 Temple Place, Suite 330, Boston, MA  02111-1307  USA
 */
#include "tree.h"

extern char *file_pathsep;

/**
 * Like strpcat, but also has the start and end of the buffer being written to
 * to bound access.  Attempts to eliminate repeated path separators.
 */
char *pathnpcat(char *dst, char *src, char *start, char *end)
{
  while (dst < end && *src) {
    // Don't allow / repeats:
    if (dst > start && strchr(file_pathsep, *(dst-1)) != NULL && strchr(file_pathsep, *src) != NULL) {
      src++;
      continue;
    }
    *(dst++) = *(src++);
  }
  *dst = '\0';
  return dst;
}

/**
 * Concatenates variable number of paths together, attempting to eliminate
 * repeated path separators. Argument list terminated with a NULL.  Path is
 * returned in a static buffer, so must be scopy'ed if it's necessary to
 * preserve it.
 */
char *pathconcat(char *str, ...)
{
  static char buf[PATH_MAX+1];
  char *p, *s, *limit = buf + PATH_MAX;
  // If str == buf, then we're appending to the path already in buf:
  if (str == buf) {
    p = strchr(buf, '\0');
  } else {
    buf[0] = 0;
    p = pathnpcat(buf, str, buf, limit);
  }

  va_list ap;
  va_start(ap, str);
  while ((s = va_arg(ap, char *)) != NULL) {
    p = pathnpcat(p, file_pathsep, buf, limit);
    p = pathnpcat(p, s, buf, limit);
    if (p == limit) break;
  }
  va_end(ap);
  return buf;
}

/**
 * Returns true if the directory is a singleton (has exactly one child that is
 * itself a directory.)
 */
bool is_singleton(struct _info *dir)
{
  if (dir->child == NULL) return false;
  if (dir->child[0] == NULL) return false;
  if (dir->child[1] != NULL) return false;
  return dir->child[0]->isdir;
}

void *xmalloc (size_t size)
{
  register void *value = malloc (size);
  if (value == NULL) {
    fprintf(stderr,"tree: virtual memory exhausted.\n");
    exit(1);
  }
  return value;
}

void *xrealloc (void *ptr, size_t size)
{
  register void *value = realloc (ptr,size);
  if (value == NULL) {
    fprintf(stderr,"tree: virtual memory exhausted.\n");
    exit(1);
  }
  return value;
}
