/*
  Copyright (c) 1990-2004 Info-ZIP.  All rights reserved.

  See the accompanying file LICENSE, version 2000-Apr-09 or later
  (the contents of which are also included in unzip.h) for terms of use.
  If, for some reason, all these files are missing, the Info-ZIP license
  also may be found at:  ftp://ftp.info-zip.org/pub/infozip/license.html
*/
/*---------------------------------------------------------------------------
    AtheOS/Syllable specific configuration section:
  ---------------------------------------------------------------------------*/

#ifndef __athcfg_h
#define __athcfg_h

   /* ensure that Unix-specific code portions are excluded */
#ifdef UNIX
#  undef UNIX
#endif
#include <sys/types.h>          /* off_t, time_t, dev_t, ... */
#include <sys/stat.h>
#include <sys/param.h>
#include <fcntl.h>              /* O_BINARY for open() w/o CR/LF translation */
#include <limits.h>
#include <time.h>
#include <unistd.h>
#include <utime.h>

#define GOT_UTIMBUF
#define DIRENT
#if (!defined(HAVE_STRNICMP) & !defined(NO_STRNICMP))
#  define NO_STRNICMP
#endif
#define INT_SPRINTF
#define SYMLINKS

#ifndef DATE_FORMAT
#  define DATE_FORMAT DF_MDY    /* GRR:  customize with locale.h somehow? */
#endif
#define lenEOL          1
#define PutNativeEOL    *q++ = native(LF);
#define SCREENSIZE(ttrows, ttcols)  screensize(ttrows, ttcols)
#define SCREENWIDTH     80
#define SCREENLWRAP     1
#if (!defined(NO_EF_UT_TIME) && !defined(USE_EF_UT_TIME))
#  define USE_EF_UT_TIME
#endif
#define SET_SYMLINK_ATTRIBS
#define SET_DIR_ATTRIB
#if (!defined(NOTIMESTAMP) && !defined(TIMESTAMP))
#  define TIMESTAMP
#endif
#define RESTORE_UIDGID

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

#endif /* !__athcfg_h */
