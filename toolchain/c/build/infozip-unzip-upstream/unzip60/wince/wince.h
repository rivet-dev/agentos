/*
  Copyright (c) 1990-2009 Info-ZIP.  All rights reserved.

  See the accompanying file LICENSE, version 2009-Jan-02 or later
  (the contents of which are also included in unzip.h) for terms of use.
  If, for some reason, all these files are missing, the Info-ZIP license
  also may be found at:  ftp://ftp.info-zip.org/pub/infozip/license.html
*/
//******************************************************************************
//
// File:        WINCE.H
//
// Description: This file declares all the Win32 APIs and C runtime functions
//              that the Info-ZIP code calls, but are not implemented natively
//              on Windows CE.  See WINCE.CPP for the implementation.
//
// Copyright:   All the source files for Pocket UnZip, except for components
//              written by the Info-ZIP group, are copyrighted 1997 by Steve P.
//              Miller.  The product "Pocket UnZip" itself is property of the
//              author and cannot be altered in any way without written consent
//              from Steve P. Miller.
//
// Disclaimer:  All project files are provided "as is" with no guarantee of
//              their correctness.  The authors are not liable for any outcome
//              that is the result of using this source.  The source for Pocket
//              UnZip has been placed in the public domain to help provide an
//              understanding of its implementation.  You are hereby granted
//              full permission to use this source in any way you wish, except
//              to alter Pocket UnZip itself.  For comments, suggestions, and
//              bug reports, please write to stevemil@pobox.com.
//
//
// Date      Name          History
// --------  ------------  -----------------------------------------------------
// 02/01/97  Steve Miller  Created (Version 1.0 using Info-ZIP UnZip 5.30)
//
//******************************************************************************

#ifndef __WINCE_H__
#define __WINCE_H__

#ifdef __cplusplus
extern "C" {
#endif

//******************************************************************************
//***** For all platforms - Our debug output function
//******************************************************************************

// If we are building for debug, we implement the DebugOut() function. If we are
// building for release, then we turn all calls to DebugOut() into no-ops.  The
// Microsoft compiler (and hopefully others) will not generate any code at all
// for the retail version of DebugOut() defined here.  This works much better
// than trying to create a variable argument macro - something C/C++ does not
// support cleanly.

#ifdef DEBUG
void DebugOut(LPCTSTR szFormat, ...);
#else
__inline void DebugOut(LPCTSTR szFormat, ...) {}
#endif


//******************************************************************************
//***** Windows NT Native
//******************************************************************************

#if !defined(_WIN32_WCE)
#ifndef UNICODE
#include <stdio.h>
#endif
#include <io.h>
#include <time.h>
#include <fcntl.h>
#include <sys\stat.h>
#endif

//******************************************************************************
//***** Windows CE Native
//******************************************************************************

#if defined(_WIN32_WCE)

#if defined(__WINCE_CPP)
   // internal, suppress "import linkage" specifier
#  define ZCRTIMP
#else
   // do not use import linkage specifier either; symbols are provided locally
#  define ZCRTIMP
#endif

#ifndef ZeroMemory
#define ZeroMemory(Destination,Length) memset(Destination, 0, Length)
#endif

#ifdef _MBCS
   // WinCE C RTL does not provide the setlocale function
#  define setlocale(category, locale)
#endif

// A few forgotten defines in Windows CE's TCHAR.H
#ifndef _stprintf
#define _stprintf wsprintf
#endif

#if _WIN32_WCE < 211 //sr551b functions in stdlib CE300
#ifndef _vsntprintf
#define _vsntprintf(d,c,f,a) wvsprintf(d,f,a)
#endif
#ifndef _vsnwprintf
#define _vsnwprintf(d,c,f,a) wvsprintf(d,f,a)
#endif
#endif //end sr551b

//******************************************************************************
//***** SYS\TYPES.H functions
//******************************************************************************

#ifndef _OFF_T_DEFINED
typedef long _off_t;
#define _OFF_T_DEFINED
#endif
#ifndef _TIME_T_DEFINED
typedef long time_t;
#define _TIME_T_DEFINED
#endif

//******************************************************************************
//***** CTYPE.H functions
//******************************************************************************
#if _WIN32_WCE < 300
ZCRTIMP int __cdecl isupper(int);
#endif
_CRTIMP int __cdecl tolower(int);
// This is a coarse approximation to ASCII isalpha(), it returns TRUE not only
// on all ASCII letters but also on punctuation chars in the range of 0x40-0x7F
#ifndef isalpha
#define isalpha(c) (((c) & 0xC0) == 0xC0)
#endif

//******************************************************************************
//***** FCNTL.H functions
//******************************************************************************

#ifndef _O_RDONLY       // do not redefine existing FCNTL.H constants

#define _O_RDONLY 0x0000   // open for reading only
#define _O_WRONLY 0x0001   // open for writing only
#define _O_RDWR   0x0002   // open for reading and writing
#define _O_APPEND 0x0008   // writes done at eof

#define _O_CREAT  0x0100   // create and open file
#define _O_TRUNC  0x0200   // open and truncate
#define _O_EXCL   0x0400   // open only if file doesn't already exist


//# define _O_TEXT    0x4000   // file mode is text (translated)
#define _O_BINARY 0x8000   // file mode is binary (untranslated)

#endif // _O_RDONLY (and alikes...) undefined

#ifndef O_RDONLY        // do not redefine existing FCNTL.H constants

#define O_RDONLY  _O_RDONLY
#define O_WRONLY  _O_WRONLY
#define O_RDWR    _O_RDWR
#define O_APPEND  _O_APPEND
#define O_CREAT   _O_CREAT
#define O_TRUNC   _O_TRUNC
#define O_EXCL    _O_EXCL
#define O_TEXT    _O_TEXT
#define O_BINARY  _O_BINARY
//#define O_RAW      _O_BINARY
//#define O_TEMPORARY   _O_TEMPORARY
//#define O_NOINHERIT   _O_NOINHERIT
//#define O_SEQUENTIAL  _O_SEQUENTIAL
//#define O_RANDOM   _O_RANDOM

#endif // O_RDONLY (and other old-fashioned constants) undefined

//******************************************************************************
//***** IO.H functions
//******************************************************************************

ZCRTIMP int __cdecl chmod(const char *, int);
ZCRTIMP int __cdecl close(int);
ZCRTIMP int __cdecl isatty(int);
ZCRTIMP long __cdecl lseek(int, long, int);
ZCRTIMP int __cdecl open(const char *, int, ...);
ZCRTIMP int __cdecl read(int, void *, unsigned int);
#if _WIN32_WCE < 211
ZCRTIMP int __cdecl setmode(int, int);
#else
# define setmode _setmode
#endif
ZCRTIMP int __cdecl unlink(const char *);


//******************************************************************************
//***** STDIO.H functions
//******************************************************************************

#if _WIN32_WCE < 211 //sr551b functions in stdlib CE300
//typedef struct _iobuf FILE;
typedef int FILE;

#define stdin  ((int*)-2)
#define stdout ((int*)-3)
#define stderr ((int*)-4)

#define EOF    (-1)

ZCRTIMP int __cdecl fflush(FILE *);
ZCRTIMP char * __cdecl fgets(char *, int, FILE *);
ZCRTIMP int __cdecl fileno(FILE *);
ZCRTIMP FILE * __cdecl fopen(const char *, const char *);
ZCRTIMP int __cdecl fprintf(FILE *, const char *, ...);
ZCRTIMP int __cdecl fclose(FILE *);
ZCRTIMP int __cdecl putc(int, FILE *);
ZCRTIMP int __cdecl sprintf(char *, const char *, ...);
#endif // _WIN32_WCE < 211
#if _WIN32_WCE >= 211
// CE falsely uses (FILE *) pointer args for UNIX style I/O functions that
// normally expect numeric file handles (e.g. setmode())
# undef fileno
# define fileno(strm)  (strm)
#endif // _WIN32_WCE < 211
#ifndef POCKET_UNZIP
ZCRTIMP void __cdecl perror(const char* errorText);
#endif
#ifdef USE_FWRITE
ZCRTIMP void __cdecl setbuf(FILE *, char *);
#endif


//******************************************************************************
//***** STDLIB.H functions
//******************************************************************************

#ifdef _MBCS
#ifndef MB_CUR_MAX
# define MB_CUR_MAX 2
#endif
ZCRTIMP int __cdecl mblen(const char *mbc, size_t mbszmax);
#endif /* _MBCS */
#if _WIN32_WCE >= 211
# define errno ((int)GetLastError())
#endif
#ifdef _WIN32_WCE_EMULATION
  // The emulation runtime library lacks a required element for setjmp/longjmp,
  // disable the recovery functionality for now.
# undef setjmp
# define setjmp(buf) 0
# undef longjmp
# define longjmp(buf, rv)
#endif

//******************************************************************************
//***** STRING.H functions
//******************************************************************************

ZCRTIMP int     __cdecl _stricmp(const char *, const char *);
ZCRTIMP char *  __cdecl _strupr(char *);
ZCRTIMP char *  __cdecl strerror(int errnum);
ZCRTIMP char *  __cdecl strrchr(const char *, int);


//******************************************************************************
//***** TIME.H functions
//******************************************************************************

#ifndef _TM_DEFINED
struct tm {
   int tm_sec;     // seconds after the minute - [0,59]
   int tm_min;     // minutes after the hour - [0,59]
   int tm_hour;    // hours since midnight - [0,23]
   int tm_mday;    // day of the month - [1,31]
   int tm_mon;     // months since January - [0,11]
   int tm_year;    // years since 1900
// int tm_wday;    // days since Sunday - [0,6]
// int tm_yday;    // days since January 1 - [0,365]
   int tm_isdst;   // daylight savings time flag
};
#define _TM_DEFINED
#endif

ZCRTIMP struct tm * __cdecl localtime(const time_t *);
// tzset is not supported on native WCE, define it as a NOP macro
#ifndef tzset
# define tzset()
#endif

//******************************************************************************
//***** SYS\STAT.H functions
//******************************************************************************

struct stat {
// _dev_t st_dev;
// _ino_t st_ino;
   unsigned short st_mode;
// short st_nlink;
// short st_uid;
// short st_gid;
// _dev_t st_rdev;
   _off_t st_size;
// time_t st_atime;
   time_t st_mtime;
// time_t st_ctime;
};

#define _S_IFMT   0170000  // file type mask
#define _S_IFDIR  0040000  // directory
//#define _S_IFCHR   0020000  // character special
//#define _S_IFIFO   0010000  // pipe
#define _S_IFREG  0100000  // regular
#define _S_IREAD  0000400  // read permission, owner
#define _S_IWRITE 0000200  // write permission, owner
#define _S_IEXEC  0000100  // execute/search permission, owner

#define S_IFMT  _S_IFMT
#define S_IFDIR  _S_IFDIR
//#define S_IFCHR  _S_IFCHR
//#define S_IFREG  _S_IFREG
#define S_IREAD  _S_IREAD
#define S_IWRITE _S_IWRITE
#define S_IEXEC  _S_IEXEC

ZCRTIMP int __cdecl stat(const char *, struct stat *);


//******************************************************************************

#endif // _WIN32_WCE

#ifdef __cplusplus
} // extern "C"
#endif

#endif // __WINCE_H__
