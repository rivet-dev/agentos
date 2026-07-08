/*
  Copyright (c) 1990-2005 Info-ZIP.  All rights reserved.

  See the accompanying file LICENSE, version 2000-Apr-09 or later
  (the contents of which are also included in unzip.h) for terms of use.
  If, for some reason, these files are missing, the Info-ZIP license
  also may be found at:  ftp://ftp.info-zip.org/pub/infozip/license.html
*/
//******************************************************************************
//
// File:        WINMAIN.H
//
// Description: This module contains all the Windows specific declarations for
//              Pocket UnZip.  See WINMAIN.CPP for a more detailed description
//              and the actual implementation.
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

#ifndef __WINMAIN_H__
#define __WINMAIN_H__

#ifdef __cplusplus
extern "C" {
#endif

//******************************************************************************
//***** Constants / Macros
//******************************************************************************

#define MRU_MAX_FILE                       4  // Should not exceed 9
#define MRU_START_ID                     501

#define WM_PRIVATE                    0x9999
#define MSG_SUBCLASS_DIALOG                1
#define MSG_INIT_DIALOG                    2
#define MSG_ADD_TEXT_TO_EDIT               3
#define MSG_PROMPT_TO_REPLACE              4
#define MSG_PROMPT_FOR_PASSWORD            5
#define MSG_UPDATE_PROGRESS_PARTIAL        6
#define MSG_UPDATE_PROGRESS_COMPLETE       7
#define MSG_OPERATION_COMPLETE             8

#define IDC_SAVE_FILE_LIST                12
#define IDC_SAVE_NAME_PROMPT            1023
#define IDC_SAVE_NAME_EDIT              1021
#define IDC_SAVE_TYPE_PROMPT            1022
#define IDC_SAVE_TYPE_LIST              1020

#define PROGRESS_MAX                   32768

#define ZFILE_ATTRIBUTE_VOLUME    0x00000008
#define ZFILE_ATTRIBUTE_ENCRYPTED 0x10000000
#define ZFILE_ATTRIBUTE_COMMENT   0x20000000

#define IMAGE_VOLUME                       0
#define IMAGE_FOLDER                       1
#define IMAGE_APPLICATION                  2
#define IMAGE_GENERIC                      3


#ifndef OFN_NOVALIDATE
#define OFN_NOVALIDATE               0x00000100
#endif

#ifndef LVS_EX_FULLROWSELECT
#define LVS_EX_FULLROWSELECT      0x00000020
#endif

// LVM_SETEXTENDEDLISTVIEWSTYLE came after VC 4.0
#ifndef LVM_SETEXTENDEDLISTVIEWSTYLE
#define LVM_SETEXTENDEDLISTVIEWSTYLE (LVM_FIRST + 54)
#endif

// LVM_GETEXTENDEDLISTVIEWSTYLE came after VC 4.0
#ifndef LVM_GETEXTENDEDLISTVIEWSTYLE
#define LVM_GETEXTENDEDLISTVIEWSTYLE (LVM_FIRST + 55)
#endif

#ifdef _WIN32_WCE
#define CheckDlgButton(hDlg, ctrl, fChecked) \
           SendDlgItemMessage(hDlg, ctrl, BM_SETCHECK, fChecked, 0)
#define IsDlgButtonChecked(hDlg, ctrl) \
           SendDlgItemMessage(hDlg, ctrl, BM_GETCHECK, 0, 0)
#endif

//******************************************************************************
//***** Types and Structures
//******************************************************************************

typedef struct _FILE_TYPE_NODE {
   struct _FILE_TYPE_NODE *pNext;
   int                     image;
   CHAR                    szExtAndDesc[2];
} FILE_TYPE_NODE, *LPFILE_TYPE_NODE;

typedef struct _FILE_NODE {
   zusz_t          uzSize;
   zusz_t          uzCompressedSize;
   DWORD           dwModified;
   DWORD           dwAttributes;
   DWORD           dwCRC;
   LPCSTR          szComment;
   LPCSTR          szType;
   CHAR            szPathAndMethod[2];
} FILE_NODE, *LPFILE_NODE;

typedef struct _COLUMN {
   LPTSTR szName;
   int    format;
} COLUMN, *LPCOLUMN;


//******************************************************************************
//***** Exported Function Prototypes
//******************************************************************************

void AddFileToListView(FILE_NODE *pFile);
LPCSTR GetFileFromPath(LPCSTR szPath);
void ForwardSlashesToBackSlashesA(LPSTR szBuffer);


//******************************************************************************
//***** Global Variables
//******************************************************************************

#ifdef GLOBAL_DECLARE
#undef GLOBAL_DECLARE
#undef GLOBAL_INIT
#endif

#ifdef __WINMAIN_CPP__
   #define GLOBAL_DECLARE
   #define GLOBAL_INIT(value) =value
#else
   #define GLOBAL_DECLARE extern
   #define GLOBAL_INIT(value)
#endif

GLOBAL_DECLARE HINSTANCE g_hInst                GLOBAL_INIT(NULL);
GLOBAL_DECLARE HWND      g_hWndMain             GLOBAL_INIT(NULL);
GLOBAL_DECLARE HWND      g_hWndEdit             GLOBAL_INIT(NULL);
GLOBAL_DECLARE HWND      g_hDlgProgress         GLOBAL_INIT(NULL);
GLOBAL_DECLARE CHAR      g_szZipFile[_MAX_PATH] GLOBAL_INIT("");

#ifdef __cplusplus
} // extern "C"
#endif

#endif // __WINMAIN_H__
