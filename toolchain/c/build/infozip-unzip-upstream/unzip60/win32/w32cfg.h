/*
  Copyright (c) 1990-2009 Info-ZIP.  All rights reserved.

  See the accompanying file LICENSE, version 2009-Jan-02 or later
  (the contents of which are also included in unzip.h) for terms of use.
  If, for some reason, all these files are missing, the Info-ZIP license
  also may be found at:  ftp://ftp.info-zip.org/pub/infozip/license.html
*/
/*---------------------------------------------------------------------------
    Win32 specific configuration section:
  ---------------------------------------------------------------------------*/

#ifndef __w32cfg_h
#define __w32cfg_h

#ifdef __CYGWIN__
/* We treat the file system underneath the Cygwin Unix emulator environment
 * as "native VFAT/NTFS" and use the WIN32 API for its special attributes...
 */
#  ifdef UNIX
#    undef UNIX
#  endif
#endif

#if (defined(_MSC_VER) && !defined(MSC))
#  define MSC
#endif

/* enable multibyte character set support by default */
#if (!defined(_MBCS) && !defined(NO_MBCS))
#  define _MBCS
#endif
#if (defined(_MBCS) && defined(NO_MBCS))
#  undef _MBCS
#endif
#if (defined(__CYGWIN__) && defined(_MBCS))
#  undef _MBCS                  /* Cygwin RTL lacks support for __mb_cur_max */
#endif
#if (defined(__DJGPP__) && !defined(__EMX__) && defined(_MBCS))
#  undef _MBCS                  /* __mb_cur_max missing for RSXNTdj 1.6 beta */
#endif

#include <sys/types.h>          /* off_t, time_t, dev_t, ... */
#include <sys/stat.h>
#include <io.h>                 /* read(), open(), etc. */
#include <time.h>
#if ((defined(__RSXNT__) || defined(__EMX__)) && !defined(tzset))
#  define tzset _tzset
#endif
#if (defined(__LCC__) && !defined(tzset))
#  define tzset _tzset
#endif
#ifdef __MINGW32__
   extern void _tzset(void);    /* this is missing in <time.h> */
#  ifndef tzset
#    define tzset _tzset
#  endif
#endif
#ifdef W32_USE_IZ_TIMEZONE
#  ifdef __BORLANDC__
#    define tzname tzname
#    define IZTZ_DEFINESTDGLOBALS
#  endif
#  ifdef __WATCOMC__
#    define IZTZ_DEFINESTDGLOBALS
#  endif
#  ifndef tzset
#    define tzset _tzset
#  endif
#  ifndef timezone
#    define timezone _timezone
#  endif
#  ifndef daylight
#    define daylight _daylight
#  endif
#  ifndef tzname
#    define tzname _tzname
#  endif
#  if (!defined(NEED__ISINDST) && !defined(__BORLANDC__))
#    define NEED__ISINDST
#  endif
#  ifdef IZTZ_GETLOCALETZINFO
#    undef IZTZ_GETLOCALETZINFO
#  endif
#  define IZTZ_GETLOCALETZINFO GetPlatformLocalTimezone
#endif /* W32_USE_IZ_TIMEZONE */
#include <memory.h>
#if (!defined(__RSXNT__) && !defined(__CYGWIN__))
#  include <direct.h>           /* mkdir() */
#endif
#include <fcntl.h>
#ifdef __CYGWIN__
#  include <unistd.h>
   extern int setmode(int, int);        /* this is missing in <fcntl.h> */
#endif
#if (defined(MSC) || defined(__WATCOMC__) || defined(__MINGW32__))
#  include <sys/utime.h>
#else
#  include <utime.h>
#endif
#define GOT_UTIMBUF

#ifdef _MBCS
#  if (!defined(__EMX__) && !defined(__DJGPP__) && !defined(__CYGWIN__))
#  if (!defined(__MINGW32__) || defined(__MSVCRT__))
#    include <stdlib.h>
#    include <mbstring.h>
     /* for MSC (and compatible compilers), use routines supplied by RTL */
#    define CLEN(ptr) _mbclen((const uch *)(ptr))
#    define PREINCSTR(ptr) (ptr = (char *)_mbsinc((const uch *)(ptr)))
#    define MBSCHR(str, c) (char *)_mbschr((const uch *)(str), (c))
#    define MBSRCHR(str, c) (char *)_mbsrchr((const uch *)(str), (c))
#  endif
#  endif
#  if (defined(__MINGW32__) && !defined(MB_CUR_MAX))
#    ifdef __MSVCRT__
       extern int *__p___mb_cur_max(void);
#      define MB_CUR_MAX (*__p___mb_cur_max())
#    else
       extern int *_imp____mb_cur_max_dll;
#      define MB_CUR_MAX (*_imp____mb_cur_max_dll)
#    endif
#  endif
#  if (defined(__LCC__) && !defined(MB_CUR_MAX))
     extern int *_imp____mb_cur_max;
#    define MB_CUR_MAX (*_imp____mb_cur_max)
#  endif
#  if (defined(__DJGPP__) && !defined(__EMX__) && !defined(MB_CUR_MAX))
     extern int *_imp____mb_cur_max;
#    define MB_CUR_MAX (*_imp____mb_cur_max)
#  endif
#endif

/* for UnZip, the "basic" part of the win32 api is sufficient */
#ifndef WIN32_LEAN_AND_MEAN
#  define WIN32_LEAN_AND_MEAN
#endif

#if defined(__FILEIO_C)
#  ifndef __CYGWIN__
#    include <conio.h>
#  endif
#  include <windows.h>
#  ifdef __RSXNT__
#    include "../win32/rsxntwin.h"
#  endif
#  ifndef TIME_ZONE_ID_INVALID
#    define TIME_ZONE_ID_INVALID  (DWORD)0xFFFFFFFFL
#  endif
#endif
#if (defined(__ENVARGS_C) || defined(__EXTRACT_C) || defined(__UNZIP_C) || \
     defined(ZCRYPT_INTERNAL))
#  include <windows.h>
#  ifdef __RSXNT__
#    include "../win32/rsxntwin.h"
#  endif
#  ifndef TIME_ZONE_ID_INVALID
#    define TIME_ZONE_ID_INVALID  (DWORD)0xFFFFFFFFL
#  endif
#endif

#ifndef Cdecl
#  define Cdecl __cdecl
#endif

/* the following definitions are considered as "obsolete" by Microsoft and
 * might be missing in some versions of <windows.h>
 */
#ifndef AnsiToOem
#  define AnsiToOem CharToOemA
#endif
#ifndef OemToAnsi
#  define OemToAnsi OemToCharA
#endif

#define DIR_END       '\\'      /* OS uses '\\' as directory separator */
#define DIR_END2      '/'       /* also check for '/' (RTL may convert) */
#ifdef DATE_FORMAT
#  undef DATE_FORMAT
#endif
#define DATE_FORMAT   dateformat()
#ifdef DATE_SEPCHAR
#  undef DATE_SEPCHAR
#endif
#define DATE_SEPCHAR  dateseparator()
#define lenEOL        2
#define PutNativeEOL  {*q++ = native(CR); *q++ = native(LF);}

#if (defined(__RSXNT__) && !defined(HAVE_MKTIME))
#  define HAVE_MKTIME           /* use mktime() in time conversion routines */
#endif
#if (defined(MSC) && !defined(HAVE_MKTIME))
#  define HAVE_MKTIME           /* use mktime() in time conversion routines */
#endif
#if (defined(__CYGWIN__) && defined(HAVE_MKTIME))
#  undef HAVE_MKTIME            /* Cygnus' mktime() implementation is buggy */
#endif
#if (defined(W32_USE_IZ_TIMEZONE) && !defined(HAVE_MKTIME))
#  define HAVE_MKTIME           /* use mktime() in time conversion routines */
#endif
#if (!defined(NO_EF_UT_TIME) && !defined(USE_EF_UT_TIME))
#  define USE_EF_UT_TIME
#endif
#if (!defined(NO_DIR_ATTRIB) && !defined(SET_DIR_ATTRIB))
#  define SET_DIR_ATTRIB
#endif
#if (!defined(NOTIMESTAMP) && !defined(TIMESTAMP))
#  define TIMESTAMP
#endif
#if (!defined(NO_NTSD_EAS) && !defined(NTSD_EAS))
#  define NTSD_EAS      /* enable NTSD support unless explicitly suppressed */
#endif
#if (defined(NTSD_EAS) && !defined(RESTORE_ACL))
#  define RESTORE_ACL   /* "restore ACLs" only needed when NTSD_EAS active */
#endif
#if (!defined(NO_UNICODE_SUPPORT) && !defined(UNICODE_SUPPORT))
#  define UNICODE_SUPPORT       /* enable UTF-8 filename support by default */
#endif
#if (defined(UNICODE_SUPPORT) && !defined(UNICODE_WCHAR))
#  define UNICODE_WCHAR         /* wchar_t is UTF-16 encoded on WIN32 */
#endif
#ifdef UTF8_MAYBE_NATIVE
#  undef UTF8_MAYBE_NATIVE      /* UTF-8 cannot be system charset on WIN32 */
#endif

/* The following compiler systems provide or use a runtime library with a
 * locale-aware isprint() implementation.  For these systems, the "enhanced"
 * unprintable charcode detection in fnfilter() gets enabled.
 */
#if (!defined(HAVE_WORKING_ISPRINT) && !defined(NO_WORKING_ISPRINT))
#  if defined(MSC) || defined(__BORLANDC__)
#    define HAVE_WORKING_ISPRINT
#  endif
#  if defined(__MINGW32__) && defined(__MSVCRT__)
#    define HAVE_WORKING_ISPRINT
#  endif
#endif

/* WIN32 runs solely on little-endian processors; enable support
 * for the 32-bit optimized CRC-32 C code by default.
 */
#ifdef IZ_CRC_BE_OPTIMIZ
#  undef IZ_CRC_BE_OPTIMIZ
#endif
#if !defined(IZ_CRC_LE_OPTIMIZ) && !defined(NO_CRC_OPTIMIZ)
#  define IZ_CRC_LE_OPTIMIZ
#endif

/* handlers for OEM <--> ANSI string conversions */
#ifdef __RSXNT__
   /* RSXNT uses OEM coded strings in functions supplied by C RTL */
#  ifdef CRTL_CP_IS_ISO
#    undef CRTL_CP_IS_ISO
#  endif
#  ifndef CRTL_CP_IS_OEM
#    define CRTL_CP_IS_OEM
#  endif
#else
   /* "real" native WIN32 compilers use ANSI coded strings in C RTL calls */
#  ifndef CRTL_CP_IS_ISO
#    define CRTL_CP_IS_ISO
#  endif
#  ifdef CRTL_CP_IS_OEM
#    undef CRTL_CP_IS_OEM
#  endif
#endif

#ifdef CRTL_CP_IS_ISO
   /* C RTL's file system support assumes ANSI coded strings */
#  define ISO_TO_INTERN(src, dst)  {if ((src) != (dst)) strcpy((dst), (src));}
#  define OEM_TO_INTERN(src, dst)  OemToAnsi(src, dst)
#  define INTERN_TO_ISO(src, dst)  {if ((src) != (dst)) strcpy((dst), (src));}
#  define INTERN_TO_OEM(src, dst)  AnsiToOem(src, dst)
#endif /* CRTL_CP_IS_ISO */
#ifdef CRTL_CP_IS_OEM
   /* C RTL's file system support assumes OEM coded strings */
#  define ISO_TO_INTERN(src, dst)  AnsiToOem(src, dst)
#  define OEM_TO_INTERN(src, dst)  {if ((src) != (dst)) strcpy((dst), (src));}
#  define INTERN_TO_ISO(src, dst)  OemToAnsi(src, dst)
#  define INTERN_TO_OEM(src, dst)  {if ((src) != (dst)) strcpy((dst), (src));}
#endif /* CRTL_CP_IS_OEM */
#define _OEM_INTERN(str1) OEM_TO_INTERN(str1, str1)
#define _ISO_INTERN(str1) ISO_TO_INTERN(str1, str1)
#ifndef WINDLL
   /* Despite best intentions, for the command-line version UzpPassword()
    * could return either character set, depending on whether running under
    * Win95 (DOS-session) or WinNT (native WinNT command interpreter)! */
#  define STR_TO_CP2(dst, src)  (AnsiToOem(src, dst), dst)
#  define STR_TO_CP3(dst, src)  (OemToAnsi(src, dst), dst)
#else
   /* The WINDLL front end is known to supply ISO/ANSI-coded passwords! */
#  define STR_TO_CP2(dst, src)  (AnsiToOem(src, dst), dst)
#endif
/* dummy defines to disable these functions, they are not needed */
#define STR_TO_OEM
#define STR_TO_ISO

/* Static variables that we have to add to Uz_Globs: */
#define SYSTEM_SPECIFIC_GLOBALS \
    int created_dir, renamed_fullpath, fnlen;\
    unsigned nLabelDrive;\
    char lastRootPath[4];\
    int lastVolOldFAT, lastVolLocTim;\
    char *rootpath, *buildpathHPFS, *buildpathFAT, *endHPFS, *endFAT;\
    ZCONST char *wildname;\
    char *dirname, matchname[FILNAMSIZ];\
    int rootlen, have_dirname, dirnamelen, notfirstcall;\
    zvoid *wild_dir;

/* created_dir, renamed_fullpath, fnlen, and nLabelDrive are used by   */
/*    both mapname() and checkdir().                                   */
/* lastRootPath, lastVolOldFAT and lastVolLocTim are used by           */
/*    IsVolumeOldFAT() and NTQueryVolInfo().                           */
/* rootlen, rootpath, buildpathHPFS, buildpathFAT, endHPFS, and endFAT */
/*    are used by checkdir().                                          */
/* wild_dir, dirname, wildname, matchname[], dirnamelen, have_dirname, */
/*    and notfirstcall are used by do_wild().                          */

/* This replacement for C-RTL-supplied getch() (or similar) functionality
 * avoids leaving unabsorbed LFs in the keyboard buffer under Windows95,
 * and supports the <ALT>+[0]<digit><digit><digit> feature.
 */
int getch_win32  OF((void));

/* Up to now, all versions of Microsoft C runtime libraries lack the support
 * for customized (non-US) switching rules between daylight saving time and
 * standard time in the TZ environment variable string.
 * But non-US timezone rules are correctly supported when timezone information
 * is read from the OS system settings in the Win32 registry.
 * The following work-around deletes any TZ environment setting from
 * the process environment.  This results in a fallback of the RTL time
 * handling code to the (correctly interpretable) OS system settings, read
 * from the registry.
 */
#ifdef USE_EF_UT_TIME
# if (defined(__WATCOMC__) || defined(__CYGWIN__) || \
      defined(W32_USE_IZ_TIMEZONE))
#   define iz_w32_prepareTZenv()
# else
#   define iz_w32_prepareTZenv()        putenv("TZ=")
# endif
#endif

/* This patch of stat() is useful for at least two compilers.  It is   */
/* difficult to take a stat() of a root directory under Windows95, so  */
/* zstat_win32() detects that case and fills in suitable values.       */
#ifndef __RSXNT__
#  ifndef W32_STATROOT_FIX
#    define W32_STATROOT_FIX
#  endif
#endif /* !__RSXNT__ */

#define W32_STAT_BANDAID
#if defined(REENTRANT)
#  define __W32STAT_GLOBALS__       Uz_Globs *pG,
#  define __W32STAT_G__             pG,
#else
#  define __W32STAT_GLOBALS__
#  define __W32STAT_G__
#endif
#ifdef SSTAT
#  undef SSTAT
#endif
#ifdef WILD_STAT_BUG
#  define SSTAT(path, pbuf) (iswild(path) || zstat_win32(__W32STAT_G__ path, pbuf))
#else
#  define SSTAT(path, pbuf) zstat_win32(__W32STAT_G__ path, pbuf)
#endif

#ifdef __WATCOMC__
#  ifdef __386__
#    ifndef WATCOMC_386
#      define WATCOMC_386
#    endif
#    define __32BIT__
#    undef far
#    define far
#    undef near
#    define near
#    undef Cdecl
#    define Cdecl

/* gaah -- Watcom's docs claim that _get_osfhandle exists, but it doesn't.  */
#    define _get_osfhandle _os_handle

/* Get asm routines to link properly without using "__cdecl": */
#    ifndef USE_ZLIB
#      pragma aux crc32         "_*" parm caller [] value [eax] modify [eax]
#      pragma aux get_crc_table "_*" parm caller [] value [eax] \
                                      modify [eax ecx edx]
#    endif /* !USE_ZLIB */
#  endif /* __386__ */
#endif /* __WATCOMC__ */

#define SCREENWIDTH 80
#define SCREENSIZE(scrrows, scrcols)  screensize(scrrows, scrcols)
int screensize(int *tt_rows, int *tt_cols);

/* on the DOS or NT console screen, line-wraps are always enabled */
#define SCREENLWRAP 1
#define TABSIZE 8


/* 64-bit-Integers & Large File Support
 * (pasted here from Zip 3b, osdep.h - Myles Bennett 7-jun-2004)
 * (updated from Zip 3.0d - Ed Gordon 6-oct-2004)
 *
 *  If this is set it is assumed that the port
 *  supports 64-bit file calls.  The types are
 *  defined here.  Any local implementations are
 *  in w32i64.c and the prototypes for the calls are
 *  in unzip.h.  Note that a port must support
 *  these calls fully or should not set
 *  LARGE_FILE_SUPPORT.
 */

/* Automatically set ZIP64_SUPPORT if supported */

#ifndef NO_ZIP64_SUPPORT
# ifndef ZIP64_SUPPORT
#   if defined(_MSC_VER) || defined(__MINGW32__) || defined(__CYGWIN__)
#     define ZIP64_SUPPORT
#   elif defined(__LCC__)
      /* LCC links against crtdll.dll -> no support of 64-bit offsets :( */
#   elif (defined(__WATCOMC__) && (__WATCOMC__ >= 1100))
#     define ZIP64_SUPPORT
#   elif (defined(__BORLANDC__) && (__BORLANDC__ >= 0x0520))
      /* Borland C RTL lacks any support to get/set 64-bit file pointer :( */
#   endif
# endif
#endif

#ifdef ZIP64_SUPPORT
  /* base type for file offsets and file sizes */
# if (defined(__GNUC__) || defined(ULONG_LONG_MAX))
    typedef long long    zoff_t;
# else
    /* all other compilers use this as intrinsic 64-bit type */
    typedef __int64      zoff_t;
# endif
# define ZOFF_T_DEFINED

  /* user-defined types and format strings for 64-bit numbers and
   * file pointer functions  (these depend on the rtl library and library
   * headers used; they are NOT compiler-specific)
   */
# if defined(_MSC_VER) || defined(__MINGW32__) || defined(__LCC__)
    /* MS C and VC, MinGW32, lcc32 */
    /* these systems use the Microsoft C RTL */

    /* 64-bit stat struct */
    typedef struct _stati64 z_stat;
#   define Z_STAT_DEFINED

#   ifdef __LCC__
      /* The LCC headers lack these declarations of MSC rtl functions in
         sys/stat.h. */
      struct _stati64 {
        unsigned int st_dev;
        unsigned short st_ino;
        unsigned short st_mode;
        short st_nlink;
        short st_uid;
        short st_gid;
        unsigned int st_rdev;
        __int64 st_size;
        time_t st_atime;
        time_t st_mtime;
        time_t st_ctime;
      };
      int _stati64(const char *, struct _stati64 *);
      int _fstati64(int, struct _stati64 *);
       __int64 _lseeki64(int, __int64, int);
#   endif /* __LCC__ */

    /* printf format size prefix for zoff_t values */
#   define FZOFFT_FMT "I64"
#   define FZOFFT_HEX_WID_VALUE "16"

#   define SHORTHDRSTATS "%9I64u  %02u%c%02u%c%02u %02u:%02u  %c"
#   define SHORTFILETRAILER " --------                   -------\n%9I64u                   %9lu file%s\n"

# elif (defined(__BORLANDC__) && (__BORLANDC__ >= 0x0520))
    /* Borland C 5.2 or newer */

    /* 64-bit stat struct */
    typedef struct stati64 z_stat;
#   define Z_STAT_DEFINED

    /* Borland C does not provide a 64-bit-capable _lseeki64(), so we
       need to use the stdio.h stream functions instead. */
#   ifndef USE_STRM_INPUT
#     define USE_STRM_INPUT
#   endif

    /* printf format size prefix for zoff_t values */
#   define FZOFFT_FMT "L"
#   define FZOFFT_HEX_WID_VALUE "16"

#   define SHORTHDRSTATS "%9Lu  %02u%c%02u%c%02u %02u:%02u  %c"
#   define SHORTFILETRAILER " --------                   -------\n%9Lu                   %9lu file%s\n"

# elif (defined(__WATCOMC__) && (__WATCOMC__ >= 1100))
    /* WATCOM C */

    /* 64-bit stat struct */
    typedef struct _stati64 z_stat;
#   define Z_STAT_DEFINED

    /* printf format size prefix for zoff_t values */
#   define FZOFFT_FMT "ll"
#   define FZOFFT_HEX_WID_VALUE "16"

#   define SHORTHDRSTATS "%9llu  %02u%c%02u%c%02u %02u:%02u  %c"
#   define SHORTFILETRAILER " --------                   -------\n%9llu                   %9lu file%s\n"

# elif (defined(__IBMC__) && (__IBMC__ >= 350))
    /* IBM C */

    /* 64-bit stat struct */

    /* printf format size prefix for zoff_t values */
#   define FZOFFT_FMT "I64"
#   define FZOFFT_HEX_WID_VALUE "16"

#   define SHORTHDRSTATS "%9I64u  %02u%c%02u%c%02u %02u:%02u  %c"
#   define SHORTFILETRAILER " --------                   -------\n%9I64u                   %9lu file%s\n"

# endif

#endif

/* If port has LARGE_FILE_SUPPORT then define here
   to make automatic unless overridden */

#ifndef LARGE_FILE_SUPPORT
# ifndef NO_LARGE_FILE_SUPPORT
#   if defined(_MSC_VER) || defined(__MINGW32__)
#     define LARGE_FILE_SUPPORT
#   elif defined(__LCC__)
      /* LCC links against crtdll.dll -> no support of 64-bit offsets :( */
#   elif defined(__CYGWIN__)
#     define LARGE_FILE_SUPPORT
#   elif (defined(__WATCOMC__) && (__WATCOMC__ >= 1100))
#     define LARGE_FILE_SUPPORT
#   elif (defined(__BORLANDC__) && (__BORLANDC__ >= 0x0520))
      /* Borland C RTL lacks any support to get/set 64-bit file pointer :( */
#   endif
# endif
#endif


#ifndef LARGE_FILE_SUPPORT
  /* No Large File Support */

  /* base type for file offsets and file sizes */
  typedef long zoff_t;
# define ZOFF_T_DEFINED

  /* stat struct */
  typedef struct stat z_stat;
# define Z_STAT_DEFINED

#  define FZOFFT_FMT "l"
#  define FZOFFT_HEX_WID_VALUE "8"


#  define SHORTHDRSTATS "%9lu  %02u%c%02u%c%02u %02u:%02u  %c"
#  define SHORTFILETRAILER " --------                   -------\n%9lu                   %9lu file%s\n"

#endif /* LARGE_FILE_SUPPORT */

#endif /* !__w32cfg_h */
