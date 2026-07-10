/*
  Copyright (c) 1990-2009 Info-ZIP.  All rights reserved.

  See the accompanying file LICENSE, version 2009-Jan-02 or later
  (the contents of which are also included in unzip.h) for terms of use.
  If, for some reason, all these files are missing, the Info-ZIP license
  also may be found at:  ftp://ftp.info-zip.org/pub/infozip/license.html
*/
//******************************************************************************
//
// File:        INTRFACE.CPP
//
// Description: This module acts as the interface between the Info-ZIP code and
//              our Windows code in WINMAIN.CPP.  We expose the needed
//              functions to query a file list, test file(s), extract file(s),
//              and display a zip file comment.  The windows code is never
//              bothered with understanding the Globals structure "Uz_Globs".
//
//              This module also catches all the callbacks from the Info-ZIP
//              code, cleans up the data provided in the callback, and then
//              forwards the information to the appropriate function in the
//              windows code.  These callbacks include status messages, file
//              lists, comments, password prompt, and file overwrite prompts.
//
//              Finally, this module implements the few functions that the
//              Info-ZIP code expects the port to implement. These functions are
//              OS dependent and are mostly related to validating file names and
//              directories, and setting file attributes and dates of saved files.
//
// Copyright:   All the source files for Pocket UnZip, except for components
//              written by the Info-ZIP group, are copyrighted 1997 by Steve P.
//              Miller.  As of June 1999, Steve P. Miller has agreed to apply
//              the Info-ZIP License (see citation on top of this module)
//              to his work.  See the contents of this License for terms
//              and conditon of using the product "Pocket UnZip".
//
// Disclaimer:  All project files are provided "as is" with no guarantee of
//              their correctness.  The authors are not liable for any outcome
//              that is the result of using this source.  The source for Pocket
//              UnZip has been placed in the public domain to help provide an
//              understanding of its implementation.  You are hereby granted
//              full permission to use this source in any way you wish, except
//              to alter Pocket UnZip itself.  For comments, suggestions, and
//              bug reports, please write to stevemil@pobox.com or the Info-ZIP
//              mailing list Zip-Bugs@lists.wku.edu.
//
// Functions:   DoListFiles
//              DoExtractOrTestFiles
//              DoGetComment
//              SetExtractToDirectory
//              InitGlobals
//              FreeGlobals
//              ExtractOrTestFilesThread
//              IsFileOrDirectory
//              SmartCreateDirectory
//              CheckForAbort2
//              SetCurrentFile
//              UzpMessagePrnt2
//              UzpInput2
//              UzpMorePause
//              UzpPassword
//              UzpReplace
//              UzpSound
//              SendAppMsg
//              win_fprintf
//              test_NT
//              utimeToFileTime
//              GetFileTimes
//              IsOldFileSystem
//              SetFileSize
//              close_outfile
//              do_wild
//              mapattr
//              mapname
//              checkdir
//              match
//              iswild
//              conv_to_rule
//              GetPlatformLocalTimezone
//              wide_to_local_string
//
//
// Date      Name          History
// --------  ------------  -----------------------------------------------------
// 02/01/97  Steve Miller  Created (Version 1.0 using Info-ZIP UnZip 5.30)
// 08/01/99  Johnny Lee, Christian Spieler, Steve Miller, and others
//                         Adapted to UnZip 5.41 (Version 1.1)
// 12/01/02  Chr. Spieler  Updated interface for UnZip 5.50
// 02/23/05  Chr. Spieler  Modified and optimized utimeToFileTime() to support
//                         the NO_W32TIMES_IZFIX compilation option
// 11/01/09  Chr. Spieler  Added wide_to_local_string() conversion function
//                         from win32.c, which is currently needed for the
//                         new UTF-8 names support (until we manage to port
//                         the complete UnZip code to native wide-char support).
//
//*****************************************************************************


//*****************************************************************************
// The following information and structure are here just for reference
//*****************************************************************************
//
// The Windows CE version of Unzip builds with the following defines set:
//
//
//    WIN32
//    _WINDOWS
//    UNICODE
//    _UNICODE
//    WIN32_LEAN_AND_MEAN
//    STRICT
//
//    POCKET_UNZIP         (Main define - Always set)
//
//    UNZIP_INTERNAL
//    WINDLL
//    DLL
//    REENTRANT
//    USE_EF_UT_TIME
//    NO_ZIPINFO
//    NO_STDDEF_H
//    NO_NTSD_EAS
//
//    USE_SMITH_CODE       (optional - See INSTALL document)
//    LZW_CLEAN            (optional - See INSTALL document)
//    NO_W32TIMES_IZFIX    (optional - See INSTALL document)
//
//    DEBUG                (When building for Debug)
//    _DEBUG               (When building for Debug)
//    NDEBUG               (When building for Retail)
//    _NDEBUG              (When building for Retail)
//
//    _WIN32_WCE=100       (When building for Windows CE native)
//
//****************************************************************************/

extern "C" {
#define __INTRFACE_CPP__
#define UNZIP_INTERNAL
#include "unzip.h"
#include "crypt.h"     // Needed to pick up CRYPT define
#include <commctrl.h>
#include "intrface.h"
#include "winmain.h"

#ifndef _WIN32_WCE
#include <process.h>   // _beginthreadex() and _endthreadex()
#endif

}
#include <tchar.h> // Must be outside of extern "C" block

#ifdef POCKET_UNZIP

//******************************************************************************
//***** "Local" Global Variables
//******************************************************************************

static USERFUNCTIONS  g_uf;
static EXTRACT_INFO  *g_pExtractInfo = NULL;
static FILE_NODE     *g_pFileLast    = NULL;
static CHAR           g_szExtractToDirectory[_MAX_PATH];
static BOOL           g_fOutOfMemory;

//******************************************************************************
//***** Local Function Prototypes
//******************************************************************************

// Internal functions of the GUI interface.
static Uz_Globs* InitGlobals(LPCSTR szZipFile);
static void FreeGlobals(Uz_Globs *pG);

#ifdef _WIN32_WCE
static DWORD WINAPI ExtractOrTestFilesThread(LPVOID lpv);
#else
static unsigned __stdcall ExtractOrTestFilesThread(void *lpv);
#endif

static void SetCurrentFile(__GPRO);

#endif // POCKET_UNZIP

// Internal helper functions for the UnZip core.
static int IsFileOrDirectory(LPCTSTR szPath);
static BOOL SmartCreateDirectory(__GPRO__ LPCSTR szDirectory, BOOL *pNewDir);
static void utimeToFileTime(time_t ut, FILETIME *pft, BOOL fOldFileSystem);
static int GetFileTimes(Uz_Globs *pG, FILETIME *pftCreated,
                        FILETIME *pftAccessed, FILETIME *pftModified);

// Check for FAT, VFAT, HPFS, etc.
static BOOL IsOldFileSystem(char *szPath);

#ifdef POCKET_UNZIP

extern "C" {

// Local variants of callbacks from Info-ZIP code.
// (These functions are not referenced by name outside this source module.)
int UZ_EXP UzpMessagePrnt2(zvoid *pG, uch *buffer, ulg size, int flag);
int UZ_EXP UzpInput2(zvoid *pG, uch *buffer, int *size, int flag);
int UZ_EXP CheckForAbort2(zvoid *pG, int fnflag, ZCONST char *zfn,
                          ZCONST char *efn, ZCONST zvoid *details);
int WINAPI UzpReplace(LPSTR szFile, unsigned nbufsiz);
void WINAPI UzpSound(void);
#ifdef Z_UINT8_DEFINED
void WINAPI SendAppMsg(z_uint8 uzSize, z_uint8 uzCompressedSize,
#else
void WINAPI SendAppMsg(ulg uzSize, ulg uzCompressedSize,
#endif
                       unsigned ratio,
                       unsigned month, unsigned day, unsigned year,
                       unsigned hour, unsigned minute, char uppercase,
                       LPCSTR szPath, LPCSTR szMethod, ulg dwCRC,
                       char chCrypt);

} // extern "C"


//******************************************************************************
//***** Our exposed interface functions to the Info-ZIP core
//******************************************************************************

int DoListFiles(LPCSTR szZipFile) {

   int result;

   // Create our Globals struct and fill it in whith some default values.
   Uz_Globs *pG = InitGlobals(szZipFile);
   if (!pG) {
      return PK_MEM;
   }

   pG->UzO.vflag = 1; // verbosely: list directory (for WIN32 it is 0 or 1)
   pG->process_all_files = TRUE; // improves speed

   g_pFileLast = NULL;
   g_fOutOfMemory = FALSE;

   // We wrap some exception handling around the entire Info-ZIP engine to be
   // safe.  Since we are running on a device with tight memory configurations,
   // all sorts of problems can arise when we run out of memory.
   __try {

      // Call the unzip routine.  We will catch the file information in a
      // callback to SendAppMsg().
      result = process_zipfiles(pG);

      // Make sure we didn't run out of memory in the process.
      if (g_fOutOfMemory) {
         result = PK_MEM;
      }

   } __except(EXCEPTION_EXECUTE_HANDLER) {

      // Catch any exception here.
      DebugOut(TEXT("Exception 0x%08X occurred in DoListFiles()"),
               GetExceptionCode());
      result = PK_EXCEPTION;
   }

   g_pFileLast = NULL;

   // It is possible that the ZIP engine change the file name a bit (like adding
   // a ".zip" if needed).  If so, we will pick up the new name.
   if ((result != PK_EXCEPTION) && pG->zipfn && *pG->zipfn) {
      strcpy(g_szZipFile, pG->zipfn);
   }

   // Free our globals.
   FreeGlobals(pG);

   return result;
}

//******************************************************************************
BOOL DoExtractOrTestFiles(LPCSTR szZipFile, EXTRACT_INFO *pei) {

   // WARNING!!!  This functions hands the EXTRACT_INFO structure of to a thread
   // to perform the actual extraction/test.  When the thread is done, it will
   // send a message to the progress dialog.  The calling function must not
   // delete the EXTRACT_INFO structure until it receives the message.  Currently,
   // this is not a problem for us since the structure lives on the stack of the
   // calling thread.  The calling thread then displays a dialog that blocks the
   // calling thread from clearing the stack until the dialog is dismissed, which
   // occurs when the dialog receives the message.

   // Create our globals so we can store the file name.
   Uz_Globs *pG = InitGlobals(szZipFile);
   if (!pG) {
      pei->result = PK_MEM;
      SendMessage(g_hDlgProgress, WM_PRIVATE, MSG_OPERATION_COMPLETE, (LPARAM)pei);
      return FALSE;
   }

   // Store a global pointer to the Extract structure so it can be reached from
   // our thread and callback functions.
   g_pExtractInfo = pei;

   // Spawn our thread
   DWORD dwThreadId;
   HANDLE hThread;

#ifdef _WIN32_WCE

   // On CE, we use good old CreateThread() since the WinCE CRT does not
   // allocate per-thread storage.
   hThread = CreateThread(NULL, 0, ExtractOrTestFilesThread, pG, 0,
                          &dwThreadId);

#else

   // On NT, we need use the CRT's thread function so that we don't leak any
   // CRT allocated memory when the thread exits.
   hThread = (HANDLE)_beginthreadex(NULL, 0, ExtractOrTestFilesThread, pG, 0,
                                    (unsigned*)&dwThreadId);

#endif

   // Bail out if our thread failed to create.
   if (!hThread) {

      DebugOut(TEXT("CreateThread() failed [%u]"), GetLastError());

      // Set our error as a memory error.
      g_pExtractInfo->result = PK_MEM;

      // Free our globals.
      FreeGlobals(pG);

      // Tell the progress dialog that we are done.
      SendMessage(g_hDlgProgress, WM_PRIVATE, MSG_OPERATION_COMPLETE, (LPARAM)pei);

      g_pExtractInfo = NULL;
      return FALSE;
   }

   // Close our thread handle since we have no use for it.
   CloseHandle(hThread);
   return TRUE;
}

//******************************************************************************
int DoGetComment(LPCSTR szZipFile) {

   int result;

   // Create our Globals struct and fill it in with some default values.
   Uz_Globs *pG = InitGlobals(szZipFile);
   if (!pG) {
      return PK_MEM;
   }

   pG->UzO.zflag = TRUE; // display the zipfile comment

   // We wrap some exception handling around the entire Info-ZIP engine to be
   // safe.  Since we are running on a device with tight memory configurations,
   // all sorts of problems can arise when we run out of memory.
   __try {

      // Call the unzip routine.  We will catch the comment string in a callback
      // to win_fprintf().
      result = process_zipfiles(pG);

   } __except(EXCEPTION_EXECUTE_HANDLER) {

      // Catch any exception here.
      DebugOut(TEXT("Exception 0x%08X occurred in DoGetComment()"),
               GetExceptionCode());
      result = PK_EXCEPTION;
   }

   // Free our globals.
   FreeGlobals(pG);

   return result;
}

//******************************************************************************
BOOL SetExtractToDirectory(LPTSTR szDirectory) {

   BOOL fNeedToAddWack = FALSE;

   // Remove any trailing wack from the path.
   int length = _tcslen(szDirectory);
   if ((length > 0) && (szDirectory[length - 1] == TEXT('\\'))) {
      szDirectory[--length] = TEXT('\0');
      fNeedToAddWack = TRUE;
   }

#ifndef _WIN32_WCE

   // Check to see if a root directory was specified.
   if ((length == 2) && isalpha(szDirectory[0]) && (szDirectory[1] == ':')) {

      // If just a root is specified, we need to only verify the drive letter.
      if (!(GetLogicalDrives() & (1 << (tolower(szDirectory[0]) - (int)'a')))) {

         // This drive does not exist.  Bail out with a failure.
         return FALSE;
      }

   } else

#endif

   // We only verify path if length is >0 since we know "\" is valid.
   if (length > 0) {

      // Verify the the path exists and that it is a directory.
      if (IsFileOrDirectory(szDirectory) != 2) {
         return FALSE;
      }
   }

   // Store the directory for when we do an extract.
   TSTRTOMBS(g_szExtractToDirectory, szDirectory, countof(g_szExtractToDirectory));

   // We always want a wack at the end of our path.
   strcat(g_szExtractToDirectory, "\\");

   // Add the wack back to the end of the path.
   if (fNeedToAddWack) {
      _tcscat(szDirectory, TEXT("\\"));
   }

   return TRUE;
}

//******************************************************************************
//***** Internal functions
//******************************************************************************

static Uz_Globs* InitGlobals(LPCSTR szZipFile)
{
   // Create our global structure - pG
   CONSTRUCTGLOBALS();

   // Bail out if we failed to allocate our Globals structure.
   if (!pG) {
      return NULL;
   }

   // Clear our USERFUNCTIONS structure
   ZeroMemory(&g_uf, sizeof(g_uf));

   // Initialize a global pointer to our USERFUNCTIONS structure that is
   // used by WINMAIN.CPP to access it (without using the pG construction).
   lpUserFunctions = &g_uf;

   // Store a global pointer to our USERFUNCTIONS structure in pG so that
   // the generic Info-ZIP code LIST.C and PROCESS.C can access it.
   pG->lpUserFunctions = &g_uf;

   // Fill in all our callback functions.
   pG->message      = UzpMessagePrnt2;
   pG->input        = UzpInput2;
   pG->mpause       = UzpMorePause;
   pG->statreportcb = CheckForAbort2;
   pG->lpUserFunctions->replace                = UzpReplace;
   pG->lpUserFunctions->sound                  = UzpSound;
   pG->lpUserFunctions->SendApplicationMessage = SendAppMsg;
   pG->lpUserFunctions->SendApplicationMessage_i32 = NULL;

#if CRYPT
   pG->decr_passwd = UzpPassword;
#endif

   // Match filenames case-sensitively.  We can do this since we can guarantee
   // exact case because the user can only select files via our UI.
   pG->UzO.C_flag = FALSE;

   // Allocate and store the ZIP file name in pG->zipfn
   if (!(pG->zipfnPtr = new char[FILNAMSIZ])) {
      FreeGlobals(pG);
      return NULL;
   }
   pG->zipfn = pG->zipfnPtr;
   strcpy(pG->zipfn, szZipFile);

   // Allocate and store the ZIP file name in pG->zipfn.  This needs to done
   // so that do_wild() does not wind up clearing out the zip file name when
   // it returns in process.c
   if (!(pG->wildzipfnPtr = new char[FILNAMSIZ])) {
      FreeGlobals(pG);
      return NULL;
   }
   pG->wildzipfn = pG->wildzipfnPtr;
   strcpy(pG->wildzipfn, szZipFile);

   return pG;
}

//******************************************************************************
static void FreeGlobals(Uz_Globs *pG)
{
   // Free our ZIP file name
   if (pG->zipfnPtr) {
      delete[] pG->zipfnPtr;
      pG->zipfnPtr = pG->zipfn = NULL;
   }

   // Free our wild name buffer
   if (pG->wildzipfnPtr) {
      delete[] pG->wildzipfnPtr;
      pG->wildzipfnPtr = pG->wildzipfn = NULL;
   }

   // Free everything else.
   DESTROYGLOBALS();
}

//******************************************************************************
#ifdef _WIN32_WCE

// On WinCE, we declare our thread function the way CreateThread() likes it.
static DWORD WINAPI ExtractOrTestFilesThread(LPVOID lpv)

#else

// On WinNT, we declare our thread function the way _beginthreadex likes it.
static unsigned __stdcall ExtractOrTestFilesThread(void *lpv)

#endif
{
   Uz_Globs *pG = (Uz_Globs*)lpv;

   if (g_pExtractInfo->fExtract) {

      pG->extract_flag = TRUE;

      switch (g_pExtractInfo->overwriteMode) {

         case OM_NEWER:         // Update (extract only newer/brand-new files)
            pG->UzO.uflag = TRUE;
            break;

         case OM_ALWAYS:        // OK to overwrite files without prompting
            pG->UzO.overwrite_all = TRUE;
            break;

         case OM_NEVER:         // Never overwrite files (no prompting)
            pG->UzO.overwrite_none = TRUE;
            break;

         default:               // Force a prompt
            pG->UzO.overwrite_all = FALSE;
            pG->UzO.overwrite_none = FALSE;
            pG->UzO.uflag = FALSE;
            break;
      }

      // Throw away paths if requested.
      pG->UzO.jflag = !g_pExtractInfo->fRestorePaths;

   } else {
      pG->UzO.tflag = TRUE;
   }

   if (g_pExtractInfo->szFileList) {
      pG->filespecs = g_pExtractInfo->dwFileCount;
      pG->pfnames = g_pExtractInfo->szFileList;
   } else {
      // Improves performance if all files are being extracted.
      pG->process_all_files = TRUE;
   }

   // Invalidate our file offset to show that we are starting a new operation.
   g_pExtractInfo->uzFileOffset = ~(zusz_t)0;

   // We wrap some exception handling around the entire Info-ZIP engine to be
   // safe.  Since we are running on a device with tight memory configurations,
   // all sorts of problems can arise when we run out of memory.
   __try {

      // Put a jump marker on our stack so the user can abort.
      int error = setjmp(dll_error_return);

      // If setjmp() returns 0, then we just set our jump marker and we can
      // continue with the operation.  If setjmp() returned something else,
      // then we reached this point because the operation was aborted and
      // set our instruction pointer back here.

      if (error > 0) {
         // We already called process_zipfiles() and were thrown back here.
         g_pExtractInfo->result = (error == 1) ? PK_BADERR : error;

      } else {
         // Entering Info-ZIP... close your eyes.
         g_pExtractInfo->result = process_zipfiles(pG);
      }

   } __except(EXCEPTION_EXECUTE_HANDLER) {

      // Catch any exception here.
      DebugOut(TEXT("Exception 0x%08X occurred in ExtractOrTestFilesThread()"),
               GetExceptionCode());
      g_pExtractInfo->result = PK_EXCEPTION;
   }

   // Free our globals.
   FreeGlobals(pG);

   // Tell the progress dialog that we are done.
   SendMessage(g_hDlgProgress, WM_PRIVATE, MSG_OPERATION_COMPLETE,
               (LPARAM)g_pExtractInfo);

   // Clear our global pointer as we are done with it.
   g_pExtractInfo = NULL;

#ifndef _WIN32_WCE
   // On NT, we need to free any CRT allocated memory.
   _endthreadex(0);
#endif

   return 0;
}

//******************************************************************************
static void SetCurrentFile(__GPRO)
{
   // Reset all our counters as we about to process a new file.
   g_pExtractInfo->uzFileOffset = (zusz_t)G.pInfo->offset;
   g_pExtractInfo->dwFile++;
   g_pExtractInfo->uzBytesWrittenThisFile = 0;
   g_pExtractInfo->uzBytesWrittenPreviousFiles += g_pExtractInfo->uzBytesTotalThisFile;
   g_pExtractInfo->uzBytesTotalThisFile = G.lrec.ucsize;
   g_pExtractInfo->szFile = G.filename;
   g_pExtractInfo->fNewLineOfText = TRUE;

   // Pass control to our GUI thread to do a full update our progress dialog.
   SendMessage(g_hWndMain, WM_PRIVATE, MSG_UPDATE_PROGRESS_COMPLETE,
               (LPARAM)g_pExtractInfo);

   // Check our abort flag.
}
#endif // POCKET_UNZIP

//******************************************************************************
static int IsFileOrDirectory(LPCTSTR szPath)
{
   // Geth the attributes of the item.
   DWORD dwAttribs = GetFileAttributes(szPath);

   // Bail out now if we could not find the path at all.
   if (dwAttribs == 0xFFFFFFFF) {
      return 0;
   }

   // Return 1 for file and 2 for directory.
   return ((dwAttribs & FILE_ATTRIBUTE_DIRECTORY) ? 2 : 1);
}

//******************************************************************************
static BOOL SmartCreateDirectory(__GPRO__ LPCSTR szDirectory, BOOL *pNewDir)
{
   // Copy path to a UNICODE buffer.
   TCHAR szBuffer[_MAX_PATH];
   MBSTOTSTR(szBuffer, szDirectory, countof(szBuffer));

   switch (IsFileOrDirectory(szBuffer)) {
      case 0:
         // Create the directory if it does not exist.
         if (!CreateDirectory(szBuffer, NULL)) {
            Info(slide, 1, ((char *)slide, "error creating directory: %s\n",
              FnFilter1( szDirectory)));
            return FALSE;
         }
         if (pNewDir != NULL) *pNewDir = TRUE;
         break;

      case 1:
         // If there is a file with the same name, then display an error.
         Info(slide, 1, ((char *)slide,
              "cannot create %s as a file with same name already exists.\n",
              FnFilter1(szDirectory)));
         return FALSE;
   }

   // If the directory already exists or was created, then return success.
   return TRUE;
}


#ifdef POCKET_UNZIP
//******************************************************************************
//***** Callbacks from Info-ZIP code.
//******************************************************************************

int UZ_EXP UzpMessagePrnt2(zvoid *pG, uch *buffer, ulg size, int flag)
{

   // Some ZIP files cause us to get called during DoListFiles(). We only handle
   // messages while processing DoExtractFiles().
   if (!g_pExtractInfo) {
      if (g_hWndEdit) {
         SendMessage(g_hWndMain, WM_PRIVATE, MSG_ADD_TEXT_TO_EDIT,
                     (LPARAM)buffer);
      } else {
#ifdef UNICODE
         DebugOut(TEXT("Unhandled call to UzpMessagePrnt2(\"%S\")"), buffer);
#else
         DebugOut(TEXT("Unhandled call to UzpMessagePrnt2(\"%s\")"), buffer);
#endif
      }
      return 0;
   }

   // When extracting, mapname() will get called for every file which in turn
   // will call SetCurrentFile().  For testing though, mapname() never gets
   // called so we need to be on the lookout for a new file.
   if (g_pExtractInfo->uzFileOffset != (zusz_t)((Uz_Globs*)pG)->pInfo->offset) {
      SetCurrentFile((Uz_Globs*)pG);
   }

   // Make sure this message was intended for us to display.
   if (!MSG_NO_WGUI(flag) && !MSG_NO_WDLL(flag)) {

      // Insert a leading newline if requested to do so.
      if (MSG_LNEWLN(flag) && !g_pExtractInfo->fNewLineOfText) {
         SendMessage(g_hWndMain, WM_PRIVATE, MSG_ADD_TEXT_TO_EDIT, (LPARAM)"\n");
         g_pExtractInfo->fNewLineOfText = TRUE;
      }

      // Since we use a proportional font, we need to do a little cleanup of the
      // text we are passed since it assumes a fixed font and adds padding to try
      // to line things up.  We remove leading whitespace on any new line of text.
      if (g_pExtractInfo->fNewLineOfText) {
         while (*buffer == ' ') {
            buffer++;
         }
      }

      // We always remove trailing whitespace.
      LPSTR psz = (LPSTR)buffer;
      LPSTR pszn;
      while ((pszn = MBSCHR(psz, ' ')) != NULL) {
         for (psz = pszn+1; *psz == ' '; psz++);
         if (*psz == '\0') {
            *pszn = '\0';
            break;
         }
      }


      // Determine if the next line of text will be a new line of text.
      g_pExtractInfo->fNewLineOfText = ((*psz == '\r') || (*psz == '\n'));

      // Change all forward slashes to back slashes in the buffer
      ForwardSlashesToBackSlashesA((LPSTR)buffer);

      // Add the cleaned-up text to our extraction log edit control.
      SendMessage(g_hWndMain, WM_PRIVATE, MSG_ADD_TEXT_TO_EDIT, (LPARAM)buffer);

      // Append a trailing newline if requested to do so.
      if (MSG_TNEWLN(flag) || MSG_MNEWLN(flag) && !g_pExtractInfo->fNewLineOfText) {
         SendMessage(g_hWndMain, WM_PRIVATE, MSG_ADD_TEXT_TO_EDIT, (LPARAM)"\n");
         g_pExtractInfo->fNewLineOfText = TRUE;
      }
   }

   return 0;
}

//******************************************************************************
int UZ_EXP UzpInput2(zvoid *pG, uch *buffer, int *size, int flag)
{
   DebugOut(TEXT("WARNING: UzpInput2(...) called"));
   return 0;
}

//******************************************************************************
void UZ_EXP UzpMorePause(zvoid *pG, const char *szPrompt, int flag)
{
   DebugOut(TEXT("WARNING: UzpMorePause(...) called"));
}

//******************************************************************************
int UZ_EXP UzpPassword(zvoid *pG, int *pcRetry, char *szPassword, int nSize,
                       const char *szZipFile, const char *szFile)
{
   // Return Values:
   //    IZ_PW_ENTERED    got some PWD string, use/try it
   //    IZ_PW_CANCEL     no password available (for this entry)
   //    IZ_PW_CANCELALL  no password, skip any further PWD request
   //    IZ_PW_ERROR      failure (no mem, no tty, ...)

#if CRYPT

   // Build the data structure for our dialog.
   DECRYPT_INFO di;
   di.retry      = *pcRetry;
   di.szPassword = szPassword;
   di.nSize      = nSize;
   di.szFile     = szFile;

   // Clear the password to be safe.
   *di.szPassword = '\0';

   // On our first call for a file, *pcRetry == 0.  If we would like to allow
   // for retries, then we set the value of *pcRetry to the number of retries we
   // are willing to allow.  We will be recalled as neccessary, each time with
   // *pcRetry being decremented once.  1 is the last retry we will get.
   *pcRetry = (*pcRetry == 0) ? MAX_PASSWORD_RETRIES : (*pcRetry - 1);

   // Pass control to our GUI thread which will prompt the user for a password.
   return SendMessage(g_hWndMain, WM_PRIVATE, MSG_PROMPT_FOR_PASSWORD, (LPARAM)&di);

#else
   return IZ_PW_CANCELALL;
#endif
}

//******************************************************************************
int UZ_EXP CheckForAbort2(zvoid *pG, int fnflag, ZCONST char *zfn,
                    ZCONST char *efn, ZCONST zvoid *details)
{
   int rval = UZ_ST_CONTINUE;

   if (g_pExtractInfo->fAbort) {

      // Add a newline to our log if we are in the middle of a line of text.
      if (!g_pExtractInfo->fNewLineOfText) {
         SendMessage(g_hWndMain, WM_PRIVATE, MSG_ADD_TEXT_TO_EDIT, (LPARAM)"\n");
      }

      // Make sure whatever file we are currently processing gets closed.
      if (((int)((Uz_Globs *)pG)->outfile != 0) &&
          ((int)((Uz_Globs *)pG)->outfile != -1)) {
         if (g_pExtractInfo->fExtract && *efn) {

            // Make sure the user is aware that this file is screwed.
            SendMessage(g_hWndMain, WM_PRIVATE, MSG_ADD_TEXT_TO_EDIT,
                        (LPARAM)"warning: ");
            SendMessage(g_hWndMain, WM_PRIVATE, MSG_ADD_TEXT_TO_EDIT,
                        (LPARAM)efn);
            SendMessage(g_hWndMain, WM_PRIVATE, MSG_ADD_TEXT_TO_EDIT,
                        (LPARAM)" is probably truncated.\n");
         }
      }

      // Display an aborted message in the log
      SendMessage(g_hWndMain, WM_PRIVATE, MSG_ADD_TEXT_TO_EDIT,
                  (LPARAM)"Operation aborted by user.\n");

      // Signal "Immediate Cancel" back to the UnZip engine.
      rval = UZ_ST_BREAK;
   }

   return rval;
}

//******************************************************************************
int WINAPI UzpReplace(LPSTR szFile, unsigned nbufsiz) {
   // Pass control to our GUI thread which will prompt the user to overwrite.
   // The nbufsiz parameter is not needed here, because this program does not
   // (yet?) contain support for renaming the extraction target.
   return SendMessage(g_hWndMain, WM_PRIVATE, MSG_PROMPT_TO_REPLACE,
                      (LPARAM)szFile);
}

//******************************************************************************
void WINAPI UzpSound(void) {
   // Do nothing.
}

//******************************************************************************
// Called from LIST.C
#ifdef Z_UINT8_DEFINED
void WINAPI SendAppMsg(z_uint8 uzSize, z_uint8 uzCompressedSize,
#else
void WINAPI SendAppMsg(ulg uzSize, ulg uzCompressedSize,
#endif
                       unsigned ratio,
                       unsigned month, unsigned day, unsigned year,
                       unsigned hour, unsigned minute, char uppercase,
                       LPCSTR szPath, LPCSTR szMethod, ulg dwCRC,
                       char chCrypt)
{
   // If we are out of memory, then just bail since we will only make things worse.
   if (g_fOutOfMemory) {
      return;
   }

   // We get our Globals structure.
   GETGLOBALS();

   // Allocate a FILE_NODE large enough to hold this file.
   int length = strlen(szPath) + strlen(szMethod);
   g_pFileLast = (FILE_NODE*)new BYTE[sizeof(FILE_NODE) +
                                      (sizeof(CHAR) * length)];

   // Bail out if we failed to allocate the node.
   if (!g_pFileLast) {
#ifdef UNICODE
      DebugOut(TEXT("Failed to create a FILE_NODE for \"%S\"."), szPath);
#else
      DebugOut(TEXT("Failed to create a FILE_NODE for \"%s\"."), szPath);
#endif
      g_fOutOfMemory = TRUE;
      return;
   }

   // Fill in our node.
   g_pFileLast->uzSize           = (zusz_t)uzSize;
   g_pFileLast->uzCompressedSize = (zusz_t)uzCompressedSize;
   g_pFileLast->dwCRC            = dwCRC;
   g_pFileLast->szComment        = NULL;
   g_pFileLast->szType           = NULL;

   // Fix the year value to contain the real year.
   year += 1900;

   // Year:   0 - 4095 (12) 1111 1111 1111 0000 0000 0000 0000 0000 (0xFFF00000)
   // Month:  1 -   12 ( 4) 0000 0000 0000 1111 0000 0000 0000 0000 (0x000F0000)
   // Day:    1 -   31 ( 5) 0000 0000 0000 0000 1111 1000 0000 0000 (0x0000F800)
   // Hour:   0 -   23 ( 5) 0000 0000 0000 0000 0000 0111 1100 0000 (0x000007C0)
   // Minute: 0 -   59 ( 6) 0000 0000 0000 0000 0000 0000 0011 1111 (0x0000003F)

   // Do some bit shifting to make the date and time fit in a DWORD.
   g_pFileLast->dwModified = (((DWORD)(year   & 0x0FFF) << 20) |
                              ((DWORD)(month  & 0x000F) << 16) |
                              ((DWORD)(day    & 0x001F) << 11) |
                              ((DWORD)(hour   & 0x001F) <<  6) |
                              ((DWORD)(minute & 0x003F)));

   // We need to get our globals structure to determine our attributes and
   // encryption information.
   g_pFileLast->dwAttributes = (pG->crec.external_file_attributes & 0xFF);
   if (chCrypt == 'E') {
      g_pFileLast->dwAttributes |= ZFILE_ATTRIBUTE_ENCRYPTED;
   }

   // Store the path and method in our string buffer.
   strcpy(g_pFileLast->szPathAndMethod, szPath);
   strcpy(g_pFileLast->szPathAndMethod + strlen(szPath) + 1, szMethod);

   // Pass the file object to our windows code to have it added to our list.
   AddFileToListView(g_pFileLast);
}

//******************************************************************************
int win_fprintf(zvoid *pG, FILE *file, unsigned int dwCount, char far *buffer)
{

   // win_fprintf() is used within Info-ZIP to write to a file as well as log
   // information.  If the "file" is a real file handle (not stdout or stderr),
   // then we write the data to the file and return.

   if ((file != stdout) && (file != stderr)) {

      DWORD dwBytesWritten = 0;
#if (defined(_WIN32_WCE) && (_WIN32_WCE < 211))
      // On WinCE all FILEs are really HANDLEs.  See WINCE.CPP for more info.
      WriteFile((HANDLE)file, buffer, dwCount, &dwBytesWritten, NULL);
#else
      dwBytesWritten = fwrite(buffer, 1, dwCount, file);
#endif

      // Update our bytes written count.
      g_pExtractInfo->uzBytesWrittenThisFile += dwBytesWritten;

      // Pass control to our GUI thread to do a partial update our progress dialog.
      SendMessage(g_hWndMain, WM_PRIVATE, MSG_UPDATE_PROGRESS_PARTIAL,
                  (LPARAM)g_pExtractInfo);

      return dwBytesWritten;
   }

   // Check to see if we are expecting a extraction progress string
   if (g_pExtractInfo) {

      // Most of our progress strings come to our UzpMessagePrnt2() callback,
      // but we occasionally get one here.  We will just forward it to
      // UzpMessagePrnt2() as if it never came here.
      UzpMessagePrnt2(pG, (uch*)buffer, dwCount, 0);
      return dwCount;
   }

   // Check to see if we are expecting a zip file comment string.
   if (g_hWndEdit) {

      // Change all forward slashes to back slashes in the buffer
      ForwardSlashesToBackSlashesA((LPSTR)buffer);

      SendMessage(g_hWndMain, WM_PRIVATE, MSG_ADD_TEXT_TO_EDIT, (LPARAM)buffer);
      return dwCount;
   }

   // Check to see if we are expecting a compressed file comment string.
   if (g_pFileLast) {
      char *p1, *p2;

      // Calcalute the size of the buffer we will need to store this comment.
      // We are going to convert all ASC values 0 - 31 (except tab, new line,
      // and CR) to ^char.
      int size = 1;
      for (p1 = buffer; *p1; INCSTR(p1)) {
         size += ((*p1 >= 32) || (*p1 == '\t') ||
                  (*p1 == '\r') || (*p1 == '\n')) ? CLEN(p1) : 2;
      }

      // Allocate a comment buffer and assign it to the last file node we saw.
      if (g_pFileLast->szComment = new CHAR[size]) {

         // Copy while formatting.
         for (p1 = buffer, p2 = (char*)g_pFileLast->szComment; *p1; INCSTR(p1)) {
            if ((*p1 >= 32) || (*p1 == '\t') ||
                (*p1 == '\r') || (*p1 == '\n')) {
               memcpy(p2, p1, CLEN(p1));
               p2 += CLEN(p1);
            } else {
               *(p2++) = '^';
               *(p2++) = 64 + *p1;
            }
         }
         *p2 = '\0';
      }

      // Update the attributes of the file node to include the comment attribute.
      g_pFileLast->dwAttributes |= ZFILE_ATTRIBUTE_COMMENT;

      // Clear the file node so we don't try to add another bogus comment to it.
      g_pFileLast = NULL;

      return dwCount;
   }

   if (dwCount >= _MAX_PATH) {
      buffer[_MAX_PATH] = '\0';
   }
#ifdef UNICODE
   DebugOut(TEXT("Unhandled call to win_fprintf(\"%S\")"), buffer);
#else
   DebugOut(TEXT("Unhandled call to win_fprintf(\"%S\")"), buffer);
#endif
   return dwCount;
}

//******************************************************************************
void WINAPI Wiz_NoPrinting(int f) {
   // Do nothing.
}

#endif // POCKET_UNZIP

//******************************************************************************
//***** Functions that Info-ZIP expects the port to write and export.
//***** Some of this code was stolen from the WIN32 port and highly modified.
//******************************************************************************

#ifdef NTSD_EAS
#ifndef SFX
//******************************************************************************
// Called from EXTRACT.C
int test_NTSD(__GPRO__ uch *eb, unsigned eb_size) {
   // This function is called when an NT security descriptor is found in the
   // extra field.  We have nothing to do, so we just return success.
   return PK_OK;
}
#endif /* !SFX */
#endif /* NTSD_EAS */

static void utimeToFileTime(time_t ut, FILETIME *pft, BOOL fOldFileSystem)
{

   // time_t    is a 32-bit value for the seconds since January 1, 1970
   // FILETIME  is a 64-bit value for the number of 100-nanosecond intervals
   //           since January 1, 1601
   // DWORDLONG is a 64-bit unsigned int that we can use to perform large math
   //           operations.


   // time_t has minimum of 1/1/1970.  Many file systems, such as FAT, have a
   // minimum date of 1/1/1980.  If extracting to one of those file systems and
   // out time_t is less than 1980, then we make it 1/1/1980.
   // (365 days/yr * 10 yrs + 3 leap yr days) * (60 secs * 60 mins * 24 hrs).
   if (fOldFileSystem && (ut < 0x12CFF780)) {
      ut = 0x12CFF780;
   }

#ifndef NO_W32TIMES_IZFIX
   // Now for the next fix for old file systems.  If we are in Daylight Savings
   // Time (DST) and the file is not in DST, then we need subtract off the DST
   // bias from the filetime.  This is due to a bug in Windows (NT, CE, and 95)
   // that causes the DST bias to be added to all file times when the system
   // is in DST, even if the file is not in DST.  This only effects old file
   // systems since they store local times instead of UTC times.  Newer file
   // systems like NTFS and CEFS store UTC times.
   if (fOldFileSystem)
#endif
   {
      // We use the CRT's localtime() and Win32's LocalTimeToFileTime()
      // functions to compute a FILETIME value that always shows the correct
      // local time in Windows' file listings.  This works because localtime()
      // correctly adds the DST bias only if the file time is in DST.
      // FileTimeToLocalTime() always adds the DST bias to the time.
      // Therefore, if the functions return different results, we know we
      // are dealing with a non-DST file during a system DST.

      FILETIME lftCRT;

      // Get the CRT result - result is a "tm" struct.
      struct tm *ptmCRT = localtime(&ut);

      // Check if localtime() returned something useful; continue with the
      // "NewFileSystem" code in case of an error. This failsafe method
      // should give an "almost" correct filetime result.
      if (ptmCRT != (struct tm *)NULL) {
         // Convert the "tm" struct to a FILETIME.
         SYSTEMTIME stCRT;
         ZeroMemory(&stCRT, sizeof(stCRT));
         if (fOldFileSystem && (ptmCRT->tm_year < 80)) {
            stCRT.wYear   = 1980;
            stCRT.wMonth  = 1;
            stCRT.wDay    = 1;
            stCRT.wHour   = 0;
            stCRT.wMinute = 0;
            stCRT.wSecond = 0;
         } else {
            stCRT.wYear   = ptmCRT->tm_year + 1900;
            stCRT.wMonth  = ptmCRT->tm_mon + 1;
            stCRT.wDay    = ptmCRT->tm_mday;
            stCRT.wHour   = ptmCRT->tm_hour;
            stCRT.wMinute = ptmCRT->tm_min;
            stCRT.wSecond = ptmCRT->tm_sec;
         }
         SystemTimeToFileTime(&stCRT, &lftCRT);
         LocalFileTimeToFileTime(&lftCRT, pft);
         // we are finished!
         return;
      }
   }
   // For "Modern" file system that stores timestamps in UTC (or as second
   // chance in case of localtime() errors) the conversion of time_t into
   // 64-bit FILETIME is a simple arithmetic rescaling calculation.
   // Compute the FILETIME for the given time_t.
   DWORDLONG dwl = ((DWORDLONG)116444736000000000 +
                   ((DWORDLONG)ut * (DWORDLONG)10000000));

   // Store the return value.
   *pft = *(FILETIME*)&dwl;

}

//******************************************************************************
static int GetFileTimes(__GPRO__ FILETIME *pftCreated,
                        FILETIME *pftAccessed, FILETIME *pftModified)
{
   // We need to check to see if this file system is limited.  This includes
   // FAT, VFAT, and HPFS.  It does not include NTFS and CEFS.  The limited
   // file systems can not support dates < 1980 and they store file local times
   // for files as opposed to UTC times.
   BOOL fOldFileSystem = IsOldFileSystem(G.filename);

#ifdef USE_EF_UT_TIME  // Always true for WinCE build

#ifdef IZ_CHECK_TZ
   if (G.extra_field && G.tz_is_valid) {
#else
   if (G.extra_field) {
#endif

      // Structure for Unix style actime, modtime, creatime
      iztimes z_utime;

      // Get any date/time we can.  This can return 0 to 3 unix time fields.
      unsigned eb_izux_flg = ef_scan_for_izux(G.extra_field,
                                              G.lrec.extra_field_length, 0,
                                              G.lrec.last_mod_dos_datetime,
                                              &z_utime, NULL);

      // We require at least a modified time.
      if (eb_izux_flg & EB_UT_FL_MTIME) {

         // We know we have a modified time, so get it first.
         utimeToFileTime(z_utime.mtime, pftModified, fOldFileSystem);

         // Get the accessed time if we have one.
         if (eb_izux_flg & EB_UT_FL_ATIME) {
            utimeToFileTime(z_utime.atime, pftAccessed, fOldFileSystem);
         }

         // Get the created time if we have one.
         if (eb_izux_flg & EB_UT_FL_CTIME) {
            utimeToFileTime(z_utime.ctime, pftCreated, fOldFileSystem);
         }

         // Return our flags.
         return (int)eb_izux_flg;
      }
   }

#endif // USE_EF_UT_TIME

   // If all else fails, we can resort to using the DOS date and time data.
   time_t ux_modtime = dos_to_unix_time(G.lrec.last_mod_dos_datetime);
   utimeToFileTime(ux_modtime, pftModified, fOldFileSystem);

   *pftAccessed = *pftModified;

   return (EB_UT_FL_MTIME | EB_UT_FL_ATIME);
}

//******************************************************************************
//***** Functions to correct time stamp bugs on old file systems.
//******************************************************************************

//******************************************************************************
// Borrowed/Modified from win32.c
static BOOL IsOldFileSystem(char *szPath) {

#ifdef _WIN32_WCE

   char szRoot[10];

   // Get the first nine characters of the path.
   strncpy(szRoot, szPath, 9);
   szRoot[9] = '\0';

   // Convert to uppercase to help with compare.
   _strupr(szRoot);

   // PC Cards are mounted off the root in a directory called "\PC Cards".
   // PC Cards are FAT, no CEOS.  We need to check if the file is being
   // extracted to the PC card.
   return !strcmp(szRoot, "\\PC CARD\\");

#else

   char szRoot[_MAX_PATH] = "\0\0\0", szFS[64];

   // Check to see if our path contains a drive letter.
   if (isalpha(szPath[0]) && (szPath[1] == ':') && (szPath[2] == '\\')) {

      // If so, then just copy the drive letter, colon, and wack to our root path.
      strncpy(szRoot, szPath, 3);

   } else {

      // Expand the path so we can get a drive letter.
      GetFullPathNameA(szPath, sizeof(szRoot), szRoot, NULL);

      // Make sure we actually got a drive letter back in our root path buffer..
      if (!isalpha(szRoot[0]) || (szRoot[1] != ':') || (szRoot[2] != '\\')) {

         // When in doubt, return TRUE.
         return TRUE;
      }
   }

   // NULL terminate after the wack to ensure we have just the root path.
   szRoot[3] = '\0';

   // Get the file system type string.
   GetVolumeInformationA(szRoot, NULL, 0, NULL, NULL, NULL, szFS, sizeof(szFS));

   // Ensure that the file system type string is uppercase.
   _strupr(szFS);

   // Return true for (V)FAT and (OS/2) HPFS format.
   return !strncmp(szFS, "FAT",  3) ||
          !strncmp(szFS, "VFAT", 4) ||
          !strncmp(szFS, "HPFS", 4);

#endif // _WIN32_WCE
}

//******************************************************************************
int SetFileSize(FILE *file, zusz_t filesize)
{
#if (defined(_WIN32_WCE) || defined(__RSXNT__))
    // For native Windows CE, it is not known whether the API supports
    // presetting a file's size.
    // RSXNT environment lacks a translation function from C file pointer
    // to Win32-API file handle.
    // So, simply do nothing.
    return 0;
#else /* !(_WIN32_WCE || __RSXNT__) */
    /* not yet verified, if that really creates an unfragmented file
      rommel@ars.de
     */
    HANDLE os_fh;
#ifdef Z_UINT8_DEFINED
    LARGE_INTEGER fsbuf;
#endif

    /* Win9x supports FAT file system, only; presetting file size does
       not help to prevent fragmentation. */
    if ((long)GetVersion() < 0) return 0;

    /* Win32-API calls require access to the Win32 file handle.
       The interface function used to retrieve the Win32 handle for
       a file opened by the C rtl is non-standard and may not be
       available for every Win32 compiler environment.
       (see also win32/win32.c of the Zip distribution)
     */
    os_fh = (HANDLE)_get_osfhandle(fileno(file));
    /* move file pointer behind the last byte of the expected file size */
#ifdef Z_UINT8_DEFINED
    fsbuf.QuadPart = filesize;
    if ((SetFilePointer(os_fh, fsbuf.LowPart, &fsbuf.HighPart, FILE_BEGIN)
         == 0xFFFFFFFF) && GetLastError() != NO_ERROR)
#else
    if (SetFilePointer(os_fh, filesize, 0, FILE_BEGIN) == 0xFFFFFFFF)
#endif
        return -1;
    /* extend/truncate file to the current position */
    if (SetEndOfFile(os_fh) == 0)
        return -1;
    /* move file position pointer back to the start of the file! */
    return (SetFilePointer(os_fh, 0, 0, FILE_BEGIN) == 0xFFFFFFFF) ? -1 : 0;
#endif /* ?(_WIN32_WCE || __RSXNT__) */
} /* end function SetFileSize() */

//******************************************************************************
void close_outfile(__GPRO)
{
   HANDLE hFile;

   TCHAR szFile[_MAX_PATH];
   MBSTOTSTR(szFile, G.filename, countof(szFile));

   /* skip restoring time stamps on user's request */
   if (uO.D_flag <= 1) {
      // Get the 3 time stamps for the file.
      FILETIME ftCreated, ftAccessed, ftModified;
      int timeFlags = GetFileTimes(__G__ &ftCreated, &ftAccessed, &ftModified);

#if (defined(_WIN32_WCE) && (_WIN32_WCE < 211))

      // Cast the outfile to a HANDLE (since that is really what it is), and
      // flush the file.  We need to flush, because any unsaved data that is
      // written to the file during CloseHandle() will step on the work done
      // by SetFileTime().
      hFile = (HANDLE)G.outfile;
      FlushFileBuffers(hFile);

#else

      // Close the file and then re-open it using the Win32 CreateFile() call.
      // SetFileTime() requires a Win32 file HANDLE created with GENERIC_WRITE
      // access.
      fclose(G.outfile);
      hFile = CreateFile(szFile, GENERIC_WRITE, FILE_SHARE_WRITE, NULL,
                         OPEN_EXISTING, FILE_ATTRIBUTE_NORMAL, NULL);

#endif

      // Set the file's date and time.
      if (hFile != INVALID_HANDLE_VALUE) {

         // Make sure we retrieved some valid time stamp(s)
         if (timeFlags) {

            // Set the various date and time fields.
            if (!SetFileTime(hFile,
                    (timeFlags & EB_UT_FL_CTIME) ? &ftCreated  : NULL,
                    (timeFlags & EB_UT_FL_ATIME) ? &ftAccessed : NULL,
                    (timeFlags & EB_UT_FL_MTIME) ? &ftModified : NULL))
            {
               DebugOut(TEXT("SetFileTime() failed [%u]"), GetLastError());
            }

         } else {
            DebugOut(TEXT("GetFileTimes() failed"));
         }

         // Close out file.
         CloseHandle(hFile);

      } else {
         DebugOut(TEXT("CreateFile() failed [%u]"), GetLastError());
      }
   }

   // If the file was successfully written, then set the attributes.
#ifdef POCKET_UNZIP
   if (!G.disk_full && !g_pExtractInfo->fAbort) {
#else
   if (!G.disk_full) {
#endif
      if (!SetFileAttributes(szFile, G.pInfo->file_attr & 0x7F)) {
         DebugOut(TEXT("SetFileAttributes() failed [%u]"), GetLastError());
      }
   }

   // Clear outfile so we know it is closed.
   G.outfile = 0;

   return;
}

//******************************************************************************
// Called by PROCESS.C
char* do_wild(__GPRO__ ZCONST char *wildspec)
{
   // This is a very slimmed down version of do_wild() taken from WIN32.C.
   // Since we don't support wildcards, we basically just return the wildspec
   // passed in as the filename.

#ifndef POCKET_UNZIP
   // Delete allocated storage for the match name
   if (G.matchname != NULL)
   {
      delete G.matchname;
      G.matchname = NULL;
   }
#endif

   // First call - must initialize everything.
   if (!G.notfirstcall) {
      G.notfirstcall = TRUE;
#ifdef POCKET_UNZIP
      return strcpy(G.matchname, wildspec);
#else
      // allocate some storage for the match name
      G.matchname = new char[strlen(wildspec) + 1];
      if (G.matchname != NULL)
         return strcpy(G.matchname, wildspec);
#endif
   }

   // Last time through - reset for new wildspec.
   G.notfirstcall = FALSE;

   return (char *)NULL;
}

//******************************************************************************
// Called from EXTRACT.C
int mapattr(__GPRO)
{
#ifdef POCKET_UNZIP
   // Check to see if we are extracting this file for viewing.  Currently, we do
   // this by checking the szMappedPath member of our extract info stucture
   // since we know OnActionView() is the only one who sets this member.

   if (g_pExtractInfo && g_pExtractInfo->szMappedPath) {

      // If we are extracting for view only, then we ignore the file's real
      // attributes and force the file to create as read-only.  We make the file
      // read-only to help prevent the user from making changes to the temporary
      // file and then trying to save the changes back to a file that we will
      // eventually delete.
      G.pInfo->file_attr = FILE_ATTRIBUTE_READONLY;

   } else
#endif
   {
      /* set archive bit for file entries (file is not backed up): */
      G.pInfo->file_attr = ((unsigned)G.crec.external_file_attributes |
        (G.crec.external_file_attributes & FILE_ATTRIBUTE_DIRECTORY ?
         0 : FILE_ATTRIBUTE_ARCHIVE)) & 0xff;
   }
   return 0;
} /* end function mapattr() */

//******************************************************************************
// Called from EXTRACT.C
//
// returns:
//  MPN_OK          - no problem detected
//  MPN_INF_TRUNC   - (on APPEND_NAME) truncated filename
//  MPN_INF_SKIP    - path doesn't exist, not allowed to create
//  MPN_ERR_SKIP    - path doesn't exist, tried to create and failed; or path
//                    exists and is not a directory, but is supposed to be
//  MPN_ERR_TOOLONG - path is too long
//  MPN_NOMEM       - can't allocate memory for filename buffers
//
//  MPN_VOL_LABEL   - Path was a volume label, skip it.
//  MPN_CREATED_DIR - Created a directory.
//
int mapname(__GPRO__ int renamed)
{
    int error = MPN_OK;
    CHAR szBuffer[countof(G.filename)] = "";
    CHAR *pIn = NULL, *pOut, *pLastSemi = NULL;
    CHAR *pPathComp, workch;
    BOOL killed_ddot = FALSE, renamed_fullpath = FALSE, created_dir = FALSE;

#ifdef POCKET_UNZIP
    // mapname() is a great place to reset all our status counters for the next
    // file to be processed since it is called for every zip file member before
    // any work is done with that member.
    SetCurrentFile(__G);
#endif

    // If Volume Label, skip the "extraction" quietly
    if (G.pInfo->vollabel) {
       return MPN_VOL_LABEL;
    }

#ifndef POCKET_UNZIP // The GUI interface does not support renaming...
    if (renamed) {
        pIn = G.filename;   // point to beginning of renamed name...
        if (*pIn) do {
            if (*pIn == '\\')   // convert backslashes to forward
                *pIn = '/';
        } while (*PREINCSTR(pIn));
        pIn = G.filename;
        // use temporary rootpath if user gave full pathname
        if (G.filename[0] == '/') {
            renamed_fullpath = TRUE;
            szBuffer[0] = '\\'; // copy the '/' and terminate
            szBuffer[1] = '\0';
            ++pIn;
        } else if (isalpha((uch)G.filename[0]) && G.filename[1] == ':') {
            renamed_fullpath = TRUE;
            pOut = szBuffer;
            *pOut++ = *pIn++;   // copy the "d:" (+ '/', possibly)
            *pOut++ = *pIn++;
            if (*pIn == '/') {
                *pOut++ = '\\';
                pIn++;          // otherwise add "./"?
            }
            *pOut = '\0';
        }
    }
#endif

    // Initialize file path buffer with our "extract to" path.
    if (!renamed_fullpath) {
#ifdef POCKET_UNZIP
        strcpy(szBuffer, g_szExtractToDirectory);
#else
        strcpy(szBuffer, G.rootpath);
#endif
        pOut = szBuffer + strlen(szBuffer);
    }
    pPathComp = pOut;

    if (!renamed) {
        // Point pIn to beginning of our internal pathname.
        // If we are junking paths, then locate the file portion of the path.
        if (uO.jflag)
            pIn = (CHAR*)MBSRCHR(G.filename, '/');
        if (pIn == NULL)
            pIn = G.filename;
        else
            ++pIn;
    }

    // Begin main loop through characters in filename.
    for ( ; (workch = *pIn) != '\0'; INCSTR(pIn)) {

        // Make sure we don't overflow our output buffer.
        if (pOut >= (szBuffer + countof(szBuffer) - 2)) {
            Info(slide, 1, ((char*)slide, "path too long: %s\n",
              FnFilter1(G.filename)));
            return MPN_ERR_TOOLONG;
        }

        // Examine the next character in our input buffer.
        switch (workch) {

          // Check for a directory wack.
          case '/':
            *pOut = '\0';
            // Skip dir traversals unless they are explicitly allowed.
            if (strcmp(pPathComp, ".") == 0) {
                // don't bother appending "./" to the path
                *pPathComp = '\0';
            } else if (!uO.ddotflag && strcmp(pPathComp, "..") == 0) {
                // "../" dir traversal detected, skip over it
                *pPathComp = '\0';
                killed_ddot = TRUE;     // set "show message" flag
            }
            // When path component is not empty, append it now.
            if (*pPathComp == '\0') {
                // Reset insert pos to start of path component.
                pOut = pPathComp;
            } else {
                if (!SmartCreateDirectory(__G__ szBuffer, &created_dir)) {
                   Info(slide, 1, ((char*)slide, "failure extracting: %s\n",
                        FnFilter1(G.filename)));
                   return MPN_ERR_SKIP;
                }
                *(pOut++) = '\\';
                pPathComp = pOut;  // Remember start pos of new path component
            }
            pLastSemi = NULL;  // Leave any directory semi-colons alone
            break;

          // Check for illegal characters and replace with underscore.
          case ':':
          case '\\':
          case '*':
          case '?':
          case '"':
          case '<':
          case '>':
          case '|':
            *(pOut++) = '_';
            break;

          // Check for start of VMS version.
          case ';':
            pLastSemi = pOut;  // Make note as to where we are.
            *(pOut++) = ';';   // Leave the semi-colon alone for now.
            break;

          default:
            // Allow European characters and spaces in filenames.
#ifdef _MBCS
            if ((UCHAR)workch >= 0x20) {
                memcpy(pOut, pIn, CLEN(pIn));
                INCSTR(pOut);
            } else {
                *(pOut++) = '_';
            }
#else
            *(pOut++) = (((UCHAR)workch >= 0x20) ? workch : '_');
#endif
       }
    }

    // Show warning when stripping insecure "parent dir" path components
    if (killed_ddot && QCOND2) {
        Info(slide, 0, ((char *)slide,
          "warning:  skipped \"../\" path component(s) in %s\n",
          FnFilter1(G.filename)));
        if (!(error & ~MPN_MASK))
            error = (error & MPN_MASK) | PK_WARN;
    }

    // Done with output buffer, terminate it.
    *pOut = '\0';

    // Remove any VMS version numbers if found (appended ";###").
    if (pLastSemi) {

        // Walk over all digits following the semi-colon.
        for (pOut = pLastSemi + 1; (*pOut >= '0') && (*pOut <= '9'); pOut++);

        // If we reached the end, then nuke the semi-colon and digits.
        if (!*pOut)
           *pLastSemi = '\0';
    }

    // Copy the mapped name back to the internal path buffer
    strcpy(G.filename, szBuffer);

#ifdef POCKET_UNZIP
    // Fill in the mapped name buffer if the original caller requested us to.
    if (g_pExtractInfo->szMappedPath) {
        strcpy(g_pExtractInfo->szMappedPath, szBuffer);
    }
#endif

    // If it is a directory, then display the "creating" status text.
    if ((pOut > szBuffer) && (lastchar(szBuffer, pOut-szBuffer) == '\\')) {
        if (created_dir) {
#ifdef UNICODE
            TCHAR szFile[_MAX_PATH];

            MBSTOTSTR(szFile, G.filename, countof(szFile));
#           define T_Fname  szFile
#else
#           define T_Fname  G.filename
#endif
            if (QCOND2) {
                Info(slide, 0, ((char *)slide, "   creating: %-22s\n",
                  FnFilter1(G.filename)));
            }

            // set file attributes:
            // The default for newly created directories is "DIR attribute
            // flags set", so there is no need to change attributes unless
            // one of the DOS style attribute flags is set. There is no need
            // to mask the readonly attribute, because it does not prevent
            // modifications in the new directory.
            if(G.pInfo->file_attr & (0x7F & ~FILE_ATTRIBUTE_DIRECTORY)) {
                if (!SetFileAttributes(T_Fname, G.pInfo->file_attr & 0x7F))
                    Info(slide, 1, ((char *)slide,
                      "\nwarning (%d): could not set file attributes for %s\n",
                      (int)GetLastError(), FnFilter1(G.filename)));
            }

            /* set dir time (note trailing '/') */
            return (error & ~MPN_MASK) | MPN_CREATED_DIR;
        }
        /* dir existed already; don't look for data to extract */
        return (error & ~MPN_MASK) | MPN_INF_SKIP;
    }

    return error;
}

//******************************************************************************
// Called from PROCESS.C
int checkdir(__GPRO__ char *pathcomp, int flag) {
#ifdef POCKET_UNZIP

    // This function is only called by free_G_buffers() from PROCESS.C with the
    // flag set to END.  We have nothing to do, so we just return success.
    return MPN_OK;

#else // !POCKET_UNZIP

#   define FN_MASK 7
#   define FUNCTION (flag & FN_MASK)
    int rc = MPN_OK;

    switch (FUNCTION) {
    case ROOT:
      {
        // User specified a root path. save the root without separator
        char* pathcompStart;

        if (pathcomp == NULL) {
            G.rootlen = 0;      // trivial NULL clause...
            break;
        }
        if (G.rootlen > 0)
            break;              // nothing to do, rootpath was already set

        G.rootlen = strlen(pathcomp);
        pathcompStart = pathcomp;
        // Strip the drive if given. CE does not support Drive
        if (pathcomp[1] == ':') {
            G.rootlen -= 2;
            pathcompStart += 2;
        }
        // Check for trailing separator, Strip if given
        // accomodate it if required.
        if (pathcomp[G.rootlen - 1] == '/' || pathcomp[G.rootlen - 1] == '\\')
            G.rootlen--;
        // Save the root
        memcpy(G.rootpath, pathcompStart, G.rootlen);
        G.rootpath[G.rootlen] = '\0';

        // Check if directory exists and try to create when neccessary.
        if (!SmartCreateDirectory(__G__ G.rootpath, NULL)) {
            rc = MPN_ERR_SKIP; // Create directory failed
        }

        // Add trailing path separator
        G.rootpath[G.rootlen++] = '\\';
        G.rootpath[G.rootlen] = '\0';
        break;
     }

    case END:
        Trace((stderr, "freeing rootpath\n"));
        if (G.rootlen > 0) {
            G.rootlen = 0;
            G.rootpath[0] = '\0';
        }
        break;

    default:
        rc = MPN_INVALID;       /* should never reach */
        break;
    }
    return rc;

#endif // !POCKET_UNZIP
} /* end function checkdir() */

#ifdef POCKET_UNZIP
//******************************************************************************
// Called from EXTRACT.C and LIST.C
int match(ZCONST char *string, ZCONST char *pattern, int ignore_case __WDLPRO)
{
   // match() for the other ports compares a file in the Zip file with some
   // command line file pattern.  In our case, we always pass in exact matches,
   // so we can simply do a string compare to see if we have a match.
   return (strcmp(string, pattern) == 0);
}

//******************************************************************************
// Called from PROCESS.C
int iswild(ZCONST char *pattern) {
   // Our file patterns never contain wild characters.  They are always exact
   // matches of file names in our Zip file.
   return FALSE;
}

#else // !POCKET_UNZIP

/************************/
/*  Function version()  */
/************************/

void version(__GPRO)
{
    // Dummy function, does nothing.
}

#ifndef WINDLL
/* Console input not supported on CE so just return -1 */
int getch_win32(void)
{
  return -1;
}
#endif /* !WINDLL */
#endif // !POCKET_UNZIP

#if (defined(UNICODE_SUPPORT))
/* convert wide character string to multi-byte character string */
char *wide_to_local_string(ZCONST zwchar *wide_string,
                           int escape_all)
{
  int i;
  wchar_t wc;
  int bytes_char;
  int default_used;
  int wsize = 0;
  int max_bytes = 9;
  char buf[9];
  char *buffer = NULL;
  char *local_string = NULL;

  for (wsize = 0; wide_string[wsize]; wsize++) ;

  if (max_bytes < MB_CUR_MAX)
    max_bytes = MB_CUR_MAX;

  if ((buffer = (char *)malloc(wsize * max_bytes + 1)) == NULL) {
    return NULL;
  }

  /* convert it */
  buffer[0] = '\0';
  for (i = 0; i < wsize; i++) {
    if (sizeof(wchar_t) < 4 && wide_string[i] > 0xFFFF) {
      /* wchar_t probably 2 bytes */
      /* could do surrogates if state_dependent and wctomb can do */
      wc = zwchar_to_wchar_t_default_char;
    } else {
      wc = (wchar_t)wide_string[i];
    }
    /* The C-RTL under WinCE does not support the generic C-style
     * Wide-to-MultiByte conversion functions (like wctomb() et. al.).
     * Therefore, we have to fall back to the underlying WinCE-API call to
     * get WCHAR-to-ANSI translation done.
     */
    bytes_char = WideCharToMultiByte(
                          CP_ACP, WC_COMPOSITECHECK,
                          &wc, 1,
                          (LPSTR)buf, sizeof(buf),
                          NULL, &default_used);
    if (default_used)
      bytes_char = -1;
    if (escape_all) {
      if (bytes_char == 1 && (uch)buf[0] <= 0x7f) {
        /* ASCII */
        strncat(buffer, buf, 1);
      } else {
        /* use escape for wide character */
        char *escape_string = wide_to_escape_string(wide_string[i]);
        strcat(buffer, escape_string);
        free(escape_string);
      }
    } else if (bytes_char > 0) {
      /* multi-byte char */
      strncat(buffer, buf, bytes_char);
    } else {
      /* no MB for this wide */
      /* use escape for wide character */
      char *escape_string = wide_to_escape_string(wide_string[i]);
      strcat(buffer, escape_string);
      free(escape_string);
    }
  }
  if ((local_string = (char *)realloc(buffer, strlen(buffer) + 1)) == NULL) {
    free(buffer);
    return NULL;
  }

  return local_string;
}
#endif /* UNICODE_SUPPORT */
