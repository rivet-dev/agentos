/*
  Copyright (c) 1990-2008 Info-ZIP.  All rights reserved.

  See the accompanying file LICENSE, version 2000-Apr-09 or later
  (the contents of which are also included in unzip.h) for terms of use.
  If, for some reason, all these files are missing, the Info-ZIP license
  also may be found at:  ftp://ftp.info-zip.org/pub/infozip/license.html
*/
/*---------------------------------------------------------------------------
    MS-DOS specific configuration section:
  ---------------------------------------------------------------------------*/

#ifndef __doscfg_h
#define __doscfg_h

#include <dos.h>           /* for REGS macro (TC) or _dos_setftime (MSC) */

#ifdef __TURBOC__          /* includes Power C */
#  include <sys/timeb.h>   /* for structure ftime */
#  ifndef __BORLANDC__     /* there appears to be a bug (?) in Borland's */
#    include <mem.h>       /*  MEM.H related to __STDC__ and far poin-   */
#  endif                   /*  ters. (dpk)  [mem.h included for memcpy]  */
#endif

#ifdef WINDLL
#  if (defined(MSC) || defined(__WATCOMC__))
#    include <sys/utime.h>
#  else /* !(MSC || __WATCOMC__) ==> may be BORLANDC, or GNU environment */
#    include <utime.h>
#  endif /* ?(MSC || __WATCOMC__) */
#endif

#ifdef __WATCOMC__
#  define DOS_STAT_BANDAID

#  ifdef __386__
#    ifndef WATCOMC_386
#      define WATCOMC_386
#    endif
#    define __32BIT__
#    undef far
#    define far
#    undef near
#    define near

/* Get asm routines to link properly without using "__cdecl": */
#    ifndef USE_ZLIB
#      pragma aux crc32         "_*" parm caller [] value [eax] modify [eax]
#      pragma aux get_crc_table "_*" parm caller [] value [eax] \
                                      modify [eax ecx edx]
#    endif /* !USE_ZLIB */
#  else /* !__386__ */
#    ifndef USE_ZLIB
#      pragma aux crc32         "_*" parm caller [] value [ax dx] \
                                      modify [ax cx dx bx]
#      pragma aux get_crc_table "_*" parm caller [] value [ax] \
                                      modify [ax cx dx bx]
#    endif /* !USE_ZLIB */
#  endif /* ?__386__ */
#endif /* __WATCOMC__ */

#ifdef __EMX__
#  ifndef __32BIT__
#    define __32BIT__
#  endif
#  define far
#  ifndef HAVE_MKTIME
#    define HAVE_MKTIME
#  endif
#endif

#if defined(__GO32__) || defined(__DJGPP__)    /* MS-DOS compiler, not OS/2 */
#  ifndef __32BIT__
#    define __32BIT__
#  endif
#  ifndef __GO32__
#    define __GO32__
#  endif
#  ifndef HAVE_MKTIME
#    define HAVE_MKTIME
#  endif
#  include <sys/timeb.h>           /* for structure ftime and ftime() */
#  if (defined(__DJGPP__) && (__DJGPP__ > 1))
#    include <unistd.h>            /* for prototypes for read/write etc. */
#    include <dir.h>               /* for FA_LABEL */
#    if ((__DJGPP__ == 2) && (__DJGPP_MINOR__ == 0))
#      include <libc/dosio.h>      /* for _USE_LFN, djgpp 2.0 only */
#    endif
#    define USE_LFN _USE_LFN       /* runtime test:  support long filenames? */
#  else
     int setmode(int, int);        /* not in older djgpp's include files */
#  endif
#endif

#ifndef __32BIT__
#  define __16BIT__
#endif

#if (defined(M_I86CM) || defined(M_I86LM)) || defined(WINDLL)
#  define MED_MEM
#endif
#if (defined(__COMPACT__) || defined(__LARGE__) || defined(__HUGE__))
#  define MED_MEM
#endif
#ifdef __16BIT__
#  ifndef MED_MEM
#    define SMALL_MEM
#  endif
#endif

#define EXE_EXTENSION ".exe"  /* OS/2 has GetLoadPath() function instead */

#ifdef __16BIT__
#  if defined(MSC) || defined(__WATCOMC__)
#    include <malloc.h>
#    define nearmalloc _nmalloc
#    define nearfree _nfree
#  endif
#  if defined(__TURBOC__) && defined(DYNALLOC_CRCTAB)
#    if defined(__COMPACT__) || defined(__LARGE__) || defined(__HUGE__)
#      undef DYNALLOC_CRCTAB
#    endif
#  endif
#  ifndef nearmalloc
#    define nearmalloc malloc
#    define nearfree free
#  endif
#  if defined(DEBUG) && defined(MSC) && (!defined(_MSC_VER) || _MSC_VER < 600)
     /* for MSC 5.1, prevent macro expansion space overflow in DEBUG mode */
#    define NO_DEBUG_IN_MACROS
#  endif
#  ifdef USE_DEFLATE64
#    if (defined(M_I86TM) || defined(M_I86SM) || defined(M_I86MM))
#      error Deflate64(tm) requires compact or large memory model
#    endif
#    if (defined(__TINY__) || defined(__SMALL__) || defined(__MEDIUM__))
#      error Deflate64(tm) requires compact or large memory model
#    endif
     /* the 64k history buffer for Deflate64 must be allocated specially */
#    define MALLOC_WORK
#    define MY_ZCALLOC
#  endif
#endif

/* 32-bit MSDOS supports the 32-bit optimized CRC-32 C code */
#ifdef IZ_CRC_BE_OPTIMIZ
# undef IZ_CRC_BE_OPTIMIZ
#endif
#ifdef __32BIT__
# if !defined(IZ_CRC_LE_OPTIMIZ) && !defined(NO_CRC_OPTIMIZ)
#  define IZ_CRC_LE_OPTIMIZ
# endif
#else /* __16BIT__ does not support optimized C crc32 code */
# ifdef IZ_CRC_LE_OPTIMIZ
#  undef IZ_CRC_LE_OPTIMIZ
# endif
#endif

/* another stat()/fopen() bug with some 16-bit compilers on Novell drives;
 * very dangerous (silently overwrites executables in other directories)
 */
#define NOVELL_BUG_WORKAROUND

/* enables additional test and message code that directs UnZip to fail safely
 * in case the "workaround" enabled above does not work as intended
 */
#define NOVELL_BUG_FAILSAFE

/* Some implementations of stat() tend to fail on "." in root directories
 * or on remote (root) directories specified by an UNC network path. This
 * patch of stat() is useful for at least the WATCOM compilers. The
 * stat_bandaid() wrapper detects stat failures on root directories and
 * fills in suitable values.
 */
#ifdef DOS_STAT_BANDAID
#  ifdef SSTAT
#    undef SSTAT
#  endif
#  ifdef WILD_STAT_BUG
#    define SSTAT(path,pbuf) (iswild(path) || stat_bandaid(path,pbuf))
#  else
#    define SSTAT stat_bandaid
#  endif
   int stat_bandaid(const char *path, struct stat *buf);
#endif

/* the TIMESTAMP feature is now supported on MSDOS, enable it per default */
#if (!defined(NOTIMESTAMP) && !defined(TIMESTAMP))
#  define TIMESTAMP
#endif

/* check that TZ environment variable is defined before using UTC times */
#if (!defined(NO_IZ_CHECK_TZ) && !defined(IZ_CHECK_TZ))
#  define IZ_CHECK_TZ
#endif

/* The optional "long filename" support available with some MSDOS compiler
 * environments running under VFAT systems (Win95) is controlled with the
 * help of the two preprocessor symbols USE_VFAT and USE_LFN:
 *  - USE_VFAT is a compile time switch that selects the long filename
 *             semantics in mapname()
 *  - USE_LFN  is a macro equating to a boolean expression indicating
 *             whether long filenames are supported. If available, this
 *             macro should make use of a runtime function checking the
 *             LFN support.
 *
 * The code in msdos.c distinguishes three cases:
 * 1.) USE_VFAT is not defined:
 *     No support of LFN is included; filenames are mapped to 8+3 plain FAT
 *     syntax unconditionally.
 *     This is achieved by ``#define MAYBE_PLAIN_FAT'' to include the plain
 *     FAT name mapping code and by ``#undef USE_LFN'' to disable bypassing
 *     of the FAT mapping at runtime.
 * 2.) USE_VFAT is defined:
 *     Support for LFN is enabled.
 *  a) USE_LFN is undefined:
 *     There is no (runtime) check available to distinguish between OS
 *     environments that support VFAT extensions and those that do not.
 *     In this case, filenames are mapped to the more liberal VFAT LFN
 *     syntax unconditionally. The internal switch MAYBE_PLAIN_FAT remains
 *     undefined to exclude to "map to plain FAT" code parts.
 *  b) USE_LFN is defined (hopefully to a boolean runtime LFN check function):
 *     "#define MAYBE_PLAIN_FAT" is applied to include the plain FAT mapping
 *     code; the programs checks at runtime whether the OS supports LFN and
 *     uses the appropiate mapping syntax.
 */
/* Some environments, like DJGPP v2, can support long filenames on VFAT
 * systems and DOS 8.3 filenames on FAT systems in the same executable.  If
 * such support is available, USE_LFN should be defined to an expression
 * that will return non-zero when long filenames API should be used, zero
 * otherwise.
 */
#ifndef USE_VFAT
#  ifdef USE_LFN
#    undef USE_LFN
#  endif
#  ifndef MAYBE_PLAIN_FAT
#    define MAYBE_PLAIN_FAT
#  endif
#else
#  ifdef USE_LFN
#    define MAYBE_PLAIN_FAT
#  endif
#endif

#ifdef ACORN_FTYPE_NFS
#  undef ACORN_FTYPE_NFS        /* no commas allowed in short filenames */
#endif

/* handlers for OEM <--> ANSI string conversions */
#ifdef WINDLL
#  if 1
     /* C RTL's file system support assumes OEM-coded strings */
#    ifdef CRTL_CP_IS_ISO
#      undef CRTL_CP_IS_ISO
#    endif
#    ifndef CRTL_CP_IS_OEM
#      define CRTL_CP_IS_OEM
#    endif
#  else
     /* C RTL's file system support assumes ISO-coded strings */
#    ifndef CRTL_CP_IS_ISO
#      define CRTL_CP_IS_ISO
#    endif
#    ifdef CRTL_CP_IS_OEM
#      undef CRTL_CP_IS_OEM
#    endif
#  endif /* ?(code page of 16bit Windows compilers) */
   /* include Win API declarations only in sources where conversion is
    * actually used (skip __EXTRACT_C, extract.c includes windll.h instead)
    */
#  if defined(__ENVARGS_C) || defined(__UNZIP_C) || defined(ZCRYPT_INTERNAL)
#    include <windows.h>
#  endif
   /* use conversion functions of Windows API */
#  ifdef CRTL_CP_IS_ISO
#   define ISO_TO_INTERN(src, dst)  {if ((src) != (dst)) strcpy((dst), (src));}
#   define OEM_TO_INTERN(src, dst)  OemToAnsi(src, dst)
#   define INTERN_TO_ISO(src, dst)  {if ((src) != (dst)) strcpy((dst), (src));}
#   define INTERN_TO_OEM(src, dst)  AnsiToOem(src, dst)
#  endif
#  ifdef CRTL_CP_IS_OEM
#   define ISO_TO_INTERN(src, dst)  AnsiToOem(src, dst)
#   define OEM_TO_INTERN(src, dst)  {if ((src) != (dst)) strcpy((dst), (src));}
#   define INTERN_TO_ISO(src, dst)  OemToAnsi(src, dst)
#   define INTERN_TO_OEM(src, dst)  {if ((src) != (dst)) strcpy((dst), (src));}
#  endif
#  define _OEM_INTERN(str1) OEM_TO_INTERN(str1, str1)
#  define _ISO_INTERN(str1) ISO_TO_INTERN(str1, str1)
   /* UzpPassword supplies ANSI-coded string regardless of C RTL's native CP */
#  define STR_TO_CP2(dst, src)  (AnsiToOem(src, dst), dst)
   /* dummy defines to disable these functions, they are not needed */
#  define STR_TO_ISO
#  define STR_TO_OEM
#else
   /* use home-brewed conversion functions; internal charset is OEM */
#  ifdef CRTL_CP_IS_ISO
#    undef CRTL_CP_IS_ISO
#  endif
#  ifndef CRTL_CP_IS_OEM
#    define CRTL_CP_IS_OEM
#  endif
#endif
#ifndef NEED_ISO_OEM_INIT
#  define NEED_ISO_OEM_INIT
#endif

/* SCREENLINES macros for 16-bit and djgpp compilers */
#ifdef __16BIT__
#  define SCREENLINES (int)(*((unsigned char far*)0x00400084L) + 1)
#  define SCREENWIDTH (int)(*(unsigned short far*)0x0040004AL)
#endif

#if defined(__GO32__) || defined(__DJGPP__)    /* djgpp v1.x and v2.x */
#  include <pc.h>
#  define SCREENLINES ScreenRows()
#  define SCREENWIDTH ScreenCols()
#endif

#ifdef __EMX__
#  define SCREENWIDTH 80
#  define SCREENSIZE(scrrows, scrcols)  screensize(scrrows, scrcols)
   int screensize(int *tt_rows, int *tt_cols);
#endif

#ifdef WATCOMC_386
#  define SCREENWIDTH 80
#  define SCREENSIZE(scrrows, scrcols)  screensize(scrrows, scrcols)
   int screensize(int *tt_rows, int *tt_cols);
#endif

#ifndef SCREENSIZE
#  define SCREENSIZE(scrrows, scrcols) { \
        if ((scrrows) != NULL) *(scrrows) = SCREENLINES; \
        if ((scrcols) != NULL) *(scrcols) = SCREENWIDTH; }
#endif

/* on the DOS console screen, line-wraps are always enabled */
#define SCREENLWRAP 1
#define TABSIZE 8

#endif /* !__doscfg_h */
