/*
  Copyright (c) 1990-2009 Info-ZIP.  All rights reserved.

  See the accompanying file LICENSE, version 2009-Jan-02 or later
  (the contents of which are also included in unzip.h) for terms of use.
  If, for some reason, all these files are missing, the Info-ZIP license
  also may be found at:  ftp://ftp.info-zip.org/pub/infozip/license.html
*/
/*---------------------------------------------------------------------------
    Unix specific configuration section:
  ---------------------------------------------------------------------------*/

#ifndef __unxcfg_h
#define __unxcfg_h


/* LARGE FILE SUPPORT - 10/6/04 EG */
/* This needs to be set before the includes so they set the right sizes */

#if (defined(NO_LARGE_FILE_SUPPORT) && defined(LARGE_FILE_SUPPORT))
#  undef LARGE_FILE_SUPPORT
#endif

/* Automatically set ZIP64_SUPPORT if LFS */
#ifdef LARGE_FILE_SUPPORT
# if (!defined(NO_ZIP64_SUPPORT) && !defined(ZIP64_SUPPORT))
#   define ZIP64_SUPPORT
# endif
#endif

/* NO_ZIP64_SUPPORT takes preceedence over ZIP64_SUPPORT */
#if defined(NO_ZIP64_SUPPORT) && defined(ZIP64_SUPPORT)
#  undef ZIP64_SUPPORT
#endif

#ifdef LARGE_FILE_SUPPORT
  /* 64-bit Large File Support */

  /* The following Large File Summit (LFS) defines turn on large file support
     on Linux (probably 2.4 or later kernel) and many other unixen */

  /* These have to be before any include that sets types so the large file
     versions of the types are set in the includes */

# define _LARGEFILE_SOURCE      /* some OSes need this for fseeko */
# define _LARGEFILE64_SOURCE
# define _FILE_OFFSET_BITS 64   /* select default interface as 64 bit */
# define _LARGE_FILES           /* some OSes need this for 64-bit off_t */
# define __USE_LARGEFILE64
#endif /* LARGE_FILE_SUPPORT */


#include <sys/types.h>          /* off_t, time_t, dev_t, ... */
#include <sys/stat.h>

#ifdef NO_OFF_T
  typedef long zoff_t;
#else
  typedef off_t zoff_t;
#endif
#define ZOFF_T_DEFINED
typedef struct stat z_stat;
#define Z_STAT_DEFINED

#ifndef COHERENT
#  include <fcntl.h>            /* O_BINARY for open() w/o CR/LF translation */
#else /* COHERENT */
#  ifdef _I386
#    include <fcntl.h>          /* Coherent 4.0.x, Mark Williams C */
#  else
#    include <sys/fcntl.h>      /* Coherent 3.10, Mark Williams C */
#  endif
#  define SHORT_SYMS
#  ifndef __COHERENT__          /* Coherent 4.2 has tzset() */
#    define tzset  settz
#  endif
#endif /* ?COHERENT */

#ifndef NO_PARAM_H
#  ifdef NGROUPS_MAX
#    undef NGROUPS_MAX      /* SCO bug:  defined again in <sys/param.h> */
#  endif
#  ifdef BSD
#    define TEMP_BSD        /* may be defined again in <sys/param.h> */
#    undef BSD
#  endif
#  include <sys/param.h>    /* conflict with <sys/types.h>, some systems? */
#  ifdef TEMP_BSD
#    undef TEMP_BSD
#    ifndef BSD
#      define BSD
#    endif
#  endif
#endif /* !NO_PARAM_H */

#ifdef __osf__
#  define DIRENT
#  ifdef BSD
#    undef BSD
#  endif
#endif /* __osf__ */

#ifdef __CYGWIN__
#  include <unistd.h>
#  define DIRENT
#  define HAVE_TERMIOS_H
#  ifndef timezone
#    define timezone _timezone
#  endif
#endif

#ifdef BSD
#  include <sys/time.h>
#  include <sys/timeb.h>
#  if (defined(_AIX) || defined(__GLIBC__) || defined(__GNU__))
#    include <time.h>
#  endif
#else
#  include <time.h>
   struct tm *gmtime(), *localtime();
#endif

#if (defined(BSD4_4) || (defined(SYSV) && defined(MODERN)))
#  include <unistd.h>           /* this includes utime.h on SGIs */
#  if (defined(BSD4_4) || defined(linux) || defined(__GLIBC__))
#    include <utime.h>
#    define GOT_UTIMBUF
#  endif
#  if (!defined(GOT_UTIMBUF) && (defined(__hpux) || defined(__SUNPRO_C)))
#    include <utime.h>
#    define GOT_UTIMBUF
#  endif
#  if (!defined(GOT_UTIMBUF) && defined(__GNU__))
#    include <utime.h>
#    define GOT_UTIMBUF
#  endif
#endif
#if (defined(__DGUX__) && !defined(GOT_UTIMBUF))
   /* DG/UX requires this because of a non-standard struct utimebuf */
#  include <utime.h>
#  define GOT_UTIMBUF
#endif

#if (defined(V7) || defined(pyr_bsd))
#  define strchr   index
#  define strrchr  rindex
#endif
#ifdef V7
#  define O_RDONLY 0
#  define O_WRONLY 1
#  define O_RDWR   2
#endif

#if defined(NO_UNICODE_SUPPORT) && defined(UNICODE_SUPPORT)
   /* disable Unicode (UTF-8) support when requested */
#  undef UNICODE_SUPPORT
#endif

#if (defined(_MBCS) && defined(NO_MBCS))
   /* disable MBCS support when requested */
#  undef _MBCS
#endif

#if (!defined(NO_SETLOCALE) && !defined(_MBCS))
# if (!defined(UNICODE_SUPPORT) || !defined(UTF8_MAYBE_NATIVE))
   /* enable setlocale here, unless this happens later for UTF-8 and/or
    * MBCS support */
#  include <locale.h>
#  ifndef SETLOCALE
#    define SETLOCALE(category, locale) setlocale(category, locale)
#  endif
# endif
#endif
#ifndef NO_SETLOCALE
# if (!defined(NO_WORKING_ISPRINT) && !defined(HAVE_WORKING_ISPRINT))
   /* enable "enhanced" unprintable chars detection in fnfilter() */
#  define HAVE_WORKING_ISPRINT
# endif
#endif

#ifdef MINIX
#  include <stdio.h>
#endif
#if (!defined(HAVE_STRNICMP) & !defined(NO_STRNICMP))
#  define NO_STRNICMP
#endif
#ifndef DATE_FORMAT
#  define DATE_FORMAT DF_MDY    /* GRR:  customize with locale.h somehow? */
#endif
#define lenEOL          1
#ifdef EBCDIC
#  define PutNativeEOL  *q++ = '\n';
#else
#  define PutNativeEOL  *q++ = native(LF);
#endif
#define SCREENSIZE(ttrows, ttcols)  screensize(ttrows, ttcols)
#define SCREENWIDTH     80
#define SCREENLWRAP     1
#define USE_EF_UT_TIME
#if (!defined(NO_LCHOWN) || !defined(NO_LCHMOD))
#  define SET_SYMLINK_ATTRIBS
#endif
#ifdef MTS
#  ifdef SET_DIR_ATTRIB
#    undef SET_DIR_ATTRIB
#  endif
#else /* !MTS */
#  define SET_DIR_ATTRIB
#  if (!defined(NOTIMESTAMP) && !defined(TIMESTAMP))   /* GRR 970513 */
#    define TIMESTAMP
#  endif
#  define RESTORE_UIDGID
#endif /* ?MTS */

/* Static variables that we have to add to Uz_Globs: */
#define SYSTEM_SPECIFIC_GLOBALS \
    int created_dir, renamed_fullpath;\
    char *rootpath, *buildpath, *end;\
    ZCONST char *wildname;\
    char *dirname, matchname[FILNAMSIZ];\
    int rootlen, have_dirname, dirnamelen, notfirstcall;\
    zvoid *wild_dir;

/* created_dir, and renamed_fullpath are used by both mapname() and    */
/*    checkdir().                                                      */
/* rootlen, rootpath, buildpath and end are used by checkdir().        */
/* wild_dir, dirname, wildname, matchname[], dirnamelen, have_dirname, */
/*    and notfirstcall are used by do_wild().                          */

#endif /* !__unxcfg_h */
