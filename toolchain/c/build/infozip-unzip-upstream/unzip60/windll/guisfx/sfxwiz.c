/*
  Copyright (c) 1990-2009 Info-ZIP.  All rights reserved.

  See the accompanying file LICENSE, version 2009-Jan-02 or later
  (the contents of which are also included in unzip.h) for terms of use.
  If, for some reason, all these files are missing, the Info-ZIP license
  also may be found at:  ftp://ftp.info-zip.org/pub/infozip/license.html
*/

/* Tell Microsoft Visual C++ 2005 (and newer) to leave us alone
 * and let us use standard C functions the way we're supposed to.
 * (These preprocessor symbols must appear before the first system
 *  header include.)
 */
#if defined(_MSC_VER) && (_MSC_VER >= 1400)
#  ifndef _CRT_SECURE_NO_WARNINGS
#    define _CRT_SECURE_NO_WARNINGS
#  endif
#endif

#include <windows.h>
#include <stdio.h>
#ifdef WIN32
#  if defined(__LCC__)
#    include <string.h>
#    include <commdlg.h>
#    include <dlgs.h>
#  endif
   #include <shlobj.h>
#else
   #include <mem.h>
   #include <stdlib.h>
   #include <dir.h>
   #include <dlgs.h>
   #include <ctype.h>
   #include <commdlg.h>
   #include <string.h>
#endif
#include "dialog.h"
#ifndef UzpMatch
#define UzpMatch match
#endif
#include "../structs.h"
#include "../decs.h"

LPUSERFUNCTIONS lpUserFunctions;
HANDLE hUF       = (HANDLE)NULL;
LPDCL lpDCL      = NULL;
HANDLE hZUF      = (HANDLE)NULL;
HANDLE hDCL      = (HANDLE)NULL;

BOOL fDoAll = FALSE;

char ** argv;

HINSTANCE hInst;
HWND hWnd;

#ifndef TCHAR
#define TCHAR char
#endif

#ifndef _MAX_PATH
#define _MAX_PATH 260
#endif

int WINAPI password(LPSTR p, int n, LPCSTR m, LPCSTR name);
int WINAPI DisplayBuf(TCHAR far *buf, unsigned long size);
int WINAPI GetReplaceDlgRetVal(LPSTR filename, unsigned fnbufsiz);
#ifdef Z_UINT8_DEFINED
void WINAPI ReceiveDllMessage(z_uint8 ucsize, z_uint8 csiz,
    unsigned cfactor, unsigned mo, unsigned dy, unsigned yr, unsigned hh,
    unsigned mm, TCHAR c, LPCSTR filename, LPCSTR methbuf, unsigned long crc,
    TCHAR fCrypt);
#else
void WINAPI ReceiveDllMessage(unsigned long ucsize, unsigned long csiz,
    unsigned cfactor, unsigned mo, unsigned dy, unsigned yr, unsigned hh,
    unsigned mm, TCHAR c, LPCSTR filename, LPCSTR methbuf, unsigned long crc,
    TCHAR fCrypt);
#endif
void WINAPI ReceiveDllMessage_NO_INT64(unsigned long ucsize_low,
    unsigned long ucsize_high, unsigned long csiz_low, unsigned long csiz_high,
    unsigned cfactor, unsigned mo, unsigned dy, unsigned yr, unsigned hh,
    unsigned mm, TCHAR c, LPCSTR filename, LPCSTR methbuf, unsigned long crc,
    TCHAR fCrypt);

char szAppName[_MAX_PATH];
char szTarget[_MAX_PATH];
char szThisApp[_MAX_PATH];
int iReturn;

DLGPROC fpProc;

#ifndef MAX_PATH
   #define MAX_PATH    _MAX_PATH            // maximum path =length
#endif
#define TRUE        1                       // true value
#define FALSE       0                       // false value

TCHAR   zfn[MAX_PATH],                      // zip filename and path
        szHomeDir[MAX_PATH];                // Original directory

/****************************************************************************

    FUNCTION: Replace(HWND, WORD, WPARAM, LPARAM)

    PURPOSE:  Processes messages for "Replace" dialog box

    MESSAGES:

    WM_INITDIALOG - initialize dialog box
    WM_COMMAND    - Input received

****************************************************************************/

BOOL WINAPI ReplaceProc(HWND hReplaceDlg, WORD wMessage,
                        WPARAM wParam, LPARAM lParam)
{
    static char __far *lpsz;
    TCHAR szTemp[MAX_PATH];

    switch (wMessage)
    {
    case WM_INITDIALOG:
        lpsz = (char __far *)lParam;
        wsprintf(szTemp, "Replace %s ?", (LPSTR)lpsz);
        SetDlgItemText(hReplaceDlg, IDM_REPLACE_TEXT, szTemp);
        return TRUE;

    case WM_COMMAND:
        switch (LOWORD(wParam))
        {
        case IDCANCEL:              /* ESC key      */
        case IDOK:                  /* Enter key    */
            EndDialog(hReplaceDlg, IDM_REPLACE_NO);
            break;
        case IDM_REPLACE_ALL:
            fDoAll = TRUE;
        case IDM_REPLACE_NONE:
        case IDM_REPLACE_YES:
        case IDM_REPLACE_NO:
            EndDialog(hReplaceDlg, wParam);
            break;
        }
        return TRUE;
    }
    return FALSE;
}

/****************************************************************************

    FUNCTION: GetDirProc(HWND, unsigned, WPARAM, LPARAM)

    PURPOSE:  Processes messages for "Set Reference Dir Procedure for
              Update Archive" dialog box

    MESSAGES:

    WM_INITDIALOG - initialize dialog box
    WM_COMMAND    - Input received

****************************************************************************/
#ifdef WIN32
BOOL WINAPI
GetDirProc(HWND hDlg, WORD wMessage, WPARAM wParam, LPARAM lParam)
{
   switch (wMessage) {
   case WM_INITDIALOG:
/*
Common control identifiers for GetOpenFileName and GetSaveFileName
Control identifier   Control Description
cmb2                 Drop-down combo box that displays the current drive
                     or folder, and that allows the user to select a
                     drive or folder to open
stc4                 Label for the cmb2 combo box
lst1                 List box that displays the contents of the current drive or folder
stc1                 Label for the lst1 list box
edt1                 Edit control that displays the name of the current file, or in which the user can type the name of the file to open
stc3                 Label for the edt1 edit control
cmb1                 Drop-down combo box that displays the list of file type filters
stc2                 Label for the cmb1 combo box
chx1                 The read-only check box
IDOK                 The OK command button (push button)
IDCANCEL             The Cancel command button (push button)
pshHelp              The Help command button (push button)

*/
      CommDlg_OpenSave_HideControl(GetParent(hDlg), cmb1);
      CommDlg_OpenSave_HideControl(GetParent(hDlg), stc2);
      CommDlg_OpenSave_HideControl(GetParent(hDlg), edt1);
      CommDlg_OpenSave_HideControl(GetParent(hDlg), stc3);
      CommDlg_OpenSave_SetControlText(GetParent(hDlg),
            IDOK, "Set");
      break;
   default:
         break;
   }
return DefWindowProc(hDlg, wMessage, wParam, lParam);
}
#else

#ifdef __BORLANDC__
#pragma argsused
#endif

BOOL WINAPI
GetDirProc(HWND hwndDlg, WORD wMessage, WPARAM wParam, LPARAM lParam)
{
HWND hTemp;

   switch (wMessage) {
   case WM_INITDIALOG:
      hTemp = GetDlgItem(hwndDlg, lst1);
      EnableWindow(hTemp, FALSE);
      ShowWindow(hTemp, SW_HIDE);
      hTemp = GetDlgItem(hwndDlg, edt1);
      EnableWindow(hTemp, FALSE);
      ShowWindow(hTemp, SW_HIDE);
      hTemp = GetDlgItem(hwndDlg, stc2);
      EnableWindow(hTemp, FALSE);
      ShowWindow(hTemp, SW_HIDE);
      hTemp = GetDlgItem(hwndDlg, stc3);
      EnableWindow(hTemp, FALSE);
      ShowWindow(hTemp, SW_HIDE);
      hTemp = GetDlgItem(hwndDlg, cmb1);
      EnableWindow(hTemp, FALSE);
      ShowWindow(hTemp, SW_HIDE);

      break;
   case WM_COMMAND:
      switch (LOWORD(wParam)) {
         case IDCANCEL:
            EndDialog(hwndDlg, FALSE);
            break;
         case IDOK:
            getcwd(szTarget, MAX_PATH);
            EndDialog(hwndDlg, TRUE);
            break;
         }
      default:
         break;
   }
   return FALSE;
}
#endif /* !WIN32 */

#ifdef __BORLANDC__
#pragma argsused
#endif

BOOL FAR PASCAL InitDialogProc (HWND hDlg, WORD wMsg, WORD wParam, LONG lParam) {
   BOOL fProcessed = TRUE;
   TCHAR szMessage[256];

   RECT rc;

   switch (wMsg) {
      case WM_INITDIALOG:

         hWnd = hDlg;
         SetWindowText(hDlg,(LPSTR) szAppName);
         SetDlgItemText(hDlg,ID_TARGET,(LPSTR)szTarget);

#ifdef WIN32
         GetCurrentDirectory(MAX_PATH, szHomeDir);
         SetCurrentDirectory(szTarget);
#else
         getcwd(szHomeDir, MAX_PATH);
         chdir(szTarget);
         setdisk(toupper(szTarget[0]) - 'A');
#endif
         GetWindowRect(hDlg, &rc);
         SetWindowPos(hDlg, NULL,
            (GetSystemMetrics(SM_CXSCREEN) - (rc.right - rc.left)) / 2,
            (GetSystemMetrics(SM_CYSCREEN) - (rc.bottom - rc.top)) / 3,
            0, 0, SWP_NOSIZE | SWP_NOZORDER);

         break;

      case WM_COMMAND:
         switch (wParam) {

            case ID_BROWSE :
                {
#ifndef WIN32
                FARPROC lpGetDirProc;
#endif
                char szTemp[MAX_PATH]="mike_~@~*";
                OPENFILENAME ofn;

                memset(&ofn, '\0', sizeof(OPENFILENAME)); /* init struct */
                ofn.lStructSize = sizeof(OPENFILENAME);
                ofn.hwndOwner = hWnd;
                ofn.hInstance = hInst;
                ofn.lpstrFilter = NULL;
                ofn.nFilterIndex = 1;

                ofn.lpstrFile = szTemp;
                ofn.nMaxFile = MAX_PATH;
                ofn.lpstrFileTitle = NULL;
                ofn.nMaxFileTitle = MAX_PATH; /* ignored ! */
                ofn.lpstrTitle = (LPSTR)"Set Extraction Directory";
                ofn.lpstrInitialDir = NULL;
                ofn.Flags = OFN_ENABLEHOOK |
#ifdef WIN32
                            OFN_EXPLORER|
#endif
                            OFN_HIDEREADONLY|OFN_NOVALIDATE;
#ifndef WIN32
                lpGetDirProc = MakeProcInstance((FARPROC)GetDirProc, hInst);
   #ifndef MSC
                (UINT CALLBACK *)ofn.lpfnHook = (UINT CALLBACK *)lpGetDirProc;
   #else
                ofn.lpfnHook = lpGetDirProc;
   #endif
#else
                ofn.lpfnHook = (LPOFNHOOKPROC)GetDirProc;
#endif
                ofn.lpTemplateName = "GETDIR";   /* see getfiles.dlg   */
                if (!GetOpenFileName(&ofn))
                   {
                   break;
                   }

                ofn.lpstrFile[ofn.nFileOffset-1] = '\0';
#ifdef WIN32
                SetCurrentDirectory(ofn.lpstrFile);
#else
                getcwd(szTemp, MAX_PATH);
                chdir(ofn.lpstrFile);
                setdisk(toupper(ofn.lpstrFile[0]) - 'A');
#endif
                lstrcpy(szTarget,ofn.lpstrFile);
                SetDlgItemText(hDlg,ID_TARGET,(LPSTR)szTarget);
                }
               break;

            case IDOK:
#ifdef WIN32
               GetCurrentDirectory(_MAX_PATH,szTarget);
#else
               getcwd(szTarget, _MAX_PATH);
#endif

               lpDCL->ncflag = 0;
               lpDCL->fQuiet = 0; // If not zero, no status messages will come through
               lpDCL->ntflag = 0;
               lpDCL->nvflag = 0;
               lpDCL->nzflag = 0;
               lpDCL->ndflag = 1;
               lpDCL->naflag = 0;
               lpDCL->nfflag = 0;
               lpDCL->noflag = 0;
               lpDCL->PromptToOverwrite = 1;
               lpDCL->ExtractOnlyNewer = 0;
               lpDCL->lpszZipFN = zfn;
               lpDCL->lpszExtractDir = NULL;
               iReturn = Wiz_SingleEntryUnzip(0, NULL, 0, NULL, lpDCL, lpUserFunctions);

/* external return codes for unzip library */
//#define PK_OK              0   /* no error */
//#define PK_COOL            0   /* no error */
//#define PK_WARN            1   /* warning error */
//#define PK_ERR             2   /* error in zipfile */
//#define PK_BADERR          3   /* severe error in zipfile */
//#define PK_MEM             4   /* insufficient memory (during initialization) */
//#define PK_MEM2            5   /* insufficient memory (password failure) */
//#define PK_MEM3            6   /* insufficient memory (file decompression) */
//#define PK_MEM4            7   /* insufficient memory (memory decompression) */
//#define PK_MEM5            8   /* insufficient memory (not yet used) */
//#define PK_NOZIP           9   /* zipfile not found */
//#define PK_PARAM          10   /* bad or illegal parameters specified */
//#define PK_FIND           11   /* no files found */
//#define PK_DISK           50   /* disk full */
//#define PK_EOF            51   /* unexpected EOF */

//#define IZ_CTRLC          80   /* user hit ^C to terminate */
//#define IZ_UNSUP          81   /* no files found: all unsup. compr/encrypt. */
//#define IZ_BADPWD         82   /* no files found: all had bad password */

/* return codes of password fetches (negative = user abort; positive = error) */
//#define IZ_PW_ENTERED      0   /* got some password string; use/try it */
//#define IZ_PW_CANCEL      -1   /* no password available (for this entry) */
//#define IZ_PW_CANCELALL   -2   /* no password, skip any further pwd. request */
//#define IZ_PW_ERROR        5   /* = PK_MEM2 : failure (no mem, no tty, ...) */
               switch (iReturn) {
                case PK_OK:
                     wsprintf(szMessage, "%s", "All files extracted OK");
                     break;
                case PK_ERR:
                     wsprintf(szMessage, "%s", "Warning occurred on one or more files");
                     break;
                case PK_BADERR:
                     wsprintf(szMessage, "%s", "Error in archive");
                     break;
                case PK_MEM:
                case PK_MEM2:
                case PK_MEM3:
                case PK_MEM4:
                case PK_MEM5:
                     wsprintf(szMessage, "%s", "Insufficient memory");
                     break;
                case PK_NOZIP:
                     wsprintf(szMessage, "%s", "Archive not found");
                     break;
                case PK_FIND:
                     wsprintf(szMessage, "%s", "No files found");
                     break;
                case PK_DISK:
                     wsprintf(szMessage, "%s", "Disk full");
                     break;
                case PK_EOF:
                     wsprintf(szMessage, "%s", "Unexpected end of file");
                     break;
                case IZ_UNSUP:
                     wsprintf(szMessage, "%s", "No files found: All unsupported");
                     break;
                case IZ_BADPWD:
                     wsprintf(szMessage, "%s", "No files found: Bad password");
                     break;
                default:
                     wsprintf(szMessage, "%s", "Unknown error");
                     break;
                }
               MessageBox(hDlg, szMessage, szAppName, MB_OK);
/* Uncomment line below to have SFXWix terminate automatically
   when done.
 */
//              EndDialog(hDlg, wParam);

               break;
               case IDCANCEL:
                    EndDialog(hDlg, wParam);
                    PostQuitMessage(0);
                    exit(0); // ..and then quit
                    break;
         }
         break;

      default:
         fProcessed = FALSE;
         break;
   }
   return(fProcessed);
}


#define WasCancelled(hDlg) (!IsWindowEnabled(GetDlgItem(hDlg,IDCANCEL)))

#ifdef __BORLANDC__
#pragma argsused
#endif
int WINAPI password(LPSTR p, int n, LPCSTR m, LPCSTR name)
{
TCHAR sz[MAX_PATH];
sprintf(sz, "%s is encrypted", name);
MessageBox(hWnd, sz, "Encryption not supported", MB_OK);
return IZ_PW_CANCELALL;
}

int WINAPI DisplayBuf(TCHAR far *buf, unsigned long size)
{
if ((buf[0] != '\n') && (buf[0] != '\r'))
   SetDlgItemText(hWnd, ID_STATUS, buf);
return (unsigned int) size;
}

#ifdef __BORLANDC__
#pragma argsused
#endif
int WINAPI GetReplaceDlgRetVal(LPSTR filename, unsigned fnbufsiz)
{
#ifndef WIN32
FARPROC lpfnprocReplace;
#endif
int ReplaceDlgRetVal;   /* replace dialog return value */

#ifdef WIN32
ReplaceDlgRetVal = DialogBoxParam(hInst, "Replace",
   hWnd, (DLGPROC)ReplaceProc, (DWORD)filename);
#else
lpfnprocReplace = MakeProcInstance(ReplaceProc, hInst);
ReplaceDlgRetVal = DialogBoxParam(hInst, "Replace",
   hWnd, lpfnprocReplace, (DWORD)filename);
FreeProcInstance(lpfnprocReplace);
#endif
return ReplaceDlgRetVal;
}

#ifdef __BORLANDC__
#pragma argsused
#endif
#ifdef Z_UINT8_DEFINED
void WINAPI ReceiveDllMessage(z_uint8 ucsize, z_uint8 csiz,
    unsigned cfactor, unsigned mo, unsigned dy, unsigned yr, unsigned hh,
    unsigned mm, TCHAR c, LPCSTR filename, LPCSTR methbuf, unsigned long crc,
    TCHAR fCrypt)
{
}
#else
void WINAPI ReceiveDllMessage(unsigned long ucsize, unsigned long csiz,
    unsigned cfactor, unsigned mo, unsigned dy, unsigned yr, unsigned hh,
    unsigned mm, TCHAR c, LPCSTR filename, LPCSTR methbuf, unsigned long crc,
    TCHAR fCrypt)
{
}
#endif
void WINAPI ReceiveDllMessage_NO_INT64(unsigned long ucsize_low,
   unsigned long ucsize_high, unsigned long csiz_low, unsigned long csiz_high,
   unsigned cfactor, unsigned mo, unsigned dy, unsigned yr, unsigned hh,
   unsigned mm, TCHAR c, LPCSTR filename, LPCSTR methbuf, unsigned long crc,
   TCHAR fCrypt)
{
}

#ifdef __BORLANDC__
#pragma argsused
#endif

int PASCAL WinMain (HINSTANCE hInstance, HINSTANCE hPrevInstance,
                    LPSTR lpszCmdLine, int nCmdShow)
{
TCHAR *ptr = NULL;
hInst = hInstance;

hDCL = GlobalAlloc( GPTR, (DWORD)sizeof(DCL));
if (!hDCL)
   {
   return 0;
   }
lpDCL = (LPDCL)GlobalLock(hDCL);
if (!lpDCL)
   {
   return 0;
   }

hUF = GlobalAlloc( GPTR, (DWORD)sizeof(USERFUNCTIONS));
if (!hUF)
   {
   return 0;
   }
lpUserFunctions = (LPUSERFUNCTIONS)GlobalLock(hUF);

if (!lpUserFunctions)
   {
   return 0;
   }

lpUserFunctions->password = password;
lpUserFunctions->print = DisplayBuf;
lpUserFunctions->sound = NULL;
lpUserFunctions->replace = GetReplaceDlgRetVal;
lpUserFunctions->SendApplicationMessage = ReceiveDllMessage;
lpUserFunctions->SendApplicationMessage_i32 = ReceiveDllMessage_NO_INT64;
lpUserFunctions->ServCallBk = NULL;



GetModuleFileName(hInstance,(LPSTR)szThisApp,sizeof(szThisApp));
lstrcpy(zfn, szThisApp);

ptr = strrchr(szThisApp, '\\');
if (ptr != NULL)
   {
   lstrcpy(szAppName, ptr);
   ptr[0] = '\0';
   lstrcpy(szTarget, szThisApp);

   iReturn = DialogBox(hInstance, MAKEINTRESOURCE(INITDIALOG),
         (HWND)NULL, (DLGPROC)InitDialogProc);
   DestroyWindow((HWND) INITDIALOG);

#ifdef WIN32
   SetCurrentDirectory(szHomeDir);
#else
   getcwd(szTarget, MAX_PATH);
   chdir(szHomeDir);
   setdisk(toupper(szHomeDir[0]) - 'A');
#endif
   }
PostQuitMessage(0);
return (0);
}
