/*
  Copyright (c) 1990-2002 Info-ZIP.  All rights reserved.

  See the accompanying file LICENSE, version 2000-Apr-09 or later
  (the contents of which are also included in unzip.h) for terms of use.
  If, for some reason, all these files are missing, the Info-ZIP license
  also may be found at:  ftp://ftp.info-zip.org/pub/infozip/license.html
*/
//******************************************************************************
//
// File:        PUNZIP.H
//
// Description: This is our global header for the entire Pocket UnZip project.
//              This header contains all global project build flags, defines,
//              constants, and macros.  It also includes all other headers that
//              are needed by the project.
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

#ifndef __PUNZIP_H__
#define __PUNZIP_H__

#ifdef __cplusplus
extern "C" {
#endif

//******************************************************************************
//***** Standard Win32 project flags
//******************************************************************************

#ifndef WIN32
#define WIN32
#endif

#ifndef _WINDOWS
#define _WINDOWS
#endif

#ifdef _WIN32_WCE   /* for native Windows CE, force UNICODE mode */
#ifndef UNICODE
#define UNICODE
#endif
#endif /* _WIN32_WCE */

#ifndef WIN32_LEAN_AND_MEAN
#define WIN32_LEAN_AND_MEAN
#endif

#ifndef STRICT
#define STRICT
#endif

#if defined(_UNICODE) && !defined(UNICODE)
#define UNICODE
#endif

#if defined(UNICODE) && !defined(_UNICODE)
#define _UNICODE
#endif

#if defined(_DEBUG) && !defined(DEBUG)
#define DEBUG
#endif

#if defined(DEBUG) && !defined(_DEBUG)
#define _DEBUG
#endif

#if defined(_NDEBUG) && !defined(NDEBUG)
#define NDEBUG
#endif

#if defined(NDEBUG) && !defined(_NDEBUG)
#define _NDEBUG
#endif


//******************************************************************************
//***** Pocket Unzip and Info-ZIP flags
//******************************************************************************

#ifndef POCKET_UNZIP
#define POCKET_UNZIP
#endif

#ifndef WINDLL
#define WINDLL
#endif

#ifndef DLL
#define DLL
#endif

#ifndef REENTRANT
#define REENTRANT
#endif

#ifndef NO_ZIPINFO
#define NO_ZIPINFO
#endif

#ifndef NO_STDDEF_H
#define NO_STDDEF_H
#endif

// Read COPYING document before enabling this define.
#if 0
#ifndef USE_SMITH_CODE
#define USE_SMITH_CODE
#endif
#endif

// Read COPYING document before enabling this define.
#if 0
#ifndef USE_UNSHRINK
#define USE_UNSHRINK
#endif
#endif



#ifdef __cplusplus
} // extern "C"
#endif

#endif // __PUNZIP_H__
