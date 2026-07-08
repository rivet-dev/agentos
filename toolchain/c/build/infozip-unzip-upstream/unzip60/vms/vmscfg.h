/*
  Copyright (c) 1990-2009 Info-ZIP.  All rights reserved.

  See the accompanying file LICENSE, version 2009-Jan-02 or later
  (the contents of which are also included in unzip.h) for terms of use.
  If, for some reason, all these files are missing, the Info-ZIP license
  also may be found at:  ftp://ftp.info-zip.org/pub/infozip/license.html
*/
/*---------------------------------------------------------------------------
    OpenVMS specific configuration section (included by unzpriv.h):
  ---------------------------------------------------------------------------*/

#ifndef __vmscfg_h   /* Prevent (unlikely) multiple inclusions. */
#define __vmscfg_h

/* Workaround for broken header files of older DECC distributions
 * that are incompatible with the /NAMES=AS_IS qualifier. */
#define cma$tis_errno_get_addr CMA$TIS_ERRNO_GET_ADDR

/* LARGE FILE SUPPORT - 10/6/04 EG */
/* This needs to be set before the includes so they set the right sizes */

#ifdef NO_LARGE_FILE_SUPPORT
# ifdef LARGE_FILE_SUPPORT
#  undef LARGE_FILE_SUPPORT
# endif
#endif

#ifdef LARGE_FILE_SUPPORT

# define _LARGEFILE             /* Define the pertinent macro. */

/* LARGE_FILE_SUPPORT implies ZIP64_SUPPORT,
   unless explicitly disabled by NO_ZIP64_SUPPORT.
*/
#  ifdef NO_ZIP64_SUPPORT
#    ifdef ZIP64_SUPPORT
#      undef ZIP64_SUPPORT
#    endif
#  else
#    ifndef ZIP64_SUPPORT
#      define ZIP64_SUPPORT
#    endif
#  endif

#endif /* def LARGE_FILE_SUPPORT */

/* 2007-02-22 SMS.
 * Enable symbolic links according to the available C RTL support,
 * unless prohibited by the user defining NO_SYMLINKS.
 */
#if !defined(__VAX) && defined(__CRTL_VER) && __CRTL_VER >= 70301000
#  ifndef NO_SYMLINKS
#     define SYMLINKS
#  endif
#endif

#ifdef SYMLINKS
#  include <unistd.h>
#endif

#  include <types.h>                    /* GRR:  experimenting... */
#  include <stat.h>
#  include <time.h>                     /* the usual non-BSD time functions */
#  include <file.h>                     /* same things as fcntl.h has */
#  include <unixio.h>
#  include <rms.h>

/* Define maximum path length according to NAM[L] member size. */
#  ifndef NAM_MAXRSS
#    ifdef NAML$C_MAXRSS
#      define NAM_MAXRSS NAML$C_MAXRSS
#    else
#      define NAM_MAXRSS NAM$C_MAXRSS
#    endif
#  endif

#  define _MAX_PATH (NAM_MAXRSS+1)      /* to define FILNAMSIZ below */

#  ifdef RETURN_CODES  /* VMS interprets standard PK return codes incorrectly */
#    define RETURN(ret) return_VMS(__G__ (ret))   /* verbose version */
#    define EXIT(ret)   return_VMS(__G__ (ret))
#  else
#    define RETURN      return_VMS                /* quiet version */
#    define EXIT        return_VMS
#  endif
#  ifdef VMSCLI
#    define USAGE(ret)  VMSCLI_usage(__G__ (ret))
#  endif
#  define DIR_BEG       '['
#  define DIR_END       ']'
#  define DIR_EXT       ".dir"
#  ifndef UZ_FNFILTER_REPLACECHAR
     /* We use '?' instead of the single char wildcard '%' as "unprintable
      * charcode" placeholder, because '%' is valid for ODS-5 names but '?'
      * is invalid. This choice may allow easier detection of "unprintables"
      * when reading the fnfilter() output.
      */
#    define UZ_FNFILTER_REPLACECHAR  '?'
#  endif
#  ifndef DATE_FORMAT
#    define DATE_FORMAT DF_MDY
#  endif
#  define lenEOL        1
#  define PutNativeEOL  *q++ = native(LF);
#  define SCREENSIZE(ttrows, ttcols)  screensize(ttrows, ttcols)
#  define SCREENWIDTH   80
#  define SCREENLWRAP   screenlinewrap()
#  if (defined(__VMS_VERSION) && !defined(VMS_VERSION))
#    define VMS_VERSION __VMS_VERSION
#  endif
#  if (defined(__VMS_VER) && !defined(__CRTL_VER))
#    define __CRTL_VER __VMS_VER
#  endif
#  if ((!defined(__CRTL_VER)) || (__CRTL_VER < 70000000))
#    define NO_GMTIME           /* gmtime() of earlier VMS C RTLs is broken */
#  else
#    if (!defined(NO_EF_UT_TIME) && !defined(USE_EF_UT_TIME))
#      define USE_EF_UT_TIME
#    endif
#    if (!defined(HAVE_STRNICMP) && !defined(NO_STRNICMP))
#      define HAVE_STRNICMP
#      ifdef STRNICMP
#        undef STRNICMP
#      endif
#      define STRNICMP  strncasecmp
#    endif
#  endif
#  ifndef HAVE_STRNICMP                 /* use our private zstrnicmp() */
#    define NO_STRNICMP                 /*  unless explicitly overridden */
#  endif
#  if (!defined(NOTIMESTAMP) && !defined(TIMESTAMP))
#    define TIMESTAMP
#  endif
#  define SET_DIR_ATTRIB
#  define RESTORE_UIDGID
   /* VMS is run on little-endian processors with 4-byte ints:
    * enable the optimized CRC-32 code */
#  ifdef IZ_CRC_BE_OPTIMIZ
#    undef IZ_CRC_BE_OPTIMIZ
#  endif
#  if !defined(IZ_CRC_LE_OPTIMIZ) && !defined(NO_CRC_OPTIMIZ)
#    define IZ_CRC_LE_OPTIMIZ
#  endif
#  if !defined(IZ_CRCOPTIM_UNFOLDTBL) && !defined(NO_CRC_OPTIMIZ)
#    define IZ_CRCOPTIM_UNFOLDTBL
#  endif
   /* Enable "better" unprintable charcodes filtering in fnfilter().
    * (On VMS, the isprint() implementation seems to detect 8-bit printable
    * characters even for the default "C" locale. A previous localization
    * setup by calling setlocale() is not neccessary.) */
#  if (!defined(NO_WORKING_ISPRINT) && !defined(HAVE_WORKING_ISPRINT))
#    define HAVE_WORKING_ISPRINT
#  endif

#ifdef NO_OFF_T
  typedef long zoff_t;
#else
  typedef off_t zoff_t;
#endif
#define ZOFF_T_DEFINED

typedef struct stat z_stat;
#define Z_STAT_DEFINED


#ifdef __DECC

    /* File open callback ID values. */
#   define OPENR_ID 1

    /* File open callback ID storage. */
    extern int openr_id;

    /* File open callback function. */
    extern int acc_cb();

    /* Option macros for open().
     * General: Stream access
     *
     * Callback function (DEC C only) sets deq, mbc, mbf, rah, wbh, ...
     */
#   define OPNZIP_RMS_ARGS "ctx=stm", "acc", acc_cb, &openr_id

#else /* !__DECC */ /* (So, GNU C, VAX C, ...)*/

#   define OPNZIP_RMS_ARGS "ctx=stm"

#endif /* ?__DECC */

#endif /* !__vmscfg_h */
