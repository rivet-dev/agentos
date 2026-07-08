/*
  Copyright (c) 1990-2003 Info-ZIP.  All rights reserved.

  See the accompanying file LICENSE, version 2000-Apr-09 or later
  (the contents of which are also included in unzip.h) for terms of use.
  If, for some reason, all these files are missing, the Info-ZIP license
  also may be found at:  ftp://ftp.info-zip.org/pub/infozip/license.html
*/
//******************************************************************************
//
// File:        WINMAIN.CPP
//
// Description: This module contains all the Windows specific code for Pocket
//              UnZip.  It contains the entire user interface.  This code knows
//              almost nothing about the Info-ZIP code.  All Info-ZIP related
//              functions are wrapped by helper functions in INTRFACE.CPP.  The
//              code in this module only calls those wrapper functions and
//              INTRFACE.CPP handles all the details and callbacks of the
//              Info-ZIP code.
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
// Functions:   WinMain
//              InitializeApplication
//              ShutdownApplication
//              RegisterUnzip
//              BuildImageList
//              WndProc
//              OnCreate
//              OnFileOpen
//              OnActionView
//              OnActionSelectAll
//              OnViewExpandedView
//              OnHelp
//              OnGetDispInfo
//              OnDeleteItem
//              OnItemChanged
//              Sort
//              CompareFunc
//              SetCaptionText
//              DrawBanner
//              AddDeleteColumns
//              ResizeColumns
//              GetZipErrorString
//              AddFileToListView
//              EnableAllMenuItems
//              CheckAllMenuItems
//              CenterWindow
//              AddTextToEdit
//              FormatValue
//              BuildAttributesString
//              BuildTypeString
//              GetFileFromPath
//              ForwardSlashesToBackSlashesA
//              ForwardSlashesToBackSlashesW
//              DeleteDirectory(LPTSTR szPath);
//              RegWriteKey
//              RegReadKey
//              WriteOptionString
//              WriteOptionInt
//              GetOptionString
//              GetOptionInt
//              DisableEditing
//              EditSubclassProc
//              GetMenuString
//              InitializeMRU
//              AddFileToMRU
//              RemoveFileFromMRU
//              ActivateMRU
//              ReadZipFileList
//              DlgProcProperties
//              MergeValues
//              CheckThreeStateBox
//              ExtractOrTestFiles
//              DlgProcExtractOrTest
//              FolderBrowser
//              DlgProcBrowser
//              SubclassSaveAsDlg
//              DlgProcExtractProgress
//              DlgProcViewProgress
//              UpdateProgress
//              PromptToReplace
//              DlgProcReplace
//              DlgProcPassword
//              DlgProcViewAssociation
//              DlgProcComment
//              DlgProcAbout
//
//
// Date      Name          History
// --------  ------------  -----------------------------------------------------
// 02/01/97  Steve Miller  Created (Version 1.0 using Info-ZIP UnZip 5.30)
//
//******************************************************************************

extern "C" {
#define __WINMAIN_CPP__
#define UNZIP_INTERNAL

#include "unzip.h"

#include "crypt.h"     // Needed to pick up CRYPT define setting and return values.

#include "unzvers.h"   // Only needed by consts.h (VERSION_DATE & VersionDate)
#include "consts.h"    // Only include once - defines constant string messages.

#include <commctrl.h>  // Common controls - mainly ListView and ImageList
#include <commdlg.h>   // Common dialogs - OpenFile dialog

#ifndef _WIN32_WCE
#include <shlobj.h>    // On NT, we use the SHBrowseForFolder() stuff.
#include <shellapi.h>  // CommandLineToArgvW() and ExtractIconEx()
#endif

#include "intrface.h"  // Interface between Info-ZIP and us
#include "winmain.h"   // Us
}
#include <tchar.h>     // Must be outside of extern "C" block


//******************************************************************************
//***** "Local" Global Variables
//******************************************************************************

static LPCTSTR         g_szAppName     = TEXT("Pocket UnZip");
static LPCTSTR         g_szClass       = TEXT("PocketUnZip");
static LPCTSTR         g_szRegKey      = TEXT("Software\\Pocket UnZip");
static LPCTSTR         g_szTempDir     = NULL;
static HWND            g_hWndList      = NULL;
static HWND            g_hWndCmdBar    = NULL;
static int             g_cyCmdBar      = 0;
static HFONT           g_hFontBanner   = NULL;
static HICON           g_hIconMain     = NULL;
static WNDPROC         g_wpSaveAsDlg   = NULL;
static WNDPROC         g_wpEdit        = NULL;
static int             g_sortColumn    = -1;
static BOOL            g_fExpandedView = FALSE;
static BOOL            g_fLoading      = FALSE;
static BOOL            g_fSkipped      = FALSE;
static BOOL            g_fViewing      = FALSE;
static HWND            g_hWndWaitFor   = NULL;
static FILE_TYPE_NODE *g_pftHead       = NULL;

#ifdef _WIN32_WCE
static LPCTSTR         g_szHelpFile    = TEXT("\\windows\\punzip.htp");
#else
static TCHAR           g_szTempDirPath[_MAX_PATH];
static LPCTSTR         g_szHelpFile    = TEXT("punzip.html");
#endif

static COLUMN g_columns[] = {
   { TEXT("Name"),       LVCFMT_LEFT  },
   { TEXT("Size"),       LVCFMT_RIGHT },
   { TEXT("Type"),       LVCFMT_LEFT  },
   { TEXT("Modified"),   LVCFMT_LEFT  },
   { TEXT("Attributes"), LVCFMT_LEFT  },
   { TEXT("Compressed"), LVCFMT_RIGHT },
   { TEXT("Ratio"),      LVCFMT_RIGHT },
   { TEXT("Method"),     LVCFMT_LEFT  },
   { TEXT("CRC"),        LVCFMT_LEFT  },
   { TEXT("Comment"),    LVCFMT_LEFT  }
};


//******************************************************************************
//***** Local Function Prototypes
//******************************************************************************

// Startup and Shutdown Functions
void InitializeApplication(LPCTSTR szZipFile);
void ShutdownApplication();
void RegisterUnzip();
void BuildImageList();

// Our Main Window's Message Handler
LRESULT CALLBACK WndProc(HWND hWnd, UINT uMsg, WPARAM wParam, LPARAM lParam);

// Event Handlers for our Main Window
int OnCreate();
void OnFileOpen();
void OnActionView();
void OnActionSelectAll();
void OnViewExpandedView();
void OnHelp();

// Event Handlers for our List View
void OnGetDispInfo(LV_DISPINFO *plvdi);
void OnDeleteItem(NM_LISTVIEW *pnmlv);
void OnItemChanged(NM_LISTVIEW *pnmlv);

// List View Sort Functions
void Sort(int sortColumn, BOOL fForce);
int CALLBACK CompareFunc(LPARAM lParam1, LPARAM lParam2, LPARAM sortColumn);

// Helper/Utility Functions
void SetCaptionText(LPCTSTR szPrefix);
void DrawBanner(HDC hdc);
void AddDeleteColumns();
void ResizeColumns();
LPCTSTR GetZipErrorString(int error);
void AddFileToListView(FILE_NODE *pFile);
void EnableAllMenuItems(UINT uMenuItem, BOOL fEnabled);
void CheckAllMenuItems(UINT uMenuItem, BOOL fChecked);
void CenterWindow(HWND hWnd);
void AddTextToEdit(LPCSTR szText);
LPTSTR FormatValue(LPTSTR szValue, zusz_t uzValue);
LPTSTR BuildAttributesString(LPTSTR szBuffer, DWORD dwAttributes);
LPCSTR BuildTypeString(FILE_NODE *pFile, LPSTR szType);
LPCSTR GetFileFromPath(LPCSTR szPath);
void ForwardSlashesToBackSlashesA(LPSTR szBuffer);
#ifdef UNICODE
   void ForwardSlashesToBackSlashesW(LPWSTR szBuffer);
#  define ForwardSlashesToBackSlashes ForwardSlashesToBackSlashesW
#else
#  define ForwardSlashesToBackSlashes ForwardSlashesToBackSlashesA
#endif
void DeleteDirectory(LPTSTR szPath);

// Registry Functions
void RegWriteKey(HKEY hKeyRoot, LPCTSTR szSubKey, LPCTSTR szValue);
BOOL RegReadKey(HKEY hKeyRoot, LPCTSTR szSubKey, LPTSTR szValue, DWORD cBytes);
void WriteOptionString(LPCTSTR szOption, LPCTSTR szValue);
void WriteOptionInt(LPCTSTR szOption, DWORD dwValue);
LPTSTR GetOptionString(LPCTSTR szOption, LPCTSTR szDefault, LPTSTR szValue, DWORD nSize);
DWORD GetOptionInt(LPCTSTR szOption, DWORD dwDefault);

// EDIT Control Subclass Functions
void DisableEditing(HWND hWndEdit);
LRESULT CALLBACK EditSubclassProc(HWND hWnd, UINT uMsg, WPARAM wParam, LPARAM lParam);

// MRU Functions
void InitializeMRU();
void AddFileToMRU(LPCSTR szFile);
void RemoveFileFromMRU(LPCTSTR szFile);
void ActivateMRU(UINT uIDItem);

// Open Zip File Functions
void ReadZipFileList(LPCTSTR wszPath);

// Zip File Properties Dialog Functions
BOOL CALLBACK DlgProcProperties(HWND hDlg, UINT uMsg, WPARAM wParam, LPARAM lParam);
void MergeValues(int *p1, int p2);
void CheckThreeStateBox(HWND hDlg, int nIDButton, int state);

// Extract/Test Dialog Functions
void ExtractOrTestFiles(BOOL fExtract);
BOOL CALLBACK DlgProcExtractOrTest(HWND hDlg, UINT uMsg, WPARAM wParam, LPARAM lParam);

// Folder Browsing Dialog Functions
BOOL FolderBrowser(LPTSTR szPath, DWORD dwLength);
BOOL CALLBACK DlgProcBrowser(HWND hDlg, UINT uMsg, WPARAM wParam, LPARAM lParam);
void SubclassSaveAsDlg();

// Extraction/Test/View Progress Dialog Functions
BOOL CALLBACK DlgProcExtractProgress(HWND hDlg, UINT uMsg, WPARAM wParam, LPARAM lParam);
BOOL CALLBACK DlgProcViewProgress(HWND hDlg, UINT uMsg, WPARAM wParam, LPARAM lParam);
void UpdateProgress(EXTRACT_INFO *pei, BOOL fFull);

// Replace File Dialog Functions
int PromptToReplace(LPCSTR szPath);
BOOL CALLBACK DlgProcReplace(HWND hDlg, UINT uMsg, WPARAM wParam, LPARAM lParam);

// Password Dialog Functions
BOOL CALLBACK DlgProcPassword(HWND hDlg, UINT uMsg, WPARAM wParam, LPARAM lParam);

// View Association Dialog Functions
BOOL CALLBACK DlgProcViewAssociation(HWND hDlg, UINT uMsg, WPARAM wParam, LPARAM lParam);

// Comment Dialog Functions
BOOL CALLBACK DlgProcComment(HWND hDlg, UINT uMsg, WPARAM wParam, LPARAM lParam);

// About Dialog Functions
BOOL CALLBACK DlgProcAbout(HWND hDlg, UINT uMsg, WPARAM wParam, LPARAM lParam);


//******************************************************************************
//***** WinMain - Our one and only entry point
//******************************************************************************

// Entrypoint is a tiny bit different on Windows CE - UNICODE command line.
#ifdef _WIN32_WCE
extern "C" int WINAPI WinMain(HINSTANCE hInstance, HINSTANCE hPrevInstance,
                              LPTSTR lpCmdLine, int nCmdShow)
#else
extern "C" int WINAPI WinMain(HINSTANCE hInstance, HINSTANCE hPrevInstance,
                              LPSTR lpCmdLine, int nCmdShow)
#endif
{
   // Wrap the whole ball of wax in a big exception handler.
   __try {

      // Store global instance handle.
      g_hInst = hInstance;

      // Create our banner font.  We need to do this before creating our window.
      // This font handle will be deleted in ShutdownApplication().
      LOGFONT lf;
      ZeroMemory(&lf, sizeof(lf));
      lf.lfHeight = 16;
      lf.lfWeight = FW_BOLD;
      lf.lfCharSet = ANSI_CHARSET;
      lf.lfOutPrecision = OUT_DEFAULT_PRECIS;
      lf.lfClipPrecision = CLIP_DEFAULT_PRECIS;
      lf.lfQuality = DEFAULT_QUALITY;
      lf.lfPitchAndFamily = DEFAULT_PITCH | FF_SWISS;
      _tcscpy(lf.lfFaceName, TEXT("MS Sans Serif"));
      g_hFontBanner = CreateFontIndirect(&lf);

      // Define the window class for our application's main window.
      WNDCLASS wc;
      ZeroMemory(&wc, sizeof(wc));
      wc.lpszClassName = g_szClass;
      wc.hInstance     = hInstance;
      wc.lpfnWndProc   = WndProc;
      wc.hbrBackground = (HBRUSH)GetStockObject(WHITE_BRUSH);

      TCHAR *szZipPath = NULL;

#ifdef _WIN32_WCE

      // Get our main window's small icon.  On Windows CE, we need to send ourself
      // a WM_SETICON in order for our task bar to update itself.
      g_hIconMain = (HICON)LoadImage(hInstance, MAKEINTRESOURCE(IDI_UNZIP),
                                     IMAGE_ICON, 16, 16, LR_DEFAULTCOLOR);
      wc.hIcon = g_hIconMain;

      // On Windows CE, we only need the WS_VISIBLE flag.
      DWORD dwStyle = WS_VISIBLE;

      // Get and store command line file (if any).
      if (lpCmdLine && *lpCmdLine) {
         szZipPath = lpCmdLine;
      }

#else

      // On NT we add a cursor, icon, and menu to our application's window class.
      wc.hCursor      = LoadCursor(NULL, IDC_ARROW);
      wc.hIcon        = LoadIcon(hInstance, MAKEINTRESOURCE(IDI_UNZIP));
      wc.lpszMenuName = MAKEINTRESOURCE(IDR_UNZIP);

      // On Windows NT, we use the standard overlapped window style.
      DWORD dwStyle = WS_OVERLAPPEDWINDOW;

      TCHAR szBuffer[_MAX_PATH];

      // Get and store command line file (if any).
      if (lpCmdLine && *lpCmdLine) {
         MBSTOTSTR(szBuffer, lpCmdLine, countof(szBuffer));
         szZipPath = szBuffer;
      }

#endif

      // Register our window class with the OS.
      if (!RegisterClass(&wc)) {
         DebugOut(TEXT("RegisterClass() failed [%u]"), GetLastError());
      }

      // Create our main window using our registered window class.
      g_hWndMain = CreateWindow(wc.lpszClassName, g_szAppName, dwStyle,
                                CW_USEDEFAULT, CW_USEDEFAULT,
                                CW_USEDEFAULT, CW_USEDEFAULT,
                                NULL, NULL, hInstance, NULL);

      // Quit now if we failed to create our main window.
      if (!g_hWndMain) {
         DebugOut(TEXT("CreateWindow() failed [%u]"), GetLastError());
         ShutdownApplication();
         return 0;
      }

      // Make sure our window is visible.  Really only needed for NT.
      ShowWindow(g_hWndMain, nCmdShow);

      // Load our keyboard accelerator shortcuts.
      MSG    msg;
      HACCEL hAccel = LoadAccelerators(g_hInst, MAKEINTRESOURCE(IDR_UNZIP));
      DWORD  dwPaintFlags = 0;

      // The message pump.  Loop until we get a WM_QUIT message.
      while (GetMessage(&msg, NULL, 0, 0)) {

         // Check to see if this is an accelerator and handle it if neccessary.
         if (!TranslateAccelerator(g_hWndMain, hAccel, &msg)) {

            // If a normal message, then dispatch it to the correct window.
            TranslateMessage(&msg);
            DispatchMessage(&msg);

            // Wait until our application is up and visible before trying to
            // initialize some of our structures and load any command line file.
            if ((msg.message == WM_PAINT) && (dwPaintFlags != 0x11)) {
               if (msg.hwnd == g_hWndWaitFor) {
                  dwPaintFlags |= 0x01;
               } else if (msg.hwnd == g_hWndList) {
                  dwPaintFlags |= 0x10;
               }
               if (dwPaintFlags == 0x11) {
                  InitializeApplication((szZipPath && *szZipPath) ?
                                        szZipPath : NULL);
               }
            }
         }
      }

      // Clean up code.
      ShutdownApplication();

      // Nice clean finish - were out of here.
      return msg.wParam;


   } __except(EXCEPTION_EXECUTE_HANDLER) {

      // Something very bad happened.  Try our best to appear somewhat graceful.
      MessageBox(NULL,
         TEXT("An internal error occurred.  Possible causes are that you are ")
         TEXT("out of memory, a ZIP file (if one is loaded) contains an ")
         TEXT("unexpected error, or there is a bug in our program (that's why ")
         TEXT("it's free).  Pocket UnZip cannot continue.  It will exit now, ")
         TEXT("but you may restart it and try again.\n\n")
         TEXT("If the problem persists, please write to stevemil@pobox.com with ")
         TEXT("any information that might help track down the problem."),
         g_szAppName, MB_ICONERROR | MB_OK);
   }

   return 1;
}


//******************************************************************************
//***** Startup and Shutdown Functions
//******************************************************************************

void InitializeApplication(LPCTSTR szZipFile) {

   // This function is called after our class is registered and all our windows
   // are created and visible to the user.

   // Show hour glass cursor.
   HCURSOR hCur = SetCursor(LoadCursor(NULL, IDC_WAIT));

   // Register UnZip in the registry to handle ".ZIP" files.
   RegisterUnzip();

   // Enumerate the system file assoications and build an image list.
   BuildImageList();

   // Load our initial MRU into our menu.
   InitializeMRU();

   // Restore/remove our cursor.
   SetCursor(hCur);

   // Clear our initialization window handle.
   g_hWndWaitFor = NULL;

   // Load our command line file if one was specified. Otherwise, just update
   // our banner to show that no file is loaded.
   if (szZipFile) {
      ReadZipFileList(szZipFile);
   } else {
      DrawBanner(NULL);
   }

   // Enable some controls.
   EnableAllMenuItems(IDM_FILE_OPEN,          TRUE);
   EnableAllMenuItems(IDM_FILE_CLOSE,         TRUE);
   EnableAllMenuItems(IDM_VIEW_EXPANDED_VIEW, TRUE);
   EnableAllMenuItems(IDM_HELP_ABOUT,         TRUE);

   // Set our temporary directory.
#ifdef _WIN32_WCE
   g_szTempDir = TEXT("\\Temporary Pocket UnZip Files");
#else
   g_szTempDir = TEXT("C:\\Temporary Pocket UnZip Files");

   // Set the drive to be the same drive as the OS installation is on.
   if (GetWindowsDirectory(g_szTempDirPath, countof(g_szTempDirPath))) {
      lstrcpy(g_szTempDirPath + 3, TEXT("Temporary Pocket UnZip Files"));
      g_szTempDir  = g_szTempDirPath;
   }
#endif
}

//******************************************************************************
void ShutdownApplication() {

   // Free our banner font.
   if (g_hFontBanner) {
      DeleteObject(g_hFontBanner);
      g_hFontBanner = NULL;
   }

   // Delete our FILE_TYPE_NODE linked list.
   for (FILE_TYPE_NODE *pft = g_pftHead; pft; ) {
      FILE_TYPE_NODE *pftNext = pft->pNext;
      delete[] (BYTE*)pft;
      pft = pftNext;
   }
   g_pftHead = NULL;

   // If there are no other instances of our application open, then delete our
   // temporary directory and all the files in it.  Any files opened for viewing
   // should be locked and will fail to delete.  This is to be expected.
   if (g_szTempDir && (FindWindow(g_szClass, NULL) == NULL)) {
      TCHAR szPath[_MAX_PATH];
      _tcscpy(szPath, g_szTempDir);
      DeleteDirectory(szPath);
   }
}

//******************************************************************************
void RegisterUnzip() {

#ifdef _WIN32_WCE

   // WARNING!  Since Windows CE does not support any way to get your binary's
   // name at runtime, we have to hard-code in "punzip.exe".  If our binary is
   // not named this or is in a non-path directory, then we will fail to
   // register ourself with the system as the default application to handle
   // ".zip" files.
   TCHAR szPath[32] = TEXT("punzip.exe");
   TCHAR szTstPath[32];

#else

   // Get our module's path and file name.  We use the short path name for the
   // registry because it is guaranteed to contain no spaces.
   TCHAR szLongPath[_MAX_PATH];
   TCHAR szPath[_MAX_PATH];
   TCHAR szTstPath[_MAX_PATH];
   GetModuleFileName(NULL, szLongPath, countof(szLongPath));
   GetShortPathName(szLongPath, szPath, countof(szPath));

#endif

   // Store a pointer to the end of our path for easy appending.
   LPTSTR szEnd = szPath + _tcslen(szPath);

   BOOL fDoRegisterPUnZip = TRUE;

   // Associate "ZIP" file extensions to our application
   if (RegReadKey(HKEY_CLASSES_ROOT, TEXT(".zip"), szTstPath, sizeof(szTstPath)))
   {
      if (_tcscmp(szTstPath, TEXT("zipfile")) != 0)
         fDoRegisterPUnZip = FALSE;
      else if (RegReadKey(HKEY_CLASSES_ROOT, TEXT("zipfile\\shell\\Open\\command"),
                          szTstPath, sizeof(szTstPath)) &&
               (_tcsncmp(szTstPath, szPath, _tcslen(szPath)) != 0))
         fDoRegisterPUnZip = FALSE;

      if (!fDoRegisterPUnZip)
      {
         fDoRegisterPUnZip =
            (IDOK == MessageBox(g_hWndMain,
                                TEXT("Currently, Pocket UnZip is not registered as default ")
                                TEXT("handler for Zip archives.\n\n")
                                TEXT("Please, confirm that Pocket UnZip should now register itself ")
                                TEXT("as default application for handling Zip archives (.zip files)"),
                                g_szAppName,
                                MB_ICONQUESTION | MB_OKCANCEL));
      }
   }
   if (fDoRegisterPUnZip) {
      RegWriteKey(HKEY_CLASSES_ROOT, TEXT(".zip"), TEXT("zipfile"));
      RegWriteKey(HKEY_CLASSES_ROOT, TEXT("zipfile"), TEXT("ZIP File"));
      RegWriteKey(HKEY_CLASSES_ROOT, TEXT("zipfile\\shell"), NULL);
      RegWriteKey(HKEY_CLASSES_ROOT, TEXT("zipfile\\shell\\Open"), NULL);
      _tcscpy(szEnd, TEXT(" %1"));
      RegWriteKey(HKEY_CLASSES_ROOT, TEXT("zipfile\\shell\\Open\\command"), szPath);

      // Register our program icon for all ZIP files.
      _stprintf(szEnd, TEXT(",-%u"), IDI_ZIPFILE);
      RegWriteKey(HKEY_CLASSES_ROOT, TEXT("zipfile\\DefaultIcon"), szPath);
   }

   // Create our application option location.
   RegWriteKey(HKEY_CURRENT_USER, TEXT("Software"), NULL);
   RegWriteKey(HKEY_CURRENT_USER, g_szRegKey, NULL);
}

//******************************************************************************
void BuildImageList() {

   // Create our global image list.
#ifdef _WIN32_WCE

   // On Windows CE, we can't spare a color for the mask, so we have to create
   // the mask in a separate monochrome bitmap.

   HIMAGELIST hil = ImageList_Create(16, 16, ILC_COLOR | ILC_MASK, 8, 8);

   // Load our default bitmaps into the image list.
   HBITMAP hBmpImageList = LoadBitmap(g_hInst, MAKEINTRESOURCE(IDB_IMAGELIST));
   HBITMAP hBmpMask = LoadBitmap(g_hInst, MAKEINTRESOURCE(IDB_IMAGELIST_MASK));
   ImageList_Add(hil, hBmpImageList, hBmpMask);
   DeleteObject(hBmpImageList);
   DeleteObject(hBmpMask);

#else

   // On Windows NT, we use magenta as a transparency mask color.
   HIMAGELIST hil = ImageList_LoadBitmap(g_hInst, MAKEINTRESOURCE(IDB_IMAGELIST),
                                         16, 8, RGB(255, 0, 255));
#endif

   // Set up for our registry file type enumeration.
   FILE_TYPE_NODE *pftLast = NULL;
   TCHAR szExtension[128], szKey[128], szDescription[_MAX_PATH], szIconFile[_MAX_PATH + 16];
   DWORD dwIndex = 0, dwCount = countof(szExtension);

   // Enumerate all the keys immediately under HKEY_CLASSES_ROOT.
   while (ERROR_SUCCESS == RegEnumKeyEx(HKEY_CLASSES_ROOT, dwIndex++, szExtension,
                                        &dwCount, NULL, NULL, NULL, NULL))
   {
      dwCount = countof(szExtension);

      // Check to see if we read an extension key (starts with a period)
      if (*szExtension != TEXT('.')) {
         continue;
      }

      // Read the actual key name for this extension.
      if (!RegReadKey(HKEY_CLASSES_ROOT, szExtension, szKey, sizeof(szKey))) {
         continue;
      }

      // Read the Description for this extension.
      RegReadKey(HKEY_CLASSES_ROOT, szKey, szDescription, sizeof(szDescription));

      HICON hIcon = NULL;
      LPTSTR szEnd = szKey + _tcslen(szKey);

      // Attempt to get an icon for this extension from the "DefaultIcon" key.
      _tcscpy(szEnd, TEXT("\\DefaultIcon"));
      if (RegReadKey(HKEY_CLASSES_ROOT, szKey, szIconFile, sizeof(szIconFile))) {

         // Look for the comma between the file name and the image.
         LPTSTR szImageId = _tcschr(szIconFile, TEXT(','));
         if (szImageId) {

            // NULL terminate the file name portion of szIconFile.
            *(szImageId++) = TEXT('\0');

            // Get the image ID value from szIconFile.
            int imageId = _ttoi(szImageId);

            // Extract the icon from the module specified in szIconFile.
            ExtractIconEx(szIconFile, imageId, NULL, &hIcon, 1);
            if (hIcon == NULL) {
               ExtractIconEx(szIconFile, imageId, &hIcon, NULL, 1);
            }
         }
      }

      // If we failed to get the icon using the "DefaultIcon" key, then try
      // using the "shell\Open\command" key.
      if (hIcon == NULL) {

         _tcscpy(szEnd, TEXT("\\shell\\Open\\command"));
         if (RegReadKey(HKEY_CLASSES_ROOT, szKey, szIconFile, sizeof(szIconFile))) {

            // Get a pointer to just the binary - strip quotes and spaces.
            LPTSTR szPath;
            if (*szIconFile == TEXT('\"')) {
               szPath = szIconFile + 1;
               if (szEnd = _tcschr(szPath, TEXT('\"'))) {
                  *szEnd = TEXT('\0');
               }
            } else {
               szPath = szIconFile;
               if (szEnd = _tcschr(szPath, TEXT(' '))) {
                  *szEnd = TEXT('\0');
               }
            }

            // Extract the icon from the module specified in szIconFile.
            ExtractIconEx(szPath, 0, NULL, &hIcon, 1);
            if (hIcon == NULL) {
               ExtractIconEx(szPath, 0, &hIcon, NULL, 1);
            }
         }
      }

      // If we found an icon, add it to our image list.
      int image = -1;
      if (hIcon) {
         image = ImageList_AddIcon(hil, hIcon);
      }

      // If no icon could be found, then check to see if this is an executable.
      if ((image == -1) && (
#ifndef _WIN32_WCE // Windows CE only recognizes EXE's as executable.
         !_tcsicmp(szExtension + 1, TEXT("bat")) ||
         !_tcsicmp(szExtension + 1, TEXT("cmd")) ||
         !_tcsicmp(szExtension + 1, TEXT("com")) ||
#endif
         !_tcsicmp(szExtension + 1, TEXT("exe"))))
      {
         image = IMAGE_APPLICATION;
      }

      // If we don't have a description or a icon, then bail on this extension.
      if (!*szDescription && (image < 0)) {
         continue;
      }

      // Create our FILE_TYPE_NODE.
      size_t length = _tcslen(szExtension) - 1 + _tcslen(szDescription);
      FILE_TYPE_NODE *pft = (FILE_TYPE_NODE*) new BYTE[
         sizeof(FILE_TYPE_NODE) + (sizeof(TCHAR) * length)];

      // Bail out if we could not create our node.
      if (!pft) {
         DebugOut(TEXT("Not enough memory to create a FILE_TYPE_NODE."));
         continue;
      }

      // Fill in the node.
      pft->pNext = NULL;
      pft->image = (image >= 0) ? image : IMAGE_GENERIC;
      TSTRTOMBS(pft->szExtAndDesc, szExtension + 1, length + 2);
      size_t sizext = (strlen(pft->szExtAndDesc) + 1);
      TSTRTOMBS(pft->szExtAndDesc + sizext,
                szDescription, length - sizext + 2);

      // Add the node to our list.
      if (pftLast) {
         pftLast->pNext = pft;
      } else {
         g_pftHead = pft;
      }
      pftLast = pft;
   }

   // Assign this image list to our tree control.
   ListView_SetImageList(g_hWndList, hil, LVSIL_SMALL);
}


//******************************************************************************
//***** Our Main Window's Message Handler
//******************************************************************************

LRESULT CALLBACK WndProc(HWND hWnd, UINT uMsg, WPARAM wParam, LPARAM lParam) {

   switch(uMsg) {
      case WM_CREATE:
         g_hWndMain = hWnd;
         return OnCreate();

      case WM_ERASEBKGND:
         DrawBanner((HDC)wParam);
         return 0;

      case WM_SIZE:
         // Resize our list view control to match our client area.
         MoveWindow(g_hWndList, 0, g_cyCmdBar + 22, LOWORD(lParam),
                    HIWORD(lParam) - (g_cyCmdBar + 22), TRUE);

#ifndef _WIN32_WCE
         // On NT we have to resize our toolbar as well.
         MoveWindow(g_hWndCmdBar, 0, 0, LOWORD(lParam), g_cyCmdBar, TRUE);
#endif
         return 0;

      case WM_SETFOCUS:
         // Always direct focus to our list control.
         SetFocus(g_hWndList);
         return 0;

      case WM_DESTROY:
         PostQuitMessage(0);
         return 0;

      case WM_HELP:
         OnHelp();
         return 0;

      case WM_PRIVATE:
         switch (wParam) {

#ifdef _WIN32_WCE
            case MSG_SUBCLASS_DIALOG:
               SubclassSaveAsDlg();
               return 0;
#endif
            case MSG_ADD_TEXT_TO_EDIT:
               AddTextToEdit((LPCSTR)lParam);
               return 0;

            case MSG_PROMPT_TO_REPLACE:
               return PromptToReplace((LPCSTR)lParam);

#if CRYPT
            case MSG_PROMPT_FOR_PASSWORD:
               return DialogBoxParam(g_hInst, MAKEINTRESOURCE(IDD_PASSWORD),
                                     g_hDlgProgress, (DLGPROC)DlgProcPassword,
                                     lParam);
#endif

            case MSG_UPDATE_PROGRESS_PARTIAL:
               UpdateProgress((EXTRACT_INFO*)lParam, FALSE);
               return 0;

            case MSG_UPDATE_PROGRESS_COMPLETE:
               UpdateProgress((EXTRACT_INFO*)lParam, TRUE);
               return 0;
         }
         return 0;

      case WM_NOTIFY:
         switch (((LPNMHDR)lParam)->code) {

            case LVN_GETDISPINFO:
               OnGetDispInfo((LV_DISPINFO*)lParam);
               return 0;

            case LVN_DELETEITEM:
               OnDeleteItem((NM_LISTVIEW*)lParam);
               return 0;

            case LVN_COLUMNCLICK:
               Sort(((NM_LISTVIEW*)lParam)->iSubItem, FALSE);
               return 0;

            case LVN_ITEMCHANGED:
               OnItemChanged((NM_LISTVIEW*)lParam);
               return 0;

            case NM_DBLCLK:
            case NM_RETURN:
               OnActionView();
               return 0;
         }

         return 0;

      case WM_COMMAND:
         switch (LOWORD(wParam)) {

            case IDM_FILE_OPEN:
               OnFileOpen();
               return 0;

            case IDM_FILE_PROPERTIES:
               DialogBox(g_hInst, MAKEINTRESOURCE(IDD_PROPERTIES), hWnd, (DLGPROC)DlgProcProperties);
               return 0;

            case IDM_FILE_CLOSE:
               SendMessage(hWnd, WM_CLOSE, 0, 0);
               return 0;

            case IDM_ACTION_EXTRACT_ALL:
               OnActionSelectAll();
               // Fall through to IDM_ACTION_EXTRACT

            case IDM_ACTION_EXTRACT:
               ExtractOrTestFiles(TRUE);
               return 0;

            case IDM_ACTION_TEST_ALL:
               OnActionSelectAll();
               // Fall through to IDM_ACTION_TEST

            case IDM_ACTION_TEST:
               ExtractOrTestFiles(FALSE);
               return 0;

            case IDM_ACTION_VIEW:
               OnActionView();
               return 0;

            case IDM_ACTION_SELECT_ALL:
               OnActionSelectAll();
               return 0;

            case IDM_VIEW_EXPANDED_VIEW:
               OnViewExpandedView();
               return 0;

            case IDM_VIEW_COMMENT:
               DialogBox(g_hInst, MAKEINTRESOURCE(IDD_COMMENT), hWnd, (DLGPROC)DlgProcComment);
               return 0;

            case IDM_HELP_ABOUT:
               DialogBox(g_hInst, MAKEINTRESOURCE(IDD_ABOUT), hWnd, (DLGPROC)DlgProcAbout);
               return 0;

            case IDHELP:
               return SendMessage(hWnd, WM_HELP, 0, 0);

            default:
               // Check to see if a MRU file was selected.
               if ((LOWORD(wParam) >= MRU_START_ID) &&
                   (LOWORD(wParam) < (MRU_START_ID + MRU_MAX_FILE)))
               {
                  ActivateMRU(LOWORD(wParam));
               }
         }
    }
    return DefWindowProc(hWnd, uMsg, wParam, lParam);
}

//******************************************************************************
//***** Event Handlers for our Main Window
//******************************************************************************

int OnCreate() {

   // Our toolbar buttons.
   static TBBUTTON tbButton[] = {
      { 0, 0,                      0, TBSTYLE_SEP,    0, 0, 0, -1 },
      { 0, IDM_FILE_OPEN,          0, TBSTYLE_BUTTON, 0, 0, 0, -1 },
      { 0, 0,                      0, TBSTYLE_SEP,    0, 0, 0, -1 },
      { 1, IDM_FILE_PROPERTIES,    0, TBSTYLE_BUTTON, 0, 0, 0, -1 },
      { 0, 0,                      0, TBSTYLE_SEP,    0, 0, 0, -1 },
      { 2, IDM_ACTION_EXTRACT,     0, TBSTYLE_BUTTON, 0, 0, 0, -1 },
      { 3, IDM_ACTION_EXTRACT_ALL, 0, TBSTYLE_BUTTON, 0, 0, 0, -1 },
      { 0, 0,                      0, TBSTYLE_SEP,    0, 0, 0, -1 },
      { 4, IDM_ACTION_TEST,        0, TBSTYLE_BUTTON, 0, 0, 0, -1 },
      { 5, IDM_ACTION_TEST_ALL,    0, TBSTYLE_BUTTON, 0, 0, 0, -1 },
      { 0, 0,                      0, TBSTYLE_SEP,    0, 0, 0, -1 },
      { 6, IDM_ACTION_VIEW,        0, TBSTYLE_BUTTON, 0, 0, 0, -1 },
      { 0, 0,                      0, TBSTYLE_SEP,    0, 0, 0, -1 },
      { 7, IDM_VIEW_EXPANDED_VIEW, 0, TBSTYLE_BUTTON, 0, 0, 0, -1 },
      { 8, IDM_VIEW_COMMENT,       0, TBSTYLE_BUTTON, 0, 0, 0, -1 }
   };

   // Our toolbar buttons' tool tip text.
   static LPTSTR szToolTips[] = {
       TEXT(""),  // Menu
       TEXT("Open (Ctrl+O)"),
       TEXT("Properties (Alt+Enter)"),
       TEXT("Extract Selected Files"),
       TEXT("Extract All Files"),
       TEXT("Test Selected Files"),
       TEXT("Test All Files"),
       TEXT("View Selected File"),
       TEXT("Expanded View"),
       TEXT("View Zip File Comment")
   };

   // Initialize the common controls.
   InitCommonControls();

   // Check to see if we have a help file.
   BOOL fHelp = (GetFileAttributes(g_szHelpFile) != 0xFFFFFFFF);

   // Set our window's icon so it can update the task bar.
   if (g_hIconMain) {
      SendMessage(g_hWndMain, WM_SETICON, FALSE, (LPARAM)g_hIconMain);
   }

   // Create the tree control.  Our main window will resize it to fit.
   g_hWndList = CreateWindow(WC_LISTVIEW, TEXT(""),
                             WS_VSCROLL | WS_CHILD | WS_VISIBLE |
                             LVS_REPORT | LVS_SHOWSELALWAYS,
                             0, 0, 0, 0, g_hWndMain, NULL, g_hInst, NULL);

#ifdef _WIN32_WCE

   // Create a command bar and add the toolbar bitmaps to it.
   g_hWndCmdBar = CommandBar_Create(g_hInst, g_hWndMain, 1);
   CommandBar_AddBitmap(g_hWndCmdBar, g_hInst, IDB_TOOLBAR, 9, 16, 16);
   CommandBar_InsertMenubar(g_hWndCmdBar, g_hInst, IDR_UNZIP, 0);
   CommandBar_AddButtons(g_hWndCmdBar, countof(tbButton), tbButton);
   CommandBar_AddAdornments(g_hWndCmdBar, fHelp ? CMDBAR_HELP : 0, 0);

   // Add tool tips to the tool bar.
   CommandBar_AddToolTips(g_hWndCmdBar, countof(szToolTips), szToolTips);

   // Store the height of the command bar for later calculations.
   g_cyCmdBar = CommandBar_Height(g_hWndCmdBar);

   // We set our wait window handle to our menu window within our command bar.
   // This is the last window that will be painted during startup of our app.
   g_hWndWaitFor = GetWindow(g_hWndCmdBar, GW_CHILD);

   // Add the help item to our help menu if we have a help file.
   if (fHelp) {
      HMENU hMenu = GetSubMenu(CommandBar_GetMenu(g_hWndCmdBar, 0), 3);
      InsertMenu(hMenu, 0, MF_BYPOSITION | MF_SEPARATOR, 0, NULL);
      InsertMenu(hMenu, 0, MF_BYPOSITION | MF_ENABLED, IDHELP, TEXT("&Help"));
   }

#else

   // Create a tool bar and add the toolbar bitmaps to it.
   g_hWndCmdBar = CreateToolbarEx(g_hWndMain, WS_CHILD | WS_VISIBLE | TBSTYLE_TOOLTIPS,
                                  1, 9, g_hInst, IDB_TOOLBAR, tbButton,
                                  countof(tbButton), 16, 16, 16, 16,
                                  sizeof(TBBUTTON));

   // Get our tool tip control.
   HWND hWndTT = (HWND)SendMessage(g_hWndCmdBar, TB_GETTOOLTIPS, 0, 0);

   // Set our tool tip strings.
   TOOLINFO ti;
   ti.cbSize = sizeof(ti);
   int tip = 0, button;
   while (SendMessage(hWndTT, TTM_ENUMTOOLS, tip++, (LPARAM)&ti)) {
      for (button = 0; button < countof(tbButton); button++) {
         if (tbButton[button].idCommand == (int)ti.uId) {
            ti.lpszText = szToolTips[tbButton[button].iBitmap + 1];
            SendMessage(hWndTT, TTM_UPDATETIPTEXT, 0, (LPARAM)&ti);
            break;
         }
      }
   }

   // Store the height of the tool bar for later calculations.
   RECT rc;
   GetWindowRect(g_hWndCmdBar, &rc);
   g_cyCmdBar = rc.bottom - rc.top;

   // We set our wait window handle to our toolbar.
   // This is the last window that will be painted during the startup of our app.
   g_hWndWaitFor = g_hWndCmdBar;

   // Add the help item to our help menu if we have a help file.
   if (fHelp) {
      HMENU hMenu = GetSubMenu(GetMenu(g_hWndMain), 3);
      InsertMenu(hMenu, 0, MF_BYPOSITION | MF_SEPARATOR, 0, NULL);
      InsertMenu(hMenu, 0, MF_BYPOSITION | MF_ENABLED, IDHELP, TEXT("&Help\tF1"));
   }

#endif // _WIN32_WCE

   // Enable Full Row Select - This feature is supported on Windows CE and was
   // introduced to Win95/NT with IE 3.0.  If the user does not have a
   // COMCTL32.DLL that supports this feature, then they will just see the
   // old standard First Column Select.
   SendMessage(g_hWndList, LVM_SETEXTENDEDLISTVIEWSTYLE, 0, LVS_EX_FULLROWSELECT |
               SendMessage(g_hWndList, LVM_GETEXTENDEDLISTVIEWSTYLE, 0, 0));

   // Get our expanded view option from the registry.
   g_fExpandedView = GetOptionInt(TEXT("ExpandedView"), FALSE);

   // Show or remove menu check for expanded view option.
   CheckAllMenuItems(IDM_VIEW_EXPANDED_VIEW, g_fExpandedView);

   // Create our columns.
   AddDeleteColumns();

   // Set our current sort column to our name column
   Sort(0, TRUE);

   return 0;
}

//******************************************************************************
void OnFileOpen() {

   TCHAR szPath[_MAX_PATH] = TEXT("");

   OPENFILENAME ofn;
   ZeroMemory(&ofn, sizeof(ofn));

   ofn.lStructSize  = sizeof(ofn);
   ofn.hwndOwner    = g_hWndMain;
   ofn.hInstance    = g_hInst;
   ofn.lpstrFilter  = TEXT("ZIP files (*.zip)\0*.zip\0SFX files (*.exe)\0*.exe\0All Files (*.*)\0*.*\0");
   ofn.nFilterIndex = 1;
   ofn.lpstrFile    = szPath;
   ofn.nMaxFile     = countof(szPath);
   ofn.Flags        = OFN_FILEMUSTEXIST | OFN_HIDEREADONLY | OFN_PATHMUSTEXIST;
   ofn.lpstrDefExt  = TEXT("zip");

   if (GetOpenFileName(&ofn)) {
      ReadZipFileList(szPath);
   }
}

//******************************************************************************
void OnActionView() {

   // We only allow a view if one item is selected.
   int count = ListView_GetSelectedCount(g_hWndList);
   if (count != 1) {
      return;
   }

   // Query the selected item for its FILE_NODE.
   LV_ITEM lvi;
   ZeroMemory(&lvi, sizeof(lvi));
   lvi.mask = LVIF_IMAGE | LVIF_PARAM;
   lvi.iItem = ListView_GetNextItem(g_hWndList, -1, LVNI_SELECTED);
   ListView_GetItem(g_hWndList, &lvi);
   FILE_NODE *pfn = (FILE_NODE*)lvi.lParam;

   // Bail out if the selected item is a folder or volume label.
   if (pfn->dwAttributes & (FILE_ATTRIBUTE_DIRECTORY | ZFILE_ATTRIBUTE_VOLUME)) {
      MessageBox(g_hWndMain, TEXT("You cannot view folders or volume labels."),
                 g_szAppName, MB_ICONINFORMATION | MB_OK);
      return;
   }

   // Make sure our temporary directory exists.
   CreateDirectory(g_szTempDir, NULL);

   TCHAR szPath[_MAX_PATH + 256];

   // Set our extraction directory to our temporary directory.
   if (!SetExtractToDirectory((LPTSTR)g_szTempDir)) {

      // Create error message.  Use szPath buffer because it is handy.
      _stprintf(szPath,
         TEXT("Could not create \"%s\"\n\n")
         TEXT("Most likely cause is that your drive is full."),
         g_szTempDir);

      // Display error message.
      MessageBox(g_hWndMain, szPath, g_szAppName, MB_ICONERROR | MB_OK);

      return;
   }

   // Create our single item file array.
   CHAR *argv[2] = { pfn->szPathAndMethod, NULL };

   // Create a buffer to store the mapped name of the file.  If the has to be
   // renamed to be compatible with our file system, then we need to know that
   // new name in order to open it correctly.
   CHAR szMappedPath[_MAX_PATH];
   *szMappedPath = '\0';

   // Configure our extract structure.
   EXTRACT_INFO ei;
   ZeroMemory(&ei, sizeof(ei));
   ei.fExtract      = TRUE;
   ei.dwFileCount   = 1;
   ei.uzByteCount   = pfn->uzSize;
   ei.szFileList    = argv;
   ei.fRestorePaths = FALSE;
   ei.overwriteMode = OM_PROMPT;
   ei.szMappedPath  = szMappedPath;

   // Clear our skipped flag and set our viewing flag.
   g_fSkipped = FALSE;
   g_fViewing = TRUE;

   // Display our progress dialog and do the extraction.
   DialogBoxParam(g_hInst, MAKEINTRESOURCE(IDD_VIEW_PROGRESS), g_hWndMain,
                  (DLGPROC)DlgProcViewProgress, (LPARAM)&ei);

   // Clear our viewing flag.
   g_fViewing = FALSE;

   // Check to see if the user skipped the file by aborting the decryption or
   // overwrite prompts.  The only other case that causes us to skip a file
   // is when the user enters the incorrect password too many times.  In this
   // case, IZ_BADPWD will be returned.
   if (g_fSkipped) {
      return;
   }
   if (ei.result == IZ_BADPWD) {
      MessageBox(g_hWndMain, TEXT("Password was incorrect.  The file has been skipped."),
                 g_szAppName, MB_ICONWARNING | MB_OK);
      return;
   }

   // Check to see if the extraction failed.
   if (ei.result != PK_OK) {

      if (ei.result == PK_ABORTED) {
         _tcscpy(szPath, GetZipErrorString(ei.result));

      } else {
         // Create error message.  Use szPath buffer because it is handy.
         _stprintf(szPath,
#ifdef UNICODE
            TEXT("Could not extract \"%S\".\n\n%s\n\nTry using the Test or ")
#else
            TEXT("Could not extract \"%s\".\n\n%s\n\nTry using the Test or ")
#endif
            TEXT("Extract action on the file for more details."),
            *szMappedPath ? szMappedPath : pfn->szPathAndMethod,
            GetZipErrorString(ei.result));
      }

      // Display error message.
      MessageBox(g_hWndMain, szPath, g_szAppName, MB_ICONERROR | MB_OK);

      // If we managed to create a bad file, then delete it.
      if (*szMappedPath) {
         MBSTOTSTR(szPath, szMappedPath, countof(szPath));
         SetFileAttributes(szPath, FILE_ATTRIBUTE_NORMAL);
         if (!DeleteFile(szPath)) {
            SetFileAttributes(szPath, FILE_ATTRIBUTE_READONLY);
         }
      }

      return;
   }

   // Convert the file name to UNICODE.
   MBSTOTSTR(szPath, szMappedPath, countof(szPath));

   // Prepare to launch the file.
   SHELLEXECUTEINFO sei;
   ZeroMemory(&sei, sizeof(sei));
   sei.cbSize      = sizeof(sei);
   sei.hwnd        = g_hWndMain;
   sei.lpDirectory = g_szTempDir;
   sei.nShow       = SW_SHOWNORMAL;

#ifdef _WIN32_WCE

   TCHAR szApp[_MAX_PATH];

   // On Windows CE, there is no default file association dialog that appears
   // when ShellExecuteEx() is given an unknown file type.  We check to see if
   // file is unknown, and display our own file association prompt.

   // Check our file image to see if this file has no associated viewer.
   if (lvi.iImage == IMAGE_GENERIC) {

      // Display our file association prompt dialog.
      if (IDOK != DialogBoxParam(g_hInst, MAKEINTRESOURCE(IDD_VIEW_ASSOCIATION),
                                 g_hWndMain, (DLGPROC)DlgProcViewAssociation,
                                 (LPARAM)szApp))
      {
         // If the user aborted the association prompt, then delete file and exit.
         SetFileAttributes(szPath, FILE_ATTRIBUTE_NORMAL);
         if (!DeleteFile(szPath)) {
            SetFileAttributes(szPath, FILE_ATTRIBUTE_READONLY);
         }
         return;
      }
      // Set the file to be the viewer app and the parameters to be the file.
      // Note: Some applications require that arguments with spaces be quoted,
      // while other applications choked when quotes we part of the filename.
      // In the end, it seems safer to leave the quotes off.
      sei.lpFile = szApp;
      sei.lpParameters = szPath;
   } else {
      sei.lpFile = szPath;
   }

#else

   // On NT, ShellExecuteEx() will prompt user for association if needed.
   sei.lpFile = szPath;

#endif

   // Launch the file.  All errors will be displayed by ShellExecuteEx().
   ShellExecuteEx(&sei);
}

//******************************************************************************
void OnActionSelectAll() {
   for (int i = ListView_GetItemCount(g_hWndList) - 1; i >= 0; i--) {
      ListView_SetItemState(g_hWndList, i, LVIS_SELECTED, LVIS_SELECTED);
   }
}

//******************************************************************************
void OnViewExpandedView() {

   // Toggle our expanded view option.
   g_fExpandedView = !g_fExpandedView;

   // Show or remove menu check and toolbar button press.
   CheckAllMenuItems(IDM_VIEW_EXPANDED_VIEW, g_fExpandedView);

   // Display the new columns.
   AddDeleteColumns();

   // Re-sort if we just did away with out sort column.
   if (!g_fExpandedView && (g_sortColumn > 3)) {
      Sort(0, TRUE);
   }

   // Write our expanded view option to the registry.
   WriteOptionInt(TEXT("ExpandedView"), g_fExpandedView);
}

//******************************************************************************
void OnHelp() {

   // Prepare to launch the help file.
   SHELLEXECUTEINFO sei;
   ZeroMemory(&sei, sizeof(sei));
   sei.cbSize      = sizeof(sei);
   sei.hwnd        = g_hWndMain;
   sei.lpFile      = g_szHelpFile;

   // Launch the file.
   ShellExecuteEx(&sei);
}


//******************************************************************************
//***** Event Handlers for our List View
//******************************************************************************

void OnGetDispInfo(LV_DISPINFO *plvdi) {

   // Make sure we have the minimum amount of data to process this event.
   if ((plvdi->item.iItem < 0) || !plvdi->item.lParam || !plvdi->item.pszText) {
      return;
   }

   // Get a pointer to the file node for this item.
   FILE_NODE *pFile = (FILE_NODE*)plvdi->item.lParam;

   CHAR szBuffer[_MAX_PATH * 2];

   switch (plvdi->item.iSubItem) {

      case 0: // Name

         // Copy the string to a temporary buffer.
         strcpy(szBuffer, pFile->szPathAndMethod);

         // Change all forward slashes to back slashes in the buffer
         ForwardSlashesToBackSlashesA(szBuffer);

         // Convert the string to UNICODE and store it in our list control.
         MBSTOTSTR(plvdi->item.pszText, szBuffer, plvdi->item.cchTextMax);

         return;

      case 1: // Size
         FormatValue(plvdi->item.pszText, pFile->uzSize);
         return;

      case 2: // Type
         MBSTOTSTR(plvdi->item.pszText, BuildTypeString(pFile, szBuffer),
                  plvdi->item.cchTextMax);
         return;

      case 3: // Modified
         int hour; hour = (pFile->dwModified >> 6) & 0x001F;
         _stprintf(plvdi->item.pszText, TEXT("%u/%u/%u %u:%02u %cM"),
                   (pFile->dwModified  >> 16) & 0x000F,
                   (pFile->dwModified  >> 11) & 0x001F,
                   ((pFile->dwModified >> 20) & 0x0FFF) % 100,
                   (hour % 12) ? (hour % 12) : 12,
                   pFile->dwModified & 0x003F,
                   hour >= 12 ? 'P' : 'A');
         return;

      case 4: // Attributes
         BuildAttributesString(plvdi->item.pszText, pFile->dwAttributes);
         return;

      case 5: // Compressed
         FormatValue(plvdi->item.pszText, pFile->uzCompressedSize);
         return;

      case 6: // Ratio
         int factor; factor = ratio(pFile->uzSize, pFile->uzCompressedSize);
         _stprintf(plvdi->item.pszText, TEXT("%d.%d%%"), factor / 10,
                   ((factor < 0) ? -factor : factor) % 10);
         return;

      case 7: // Method
         MBSTOTSTR(plvdi->item.pszText, pFile->szPathAndMethod + strlen(pFile->szPathAndMethod) + 1,
                  plvdi->item.cchTextMax);
         return;

      case 8: // CRC
         _stprintf(plvdi->item.pszText, TEXT("%08X"), pFile->dwCRC);
         return;

      case 9: // Comment
         MBSTOTSTR(plvdi->item.pszText, pFile->szComment ? pFile->szComment : "",
                   plvdi->item.cchTextMax);
         return;
   }
}

//******************************************************************************
void OnDeleteItem(NM_LISTVIEW *pnmlv) {
   if (pnmlv->lParam) {

      // Free any comment string associated with this item.
      if (((FILE_NODE*)pnmlv->lParam)->szComment) {
         delete[] (CHAR*)((FILE_NODE*)pnmlv->lParam)->szComment;
      }

      // Free the item itself.
      delete[] (LPBYTE)pnmlv->lParam;
   }
}

//******************************************************************************
void OnItemChanged(NM_LISTVIEW *pnmlv) {
   int count = ListView_GetSelectedCount(pnmlv->hdr.hwndFrom);
   EnableAllMenuItems(IDM_FILE_PROPERTIES, count > 0);
   EnableAllMenuItems(IDM_ACTION_EXTRACT,  count > 0);
   EnableAllMenuItems(IDM_ACTION_TEST,     count > 0);
   EnableAllMenuItems(IDM_ACTION_VIEW,     count == 1);
}

//******************************************************************************
//***** List View Sort Functions
//******************************************************************************

void Sort(int sortColumn, BOOL fForce) {

   // Do not change the column header text if it is already correct.
   if (sortColumn != g_sortColumn) {

      TCHAR szColumn[32];
      LV_COLUMN lvc;
      lvc.mask = LVCF_TEXT;
      lvc.pszText = szColumn;

      // Remove the '^' from the current sort column.
      if (g_sortColumn != -1) {
         _stprintf(szColumn, (g_columns[g_sortColumn].format == LVCFMT_LEFT) ?
                   TEXT("%s   ") : TEXT("   %s"), g_columns[g_sortColumn].szName);
         ListView_SetColumn(g_hWndList, g_sortColumn, &lvc);
      }

      // Set the new sort column.
      g_sortColumn = sortColumn;

      // Add the '^' to the new sort column.
      _stprintf(szColumn, (g_columns[g_sortColumn].format == LVCFMT_LEFT) ?
                TEXT("%s ^") : TEXT("^ %s"), g_columns[g_sortColumn].szName);
      ListView_SetColumn(g_hWndList, g_sortColumn, &lvc);

      // Sort the list by the new column.
      ListView_SortItems(g_hWndList, CompareFunc, g_sortColumn);

   } else if (fForce) {
      // Force the list to sort by the same column.
      ListView_SortItems(g_hWndList, CompareFunc, g_sortColumn);
   }
}

//******************************************************************************
int CALLBACK CompareFunc(LPARAM lParam1, LPARAM lParam2, LPARAM sortColumn) {
   FILE_NODE *pFile1 = (FILE_NODE*)lParam1, *pFile2 = (FILE_NODE*)lParam2;
   TCHAR szBuffer1[8], szBuffer2[8];

   // Return Negative value if the first item should precede the second.
   // Return Positive value if the first item should follow the second.
   // Return Zero if the two items are equivalent.

   int result = 0;

   // Compute the relationship based on the current sort column
   switch (sortColumn) {

      case 1: // Size - Smallest to Largest
         if (pFile1->uzSize != pFile2->uzSize) {
            result = ((pFile1->uzSize < pFile2->uzSize) ? -1 : 1);
         }
         break;

      case 2: { // Type - Volume Label's first, then directories, then files
         int f1 = (pFile1->dwAttributes & ZFILE_ATTRIBUTE_VOLUME)   ? 1 :
                  (pFile1->dwAttributes & FILE_ATTRIBUTE_DIRECTORY) ? 2 : 3;
         int f2 = (pFile2->dwAttributes & ZFILE_ATTRIBUTE_VOLUME)   ? 1 :
                  (pFile2->dwAttributes & FILE_ATTRIBUTE_DIRECTORY) ? 2 : 3;
         if ((f1 == 3) && (f2 == 3)) {
            CHAR szType1[128];
            CHAR szType2[128];
            result = _stricmp(BuildTypeString(pFile1, szType1),
                              BuildTypeString(pFile2, szType2));
         } else {
            result = f1 - f2;
         }
         break;
      }

      case 3: // Modified - Newest to Oldest
         if (pFile1->dwModified != pFile2->dwModified) {
            result = ((pFile1->dwModified > pFile2->dwModified) ? -1 : 1);
         }
         break;

      case 4: // Attributes - String Sort
         result = _tcscmp(BuildAttributesString(szBuffer1, pFile1->dwAttributes),
                          BuildAttributesString(szBuffer2, pFile2->dwAttributes));
         break;

      case 5: // Compressed Size - Smallest to Largest
         if (pFile1->uzCompressedSize != pFile2->uzCompressedSize) {
            result = ((pFile1->uzCompressedSize < pFile2->uzCompressedSize) ? -1 : 1);
         }
         break;

      case 6: // Ratio - Smallest to Largest
         int factor1, factor2;
         factor1 = ratio(pFile1->uzSize, pFile1->uzCompressedSize);
         factor2 = ratio(pFile2->uzSize, pFile2->uzCompressedSize);
         result = factor1 - factor2;
         break;

      case 7: // Method - String Sort
         result = _stricmp(pFile1->szPathAndMethod + strlen(pFile1->szPathAndMethod) + 1,
                           pFile2->szPathAndMethod + strlen(pFile2->szPathAndMethod) + 1);
         break;

      case 8: // CRC - Smallest to Largest
         if (pFile1->dwCRC != pFile2->dwCRC) {
            result = ((pFile1->dwCRC < pFile2->dwCRC) ? -1 : 1);
         }
         break;

      case 9: // Comment - String Sort
         result = _stricmp(pFile1->szComment ? pFile1->szComment : "",
                           pFile2->szComment ? pFile2->szComment : "");
         break;
   }

   // If the sort resulted in a tie, we use the name to break the tie.
   if (result == 0) {
      result = _stricmp(pFile1->szPathAndMethod, pFile2->szPathAndMethod);
   }

   return result;
}


//******************************************************************************
//***** Helper/Utility Functions
//******************************************************************************

void SetCaptionText(LPCTSTR szPrefix) {
   TCHAR szCaption[_MAX_PATH + 32];
   if (szPrefix) {
      _stprintf(szCaption, TEXT("%s - "), szPrefix);
   } else {
      *szCaption = 0;
   }
   if (*g_szZipFile) {
      size_t lenPrefix = _tcslen(szCaption);
      MBSTOTSTR(szCaption + lenPrefix, GetFileFromPath(g_szZipFile),
                countof(szCaption) - lenPrefix);
   } else {
      _tcscat(szCaption, TEXT("Pocket UnZip"));
   }
   SetWindowText(g_hWndMain, szCaption);
}

//******************************************************************************
void DrawBanner(HDC hdc) {

   // If we were not passed in a DC, then get one now.
   BOOL fReleaseDC = FALSE;
   if (!hdc) {
      hdc = GetDC(g_hWndMain);
      fReleaseDC = TRUE;
   }

   // Compute the banner rectangle.
   RECT rc;
   GetClientRect(g_hWndMain, &rc);
   rc.top += g_cyCmdBar;
   rc.bottom = rc.top + 22;

   // Fill in the background with a light grey brush.
   FillRect(hdc, &rc, (HBRUSH)GetStockObject(LTGRAY_BRUSH));

   // Draw a highlight line across the top of our banner.
   POINT pt[2] = { { rc.left, rc.top + 1 }, { rc.right, rc.top + 1 } };

   SelectObject(hdc, GetStockObject(WHITE_PEN));
   Polyline(hdc, pt, 2);

   // Get the ZIP file image.  We do this only once and cache the result.
   // Note that you do not need to free icons as they are a resource.
   static HICON hIcon = NULL;
   if (!hIcon) {
      hIcon = (HICON)LoadImage(g_hInst, MAKEINTRESOURCE(IDI_ZIPFILE),
                               IMAGE_ICON, 16, 16, LR_DEFAULTCOLOR);
   }

   // Draw the ZIP file image.
   DrawIconEx(hdc, rc.left + 6, rc.top + 3, hIcon, 16, 16, 0, NULL, DI_NORMAL);

   // Set our font and colors.
   HFONT hFontStock = (HFONT)SelectObject(hdc, g_hFontBanner);
   SetTextColor(hdc, RGB(0, 0, 0));
   SetBkMode(hdc, TRANSPARENT);

   rc.left   += 26;
   rc.right  -= 48;
   rc.bottom -=  2;

   // Decide what text to display.
   TCHAR szPath[_MAX_PATH + 16];
   if (g_hWndWaitFor) {
      _tcscpy(szPath, TEXT("Initializing..."));
   } else if (*g_szZipFile) {
      if (g_fLoading) {
#ifdef UNICODE
         _stprintf(szPath, TEXT("Loading %S"), g_szZipFile);
#else
         _stprintf(szPath, TEXT("Loading %s"), g_szZipFile);
#endif
      } else {
         MBSTOTSTR(szPath, g_szZipFile, countof(szPath));
      }
   } else {
      _tcscpy(szPath, TEXT("No File Loaded"));
   }

   // Draw the banner text.
   DrawText(hdc, szPath, _tcslen(szPath), &rc,
            DT_NOPREFIX | DT_SINGLELINE | DT_LEFT | DT_VCENTER);

   // Remove all non stock objects from the DC
   SelectObject(hdc, hFontStock);

   // Free our DC if we created it.
   if (fReleaseDC) {
      ReleaseDC(g_hWndMain, hdc);
   }
}

//******************************************************************************
void AddDeleteColumns() {

   static int curColumns = 0;
   int column, newColumns = (g_fExpandedView ? countof(g_columns) : 4);

   // Are we adding columns?
   if (newColumns > curColumns) {

      // Set up column structure.
      TCHAR szColumn[32];
      LV_COLUMN lvc;
      lvc.mask = LVCF_TEXT | LVCF_FMT;
      lvc.pszText = szColumn;

      // Loop through each column we need to add.
      for (column = curColumns; column < newColumns; column++) {

         // Build the real column string.
         _stprintf(szColumn, (g_columns[column].format == LVCFMT_LEFT) ?
                   TEXT("%s   ") : TEXT("   %s"), g_columns[column].szName);

         // Insert the column with the correct format.
         lvc.fmt = g_columns[column].format;
         ListView_InsertColumn(g_hWndList, column, &lvc);
      }

   // Otherwise, we are removing columns.
   } else {

      // Loop through each column we need to delete and delete them.
      for (column = curColumns - 1; column >= newColumns; column--) {
         ListView_DeleteColumn(g_hWndList, column);
      }
   }

   // Store our new column count statically to help us with the next call to
   // AddDeleteColumns().
   curColumns = newColumns;

   // Re-calcualte our column widths.
   ResizeColumns();
}

//******************************************************************************
void ResizeColumns() {

   // Hide the window since we are going to be doing some column shifting.
   ShowWindow(g_hWndList, SW_HIDE);

   // Resize all the columns to best fit both the column data and the header.
   for (int column = 0; column < countof(g_columns); column++) {
      ListView_SetColumnWidth(g_hWndList, column, LVSCW_AUTOSIZE_USEHEADER);
   }

   // Show the window again.
   ShowWindow(g_hWndList, SW_SHOW);
}

//******************************************************************************
LPCTSTR GetZipErrorString(int error) {

   switch (error) {

      case PK_OK: // no error
         return TEXT("Operation completed successfully.");

      case PK_WARN: // warning error
         return TEXT("There were warnings during the operation.");

      case PK_ERR:    // error in zipfile
      case PK_BADERR: // severe error in zipfile
         return TEXT("The operation could not be successfully completed.  ")
                TEXT("Possible causes are that the ZIP file contains errors, ")
                TEXT("or that an error occurred while trying to create a ")
                TEXT("directory or file.");

      case PK_MEM:  // insufficient memory
      case PK_MEM2: // insufficient memory
      case PK_MEM3: // insufficient memory
      case PK_MEM4: // insufficient memory
      case PK_MEM5: // insufficient memory
         return TEXT("There is not enough memory to perform the operation.  ")
                TEXT("Try closing other running applications or adjust your ")
                TEXT("memory configuration.");

      case PK_NOZIP: // zipfile not found or corrupt.
         return TEXT("The ZIP file either contains errors or could not be found.");

      case PK_PARAM: // bad or illegal parameters specified
         break; // Not used in the Windows CE port.

      case PK_FIND: // no files found in ZIP file
         return TEXT("The ZIP file contains errors that prevented the ")
                TEXT("operation from completing successfully.  A possible ")
                TEXT("cause is that one or more of the files listed as being ")
                TEXT("in the ZIP file could not actually be found within the ")
                TEXT("ZIP file itself.");

      case PK_DISK: // disk full or file locked
         return TEXT("An error occurred while attempting to save a file.  ")
                TEXT("Possible causes are that your file storage is full or ")
                TEXT("read only, or that a file with the same name already ")
                TEXT("exists and is locked by another application.");

      case PK_EOF: // unexpected end of file
         return TEXT("The ZIP file contains errors that prevented the ")
                TEXT("operation from completing successfully.  A possible ")
                TEXT("cause is that your ZIP file is incomplete and might be ")
                TEXT("truncated.");

      case IZ_UNSUP:  // no files found: all unsup. compr/encrypt.
         return TEXT("None of the files could be processed because they were ")
                TEXT("all compressed using an unsupported compression or ")
                TEXT("encryption algorithm.");

      case IZ_BADPWD: // no files found: all had bad password.
         return TEXT("None of the files could be processed because all the ")
                TEXT("password(s) specified were incorrect.");

      case PK_EXCEPTION: // exception occurred
         return TEXT("An internal error occurred.  Possible causes are that ")
                TEXT("you are out of memory, you are out of file storage ")
                TEXT("space, the ZIP file contains unexpected errors, or there ")
                TEXT("is a bug in our program (that's why it's free).");

      case IZ_CTRLC:  // canceled by user's interaction
      case PK_ABORTED: // user aborted
         return TEXT("The operation was aborted.");
   }

   return TEXT("An unknown error occurred while processing the ZIP file.");
}

//******************************************************************************
void AddFileToListView(FILE_NODE *pFile) {

   // Set up our List View Item structure.
   LV_ITEM lvi;
   ZeroMemory(&lvi, sizeof(lvi));
   lvi.mask    = LVIF_TEXT | LVIF_IMAGE | LVIF_PARAM;
   lvi.pszText = LPSTR_TEXTCALLBACK;
   lvi.lParam  = (LPARAM)pFile;
   lvi.iImage  = IMAGE_GENERIC;

   // Special case Volume Labels.
   if (pFile->dwAttributes & ZFILE_ATTRIBUTE_VOLUME) {
      pFile->szType = "Volume Label";
      lvi.iImage = IMAGE_VOLUME;

   // Special case folders.
   } else if (pFile->dwAttributes & FILE_ATTRIBUTE_DIRECTORY) {
      pFile->szType = "Folder";
      lvi.iImage = IMAGE_FOLDER;

   // Do a lookup on the file extension.
   } else {

      // Locate the file portion of our path.
      LPCSTR pszFile = GetFileFromPath(pFile->szPathAndMethod);

      // Find the extension portion of our file.
      LPCSTR pszExt = MBSRCHR(pszFile, '.');

      // Search our known extension list for this extension.
      if (pszExt && *(pszExt + 1)) {

         // Loop through our linked list
         for (FILE_TYPE_NODE *pft = g_pftHead; pft; pft = pft->pNext) {

            // Check for a match.
            if (!_stricmp(pszExt + 1, pft->szExtAndDesc)) {

               // We found a match, store the image and type string and exit loop.
               lvi.iImage = pft->image;
               pFile->szType = pft->szExtAndDesc + strlen(pft->szExtAndDesc) + 1;
               if (!*pFile->szType) {
                  pFile->szType = NULL;
               }
               break;
            }
         }
      }
   }

   // Add the item to our list.
   ListView_InsertItem(g_hWndList, &lvi);
}

//******************************************************************************
void EnableAllMenuItems(UINT uMenuItem, BOOL fEnabled) {
#ifdef _WIN32_WCE
   HMENU hMenu = CommandBar_GetMenu(g_hWndCmdBar, 0);
#else
   HMENU hMenu = GetMenu(g_hWndMain);
#endif
   EnableMenuItem(hMenu, uMenuItem, fEnabled ? MF_ENABLED : MF_GRAYED);
   SendMessage(g_hWndCmdBar, TB_ENABLEBUTTON, uMenuItem, MAKELONG(fEnabled, 0));
}

//******************************************************************************
void CheckAllMenuItems(UINT uMenuItem, BOOL fChecked) {
#ifdef _WIN32_WCE
   HMENU hMenu = CommandBar_GetMenu(g_hWndCmdBar, 0);
#else
   HMENU hMenu = GetMenu(g_hWndMain);
#endif
   CheckMenuItem(hMenu, uMenuItem, fChecked ? MF_CHECKED : MF_UNCHECKED);
   SendMessage(g_hWndCmdBar, TB_PRESSBUTTON, uMenuItem, MAKELONG(fChecked, 0));
}

//******************************************************************************
void CenterWindow(HWND hWnd) {

   RECT rc, rcParent;

   // Get our window rectangle.
   GetWindowRect(hWnd, &rc);

   // Get our parent's window rectangle.
   GetWindowRect(GetParent(hWnd), &rcParent);

   // Center our window over our parent's window.
   SetWindowPos(hWnd, NULL,
      rcParent.left + ((rcParent.right  - rcParent.left) - (rc.right  - rc.left)) / 2,
      rcParent.top  + ((rcParent.bottom - rcParent.top ) - (rc.bottom - rc.top )) / 2,
      0, 0, SWP_NOZORDER | SWP_NOSIZE);
}

//******************************************************************************
void AddTextToEdit(LPCSTR szText) {

   if (!g_hWndEdit) {
      return;
   }

   // Add the characters one by one to our edit box while performing the
   // the following newline conversions:
   //    Single CR -> CR/LF
   //    Single LF -> CR/LF
   //    CR and LF -> CR/LF
   //    LF and CR -> CR/LF
   //    0 - 31    -> ^char

   TCHAR szOut[256], *pszOut = szOut;
   CHAR *pszIn = (LPSTR)szText, cPrev = '\0';

   while (*pszIn) {

      if (*pszIn == '\n') {
         if (cPrev == '\r') {
            cPrev = '\0';
         } else {
            *(pszOut++) = TEXT('\r');
            *(pszOut++) = TEXT('\n');
            cPrev = '\n';
         }

      } else if (*pszIn == '\r') {
         if (cPrev == '\n') {
            cPrev = '\0';
         } else {
            *(pszOut++) = TEXT('\r');
            *(pszOut++) = TEXT('\n');
            cPrev = '\r';
         }

      } else if ((*pszIn < 32) && (*pszIn != '\t')) {
         *(pszOut++) = (TCHAR)'^';
         *(pszOut++) = (TCHAR)(64 + *pszIn);
         cPrev = *pszIn;

      } else {
         *(pszOut++) = (TCHAR)*pszIn;
         cPrev = *pszIn;
      }
      pszIn++;

      // If our out buffer is full, then dump it to the edit box.
      if ((pszOut - szOut) > 253) {
         *pszOut = TEXT('\0');
         SendMessage(g_hWndEdit, EM_SETSEL, -1, -1);
         SendMessage(g_hWndEdit, EM_REPLACESEL, FALSE, (LPARAM)szOut);
         pszOut = szOut;
      }
   }

   // One final flush of any partially full out buffer.
   if (pszOut > szOut) {
      *pszOut = TEXT('\0');
      SendMessage(g_hWndEdit, EM_SETSEL, -1, -1);
      SendMessage(g_hWndEdit, EM_REPLACESEL, FALSE, (LPARAM)szOut);
   }
}

//******************************************************************************
LPTSTR FormatValue(LPTSTR szValue, zusz_t uzValue) {
#ifdef ZIP64_SUPPORT
    DWORD dw = 0, dwGroup[8] = { 0, 0, 0, 0, 0, 0, 0, 0 };
#else
    DWORD dw = 0, dwGroup[4] = { 0, 0, 0, 0 };
#endif
   while (uzValue) {
      dwGroup[dw++] = (DWORD)(uzValue % 1000);
      uzValue /= 1000;
   }
   switch (dw) {
      case 2:  _stprintf(szValue, TEXT("%u,%03u"), dwGroup[1], dwGroup[0]); break;
      case 3:  _stprintf(szValue, TEXT("%u,%03u,%03u"), dwGroup[2], dwGroup[1], dwGroup[0]); break;
      case 4:  _stprintf(szValue, TEXT("%u,%03u,%03u,%03u"), dwGroup[3], dwGroup[2], dwGroup[1], dwGroup[0]); break;
#ifdef ZIP64_SUPPORT
      case 5:
          _stprintf(szValue, TEXT("%u,%03u,%03u,%03u,%03u"),
                    dwGroup[4], dwGroup[3], dwGroup[2], dwGroup[1], dwGroup[0]);
          break;
      case 6:
          _stprintf(szValue, TEXT("%u,%03u,%03u,%03u,%03u,%03u"), dwGroup[5],
                    dwGroup[4], dwGroup[3], dwGroup[2], dwGroup[1], dwGroup[0]);
          break;
      case 7:
          _stprintf(szValue, TEXT("%u,%03u,%03u,%03u,%03u,%03u,%03u"), dwGroup[6], dwGroup[5],
                    dwGroup[4], dwGroup[3], dwGroup[2], dwGroup[1], dwGroup[0]);
          break;
      case 8:
          _stprintf(szValue, TEXT("%u,%03u,%03u,%03u,%03u,%03u,%03u,%03u"), dwGroup[7], dwGroup[6], dwGroup[5],
                    dwGroup[4], dwGroup[3], dwGroup[2], dwGroup[1], dwGroup[0]);
#endif
      default: _stprintf(szValue, TEXT("%u"), dwGroup[0]);
   }
   return szValue;
}

//******************************************************************************
LPTSTR BuildAttributesString(LPTSTR szBuffer, DWORD dwAttributes) {
   // Build the attribute string according to the flags specified for this file.
   _stprintf(szBuffer, TEXT("%s%s%s%s%s%s%s%s"),
             (dwAttributes & ZFILE_ATTRIBUTE_VOLUME)    ? TEXT("V") : TEXT(""),
             (dwAttributes & FILE_ATTRIBUTE_DIRECTORY)  ? TEXT("D") : TEXT(""),
             (dwAttributes & FILE_ATTRIBUTE_READONLY)   ? TEXT("R") : TEXT(""),
             (dwAttributes & FILE_ATTRIBUTE_ARCHIVE)    ? TEXT("A") : TEXT(""),
             (dwAttributes & FILE_ATTRIBUTE_HIDDEN)     ? TEXT("H") : TEXT(""),
             (dwAttributes & FILE_ATTRIBUTE_SYSTEM)     ? TEXT("S") : TEXT(""),
             (dwAttributes & ZFILE_ATTRIBUTE_ENCRYPTED) ? TEXT("E") : TEXT(""),
             (dwAttributes & ZFILE_ATTRIBUTE_COMMENT)   ? TEXT("C") : TEXT(""));
   return szBuffer;
}

//******************************************************************************
LPCSTR BuildTypeString(FILE_NODE *pFile, LPSTR szType) {

   // First check to see if we have a known description.
   if (pFile->szType) {
      return pFile->szType;
   }

   // Locate the file portion of our path.
   LPCSTR pszFile = GetFileFromPath(pFile->szPathAndMethod);

   // Get the extension portion of the file.
   LPCSTR pszExt = MBSRCHR(pszFile, '.');

   // If we have an extension create a type name for this file.
   if (pszExt && *(pszExt + 1)) {
      strcpy(szType, pszExt + 1);
      _strupr(szType);
      strcat(szType, " File");
      return szType;
   }

   // If no extension, then use the default "File".
   return "File";
}

//******************************************************************************
LPCSTR GetFileFromPath(LPCSTR szPath) {
   LPCSTR p1 = MBSRCHR(szPath, '/'), p2 = MBSRCHR(szPath, '\\');
   if (p1 && (p1 > p2)) {
      return p1 + 1;
   } else if (p2) {
      return p2 + 1;
   }
   return szPath;
}

//******************************************************************************
void ForwardSlashesToBackSlashesA(LPSTR szBuffer) {
   while (*szBuffer) {
      if (*szBuffer == '/') {
         *szBuffer = '\\';
      }
      INCSTR(szBuffer);
   }
}

//******************************************************************************
void ForwardSlashesToBackSlashesW(LPWSTR szBuffer) {
   while (*szBuffer) {
      if (*szBuffer == L'/') {
         *szBuffer = L'\\';
      }
      szBuffer++;
   }
}

//******************************************************************************
void DeleteDirectory(LPTSTR szPath) {

   // Make note to where the end of our path is.
   LPTSTR szEnd = szPath + _tcslen(szPath);

   // Add our search spec to the path.
   _tcscpy(szEnd, TEXT("\\*.*"));

   // Start a directory search.
   WIN32_FIND_DATA w32fd;
   HANDLE hFind = FindFirstFile(szPath, &w32fd);

   // Loop through all entries in this directory.
   if (hFind != INVALID_HANDLE_VALUE) {

      do {
         // Append the file/directory name to the path.
         _tcscpy(szEnd + 1, w32fd.cFileName);

         // Check to see if this entry is a subdirectory.
         if (w32fd.dwFileAttributes & FILE_ATTRIBUTE_DIRECTORY) {

            // Ignore current directory (.) and previous directory (..)
            if (_tcscmp(w32fd.cFileName, TEXT("."))   &&
                _tcscmp(w32fd.cFileName, TEXT("..")))
            {
               // Recurse into DeleteDirectory() to delete subdirectory.
               DeleteDirectory(szPath);
            }

         // Otherwise, it must be a file.
         } else {

            // If the file is marked as read-only, then change to read/write.
            if (w32fd.dwFileAttributes & FILE_ATTRIBUTE_READONLY) {
               SetFileAttributes(szPath, FILE_ATTRIBUTE_NORMAL);
            }

            // Attempt to delete the file.  If we fail and the file used to be
            // read-only, then set the read-only bit back on it.
            if (!DeleteFile(szPath) &&
                (w32fd.dwFileAttributes & FILE_ATTRIBUTE_READONLY))
            {
               SetFileAttributes(szPath, FILE_ATTRIBUTE_READONLY);
            }
         }

      // Get the next directory entry.
      } while (FindNextFile(hFind, &w32fd));

      // Close the directory search.
      FindClose(hFind);
   }

   // Remove the directory.
   *szEnd = TEXT('\0');
   RemoveDirectory(szPath);
}


//******************************************************************************
//***** Registry Functions
//******************************************************************************

void RegWriteKey(HKEY hKeyRoot, LPCTSTR szSubKey, LPCTSTR szValue) {
   HKEY  hKey = NULL;
   DWORD dwDisposition;

   if (RegCreateKeyEx(hKeyRoot, szSubKey, 0, NULL, 0, KEY_SET_VALUE, NULL, &hKey, &dwDisposition) == ERROR_SUCCESS) {
      if (szValue) {
         RegSetValueEx(hKey, NULL, 0, REG_SZ, (LPBYTE)szValue,
                       sizeof(TCHAR) * (_tcslen(szValue) + 1));
      }
      RegCloseKey(hKey);
   }
}

//******************************************************************************
BOOL RegReadKey(HKEY hKeyRoot, LPCTSTR szSubKey, LPTSTR szValue, DWORD cBytes) {
   *szValue = TEXT('\0');
   HKEY hKey = NULL;
   LRESULT lResult = -1;

   if (RegOpenKeyEx(hKeyRoot, szSubKey, 0, KEY_QUERY_VALUE, &hKey) == ERROR_SUCCESS) {
      lResult = RegQueryValueEx(hKey, NULL, NULL, NULL, (LPBYTE)szValue, &cBytes);
      RegCloseKey(hKey);
   }
   return ((lResult == ERROR_SUCCESS) && *szValue);
}

//******************************************************************************
void WriteOptionString(LPCTSTR szOption, LPCTSTR szValue) {
   HKEY hKey = NULL;

   if (RegOpenKeyEx(HKEY_CURRENT_USER, g_szRegKey, 0, KEY_SET_VALUE, &hKey) == ERROR_SUCCESS) {
      RegSetValueEx(hKey, szOption, 0, REG_SZ, (LPBYTE)szValue,
                    sizeof(TCHAR) * (_tcslen(szValue) + 1));
      RegCloseKey(hKey);
   }
}

//******************************************************************************
void WriteOptionInt(LPCTSTR szOption, DWORD dwValue) {
   HKEY hKey = NULL;

   if (RegOpenKeyEx(HKEY_CURRENT_USER, g_szRegKey, 0, KEY_SET_VALUE, &hKey) == ERROR_SUCCESS) {
      RegSetValueEx(hKey, szOption, 0, REG_DWORD, (LPBYTE)&dwValue, sizeof(DWORD));
      RegCloseKey(hKey);
   }
}

//******************************************************************************
LPTSTR GetOptionString(LPCTSTR szOption, LPCTSTR szDefault, LPTSTR szValue, DWORD nSize) {
   HKEY hKey = NULL;
   LONG lResult = -1;

   if (RegOpenKeyEx(HKEY_CURRENT_USER, g_szRegKey, 0, KEY_QUERY_VALUE, &hKey) == ERROR_SUCCESS) {
      lResult = RegQueryValueEx(hKey, szOption, NULL, NULL, (LPBYTE)szValue, &nSize);
      RegCloseKey(hKey);
   }
   if (lResult != ERROR_SUCCESS) {
      _tcscpy(szValue, szDefault);
   }
   return szValue;
}

//******************************************************************************
DWORD GetOptionInt(LPCTSTR szOption, DWORD dwDefault) {
   HKEY  hKey = NULL;
   LONG  lResult = -1;
   DWORD dwValue;
   DWORD nSize = sizeof(dwValue);

   if (RegOpenKeyEx(HKEY_CURRENT_USER, g_szRegKey, 0, KEY_QUERY_VALUE, &hKey) == ERROR_SUCCESS) {
      lResult = RegQueryValueEx(hKey, szOption, NULL, NULL, (LPBYTE)&dwValue, &nSize);
      RegCloseKey(hKey);
   }
   return (lResult == ERROR_SUCCESS) ? dwValue : dwDefault;
}

//******************************************************************************
//***** EDIT Control Subclass Functions
//******************************************************************************

void DisableEditing(HWND hWndEdit) {

   // Make sure the control does not have ES_READONLY or ES_WANTRETURN styles.
   DWORD dwStyle = (DWORD)GetWindowLong(hWndEdit, GWL_STYLE);
   if (dwStyle & (ES_READONLY | ES_WANTRETURN)) {
      SetWindowLong(hWndEdit, GWL_STYLE, dwStyle & ~(ES_READONLY | ES_WANTRETURN));
   }

   // Subclass the control so we can intercept certain keys.
   g_wpEdit = (WNDPROC)GetWindowLong(hWndEdit, GWL_WNDPROC);
   SetWindowLong(hWndEdit, GWL_WNDPROC, (LONG)EditSubclassProc);
}

//******************************************************************************
LRESULT CALLBACK EditSubclassProc(HWND hWnd, UINT uMsg, WPARAM wParam, LPARAM lParam) {

   BOOL fCtrl, fShift;

   switch (uMsg) {
      // For cut, paste, delete, and undo, the control post itself a message.
      // we throw away that message.  This works as a fail-safe in case we miss
      // some keystroke that causes one of these operations.  This also disables
      // the context menu on NT from causing one of these actions to occur.
      case WM_CUT:
      case WM_PASTE:
      case WM_CLEAR:
      case WM_UNDO:
         MessageBeep(0);
         return 0;

      // WM_CHAR is used for normal characters. A-Z, numbers, symbols, enter,
      // backspace, esc, and tab. In does not include del or movement keys.
      case WM_CHAR:
         fCtrl  = (GetKeyState(VK_CONTROL) & 0x8000) ? TRUE : FALSE;

         // We only allow CTRL-C (copy), plain ESC, plain TAB, plain ENTER.
         if (( fCtrl && (wParam == 3))         ||
             (!fCtrl && (wParam == VK_ESCAPE)) ||
             (!fCtrl && (wParam == VK_RETURN)) ||
             (!fCtrl && (wParam == VK_TAB)))
         {
            break;
         }
         MessageBeep(0);
         return 0;

      // WM_KEYDOWN handles del, insert, arrows, pg up/down, home/end.
      case WM_KEYDOWN:
         fCtrl  = (GetKeyState(VK_CONTROL) & 0x8000) ? TRUE : FALSE;
         fShift = (GetKeyState(VK_SHIFT)   & 0x8000) ? TRUE : FALSE;

         // Skip all forms of DELETE, SHIFT-INSERT (paste),
         // CTRL-RETURN (hard-return), and CTRL-TAB (hard-tab).
         if ((          (wParam == VK_DELETE)) ||
             (fShift && (wParam == VK_INSERT)) ||
             (fCtrl  && (wParam == VK_RETURN)) ||
             (fCtrl  && (wParam == VK_TAB)))
         {
            MessageBeep(0);
            return 0;
         }
         break;
   }
   return CallWindowProc(g_wpEdit, hWnd, uMsg, wParam, lParam);
}


//******************************************************************************
//***** MRU Functions
//******************************************************************************

#ifdef _WIN32_WCE
int GetMenuString(HMENU hMenu, UINT uIDItem, LPTSTR lpString, int nMaxCount,
                  UINT uFlag) {
   MENUITEMINFO mii;
   ZeroMemory(&mii, sizeof(mii));
   mii.cbSize = sizeof(mii);
   mii.fMask = MIIM_TYPE;
   mii.dwTypeData = lpString;
   mii.cch = nMaxCount;
   return (GetMenuItemInfo(hMenu, uIDItem, uFlag == MF_BYPOSITION, &mii) ?
           mii.cch : 0);
}
#endif

//******************************************************************************
void InitializeMRU() {

   TCHAR szMRU[MRU_MAX_FILE][_MAX_PATH + 4], szOption[8];
   int   i, j;

   // Get our menu handle.
#ifdef _WIN32_WCE
   HMENU hMenu = GetSubMenu(CommandBar_GetMenu(g_hWndCmdBar, 0), 0);
#else
   HMENU hMenu = GetSubMenu(GetMenu(g_hWndMain), 0);
#endif

   // Read all our current MRUs from the registry.
   for (i = 0, j = 0; i < MRU_MAX_FILE; i++) {

      // Build option name for current MRU and read from registry.
      _stprintf(szOption, TEXT("MRU%d"), i+1);
      GetOptionString(szOption, TEXT(""), &szMRU[i][3], sizeof(TCHAR) * _MAX_PATH);

      // If this MRU exists, then add it.
      if (szMRU[i][3]) {

         // Build the accelerator prefix for this menu item.
         szMRU[i][0] = TEXT('&');
         szMRU[i][1] = TEXT('1') + j;
         szMRU[i][2] = TEXT(' ');

         // Add the item to our menu.
         InsertMenu(hMenu, 4 + j, MF_BYPOSITION | MF_STRING, MRU_START_ID + j,
                    szMRU[i]);

         // Increment our actual MRU count.
         j++;
      }
   }
}

//******************************************************************************
void AddFileToMRU(LPCSTR szFile) {

   TCHAR szMRU[MRU_MAX_FILE + 1][_MAX_PATH + 4], szOption[8];
   int   i, j;

   // Store the new file in our first MRU index.
   MBSTOTSTR(&szMRU[0][3], szFile, _MAX_PATH);

   //---------------------------------------------------------------------------
   // We first read the current MRU list from the registry, merge in our new
   // file at the top, and then write back to the registry.  The registry merge
   // is done to allow multiple instances of Pocket UnZip to maintain a global
   // MRU list independent to this current instance's MRU list.
   //---------------------------------------------------------------------------

   // Read all our current MRUs from the registry.
   for (i = 1; i <= MRU_MAX_FILE; i++) {

      // Build option name for current MRU and read from registry.
      _stprintf(szOption, TEXT("MRU%d"), i);
      GetOptionString(szOption, TEXT(""), &szMRU[i][3], sizeof(TCHAR) * _MAX_PATH);
   }

   // Write our new merged MRU list back to the registry.
   for (i = 0, j = 0; (i <= MRU_MAX_FILE) && (j < MRU_MAX_FILE); i++) {

      // If this MRU exists and is different then our new file, then add it.
      if ((i == 0) || (szMRU[i][3] && _tcsicmp(&szMRU[0][3], &szMRU[i][3]))) {

         // Build option name for current MRU and write to registry.
         _stprintf(szOption, TEXT("MRU%d"), ++j);
         WriteOptionString(szOption, &szMRU[i][3]);
      }
   }

   //---------------------------------------------------------------------------
   // The next thing we need to do is read our local MRU from our File menu,
   // merge in our new file, and store the new list back to our File menu.
   //---------------------------------------------------------------------------

   // Get our menu handle.
#ifdef _WIN32_WCE
   HMENU hMenu = GetSubMenu(CommandBar_GetMenu(g_hWndCmdBar, 0), 0);
#else
   HMENU hMenu = GetSubMenu(GetMenu(g_hWndMain), 0);
#endif

   // Read all our current MRUs from our File Menu.
   for (i = 1; i <= MRU_MAX_FILE; i++) {

      // Query our file Menu for a MRU file.
      if (GetMenuString(hMenu, MRU_START_ID + i - 1, szMRU[i],
                        countof(szMRU[0]), MF_BYCOMMAND))
      {
         // Delete this item from the menu for now.
         DeleteMenu(hMenu, MRU_START_ID + i - 1, MF_BYCOMMAND);
      } else {
         szMRU[i][3] = TEXT('\0');
      }
   }

   // Write our new merged MRU list back to the File menu.
   for (i = 0, j = 0; (i <= MRU_MAX_FILE) && (j < MRU_MAX_FILE); i++) {

      // If this MRU exists and is different then our new file, then add it.
      if ((i == 0) || (szMRU[i][3] && _tcsicmp(&szMRU[0][3], &szMRU[i][3]))) {

         // Build the accelerator prefix for this menu item.
         szMRU[i][0] = TEXT('&');
         szMRU[i][1] = TEXT('1') + j;
         szMRU[i][2] = TEXT(' ');

         // Add the item to our menu.
         InsertMenu(hMenu, 4 + j, MF_BYPOSITION | MF_STRING, MRU_START_ID + j,
                    szMRU[i]);

         // Increment our actual MRU count.
         j++;
      }
   }
}

//******************************************************************************
void RemoveFileFromMRU(LPCTSTR szFile) {

   TCHAR szMRU[MRU_MAX_FILE][_MAX_PATH + 4], szOption[8];
   int   i, j;
   BOOL  fFound;

   //---------------------------------------------------------------------------
   // We first look for this file in our global MRU stored in the registry.  We
   // read the current MRU list from the registry, and then write it back while
   // removing all occurrances of the file specified.
   //---------------------------------------------------------------------------

   // Read all our current MRUs from the registry.
   for (i = 0, fFound = FALSE; i < MRU_MAX_FILE; i++) {

      // Build option name for current MRU and read from registry.
      _stprintf(szOption, TEXT("MRU%d"), i+1);
      GetOptionString(szOption, TEXT(""), &szMRU[i][3], sizeof(TCHAR) * _MAX_PATH);

      // Check for a match.
      if (!_tcsicmp(szFile, &szMRU[i][3])) {
         szMRU[i][3] = TEXT('\0');
         fFound = TRUE;
      }
   }

   // Only write the MRU back to the registry if we found a file to remove.
   if (fFound) {

      // Write the updated MRU list back to the registry.
      for (i = 0, j = 0; i < MRU_MAX_FILE; i++) {

         // If this MRU still exists, then add it.
         if (szMRU[i][3]) {

            // Build option name for current MRU and write to registry.
            _stprintf(szOption, TEXT("MRU%d"), ++j);
            WriteOptionString(szOption, &szMRU[i][3]);
         }
      }

      // If our list got smaller, clear the unused items in the registry.
      while (j++ < MRU_MAX_FILE) {
         _stprintf(szOption, TEXT("MRU%d"), j);
         WriteOptionString(szOption, TEXT(""));
      }
   }

   //---------------------------------------------------------------------------
   // We next thing we do is look for this file in our local MRU stored in our
   // File menu.  We read the current MRU list from the menu, and then write it
   // back while removing all occurrances of the file specified.
   //---------------------------------------------------------------------------

   // Get our menu handle.
#ifdef _WIN32_WCE
   HMENU hMenu = GetSubMenu(CommandBar_GetMenu(g_hWndCmdBar, 0), 0);
#else
   HMENU hMenu = GetSubMenu(GetMenu(g_hWndMain), 0);
#endif

   // Read all our current MRUs from our File Menu.
   for (i = 0, fFound = FALSE; i < MRU_MAX_FILE; i++) {

      // Query our file Menu for a MRU file.
      if (!GetMenuString(hMenu, MRU_START_ID + i, szMRU[i], countof(szMRU[0]),
          MF_BYCOMMAND))
      {
         szMRU[i][3] = TEXT('\0');
      }

      // Check for a match.
      if (!_tcsicmp(szFile, &szMRU[i][3])) {
         szMRU[i][3] = TEXT('\0');
         fFound = TRUE;
      }
   }

   // Only update menu if we found a file to remove.
   if (fFound) {

      // Clear out our menu's MRU list.
      for (i = MRU_START_ID; i < (MRU_START_ID + MRU_MAX_FILE); i++) {
         DeleteMenu(hMenu, i, MF_BYCOMMAND);
      }

      // Write the rest of our MRU list back to the menu.
      for (i = 0, j = 0; i < MRU_MAX_FILE; i++) {

         // If this MRU still exists, then add it.
         if (szMRU[i][3]) {

            // Build the accelerator prefix for this menu item.
            szMRU[i][0] = TEXT('&');
            szMRU[i][1] = TEXT('1') + j;
            szMRU[i][2] = TEXT(' ');

            // Add the item to our menu.
            InsertMenu(hMenu, 4 + j, MF_BYPOSITION | MF_STRING, MRU_START_ID + j,
                       szMRU[i]);

            // Increment our actual MRU count.
            j++;
         }
      }
   }
}

//******************************************************************************
void ActivateMRU(UINT uIDItem) {
   TCHAR szFile[_MAX_PATH + 4];

   // Get our menu handle.
#ifdef _WIN32_WCE
   HMENU hMenu = GetSubMenu(CommandBar_GetMenu(g_hWndCmdBar, 0), 0);
#else
   HMENU hMenu = GetSubMenu(GetMenu(g_hWndMain), 0);
#endif

   // Query our menu for the selected MRU.
   if (GetMenuString(hMenu, uIDItem, szFile, countof(szFile), MF_BYCOMMAND)) {

      // Move past 3 character accelerator prefix and open the file.
      ReadZipFileList(&szFile[3]);
   }
}


//******************************************************************************
//***** Open Zip File Functions
//******************************************************************************

void ReadZipFileList(LPCTSTR wszPath) {

   // Show wait cursor.
   HCURSOR hCur = SetCursor(LoadCursor(NULL, IDC_WAIT));

   TSTRTOMBS(g_szZipFile, wszPath, countof(g_szZipFile));

   // Update our banner to show that we are loading.
   g_fLoading = TRUE;
   DrawBanner(NULL);

   // Update our caption to show that we are loading.
   SetCaptionText(TEXT("Loading"));

   // Clear our list view.
   ListView_DeleteAllItems(g_hWndList);

   // Ghost all our Unzip related menu items.
   EnableAllMenuItems(IDM_FILE_PROPERTIES,    FALSE);
   EnableAllMenuItems(IDM_ACTION_EXTRACT,     FALSE);
   EnableAllMenuItems(IDM_ACTION_EXTRACT_ALL, FALSE);
   EnableAllMenuItems(IDM_ACTION_TEST,        FALSE);
   EnableAllMenuItems(IDM_ACTION_TEST_ALL,    FALSE);
   EnableAllMenuItems(IDM_ACTION_VIEW,        FALSE);
   EnableAllMenuItems(IDM_ACTION_SELECT_ALL,  FALSE);
   EnableAllMenuItems(IDM_VIEW_COMMENT,       FALSE);

   // Let Info-ZIP and our callbacks do the work.
   SendMessage(g_hWndList, WM_SETREDRAW, FALSE, 0);
   int result = DoListFiles(g_szZipFile);
   SendMessage(g_hWndList, WM_SETREDRAW, TRUE, 0);

   // Restore/remove cursor.
   SetCursor(hCur);

   // Update our column widths
   ResizeColumns();

   if ((result == PK_OK) || (result == PK_WARN)) {

      // Sort the items by name.
      Sort(0, TRUE);

      // Update this file to our MRU list and menu.
      AddFileToMRU(g_szZipFile);

      // Enabled the comment button if the zip file has a comment.
      if (lpUserFunctions->cchComment) {
         EnableAllMenuItems(IDM_VIEW_COMMENT, TRUE);
      }

      // Update other items that are related to having a Zip file loaded.
      EnableAllMenuItems(IDM_ACTION_EXTRACT_ALL, TRUE);
      EnableAllMenuItems(IDM_ACTION_TEST_ALL,    TRUE);
      EnableAllMenuItems(IDM_ACTION_SELECT_ALL,  TRUE);

   } else {

      // Make sure we didn't partially load and added a few files.
      ListView_DeleteAllItems(g_hWndList);

      // If the file itself is bad or missing, then remove it from our MRU.
      if ((result == PK_ERR) || (result == PK_BADERR) || (result == PK_NOZIP) ||
          (result == PK_FIND) || (result == PK_EOF))
      {
         RemoveFileFromMRU(wszPath);
      }

      // Display an error.
      TCHAR szError[_MAX_PATH + 128];
      _stprintf(szError, TEXT("Failure loading \"%s\".\n\n"), wszPath);
      _tcscat(szError, GetZipErrorString(result));
      MessageBox(g_hWndMain, szError, g_szAppName, MB_OK | MB_ICONERROR);

      // Clear our file status.
      *g_szZipFile = '\0';
   }

   // Update our caption to show that we are done loading.
   SetCaptionText(NULL);

   // Update our banner to show that we are done loading.
   g_fLoading = FALSE;
   DrawBanner(NULL);
}


//******************************************************************************
//***** Zip File Properties Dialog Functions
//******************************************************************************

BOOL CALLBACK DlgProcProperties(HWND hDlg, UINT uMsg, WPARAM wParam, LPARAM lParam) {

   switch (uMsg) {

      case WM_INITDIALOG: {

         // Add "General" and "Comments" tabs to tab control.  We are using a
         // poor man's version of a property sheet.  We display our 2 pages
         // by showing and hiding controls as necessary.  For our purposes,
         // this is much easier than dealing with separate property pages.

         TC_ITEM tci;
         tci.mask = TCIF_TEXT;
         tci.pszText = TEXT("General");
         TabCtrl_InsertItem(GetDlgItem(hDlg, IDC_TAB), 0, &tci);
         tci.pszText = TEXT("Comment");
         TabCtrl_InsertItem(GetDlgItem(hDlg, IDC_TAB), 1, &tci);

#ifdef _WIN32_WCE
         // Add "Ok" button to caption bar.
         SetWindowLong(hDlg, GWL_EXSTYLE, WS_EX_CAPTIONOKBTN |
                       GetWindowLong(hDlg, GWL_EXSTYLE));
#endif
         // Center us over our parent.
         CenterWindow(hDlg);

         int    directory = -1, readOnly = -1, archive = -1, hidden = -1;
         int    system = -1, encrypted = -1;
         int    year = -1, month = -1, day = -1, hour = -1, minute = -1, pm = -1;
         zusz_t uzSize = 0, uzCompressedSize = 0;
         LPCSTR szPath = NULL, szMethod = NULL, szComment = NULL;
         DWORD  dwCRC = 0, dwCount = 0, dwCommentCount = 0;
         TCHAR  szBuffer[MAX_PATH];

         // Loop through all selected items.
         LV_ITEM lvi;
         ZeroMemory(&lvi, sizeof(lvi));
         lvi.mask = LVIF_PARAM;
         lvi.iItem = -1;
         while ((lvi.iItem = ListView_GetNextItem(g_hWndList, lvi.iItem, LVNI_SELECTED)) != -1) {

            // Get the FILE_NODE for the selected item.
            ListView_GetItem(g_hWndList, &lvi);
            FILE_NODE *pFile = (FILE_NODE*)lvi.lParam;

            // Merge this file's attributes into our accumulative attributes.
            MergeValues(&directory, (pFile->dwAttributes & FILE_ATTRIBUTE_DIRECTORY)  != 0);
            MergeValues(&readOnly,  (pFile->dwAttributes & FILE_ATTRIBUTE_READONLY)   != 0);
            MergeValues(&archive,   (pFile->dwAttributes & FILE_ATTRIBUTE_ARCHIVE)    != 0);
            MergeValues(&hidden,    (pFile->dwAttributes & FILE_ATTRIBUTE_HIDDEN)     != 0);
            MergeValues(&system,    (pFile->dwAttributes & FILE_ATTRIBUTE_SYSTEM)     != 0);
            MergeValues(&encrypted, (pFile->dwAttributes & ZFILE_ATTRIBUTE_ENCRYPTED) != 0);

            // Merge this file's date/time into our accumulative date/time.
            int curHour = (pFile->dwModified >> 6) & 0x001F;
            MergeValues(&year,   (pFile->dwModified >> 20) & 0x0FFF);
            MergeValues(&month,  (pFile->dwModified >> 16) & 0x000F);
            MergeValues(&day,    (pFile->dwModified >> 11) & 0x001F);
            MergeValues(&hour,   (curHour % 12) ? (curHour % 12) : 12);
            MergeValues(&minute, pFile->dwModified & 0x003F);
            MergeValues(&pm,     curHour >= 12);

            // Store this file's name.
            szPath = pFile->szPathAndMethod;

            // Store this file's CRC.
            dwCRC = pFile->dwCRC;

            // Add the size and compressed size to our accumulative sizes.
            uzSize += pFile->uzSize;
            uzCompressedSize += pFile->uzCompressedSize;

            // Merge in our compression method.
            LPCSTR szCurMethod = pFile->szPathAndMethod + strlen(pFile->szPathAndMethod) + 1;
            if ((szMethod == NULL) || !strcmp(szMethod, szCurMethod)) {
               szMethod = szCurMethod;
            } else {
               szMethod = "Multiple Methods";
            }

            // Increment our file count.
            dwCount++;

            // Increment our comment count if this file has a comment.
            if (pFile->szComment) {
               szComment = pFile->szComment;
               dwCommentCount++;
            }
         };

         if (dwCount > 1) {

            // If multiple items selected, then display a selected count string
            // in place of the file name.
            _stprintf(szBuffer, TEXT("%u items selected."), dwCount);
            SetDlgItemText(hDlg, IDC_FILE, szBuffer);

            // Display "Multiple" for CRC if multiple items selected.
            SetDlgItemText(hDlg, IDC_CRC, TEXT("Multiple CRCs"));

         } else {

            // Set the file name text for the single item selected.
            MBSTOTSTR(szBuffer, szPath, countof(szBuffer));
            ForwardSlashesToBackSlashes(szBuffer);
            SetDlgItemText(hDlg, IDC_FILE, szBuffer);

            // Set the CRC text for the single item selected.
            _stprintf(szBuffer, TEXT("0x%08X"), dwCRC);
            SetDlgItemText(hDlg, IDC_CRC, szBuffer);
         }

         // Set the Size tally text.
         FormatValue(szBuffer, uzSize);
         _tcscat(szBuffer, (dwCount > 1) ? TEXT(" bytes total") : TEXT(" bytes"));
         SetDlgItemText(hDlg, IDC_FILE_SIZE, szBuffer);

         // Set the Compressed Size tally text.
         FormatValue(szBuffer, uzCompressedSize);
         _tcscat(szBuffer, (dwCount > 1) ? TEXT(" bytes total") : TEXT(" bytes"));
         SetDlgItemText(hDlg, IDC_COMPRESSED_SIZE, szBuffer);

         // Set the Compression Factor text.
         int factor = ratio(uzSize, uzCompressedSize);
         _stprintf(szBuffer, TEXT("%d.%d%%"), factor / 10,
                   ((factor < 0) ? -factor : factor) % 10);
         SetDlgItemText(hDlg, IDC_COMPRESSON_FACTOR, szBuffer);

         // Set the Compression Method text.
         MBSTOTSTR(szBuffer, szMethod, countof(szBuffer));
         SetDlgItemText(hDlg, IDC_COMPRESSION_METHOD, szBuffer);

         // Set the Attribute check boxes.
         CheckThreeStateBox(hDlg, IDC_DIRECTORY, directory);
         CheckThreeStateBox(hDlg, IDC_READONLY,  readOnly);
         CheckThreeStateBox(hDlg, IDC_ARCHIVE,   archive);
         CheckThreeStateBox(hDlg, IDC_HIDDEN,    hidden);
         CheckThreeStateBox(hDlg, IDC_SYSTEM,    system);
         CheckThreeStateBox(hDlg, IDC_ENCRYPTED, encrypted);

         // Build and set the Modified Date text.  The MS compiler does not
         // consider "??/" to be a valid string.  "??/" is a trigraph that is
         // turned into "\" by the preprocessor and causes grief for the compiler.
         LPTSTR psz = szBuffer;
         psz += ((month  < 0) ? _stprintf(psz, TEXT("?\?/")) :
                                _stprintf(psz, TEXT("%u/"), month));
         psz += ((day    < 0) ? _stprintf(psz, TEXT("?\?/")) :
                                _stprintf(psz, TEXT("%u/"), day));
         psz += ((year   < 0) ? _stprintf(psz, TEXT("?\? ")) :
                                _stprintf(psz, TEXT("%u "), year % 100));
         psz += ((hour   < 0) ? _stprintf(psz, TEXT("?\?:")) :
                                _stprintf(psz, TEXT("%u:"), hour));
         psz += ((minute < 0) ? _stprintf(psz, TEXT("?\? ")) :
                                _stprintf(psz, TEXT("%02u "), minute));
         psz += ((pm     < 0) ? _stprintf(psz, TEXT("?M")) :
                                _stprintf(psz, TEXT("%cM"), pm ? TEXT('P') : TEXT('A')));
         SetDlgItemText(hDlg, IDC_MODIFIED, szBuffer);

         // Store a global handle to our edit control.
         g_hWndEdit = GetDlgItem(hDlg, IDC_COMMENT);

         // Disable our edit box from being edited.
         DisableEditing(g_hWndEdit);

         // Stuff the appropriate message into the Comment edit control.
         if (dwCommentCount == 0) {
            if (dwCount == 1) {
               AddTextToEdit("This file does not have a comment.");
            } else {
               AddTextToEdit("None of the selected files have a comment.");
            }
         } else if (dwCount == 1) {
            AddTextToEdit(szComment);
         } else {
            CHAR szTemp[64];
            _stprintf(szBuffer, TEXT("%u of the selected files %s a comment."),
                      dwCommentCount, (dwCommentCount == 1)? TEXT("has") : TEXT("have"));
            TSTRTOMBS(szTemp, szBuffer, countof(szTemp));
            AddTextToEdit(szTemp);
         }
         g_hWndEdit = NULL;


         // Whooh, done with WM_INITDIALOG
         return TRUE;
      }

      case WM_NOTIFY:
         // Check to see if tab control was changed to new tab.
         if (((NMHDR*)lParam)->code == TCN_SELCHANGE) {
            HWND hWndTab     = ((NMHDR*)lParam)->hwndFrom;
            HWND hWndComment = GetDlgItem(hDlg, IDC_COMMENT);
            HWND hWnd        = GetWindow(hDlg, GW_CHILD);

            // If General tab selected, hide comment edit box and show all other controls.
            if (TabCtrl_GetCurSel(hWndTab) == 0) {
               while (hWnd) {
                  ShowWindow(hWnd, ((hWnd == hWndTab) || (hWnd != hWndComment)) ?
                             SW_SHOW : SW_HIDE);
                  hWnd = GetWindow(hWnd, GW_HWNDNEXT);
               }

            // If Comment tab selected, hide all controls except comment edit box.
            } else {
               while (hWnd) {
                  ShowWindow(hWnd, ((hWnd == hWndTab) || (hWnd == hWndComment)) ?
                             SW_SHOW : SW_HIDE);
                  hWnd = GetWindow(hWnd, GW_HWNDNEXT);
               }
            }
         }
         return FALSE;

      case WM_COMMAND:
         // Exit the dialog on OK (Enter) or CANCEL (Esc).
         if ((LOWORD(wParam) == IDOK) || (LOWORD(wParam) == IDCANCEL)) {
            EndDialog(hDlg, LOWORD(wParam));
         }
         return FALSE;
   }
   return FALSE;
}

//******************************************************************************
void MergeValues(int *p1, int p2) {
   if ((*p1 == -1) || (*p1 == p2)) {
      *p1 = p2;
   } else {
      *p1 = -2;
   }
}

//******************************************************************************
void CheckThreeStateBox(HWND hDlg, int nIDButton, int state) {
   CheckDlgButton(hDlg, nIDButton, (state == 0) ? BST_UNCHECKED :
                                   (state == 1) ? BST_CHECKED :
                                                  BST_INDETERMINATE);
}


//******************************************************************************
//***** Extract/Test Dialog Functions
//******************************************************************************

void ExtractOrTestFiles(BOOL fExtract) {

   EXTRACT_INFO ei;
   ZeroMemory(&ei, sizeof(ei));

   // Set our Extract or Test flag.
   ei.fExtract = fExtract;

   // Get the number of selected items and make sure we have at least one item.
   if ((ei.dwFileCount = ListView_GetSelectedCount(g_hWndList)) <= 0) {
      return;
   }

   // If we are not extracting/testing all, then create and buffer large enough to
   // hold the file list for all the selected files.
   if ((int)ei.dwFileCount != ListView_GetItemCount(g_hWndList)) {
      ei.szFileList = new LPSTR[ei.dwFileCount + 1];
      if (!ei.szFileList) {
         MessageBox(g_hWndMain, GetZipErrorString(PK_MEM), g_szAppName,
                    MB_ICONERROR | MB_OK);
         return;
      }
   }

   ei.dwFileCount = 0;
   ei.uzByteCount = 0;

   LV_ITEM lvi;
   ZeroMemory(&lvi, sizeof(lvi));
   lvi.mask = LVIF_PARAM;
   lvi.iItem = -1;

   // Walk through all the selected files to build our counts and set our file
   // list pointers into our FILE_NODE paths for each selected item.
   while ((lvi.iItem = ListView_GetNextItem(g_hWndList, lvi.iItem, LVNI_SELECTED)) >= 0) {
      ListView_GetItem(g_hWndList, &lvi);
      if (ei.szFileList) {
         ei.szFileList[ei.dwFileCount] = ((FILE_NODE*)lvi.lParam)->szPathAndMethod;
      }
      ei.dwFileCount++;
      ei.uzByteCount += ((FILE_NODE*)lvi.lParam)->uzSize;
   }
   if (ei.szFileList) {
      ei.szFileList[ei.dwFileCount] = NULL;
   }

   // If we are extracting, display the extract dialog to query for parameters.
   if (!fExtract || (DialogBoxParam(g_hInst, MAKEINTRESOURCE(IDD_EXTRACT), g_hWndMain,
                                    (DLGPROC)DlgProcExtractOrTest, (LPARAM)&ei) == IDOK))
   {
      // Display our progress dialog and do the extraction/test.
      DialogBoxParam(g_hInst, MAKEINTRESOURCE(IDD_EXTRACT_PROGRESS), g_hWndMain,
                     (DLGPROC)DlgProcExtractProgress, (LPARAM)&ei);
   }

   // Free our file list buffer if we created one.
   if (ei.szFileList) {
      delete[] ei.szFileList;
   }
}

//******************************************************************************
BOOL CALLBACK DlgProcExtractOrTest(HWND hDlg, UINT uMsg, WPARAM wParam, LPARAM lParam) {

   static EXTRACT_INFO *pei;
   TCHAR  szPath[_MAX_PATH];

   switch (uMsg) {

      case WM_INITDIALOG:

         // Store our extract information structure.
         pei = (EXTRACT_INFO*)lParam;

         // Load our settings.
         pei->fRestorePaths = GetOptionInt(TEXT("RestorePaths"), TRUE);
         pei->overwriteMode = (OVERWRITE_MODE)GetOptionInt(TEXT("OverwriteMode"), OM_PROMPT);

         // Load and set our path string.
         GetOptionString(TEXT("ExtractToDirectory"), TEXT("\\"), szPath, sizeof(szPath));
         SetDlgItemText(hDlg, IDC_EXTRACT_TO, szPath);

         // Set the state of all the controls.
         SetDlgItemText(hDlg, IDC_FILE_COUNT, FormatValue(szPath, pei->dwFileCount));
         SetDlgItemText(hDlg, IDC_BYTE_COUNT, FormatValue(szPath, pei->uzByteCount));
         CheckDlgButton(hDlg, IDC_RESTORE_PATHS, pei->fRestorePaths);
         CheckDlgButton(hDlg, IDC_OVERWRITE_PROMPT, pei->overwriteMode == OM_PROMPT);
         CheckDlgButton(hDlg, IDC_OVERWRITE_NEWER,  pei->overwriteMode == OM_NEWER);
         CheckDlgButton(hDlg, IDC_OVERWRITE_ALWAYS, pei->overwriteMode == OM_ALWAYS);
         CheckDlgButton(hDlg, IDC_OVERWRITE_NEVER,  pei->overwriteMode == OM_NEVER);

         // Limit our edit control to max path.
         SendDlgItemMessage(hDlg, IDC_EXTRACT_TO, EM_LIMITTEXT, sizeof(szPath) - 1, 0);

         // Center our dialog.
         CenterWindow(hDlg);
         return TRUE;

      case WM_COMMAND:
         switch (LOWORD(wParam)) {

            case IDOK:

               // Force us to read and validate the extract to directory.
               SendMessage(hDlg, WM_COMMAND, MAKELONG(IDC_EXTRACT_TO, EN_KILLFOCUS), 0);

               // Get our current path string.
               GetDlgItemText(hDlg, IDC_EXTRACT_TO, szPath, countof(szPath));

               // Verify our "extract to" path is valid.
               if (!SetExtractToDirectory(szPath)) {
                  MessageBox(hDlg, TEXT("The directory you entered is invalid or does not exist."),
                             g_szAppName, MB_ICONERROR | MB_OK);
                  SetFocus(GetDlgItem(hDlg, IDC_EXTRACT_TO));
                  return FALSE;
               }

               // Query other control values.
               pei->fRestorePaths = IsDlgButtonChecked(hDlg, IDC_RESTORE_PATHS);
               pei->overwriteMode =
                  IsDlgButtonChecked(hDlg, IDC_OVERWRITE_NEWER)  ? OM_NEWER  :
                  IsDlgButtonChecked(hDlg, IDC_OVERWRITE_ALWAYS) ? OM_ALWAYS :
                  IsDlgButtonChecked(hDlg, IDC_OVERWRITE_NEVER)  ? OM_NEVER  : OM_PROMPT;

               // Write our settings.
               WriteOptionInt(TEXT("RestorePaths"), pei->fRestorePaths);
               WriteOptionInt(TEXT("OverwriteMode"), pei->overwriteMode);
               WriteOptionString(TEXT("ExtractToDirectory"), szPath);

               // Fall through to IDCANCEL

            case IDCANCEL:
               EndDialog(hDlg, LOWORD(wParam));
               return FALSE;

            case IDC_EXTRACT_TO:

               // Make sure the path ends in a wack (\).
               if (HIWORD(wParam) == EN_KILLFOCUS) {
                  GetDlgItemText(hDlg, IDC_EXTRACT_TO, szPath, countof(szPath));
                  size_t length = _tcslen(szPath);
                  if ((length == 0) || szPath[length - 1] != TEXT('\\')) {
                     szPath[length    ] = TEXT('\\');
                     szPath[length + 1] = TEXT('\0');
                     SetDlgItemText(hDlg, IDC_EXTRACT_TO, szPath);
                  }
               }
               return FALSE;

            case IDC_BROWSE:
               GetDlgItemText(hDlg, IDC_EXTRACT_TO, szPath, countof(szPath));
               if (FolderBrowser(szPath, countof(szPath))) {
                  SetDlgItemText(hDlg, IDC_EXTRACT_TO, szPath);
               }
               return FALSE;
         }
         return FALSE;
   }
   return FALSE;
}


//******************************************************************************
//***** Folder Browsing Dialog Functions
//******************************************************************************

BOOL FolderBrowser(LPTSTR szPath, DWORD dwLength) {

#ifdef _WIN32_WCE

   // On Windows CE, we use a common save-as dialog to query the diretory.  We
   // display the dialog in this function, and then we sublass it.  Our subclass
   // functions tweaks the dialog a bit and and returns the path.

   ForwardSlashesToBackSlashes(szPath);

   TCHAR szInitialDir[_MAX_PATH];
   _tcscpy(szInitialDir, szPath);

   // Remove trailing wacks from path - The common dialog doesn't like them.
   size_t length = _tcslen(szInitialDir);
   while ((length > 0) && (szInitialDir[length - 1] == TEXT('\\'))) {
      szInitialDir[--length] = TEXT('\0');
   }

   // Set up the parameters for our save-as dialog.
   OPENFILENAME ofn;
   ZeroMemory(&ofn, sizeof(ofn));
   ofn.lStructSize     = sizeof(ofn);
   ofn.hwndOwner       = g_hWndMain;
   ofn.hInstance       = g_hInst;
   ofn.lpstrFilter     = TEXT(" \0!\0");
   ofn.nFilterIndex    = 1;
   ofn.lpstrFile       = szPath;
   ofn.nMaxFile        = dwLength;
   ofn.lpstrInitialDir = *szInitialDir ? szInitialDir : NULL;
   ofn.lpstrTitle      = TEXT("Extract To");
   ofn.Flags           = OFN_HIDEREADONLY | OFN_NOVALIDATE | OFN_NOTESTFILECREATE;

   // Post a message to our main window telling it that we are about to create
   // a save as dialog.  Our main window will receive this message after the
   // save as dialog is created.  This gives us a change to subclass the save as
   // dialog.
   PostMessage(g_hWndMain, WM_PRIVATE, MSG_SUBCLASS_DIALOG, 0);

   // Create and display the common save-as dialog.
   if (GetSaveFileName(&ofn)) {

      // If success, then remove are special "!" filename from the end.
      szPath[_tcslen(szPath) - 1] = TEXT('\0');
      return TRUE;
   }
   return FALSE;

#else // !_WIN32_WCE

   // On Windows NT, the shell provides us with a nice folder browser dialog.
   // We don't need to jump through any hoops to make it work like on Windows CE.
   // The only problem is that on VC 4.0, the libraries don't export the UNICODE
   // shell APIs because only Win95 had a shell library at the time.  The
   // following code requires headers and libs from VC 4.2 or later.

   // Set up our BROWSEINFO structure.
   BROWSEINFO bi;
   ZeroMemory(&bi, sizeof(bi));
   bi.hwndOwner = g_hWndMain;
   bi.pszDisplayName = szPath;
   bi.lpszTitle = TEXT("Extract To");
   bi.ulFlags = BIF_RETURNONLYFSDIRS;

   // Prompt user for path.
   LPITEMIDLIST piidl = SHBrowseForFolder(&bi);
   if (!piidl) {
      return FALSE;
   }

   // Build path string.
   SHGetPathFromIDList(piidl, szPath);

   // Free the PIDL returned by SHBrowseForFolder.
   LPMALLOC pMalloc = NULL;
   SHGetMalloc(&pMalloc);
   pMalloc->Free(piidl);

   // Add trailing wack if one is not present.
   size_t length = _tcslen(szPath);
   if ((length > 0) && (szPath[length - 1] != TEXT('\\'))) {
      szPath[length++] = TEXT('\\');
      szPath[length]   = TEXT('\0');
   }

   return TRUE;

#endif // _WIN32_WCE
}

//******************************************************************************
#ifdef _WIN32_WCE
BOOL CALLBACK DlgProcBrowser(HWND hDlg, UINT uMsg, WPARAM wParam, LPARAM lParam) {

   // This is our subclass of Windows CE's common save-as dialog.  We intercept
   // the messages we care about and forward everything else to the original
   // window procedure for the dialog.

   if (uMsg == WM_PRIVATE) { // wParam always equals MSG_INIT_DIALOG

      RECT rc1, rc2;

      // Get the window rectangle for the name edit control.
      HWND hWnd = GetDlgItem(hDlg, IDC_SAVE_NAME_EDIT);
      GetWindowRect(hWnd, &rc1);
      POINT pt1 = { rc1.left, rc1.top };
      ScreenToClient(hDlg, &pt1);

      // Hide all the windows we don't want.
      ShowWindow(hWnd, SW_HIDE);
      ShowWindow(GetDlgItem(hDlg, IDC_SAVE_NAME_PROMPT), SW_HIDE);
      ShowWindow(GetDlgItem(hDlg, IDC_SAVE_TYPE_PROMPT), SW_HIDE);
      ShowWindow(GetDlgItem(hDlg, IDC_SAVE_TYPE_LIST), SW_HIDE);

      // Get the window rectangle for the file list.
      hWnd = GetDlgItem(hDlg, IDC_SAVE_FILE_LIST);
      GetWindowRect(hWnd, &rc2);
      POINT pt2 = { rc2.left, rc2.top };
      ScreenToClient(hDlg, &pt2);

      // Resize the file list to fill the dialog.
      MoveWindow(hWnd, pt2.x, pt2.y, rc2.right - rc2.left, rc1.bottom - rc2.top, TRUE);

   } else if ((uMsg == WM_COMMAND) && (LOWORD(wParam) == IDOK)) {

      // Get our file list window.
      HWND hWnd = GetDlgItem(hDlg, IDC_SAVE_FILE_LIST);

      // Check to see if a directory is selected.
      if (ListView_GetNextItem(hWnd, -1, LVNI_SELECTED) >= 0) {

         // If a directory is highlighted, then we post ourself a "Ok".  The "Ok"
         // we are processing now will cause us to change into the highlighted
         // directory, and our posted "Ok" will close the dialog in that directory.
         PostMessage(hDlg, uMsg, wParam, lParam);

      } else {
         // If no directory is selected, then enter the imaginary filename "!"
         // into the name edit control and let the "Ok" end this dialog. The
         // result will be the correct path with a "\!" at the end.
         SetDlgItemText(hDlg, IDC_SAVE_NAME_EDIT, TEXT("!"));
      }
   }

   // Pass all messages to the base control's window proc.
   return CallWindowProc(g_wpSaveAsDlg, hDlg, uMsg, wParam, lParam);
}
#endif // _WIN32_WCE

//******************************************************************************
#ifdef _WIN32_WCE
void SubclassSaveAsDlg() {

   // Get our current thread ID so we can compare it to other thread IDs.
   DWORD dwThreadId = GetCurrentThreadId();

   // Get the the top window in the z-order that is a child of the desktop.
   // Dialogs are always children of the desktop on CE.  This first window
   // should be the dialog we are looking for, but we will walk the window list
   // just in case.
   HWND hWnd = GetWindow(g_hWndMain, GW_HWNDFIRST);

   // Walk the window list.
   while (hWnd) {

      // Check to see if this window was created by us and has controls from a
      // common "save as" dialog.
      if ((GetWindowThreadProcessId(hWnd, NULL) == dwThreadId) &&
           GetDlgItem(hWnd, IDC_SAVE_FILE_LIST) &&
           GetDlgItem(hWnd, IDC_SAVE_NAME_EDIT))
      {
         // We found our dialog.  Subclass it.
         g_wpSaveAsDlg = (WNDPROC)GetWindowLong(hWnd, GWL_WNDPROC);
         SetWindowLong(hWnd, GWL_WNDPROC, (LONG)DlgProcBrowser);

         // Send our new dialog a message so it can do its initialization.
         SendMessage(hWnd, WM_PRIVATE, MSG_INIT_DIALOG, 0);
      }

      // Get the next window in our window list.
      hWnd = GetWindow(hWnd, GW_HWNDNEXT);
   }
}
#endif // _WIN32_WCE


//******************************************************************************
//***** Extraction/Test/View Progress Dialog Functions
//******************************************************************************

BOOL CALLBACK DlgProcExtractProgress(HWND hDlg, UINT uMsg, WPARAM wParam, LPARAM lParam) {

   static EXTRACT_INFO *pei;
   static BOOL fComplete;
   static HWND hWndButton;
   TCHAR szBuffer[32];

   switch (uMsg) {

      case WM_INITDIALOG:

         // Globally store our handle so our worker thread can post to us.
         g_hDlgProgress = hDlg;

         // Get a pointer to our extract information structure.
         pei = (EXTRACT_INFO*)lParam;

         // Clear our complete flag.  It will be set to TRUE when done.
         fComplete = FALSE;

         // Get and store our edit control.
         g_hWndEdit = GetDlgItem(hDlg, IDC_LOG);

         // Disable our edit box from being edited.
         DisableEditing(g_hWndEdit);

         // Store a static handle for our Abort/Close button.
         hWndButton = GetDlgItem(hDlg, IDCANCEL);

#ifdef _WIN32_WCE

         // Set our No-Drag style
         SetWindowLong(hDlg, GWL_EXSTYLE, WS_EX_NODRAG |GetWindowLong(hDlg, GWL_EXSTYLE));

         RECT rc1, rc2, rcEdit;

         // Get our current client size.
         GetClientRect(hDlg, &rc1);

         // Get the window rectangle for the edit control in client coordinates.
         GetWindowRect(g_hWndEdit, &rcEdit);
         ScreenToClient(hDlg, ((POINT*)&rcEdit));
         ScreenToClient(hDlg, ((POINT*)&rcEdit) + 1);

         // Resize our dialog to be full screen (same size as parent).
         GetWindowRect(g_hWndMain, &rc2);
         MoveWindow(hDlg, rc2.left, rc2.top, rc2.right - rc2.left,
                    rc2.bottom - rc2.top + 1, FALSE);

         // Get our new client size.
         GetClientRect(hDlg, &rc2);

         // Resize our edit box to fill the client.
         MoveWindow(g_hWndEdit, rcEdit.left, rcEdit.top,
                    (rcEdit.right  - rcEdit.left) + (rc2.right  - rc1.right),
                    (rcEdit.bottom - rcEdit.top)  + (rc2.bottom - rc1.bottom),
                    FALSE);

#else
         // On NT, we just center our dialog over our parent.
         CenterWindow(hDlg);
#endif

         // Store some globals until the extract/test finishes.
         pei->hWndEditFile       = GetDlgItem(hDlg, IDC_FILE);
         pei->hWndProgFile       = GetDlgItem(hDlg, IDC_FILE_PROGRESS);
         pei->hWndProgTotal      = GetDlgItem(hDlg, IDC_TOTAL_PROGRESS);
         pei->hWndPercentage     = GetDlgItem(hDlg, IDC_PERCENTAGE);
         pei->hWndFilesProcessed = GetDlgItem(hDlg, IDC_FILES_PROCESSED);
         pei->hWndBytesProcessed = GetDlgItem(hDlg, IDC_BYTES_PROCESSED);

         if (pei->fExtract) {
            // Set our main window's caption.
            SetCaptionText(TEXT("Extracting"));

         } else {
            // Set our main window's caption.
            SetCaptionText(TEXT("Testing"));

            // Hide the current file progress for test since it never moves.
            ShowWindow(pei->hWndProgFile, SW_HIDE);
         }

         // Set the ranges on our progress bars.
         SendMessage(pei->hWndProgFile,  PBM_SETRANGE, 0,
                     MAKELPARAM(0, PROGRESS_MAX));
         SendMessage(pei->hWndProgTotal, PBM_SETRANGE, 0,
                     MAKELPARAM(0, PROGRESS_MAX));

         // Set our file and byte totals.
         SetDlgItemText(hDlg, IDC_FILES_TOTAL,
                        FormatValue(szBuffer, pei->dwFileCount));
         SetDlgItemText(hDlg, IDC_BYTES_TOTAL,
                        FormatValue(szBuffer, pei->uzByteCount));

         // Launch our Extract/Test thread and wait for WM_PRIVATE
         DoExtractOrTestFiles(g_szZipFile, pei);

         return TRUE;


      case WM_PRIVATE: // Sent with wParam equal to MSG_OPERATION_COMPLETE when
                       // test/extract is complete.

         // Check to see if the operation was a success
         if ((pei->result == PK_OK) || (pei->result == PK_WARN)) {

            // Set all our fields to their "100%" settings.
            SendMessage(pei->hWndProgFile,  PBM_SETPOS, PROGRESS_MAX, 0);
            SendMessage(pei->hWndProgTotal, PBM_SETPOS, PROGRESS_MAX, 0);
            SetWindowText(pei->hWndPercentage, TEXT("100%"));
            SetDlgItemText(hDlg, IDC_FILES_PROCESSED,
                           FormatValue(szBuffer, pei->dwFileCount));
            SetDlgItemText(hDlg, IDC_BYTES_PROCESSED,
                           FormatValue(szBuffer, pei->uzByteCount));
         }

         // Update our status text.
         SetWindowText(pei->hWndEditFile,
            (pei->result == PK_OK)      ? TEXT("Completed.  There were no warnings or errors.") :
            (pei->result == PK_WARN)    ? TEXT("Completed.  There was one or more warnings.") :
            (pei->result == PK_ABORTED) ? TEXT("Aborted.  There may be warnings or errors.") :
                                          TEXT("Completed.  There was one or more errors."));

         // Clear our global edit handle.
         g_hWndEdit = NULL;

         // Update our caption to show that we are done extracting/testing.
         SetCaptionText(NULL);

         // Change our abort button to now read "Close".
         SetWindowText(hWndButton, TEXT("&Close"));
         EnableWindow(hWndButton, TRUE);

         // Display an error dialog if an error occurred.
         if ((pei->result != PK_OK) && (pei->result != PK_WARN)) {
            MessageBox(hDlg, GetZipErrorString(pei->result),
                       g_szAppName, MB_ICONERROR | MB_OK);
         }

         // We are done.  Allow the user to close the dialog.
         fComplete = TRUE;
         return FALSE;

      case WM_COMMAND:
         switch (LOWORD(wParam)) {
            case IDCANCEL:
               // If abort is pressed, then set a flag that our worker thread
               // periodically checks to decide if it needs to bail out.
               if (!fComplete && !pei->fAbort) {
                  pei->fAbort = TRUE;
                  SetWindowText(hWndButton, TEXT("Aborting..."));
                  EnableWindow(hWndButton, FALSE);
                  return FALSE;
               }
               // fall through to IDOK

            case IDOK:
               // Don't allow dialog to close until extract/test is complete.
               if (fComplete) {
                  g_hDlgProgress = NULL;
                  EndDialog(hDlg, LOWORD(wParam));
               }
               return FALSE;
         }
         return FALSE;
   }
   return FALSE;
}

//******************************************************************************
BOOL CALLBACK DlgProcViewProgress(HWND hDlg, UINT uMsg, WPARAM wParam, LPARAM lParam) {

   static EXTRACT_INFO *pei;

   switch (uMsg) {

      case WM_INITDIALOG:

         // Globally store our handle so our worker thread can post to us.
         g_hDlgProgress = hDlg;

         // Get a pointer to our extract information structure.
         pei = (EXTRACT_INFO*)lParam;

         // Center our dialog over our parent.
         CenterWindow(hDlg);

         // Store some globals until the extract finishes.
         pei->hWndProgFile = GetDlgItem(hDlg, IDC_FILE_PROGRESS);

         // Set the ranges on our progress bar.
         SendDlgItemMessage(hDlg, IDC_FILE_PROGRESS, PBM_SETRANGE, 0,
                            MAKELPARAM(0, PROGRESS_MAX));

         // Launch our Extract thread and wait for WM_PRIVATE message.
         DoExtractOrTestFiles(g_szZipFile, pei);

         return TRUE;

      case WM_PRIVATE: // Sent with wParam equal to MSG_OPERATION_COMPLETE when
                       // test/extract is complete.

         // We are done.  Close our dialog.  Any errors will be reported by
         // OnActionView().
         g_hDlgProgress = NULL;
         EndDialog(hDlg, LOWORD(wParam));
         return FALSE;

      case WM_COMMAND:
         // If abort is pressed, then set a flag that our worker thread
         // periodically checks to decide if it needs to bail out.
         if ((LOWORD(wParam) == IDCANCEL) && !pei->fAbort) {
            pei->fAbort = TRUE;
            SetWindowText(GetDlgItem(hDlg, IDCANCEL), TEXT("Aborting..."));
            EnableWindow(GetDlgItem(hDlg, IDCANCEL), FALSE);
            return FALSE;
         }
   }

   return FALSE;
}

//******************************************************************************
void UpdateProgress(EXTRACT_INFO *pei, BOOL fFull) {

   DWORD dwFile, dwTotal, dwPercentage;
   TCHAR szBuffer[_MAX_PATH + 32];

   // Compute our file progress bar position.
   if (pei->uzBytesTotalThisFile) {
      dwFile = (DWORD)(((DWORDLONG)PROGRESS_MAX *
                        (DWORDLONG)pei->uzBytesWrittenThisFile) /
                        (DWORDLONG)pei->uzBytesTotalThisFile);
   } else {
      dwFile = PROGRESS_MAX;
   }

   // Set our file progress indicators.
   SendMessage(pei->hWndProgFile,  PBM_SETPOS, dwFile,  0);

   // If we are only updating our View Progress dialog, then we are done.
   if (!pei->hWndProgTotal) {
      return;
   }

   // Compute our total progress bar position.
   dwTotal = (DWORD)(((DWORDLONG)PROGRESS_MAX *
                      (DWORDLONG)(pei->uzBytesWrittenPreviousFiles +
                                  pei->uzBytesWrittenThisFile +
                                  pei->dwFile)) /
                      (DWORDLONG)(pei->uzByteCount +
                                  pei->dwFileCount));
   dwPercentage = dwTotal / (PROGRESS_MAX / 100);

   // Set our total progress indicators.
   SendMessage(pei->hWndProgTotal, PBM_SETPOS, dwTotal, 0);

   // Set our total percentage text.
   _stprintf(szBuffer, TEXT("%u%%"), dwPercentage);
   SetWindowText(pei->hWndPercentage, szBuffer);

   // Set our current file and byte process counts.
   FormatValue(szBuffer, pei->dwFile - 1);
   SetWindowText(pei->hWndFilesProcessed, szBuffer);
   FormatValue(szBuffer, pei->uzBytesWrittenPreviousFiles +
               pei->uzBytesWrittenThisFile);
   SetWindowText(pei->hWndBytesProcessed, szBuffer);


   if (fFull) {

      // Build our message string.
      _tcscpy(szBuffer, pei->fExtract ? TEXT("Extract") : TEXT("Test"));
      size_t preflen = _tcslen(szBuffer);
      MBSTOTSTR(szBuffer+preflen, pei->szFile,countof(szBuffer)-preflen);

      // Change all forward slashes to back slashes in the buffer.
      ForwardSlashesToBackSlashes(szBuffer);

      // Update the file name in our dialog.
      SetWindowText(pei->hWndEditFile, szBuffer);
   }
}


//******************************************************************************
//***** Replace File Dialog Functions
//******************************************************************************

int PromptToReplace(LPCSTR szPath) {

   // Check to see if we are extracting for view only.
   if (g_fViewing) {

      // Build prompt.
      TCHAR szMessage[_MAX_PATH + 128];
      _stprintf(szMessage,
#ifdef UNICODE
         TEXT("A file named \"%S\" has already been extracted for viewing.  ")
#else
         TEXT("A file named \"%s\" has already been extracted for viewing.  ")
#endif
         TEXT("That file might be opened and locked for viewing by another application.\n\n")
         TEXT("Would you like to attempt to overwrite it with the new file?"),
         GetFileFromPath(szPath));

      // Display prompt.
      if (IDYES == MessageBox(g_hDlgProgress, szMessage, g_szAppName,
                              MB_ICONWARNING | MB_YESNO))
      {
         // Tell Info-ZIP to continue with extraction.
         return IDM_REPLACE_YES;
      }

      // Remember that the file was skipped and tell Info-ZIP to abort extraction.
      g_fSkipped = TRUE;
      return IDM_REPLACE_NO;
   }

   // Otherwise, do the normal replace prompt dialog.
   return DialogBoxParam(g_hInst, MAKEINTRESOURCE(IDD_REPLACE), g_hWndMain,
                         (DLGPROC)DlgProcReplace, (LPARAM)szPath);
}

//******************************************************************************
BOOL CALLBACK DlgProcReplace(HWND hDlg, UINT uMsg, WPARAM wParam, LPARAM lParam) {
   TCHAR szMessage[_MAX_PATH + 32];

   switch (uMsg) {

      case WM_INITDIALOG:

         // Play the question tone to alert the user.
         MessageBeep(MB_ICONQUESTION);

         // Display a message with the file name.
#ifdef UNICODE
         _stprintf(szMessage, TEXT("\"%S\" already exists."), (LPCSTR)lParam);
#else
         _stprintf(szMessage, TEXT("\"%s\" already exists."), (LPCSTR)lParam);
#endif

         // Change all forward slashes to back slashes in the buffer.
         ForwardSlashesToBackSlashes(szMessage);

         // Display the file string.
         SetDlgItemText(hDlg, IDC_FILE, szMessage);

         // Center our dialog over our parent.
         CenterWindow(hDlg);
         return TRUE;

      case WM_COMMAND:
         switch (LOWORD(wParam)) {

            case IDCANCEL:
            case IDOK:
               EndDialog(hDlg, IDM_REPLACE_NO);
               break;

            case IDM_REPLACE_ALL:
            case IDM_REPLACE_NONE:
            case IDM_REPLACE_YES:
            case IDM_REPLACE_NO:
               EndDialog(hDlg, wParam);
               break;
         }
         return FALSE;
   }
   return FALSE;
}


//******************************************************************************
//***** Password Dialog Functions
//******************************************************************************

#if CRYPT

BOOL CALLBACK DlgProcPassword(HWND hDlg, UINT uMsg, WPARAM wParam, LPARAM lParam) {

   // Return Values:
   //    IZ_PW_ENTERED    got some PWD string, use/try it
   //    IZ_PW_CANCEL     no password available (for this entry)
   //    IZ_PW_CANCELALL  no password, skip any further PWD request
   //    IZ_PW_ERROR      failure (no mem, no tty, ...)

   static DECRYPT_INFO *pdi;
   TCHAR szMessage[_MAX_PATH + 32];

   switch (uMsg) {

      case WM_INITDIALOG:

         // Play the question tone to alert the user.
         MessageBeep(MB_ICONQUESTION);

#ifdef _WIN32_WCE
         // Add "Ok" button to caption bar.
         SetWindowLong(hDlg, GWL_EXSTYLE, WS_EX_CAPTIONOKBTN |
                       GetWindowLong(hDlg, GWL_EXSTYLE));
#endif

         // Store our decrypt information structure.
         pdi = (DECRYPT_INFO*)lParam;

         // Display a message with the file name.
#ifdef UNICODE
         _stprintf(szMessage, TEXT("\"%S\" is encrypted."), pdi->szFile);
#else
         _stprintf(szMessage, TEXT("\"%s\" is encrypted."), pdi->szFile);
#endif

         // Change all forward slashes to back slashes in the buffer.
         ForwardSlashesToBackSlashes(szMessage);

         // Display the message with the file name.
         SetDlgItemText(hDlg, IDC_FILE, szMessage);

         // Display the appropriate prompt.
         if (pdi->retry) {
            _stprintf(szMessage, TEXT("Password was incorrect. Please re-enter (%d/%d)."),
                     MAX_PASSWORD_RETRIES - pdi->retry + 2, MAX_PASSWORD_RETRIES + 1);
            SetDlgItemText(hDlg, IDC_PROMPT, szMessage);
         } else {
            SetDlgItemText(hDlg, IDC_PROMPT, TEXT("Please enter the password."));
         }

         // Limit the password to the size of the password buffer we have been given.
         SendDlgItemMessage(hDlg, IDC_PASSWORD, EM_LIMITTEXT, pdi->nSize - 1, 0);

         // Center our dialog over our parent.
         CenterWindow(hDlg);
         return TRUE;

      case WM_COMMAND:
         switch (LOWORD(wParam)) {

            case IDOK:

               // Store the password in our return password buffer.
               GetDlgItemText(hDlg, IDC_PASSWORD, szMessage, countof(szMessage));
               TSTRTOMBS(pdi->szPassword, szMessage, pdi->nSize);
               EndDialog(hDlg, IZ_PW_ENTERED);
               return FALSE;

            case IDCANCEL:
               g_fSkipped = TRUE;
               EndDialog(hDlg, IZ_PW_CANCEL);
               return FALSE;

            case IDC_SKIP_ALL:
               g_fSkipped = TRUE;
               EndDialog(hDlg, IZ_PW_CANCELALL);
               return FALSE;
         }
         return FALSE;
   }
   return FALSE;
}

#endif // CRYPT

//******************************************************************************
//***** View Association Dialog Functions
//******************************************************************************

BOOL CALLBACK DlgProcViewAssociation(HWND hDlg, UINT uMsg, WPARAM wParam, LPARAM lParam) {

   static LPTSTR szApp;

   switch (uMsg) {

      case WM_INITDIALOG:
         // Store the path buffer for our application.
         szApp = (LPTSTR)lParam;

         // Read our default viewer from the registry.
#ifdef _WIN32_WCE
         GetOptionString(TEXT("FileViewer"), TEXT("\\Windows\\PWord.exe"),
                         szApp, sizeof(TCHAR) * _MAX_PATH);
#else
         GetOptionString(TEXT("FileViewer"), TEXT("notepad.exe"),
                         szApp, sizeof(TCHAR) * _MAX_PATH);
#endif

         // Limit our edit control to our buffer size.
         SendDlgItemMessage(hDlg, IDC_PATH, EM_LIMITTEXT, _MAX_PATH - 1, 0);

         // Set our path string in our dialog.
         SetDlgItemText(hDlg, IDC_PATH, szApp);

         // Center our dialog over our parent.
         CenterWindow(hDlg);
         return TRUE;

      case WM_COMMAND:
         switch (LOWORD(wParam)) {

            case IDOK:
               // Get the text currently in the path edit box and store it.
               GetDlgItemText(hDlg, IDC_PATH, szApp, _MAX_PATH);
               WriteOptionString(TEXT("FileViewer"), szApp);
               // Fall through

            case IDCANCEL:
               EndDialog(hDlg, LOWORD(wParam));
               break;

            case IDC_BROWSE:
               // Get the text currently in the path edit box.
               GetDlgItemText(hDlg, IDC_PATH, szApp, _MAX_PATH);

               // Get the direcory from the path text.
               ForwardSlashesToBackSlashes(szApp);
               TCHAR szInitialDir[_MAX_PATH], *szFile;
               _tcscpy(szInitialDir, szApp);
               if (szFile = _tcsrchr(szInitialDir, TEXT('\\'))) {
                  *szFile = TEXT('\0');
               }

               // Prepare to display browse dialog.
               OPENFILENAME ofn;
               ZeroMemory(&ofn, sizeof(ofn));
               ofn.lStructSize     = sizeof(ofn);
               ofn.hwndOwner       = hDlg;
               ofn.hInstance       = g_hInst;
               ofn.lpstrFilter     = TEXT("Programs (*.exe)\0*.exe\0All Files (*.*)\0*.*\0");
               ofn.nFilterIndex    = 1;
               ofn.lpstrFile       = szApp;
               ofn.nMaxFile        = _MAX_PATH;
               ofn.lpstrInitialDir = szInitialDir;
               ofn.lpstrTitle      = TEXT("Open With...");
               ofn.Flags           = OFN_FILEMUSTEXIST | OFN_HIDEREADONLY | OFN_PATHMUSTEXIST;
               ofn.lpstrDefExt     = TEXT("exe");

               // Display the browse dialog and update our path edit box if neccessary.
               if (GetOpenFileName(&ofn)) {
                  SetDlgItemText(hDlg, IDC_PATH, szApp);
               }
               break;
         }
         return FALSE;
   }
   return FALSE;
}


//******************************************************************************
//***** Comment Dialog Functions
//******************************************************************************

BOOL CALLBACK DlgProcComment(HWND hDlg, UINT uMsg, WPARAM wParam, LPARAM lParam) {
   RECT    rc;
   HCURSOR hCur;
   int     result;

   switch (uMsg) {

      case WM_INITDIALOG:
         // Get the handle to our edit box and store it globally.
         g_hWndEdit = GetDlgItem(hDlg, IDC_COMMENT);

         // Disable our edit box from being edited.
         DisableEditing(g_hWndEdit);

#ifdef _WIN32_WCE
         // Add "Ok" button to caption bar and make window No-Drag.
         SetWindowLong(hDlg, GWL_EXSTYLE, WS_EX_CAPTIONOKBTN | WS_EX_NODRAG |
                       GetWindowLong(hDlg, GWL_EXSTYLE));

         // On CE, we resize our dialog to be full screen (same size as parent).
         GetWindowRect(g_hWndMain, &rc);
         MoveWindow(hDlg, rc.left, rc.top, rc.right - rc.left,
                    rc.bottom - rc.top + 1, FALSE);
#else
         // On NT we just center the dialog.
         CenterWindow(hDlg);
#endif

         // Set our edit control to be the full size of our dialog.
         GetClientRect(hDlg, &rc);
         MoveWindow(g_hWndEdit, rc.left, rc.top, rc.right - rc.left, rc.bottom - rc.top, FALSE);

         // Show hour glass cursor while processing comment.
         hCur = SetCursor(LoadCursor(NULL, IDC_WAIT));

         // Let Info-ZIP and our callbacks do the work.
         result = DoGetComment(g_szZipFile);

         // Restore/remove our cursor.
         SetCursor(hCur);

         // Display an error dialog if an error occurred.
         if ((result != PK_OK) && (result != PK_WARN)) {
            MessageBox(g_hWndMain, GetZipErrorString(result), g_szAppName,
                       MB_ICONERROR | MB_OK);
         }

         // Clear our global edit box handle as we are done with it.
         g_hWndEdit = NULL;

         // Return FALSE to prevent edit box from gaining focus and showing highlight.
         return FALSE;

      case WM_COMMAND:
         if ((LOWORD(wParam) == IDOK) || (LOWORD(wParam) == IDCANCEL)) {
            EndDialog(hDlg, LOWORD(wParam));
         }
         return FALSE;
   }
   return FALSE;
}


//******************************************************************************
//***** About Dialog Functions
//******************************************************************************

BOOL CALLBACK DlgProcAbout(HWND hDlg, UINT uMsg, WPARAM wParam, LPARAM lParam) {

   switch (uMsg) {

      case WM_INITDIALOG:

#ifdef _WIN32_WCE
         // Add "Ok" button to caption bar.
         SetWindowLong(hDlg, GWL_EXSTYLE, WS_EX_CAPTIONOKBTN |
                       GetWindowLong(hDlg, GWL_EXSTYLE));
#endif

         // Fill in a few static members.
         // (For VER_FULLVERSION_STR and VER_COMMENT_STR, the TEXT() macro is
         //  not applicable, because they are defined as a set of concatenated
         //  string constants. These strings need to be converted to UNICODE
         //  at runtime, sigh.)
         TCHAR szBuffer[128];
         SetDlgItemText(hDlg, IDC_PRODUCT, TEXT(VER_PRODUCT_STR));
#ifdef UNICODE
         _stprintf(szBuffer, TEXT("Freeware Version %S"), VER_FULLVERSION_STR);
#else
         _stprintf(szBuffer, TEXT("Freeware Version %s"), VER_FULLVERSION_STR);
#endif
         SetDlgItemText(hDlg, IDC_VERSION, szBuffer);
         _stprintf(szBuffer, TEXT("Developed by %s"), TEXT(VER_DEVELOPER_STR));
         SetDlgItemText(hDlg, IDC_DEVELOPER, szBuffer);
         SetDlgItemText(hDlg, IDC_COPYRIGHT, TEXT(VER_COPYRIGHT_STR));
#ifdef UNICODE
         _stprintf(szBuffer, TEXT("%S"), VER_COMMENT_STR);
         SetDlgItemText(hDlg, IDC_COMMENT, szBuffer);
#else
         SetDlgItemText(hDlg, IDC_COMMENT, VER_COMMENT_STR);
#endif

         // Center the dialog over our parent.
         CenterWindow(hDlg);
         return TRUE;

      case WM_COMMAND:
         if ((LOWORD(wParam) == IDOK) || (LOWORD(wParam) == IDCANCEL)) {
            EndDialog(hDlg, 0);
         }
         return FALSE;
   }
   return FALSE;
}
