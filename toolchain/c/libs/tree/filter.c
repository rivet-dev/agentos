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

extern char xpattern[PATH_MAX];

struct ignorefile *filterstack = NULL;

void gittrim(char *s)
{
  ssize_t i, e = (ssize_t)strlen(s)-1;

  if (e < 0) return;
  while (e > 0 && (s[e] == '\n' || s[e] == '\r')) e--;

  for(i = e; i >= 0; i--) {
    if (s[i] != ' ') break;
    if (i && s[i-1] != '\\') e--;
  }
  s[e+1] = '\0';
  for(i = e = 0; s[i] != '\0';) {
    if (s[i] == '\\') i++;
    s[e++] = s[i++];
  }
  s[e] = '\0';
}

struct pattern *new_pattern(char *pattern)
{
  struct pattern *p = xmalloc(sizeof(struct pattern));
  char *sl;

  p->pattern = scopy(pattern + ((pattern[0] == '/')? 1 : 0));
  sl = strchr(pattern, '/');
  p->relative = (sl == NULL || (sl && !*(sl+1)));
  p->next = NULL;
  return p;
}

bool is_file(const char *path)
{
  struct stat st;
  if (stat(path, &st) < 0) return false;
  return S_ISREG(st.st_mode);
}

bool is_dir(const char *path)
{
  struct stat st;
  if (stat(path, &st) < 0) return false;
  return S_ISDIR(st.st_mode);
}

/**
 * Search up the directory tree for .gitignore files, stopping at a directory
 * that contains a .git directory, or at /, whichever occurs first. The depth
 * parameter is just a sanity check to insure we can't get into a loop somehow,
 * even though that should be impossible.
 */
struct ignorefile *gitignore_search(const char *startpath, int depth)
{
  struct ignorefile *pign = NULL, *ign = NULL;
  char path[PATH_MAX+1];

  // strcpy(rpath, startpath);

  // Stop when we hit a directory with a .git directory. we'll assume it's the
  // git root:
  snprintf(path, PATH_MAX, "%.*s/.git", PATH_MAX-6, startpath);
  if (is_dir(path)) {
    // Add it's .git/config/exclude
    snprintf(path, PATH_MAX, "%.*s/.git/info/exclude", PATH_MAX-21, startpath);
    if (is_file(path)) push_filterstack(pign = new_ignorefile(startpath, path, false));
  } else {
    if (realpath(startpath, path) == NULL) return NULL;
    if (strcmp(path, "/") != 0 && depth < 2048) {
      // Otherwise if we haven't reached /, then keep searching upward:
      snprintf(path, PATH_MAX, "%.*s/..", PATH_MAX-4, startpath);
      pign = gitignore_search(path, depth+1);
    }
  }

  snprintf(path, PATH_MAX, "%.*s/.gitignore", PATH_MAX-12, startpath);
  if (is_file(path)) push_filterstack(ign = new_ignorefile(startpath, path, false));

  return ign == NULL? pign : ign;
}

struct ignorefile *new_ignorefile(const char *basepath, const char *path, bool checkparents)
{
  char buf[PATH_MAX];
  struct ignorefile *ig;
  struct pattern *remove = NULL, *remend, *p;
  struct pattern *reverse = NULL, *revend;
  int rev;
  FILE *fp;

  if (!is_file(path)) {
    snprintf(buf, PATH_MAX, "%s/.gitignore", path);
    fp = fopen(buf, "r");

    // This probably will never actually happen anymore:
    if (fp == NULL && checkparents) {
      return gitignore_search(path, 0);
    }
  } else fp = fopen(path, "r");
  if (fp == NULL) return NULL;

  while (fgets(buf, PATH_MAX, fp) != NULL) {
    if (buf[0] == '#') continue;
    rev = (buf[0] == '!');
    gittrim(buf);
    if (strlen(buf) == 0) continue;
    p = new_pattern(buf + (rev? 1 : 0));
    // printf("Adding pattern: %c%s\n", rev? '!' : ' ', buf);
    if (rev) {
      if (reverse == NULL) reverse = revend = p;
      else {
	revend->next = p;
	revend = p;
      }
    } else {
      if (remove == NULL) remove = remend = p;
      else {
	remend->next = p;
	remend = p;
      }
    }
  }

  fclose(fp);

  ig = xmalloc(sizeof(struct ignorefile));
  ig->remove = remove;
  ig->reverse = reverse;
  ig->path = scopy(basepath);
  ig->next = NULL;

  return ig;
}

void push_filterstack(struct ignorefile *ig)
{
  if (ig == NULL) return;
  ig->next = filterstack;
  filterstack = ig;
}

struct ignorefile *pop_filterstack(void)
{
  struct ignorefile *ig;
  struct pattern *p, *c;

  ig = filterstack;
  if (ig == NULL) return NULL;

  filterstack = filterstack->next;

  for(p=c=ig->remove; p != NULL; c = p) {
    p=p->next;
    free(c->pattern);
  }
  for(p=c=ig->reverse; p != NULL; c = p) {
    p=p->next;
    free(c->pattern);
  }
  free(ig->path);
  free(ig);
  return NULL;
}

struct ignorefile *flush_filterstack(void)
{
  while (filterstack != NULL) pop_filterstack();
  return NULL;
}

/**
 * true if remove filter matches and no reverse filter matches.
 */
bool filtercheck(const char *path, const char *name, int isdir)
{
  bool filter = false;
  struct ignorefile *ig;
  struct pattern *p;

  // printf("Checking [%s / %s %d]\n", path, name, isdir);

  for(ig = filterstack; !filter && ig; ig = ig->next) {
    int fpos = sprintf(xpattern, "%s/", ig->path);

    for(p = ig->remove; p != NULL; p = p->next) {
      if (p->relative) {
	if (patmatch(name, p->pattern, isdir) == 1) {
	  filter = true;
	  // printf(" --r %s %s %d\n", name, p->pattern, filter);
	  break;
	}
      } else {
	sprintf(xpattern + fpos, "%s", p->pattern);
	if (patmatch(path, xpattern, isdir) == 1) {
	  filter = true;
	  // printf(" --a %s %s %d\n", name, xpattern, filter);
	  break;
	}
      }
     }
  }
  if (!filter) return false;

  for(ig = filterstack; ig; ig = ig->next) {
    int fpos = sprintf(xpattern, "%s/", ig->path);

    for(p = ig->reverse; p != NULL; p = p->next) {
      if (p->relative) {
	if (patmatch(name, p->pattern, isdir) == 1) return false;
      } else {
	sprintf(xpattern + fpos, "%s", p->pattern);
	if (patmatch(path, xpattern, isdir) == 1) return false;
      }
    }
  }

  return true;
}
