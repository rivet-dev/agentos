/*
  Copyright (c) 1990-2008 Info-ZIP.  All rights reserved.

  See the accompanying file LICENSE, version 2007-Mar-04 or later
  (the contents of which are also included in unzip.h) for terms of use.
  If, for some reason, all these files are missing, the Info-ZIP license
  also may be found at:  ftp://ftp.info-zip.org/pub/infozip/license.html
*/
/* vmmvs.h:  include file for both VM/CMS and MVS ports of UnZip */
#ifndef __vmmvs_h               /* prevent multiple inclusions */
#define __vmmvs_h

#ifndef NULL
#  define NULL (zvoid *)0
#endif

#ifdef MVS
#  define _POSIX_SOURCE    /* tell MVS we want full definitions */
#  define NO_STRNICMP      /* MVS has no strnicmp() */
#  include <features.h>
   /* MVS complains if a function has the same name as a csect. */
#  if defined(__UNZIP_C)
#    pragma csect(STATIC,"unzip_s")
#  elif defined(__CRC32_C)
#    pragma csect(STATIC,"crc32_s")
#  elif defined(__ENVARGS_C)
#    pragma csect(STATIC,"envarg_s")
#  elif defined(__EXPLODE_C)
#    pragma csect(STATIC,"explod_s")
#  elif defined(__INFLATE_C)
#    pragma csect(STATIC,"inflat_s")
#  elif defined(__MATCH_C)
#    pragma csect(STATIC,"match_s")
#  elif defined(__UNREDUCE_C)
#    pragma csect(STATIC,"unredu_s")
#  elif defined(__UNSHRINK_C)
#    pragma csect(STATIC,"unshri_s")
#  elif defined(__ZIPINFO_C)
#    pragma csect(STATIC,"zipinf_s")
#  endif
#endif /* MVS */

#include <time.h>               /* the usual non-BSD time functions */
#ifdef VM_CMS
#  include "vmstat.h"
#endif
#ifdef MVS
#  include <sys/stat.h>
#endif

#define PASSWD_FROM_STDIN
                  /* Kludge until we know how to open a non-echo tty channel */

#define EBCDIC
/* In the context of Info-ZIP, a portable "text" mode file implies the use of
   an ASCII-compatible (ISO 8859-1, or other extended ASCII) code page. */

#ifdef MORE
#  undef MORE
#endif

/* Workarounds for missing RTL functionality */
#define isatty(t) 1

#ifdef UNZIP                    /* definitions for UNZIP */

#define INBUFSIZ 8192

#define USE_STRM_INPUT
#define USE_FWRITE

#define PATH_MAX 128

#ifndef QUERY_TRNEWLN
#  define QUERY_TRNEWLN         /* terminate interaction queries with '\n' */
#endif

#ifndef DATE_FORMAT
#  define DATE_FORMAT DF_MDY
#endif
#define lenEOL        1
/* The use of "ebcdic[LF]" is not reliable; VM/CMS C/370 uses the
 * EBCDIC specific "NL" ('NewLine') control character (and not the EBCDIC
 * equivalent of the ASCII "LF" ('LineFeed')) as line terminator!
 * To work around this problem, we explicitely emit the C compiler's native
 * '\n' line terminator.
 */
#if 0
#define PutNativeEOL  *q++ = native(LF);
#else
#define PutNativeEOL  *q++ = '\n';
#endif

#endif /* UNZIP */

#endif /* !__vmmvs_h */
