/*
  Copyright (c) 1990-2002 Info-ZIP.  All rights reserved.

  See the accompanying file LICENSE, version 2000-Apr-09 or later
  (the contents of which are also included in unzip.h) for terms of use.
  If, for some reason, all these files are missing, the Info-ZIP license
  also may be found at:  ftp://ftp.info-zip.org/pub/infozip/license.html
*/
//******************************************************************************
//
// File:        INTRFACE.H
//
// Description: This module acts as the interface between the Info-ZIP code and
//              our Windows code in WINMAIN.CPP.  See INTRFACE.CPP for a more
//              detailed description and the actual implementation.
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

#ifndef __INTRFACE_H__
#define __INTRFACE_H__

#ifdef __cplusplus
extern "C" {
#endif

#ifdef POCKET_UNZIP
//******************************************************************************
//***** Types and Structures
//******************************************************************************

typedef enum _OVERWRITE_MODE {
   OM_PROMPT = 0,
   OM_NEWER,
   OM_ALWAYS,
   OM_NEVER
} OVERWRITE_MODE, *LPOVERWRITE_MODE;

typedef struct _EXTRACT_INFO {
   zusz_t          uzByteCount;   // Total bytes to extract/test
   DWORD           dwFileCount;   // Number of files to extract/test.
   LPSTR          *szFileList;    // ARGV list of files, NULL for all files.
   LPSTR           szMappedPath;  // Used to store mapped name. May be NULL.
   OVERWRITE_MODE  overwriteMode; // How to handle file overwrites.
   BOOL            fExtract;      // TRUE for extract, FALSE for test
   BOOL            fRestorePaths; // TRUE to restore paths, FALSE to junk them.
   BOOL            fAbort;        // Set during operation by UI to abort.
   int             result;        // Result code from extraction/test.

   // Window handles for the various controls in our progress dialogs.
   HWND            hWndEditFile;
   HWND            hWndProgFile;
   HWND            hWndProgTotal;
   HWND            hWndPercentage;
   HWND            hWndFilesProcessed;
   HWND            hWndBytesProcessed;

   // Values used to keep track of our progress.
   zusz_t          uzBytesTotalThisFile;
   zusz_t          uzBytesWrittenThisFile;
   zusz_t          uzBytesWrittenPreviousFiles;
   zusz_t          uzFileOffset;
   DWORD           dwFile;
   LPCSTR          szFile;
   BOOL            fNewLineOfText;

} EXTRACT_INFO, *LPEXTRACT_INFO;

typedef struct _DECRYPT_INFO {
   int    retry;
   LPSTR  szPassword;
   DWORD  nSize;
   LPCSTR szFile;
} DECRYPT_INFO, *LPDECRYPT_INFO;

//******************************************************************************
//***** Function Prototypes
//******************************************************************************

// Our exposed interface functions to the Info-ZIP core.
int  DoListFiles(LPCSTR szZipFile);
BOOL DoExtractOrTestFiles(LPCSTR szZipFile, EXTRACT_INFO *pei);
int  DoGetComment(LPCSTR szZipFile);
BOOL SetExtractToDirectory(LPTSTR szDirectory);

// "Internal" callbacks from Info-ZIP code.
// (The "official" callback functions are declared in the UnZip DLL headers,
// see "unzip.h".)
void WINAPI Wiz_NoPrinting(int f);
int  win_fprintf(zvoid *pG, FILE *file, unsigned int dwCount, char far *buffer);


//******************************************************************************
//***** Global Variables
//******************************************************************************

#ifdef GLOBAL_DECLARE
#undef GLOBAL_DECLARE
#undef GLOBAL_INIT
#endif

#ifdef __INTRFACE_CPP__
   #define GLOBAL_DECLARE
   #define GLOBAL_INIT(value) =value
#else
   #define GLOBAL_DECLARE extern
   #define GLOBAL_INIT(value)
#endif

GLOBAL_DECLARE jmp_buf         dll_error_return;
GLOBAL_DECLARE LPDCL           lpDCL           GLOBAL_INIT(NULL);
GLOBAL_DECLARE LPUSERFUNCTIONS lpUserFunctions GLOBAL_INIT(NULL);

#endif // POCKET_UNZIP

#ifdef __cplusplus
} // extern "C"
#endif

#endif // __INTRFACE_H__
