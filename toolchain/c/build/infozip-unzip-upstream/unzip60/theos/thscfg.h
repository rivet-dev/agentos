/*
  Copyright (c) 1990-2000 Info-ZIP.  All rights reserved.

  See the accompanying file LICENSE, version 2000-Apr-09 or later
  (the contents of which are also included in unzip.h) for terms of use.
  If, for some reason, all these files are missing, the Info-ZIP license
  also may be found at:  ftp://ftp.info-zip.org/pub/infozip/license.html
*/
#ifndef __theos_thscfg_h
#define __theos_thscfg_h

#include <unistd.h>        /* dup proto & unix system calls live here */
#define _HANDLE_DEFINED
#include <fcntl.h>         /* O_BINARY for open() w/o CR/LF translation */
#include <conio.h>
#include <time.h>
#include <sys/types.h>     /* off_t, time_t, dev_t, ... */
#include "theos/stat.h"
#include <sys/utime.h>
#define GOT_UTIMBUF
#define DATE_FORMAT   dateformat()
#define SCREENLINES   screenlines()
#define USE_EF_UT_TIME
#define DIR_END '/'
#define INT_SPRINTF
#define lenEOL        1
#define PutNativeEOL  *q++ = native(CR);
#define PIPE_ERROR (errno = 9999)
#define isatty(a) _isatty(a)
#undef match
int open(const char *name, int mode, ...);
size_t read(int fd, char *buf, size_t len);
size_t write(int fd, char *buf, size_t len);
unsigned long lseek(int fd, long offset, int base);
int close(int);
int _fprintf(FILE *fp, const char *fmt, ...);
int _isatty(int);
typedef unsigned char uid_t;
typedef unsigned char gid_t;

extern int _sprintf(char *s, const char *fmt, ...);

#ifndef NO_BOGUS_SPC
# include <stdio.h>
# undef fprintf
# undef sprintf
# define fprintf _fprintf
# define sprintf _sprintf
#else /* NO_BOGUS_SPC */
#ifndef Info   /* may already have been defined for redirection */
#  ifdef FUNZIP
#    define Info(buf,flag,sprf_arg) \
     fprintf((flag)&1? stderr : stdout, (char *)(_sprintf sprf_arg, (buf)))
#  else
#    define Info(buf,flag,sprf_arg) \
     UzpMessagePrnt((zvoid *)&G, (uch *)(buf), (ulg)_sprintf sprf_arg, (flag))
#  endif
#endif /* !Info */
#endif /* ?NO_BOGUS_SPC */

#endif /* !__theos_thscfg_h */

