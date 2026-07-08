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

#define _GNU_SOURCE

#include <stdlib.h>
#include <stdio.h>
#include <stdarg.h>
#include <time.h>
#include <string.h>
#include <strings.h>
#include <sys/time.h>
#include <sys/types.h>
#include <sys/stat.h>
#include <dirent.h>
#include <ctype.h>
#include <unistd.h>
#include <limits.h>
#include <pwd.h>
#include <grp.h>
#ifdef __EMX__  /* for OS/2 systems */
#  define INCL_DOSFILEMGR
#  define INCL_DOSNLS
#  include <os2.h>
#  include <sys/nls.h>
#  include <io.h>
  /* On many systems stat() function is identical to lstat() function.
   * But the OS/2 does not support symbolic links and doesn't have lstat() function.
   */
#  define         lstat          stat
#  define         strcasecmp     stricmp
  /* Following two functions, getcwd() and chdir() don't support for drive letters.
   * To implement support them, use _getcwd2() and _chdir2().
   */
#  define getcwd _getcwd2
#  define chdir _chdir2
#endif

#include <locale.h>
#include <langinfo.h>
#include <wchar.h>
#include <wctype.h>
#include <stdbool.h>

#ifdef __ANDROID
#define mbstowcs(w,m,x) mbsrtowcs(w,(const char**)(& #m),x,NULL)
#endif

/* Start using PATH_MAX instead of the magic number 4096 everywhere. */
#ifndef PATH_MAX
#define PATH_MAX 4096
#endif

#ifndef INFO_PATH
#define INFO_PATH "/usr/share/finfo/global_info"
#endif

#ifdef __linux__
#include <sys/xattr.h>
#include <fcntl.h>
# define ENV_STDDATA_FD  "STDDATA_FD"
# ifndef STDDATA_FILENO
#  define STDDATA_FILENO 3
# endif
#endif

/* Should probably use strdup(), but we like our xmalloc() */
#define scopy(x)	strcpy(xmalloc(strlen(x)+1),(x))
#define MINIT		30	/* number of dir entries to initially allocate */
#define MINC		20	/* allocation increment */

#define UNUSED(x)	((void)x)

/* tree.c / global */
struct Flags {
  // TODO: Change these single letter flags to more meaningful names
  bool a, c, d, f, g, h, l, p, q, s, u;
  bool D, F, H, J, N, Q, R, X;
  bool inode, dev, si, du, prune, hyper;
  bool noindent, force_color, nocolor, xdev, noreport, nolinks;
  bool ignorecase, matchdirs, fromfile, metafirst, gitignore, showinfo;
  bool reverse, fflinks, htmloffset, acl, selinux, condense_singletons;
  bool colorize, ansilines, linktargetcolor, remove_space;
  int flimit, compress_indent;
};

struct _info {
  char *name;
  char *lnk;
  bool isdir;
  bool issok;
  bool isfifo;
  bool isexe;
  bool orphan;
#ifdef __linux__
  bool hasacl;
  char *secontext;
#endif
  mode_t mode, lnkmode;
  uid_t uid;
  gid_t gid;
  off_t size;
  time_t atime, ctime, mtime;
  dev_t dev, ldev;
  ino_t inode, linode;
  #ifdef __EMX__
  long attr;
  #endif
  char *err;
  const char *tag;
  size_t condensed;
  char **comment;
  struct _info **child, *next, *tchild;
};

/* color.c */
struct extensions {
  char *ext;
  char *term_flg;
  struct extensions *nxt;
};
struct linedraw {
  const char **name, *copy, *vert[3], *vert_left[3], *corner[3];
  const char *ctop, *cbot, *cmid, *cext, *csingle;
};
struct meta_ids {
  char *name;
  char *term_flg;
};

/* filter.c */
struct pattern {
  char *pattern;
  int relative;
  struct pattern *next;
};

struct ignorefile {
  char *path;
  struct pattern *remove, *reverse;
  struct ignorefile *next;
};

/* info.c */
struct comment {
  struct pattern *pattern;
  char **desc;
  struct comment *next;
};

struct infofile {
  char *path;
  struct comment *comments;
  struct infofile *next;
};

/* list.c */
struct totals {
  size_t files, dirs;
  off_t size;
};

struct listingcalls {
  void (*intro)(void);
  void (*outtro)(void);
  int (*printinfo)(char *dirname, struct _info *file, int level);
  int (*printfile)(char *dirname, char *filename, struct _info *file, int descend);
  int (*error)(char *error);
  void (*newline)(struct _info *file, int level, int postdir, int needcomma);
  void (*close)(struct _info *file, int level, int needcomma);
  void (*report)(struct totals tot);
};

/* Function prototypes: */
/* color.c */
void parse_dir_colors(void);
bool color(mode_t mode, const char *name, bool orphan, bool islink);
void endcolor(void);
void fancy(FILE *out, char *s);
const char *getcharset(void);
void initlinedraw(bool help);

/* file.c */
struct _info **file_getfulltree(char *d, u_long lev, dev_t dev, off_t *size, char **err);
struct _info **tabedfile_getfulltree(char *d, u_long lev, dev_t dev, off_t *size, char **err);

/* filter.c */
void gittrim(char *s);
struct pattern *new_pattern(char *pattern);
struct ignorefile *gitignore_search(const char *startpath, int depth);
bool filtercheck(const char *path, const char *name, int isdir);
struct ignorefile *new_ignorefile(const char *basepath, const char *path, bool checkparents);
void push_filterstack(struct ignorefile *ig);
struct ignorefile *pop_filterstack(void);
struct ignorefile *flush_filterstack(void);

/* hash.c */
void init_hashes(void);
char *uidtoname(uid_t uid);
char *gidtoname(gid_t gid);
bool findino(ino_t, dev_t);
void saveino(ino_t, dev_t);
#ifdef __linux__
char *strhash(char *str);
#endif

/* html.c */
bool url_encode(FILE *fd, char *s);
void html_intro(void);
void html_outtro(void);
int html_printinfo(char *dirname, struct _info *file, int level);
int html_printfile(char *dirname, char *filename, struct _info *file, int descend);
int html_error(char *error);
void html_newline(struct _info *file, int level, int postdir, int needcomma);
void html_close(struct _info *file, int level, int needcomma);
void html_report(struct totals tot);
void html_encode(FILE *fd, char *s);

/* info.c */
struct infofile *new_infofile(const char *path, bool checkparents);
void push_infostack(struct infofile *inf);
struct infofile *pop_infostack(void);
struct comment *infocheck(const char *path, const char *name, int top, bool isdir);
void printcomment(size_t line, size_t lines, char *s);

/* json.c */
void json_indent(int maxlevel);
void json_fillinfo(struct _info *ent);
void json_intro(void);
void json_outtro(void);
int json_printinfo(char *dirname, struct _info *file, int level);
int json_printfile(char *dirname, char *filename, struct _info *file, int descend);
int json_error(char *error);
void json_newline(struct _info *file, int level, int postdir, int needcomma);
void json_close(struct _info *file, int level, int needcomma);
void json_report(struct totals tot);

/* list.c */
void null_intro(void);
void null_outtro(void);
void null_close(struct _info *file, int level, int needcomma);
void emit_tree(char **dirname, bool needfulltree);
struct totals listdir(char *dirname, struct _info **dir, int lev, dev_t dev, bool hasfulltree);

/* tree.c */
void setoutput(const char *filename);
void print_version(int nl);
void usage(int);
void push_files(const char *dir, struct ignorefile **ig, struct infofile **inf, bool top);
int patignore(const char *name, bool isdir, bool checkpaths);
int patinclude(const char *name, bool isdir, bool checkpaths);
struct _info **unix_getfulltree(char *d, u_long lev, dev_t dev, off_t *size, char **err);
struct _info **read_dir(char *dir, ssize_t *n, int infotop);

int filesfirst(struct _info **, struct _info **);
int dirsfirst(struct _info **, struct _info **);
int alnumsort(struct _info **, struct _info **);
int versort(struct _info **a, struct _info **b);
int reversealnumsort(struct _info **, struct _info **);
int mtimesort(struct _info **, struct _info **);
int ctimesort(struct _info **, struct _info **);
int sizecmp(off_t a, off_t b);
int fsizesort(struct _info **a, struct _info **b);

int patmatch(const char *buf, const char *pat, bool isdir);
void indent(int maxlevel);
void free_dir(struct _info **);
#ifdef __EMX__
char *prot(long);
#else
char *prot(mode_t);
#endif
char *do_date(time_t);
void printit(const char *);
int psize(char *buf, off_t size);
char Ftype(mode_t mode);
struct _info *stat2info(const struct stat *st);
char *fillinfo(char *buf, const struct _info *ent);

/* unix.c */
int unix_printinfo(char *dirname, struct _info *file, int level);
int unix_printfile(char *dirname, char *filename, struct _info *file, int descend);
int unix_error(char *error);
void unix_newline(struct _info *file, int level, int postdir, int needcomma);
void unix_report(struct totals tot);

/* util.c */
char *pathconcat(char *str, ...);
bool is_singleton(struct _info *dir);
void *xmalloc(size_t);
void *xrealloc(void *, size_t);

/* xml.c */
void xml_intro(void);
void xml_outtro(void);
int xml_printinfo(char *dirname, struct _info *file, int level);
int xml_printfile(char *dirname, char *filename, struct _info *file, int descend);
int xml_error(char *error);
void xml_newline(struct _info *file, int level, int postdir, int needcomma);
void xml_close(struct _info *file, int level, int needcomma);
void xml_report(struct totals tot);

/* We use the strverscmp.c file if we're not linux: */
#if !defined(__linux__) || defined(__ANDROID__)
int strverscmp (const char *s1, const char *s2);
#endif
