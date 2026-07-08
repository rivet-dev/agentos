/*
  Copyright (c) 1990-2009 Info-ZIP.  All rights reserved.

  See the accompanying file LICENSE, version 2009-Jan-02 or later
  (the contents of which are also included in unzip.h) for terms of use.
  If, for some reason, all these files are missing, the Info-ZIP license
  also may be found at:  ftp://ftp.info-zip.org/pub/infozip/license.html
*/
/*
   This is a very simplistic example of how to load and make a call into the
   dll. This has been compiled and tested for a 32-bit console version, but
   not under 16-bit windows. However, the #ifdef's have been left in for the
   16-bit code, simply as an example.

 */

#ifndef WIN32   /* this code is currently only tested for 32-bit console */
#  define WIN32
#endif

#if defined(__WIN32__) && !defined(WIN32)
#  define WIN32
#endif

/* Tell Microsoft Visual C++ 2005 to leave us alone and
 * let us use standard C functions the way we're supposed to.
 */
#if defined(_MSC_VER) && (_MSC_VER >= 1400)
#  ifndef _CRT_SECURE_NO_WARNINGS
#    define _CRT_SECURE_NO_WARNINGS
#  endif
#  ifndef _CRT_NONSTDC_NO_WARNINGS
#    define _CRT_NONSTDC_NO_WARNINGS
#  endif
#endif

#include <stddef.h>
#include <sys/types.h>
#include <sys/stat.h>
#include <time.h>
#include <string.h>
#include "uzexampl.h"
#include "../unzvers.h"
#ifdef WIN32
#  include <winver.h>
#else
#  include <ver.h>
#endif

#ifndef _MAX_PATH
#  define _MAX_PATH 260           /* max total file or directory name path */
#endif

#ifdef WIN32
#define UNZ_DLL_NAME "UNZIP32.DLL\0"
#else
#define UNZ_DLL_NAME "UNZIP16.DLL\0"
#endif

#define DLL_WARNING "Cannot find %s."\
            " The Dll must be in the application directory, the path, "\
            "the Windows directory or the Windows System directory."
#define DLL_VERSION_WARNING "%s has the wrong version number."\
            " Insure that you have the correct dll's installed, and that "\
            "an older dll is not in your path or Windows System directory."

int hFile;              /* file handle */

LPUSERFUNCTIONS lpUserFunctions;
HANDLE hUF = (HANDLE)NULL;
LPDCL lpDCL = NULL;
HANDLE hDCL = (HANDLE)NULL;
HINSTANCE hUnzipDll;
HANDLE hZCL = (HANDLE)NULL;
#ifdef WIN32
DWORD dwPlatformId = 0xFFFFFFFF;
#endif
static ZCONST UzpVer *lpUzVersInfo = NULL;


/* Forward References */
int WINAPI DisplayBuf(LPSTR, unsigned long);
int WINAPI GetReplaceDlgRetVal(LPSTR, unsigned);
int WINAPI password(LPSTR, int, LPCSTR, LPCSTR);

ZCONST UzpVer * UZ_EXP UzpVersion  OF((void));
_DLL_UZVER pUzpVersion;
_DLL_UNZIP pWiz_SingleEntryUnzip;

static void FreeUpMemory(void);

int main(int argc, char **argv)
{
int exfc, infc;
char **exfv, **infv;
char *x_opt;
DWORD dwVerInfoSize;
DWORD dwVerHnd;
char szFullPath[_MAX_PATH];
int retcode;
#ifdef WIN32
char *ptr;
#else
HFILE hfile;
OFSTRUCT ofs;
#endif
HANDLE  hMem;         /* handle to mem alloc'ed */

if (argc < 2)   /* We must have an archive to unzip */
   {
   char *progname = strrchr(argv[0], '\\');

   if (progname != NULL)
      progname++;
   else
      {
      progname = argv[0];
      if (progname == NULL || *progname == '\0') progname = "example";
      }
   printf("usage: %s <zipfile> [entry1 [entry2 [...]]] [-x xentry1 [...]]",
          progname);
   return 0;
   }

hDCL = GlobalAlloc( GPTR, (DWORD)sizeof(DCL));
if (!hDCL)
   {
   return -1;
   }
lpDCL = (LPDCL)GlobalLock(hDCL);
if (!lpDCL)
   {
   GlobalFree(hDCL);
   return -1;
   }

hUF = GlobalAlloc( GPTR, (DWORD)sizeof(USERFUNCTIONS));
if (!hUF)
   {
   GlobalUnlock(hDCL);
   GlobalFree(hDCL);
   return -1;
   }
lpUserFunctions = (LPUSERFUNCTIONS)GlobalLock(hUF);

if (!lpUserFunctions)
   {
   GlobalFree(hUF);
   GlobalUnlock(hDCL);
   GlobalFree(hDCL);
   return -1;
   }

lpUserFunctions->password = password;
lpUserFunctions->print = DisplayBuf;
lpUserFunctions->sound = NULL;
lpUserFunctions->replace = GetReplaceDlgRetVal;
lpUserFunctions->SendApplicationMessage = ReceiveDllMessage;

/* First we go look for the unzip dll */
#ifdef WIN32
if (SearchPath(
    NULL,               /* address of search path               */
    UNZ_DLL_NAME,       /* address of filename                  */
    NULL,               /* address of extension                 */
    _MAX_PATH,           /* size, in characters, of buffer       */
    szFullPath,         /* address of buffer for found filename */
    &ptr                /* address of pointer to file component */
   ) == 0)
#else
hfile = OpenFile(UNZ_DLL_NAME,  &ofs, OF_SEARCH);
if (hfile == HFILE_ERROR)
#endif
   {
   char str[256];
   wsprintf (str, DLL_WARNING, UNZ_DLL_NAME);
   printf("%s\n", str);
   FreeUpMemory();
   return -1;
   }
#ifndef WIN32
else
   lstrcpy(szFullPath, ofs.szPathName);
_lclose(hfile);
#endif

/* Now we'll check the unzip dll version information. Note that this is
   not the same information as is returned from a call to UzpVersion()
 */
dwVerInfoSize =
    GetFileVersionInfoSize(szFullPath, &dwVerHnd);

if (dwVerInfoSize)
   {
   BOOL  fRet, fRetName;
   char str[256];
   LPSTR   lpstrVffInfo; /* Pointer to block to hold info */
   LPSTR lszVer = NULL;
   LPSTR lszVerName = NULL;
   UINT  cchVer = 0;

   /* Get a block big enough to hold the version information */
   hMem          = GlobalAlloc(GMEM_MOVEABLE, dwVerInfoSize);
   lpstrVffInfo  = GlobalLock(hMem);

   /* Get the version information */
   if (GetFileVersionInfo(szFullPath, 0L, dwVerInfoSize, lpstrVffInfo))
      {
      fRet = VerQueryValue(lpstrVffInfo,
               TEXT("\\StringFileInfo\\040904E4\\FileVersion"),
              (LPVOID)&lszVer,
              &cchVer);
      fRetName = VerQueryValue(lpstrVffInfo,
               TEXT("\\StringFileInfo\\040904E4\\CompanyName"),
               (LPVOID)&lszVerName,
               &cchVer);
      if (!fRet || !fRetName ||
         (lstrcmpi(lszVer, UNZ_DLL_VERSION) != 0) ||
         (lstrcmpi(lszVerName, IZ_COMPANY_NAME) != 0))
         {
         wsprintf (str, DLL_VERSION_WARNING, UNZ_DLL_NAME);
         printf("%s\n", str);
         GlobalUnlock(hMem);
         GlobalFree(hMem);
         FreeUpMemory();
         return -1;
         }
      }
      /* free memory */
   GlobalUnlock(hMem);
   GlobalFree(hMem);
   }
else
   {
   char str[256];
   wsprintf (str, DLL_VERSION_WARNING, UNZ_DLL_NAME);
   printf("%s\n", str);
   FreeUpMemory();
   return -1;
   }
/* Okay, now we know that the dll exists, and has the proper version
 * information in it. We can go ahead and load it.
 */
hUnzipDll = LoadLibrary(UNZ_DLL_NAME);
#ifndef WIN32
if (hUnzipDll > HINSTANCE_ERROR)
#else
if (hUnzipDll != NULL)
#endif
   {
   pUzpVersion =
     (_DLL_UZVER)GetProcAddress(hUnzipDll, "UzpVersion");
   pWiz_SingleEntryUnzip =
     (_DLL_UNZIP)GetProcAddress(hUnzipDll, "Wiz_SingleEntryUnzip");
   }
else
   {
   char str[256];
   wsprintf (str, "Could not load %s", UNZ_DLL_NAME);
   printf("%s\n", str);
   FreeUpMemory();
   return -1;
   }

/*
   Before we actually start with the extraction process, we should first
   check whether the API of the loaded dll is compatible with the API
   definition used to compile this frontend program.
 */
lpUzVersInfo = (*pUzpVersion)();

/* The UnZip WinDLL code may change quite frequently.  To be safe, we
 * require the DLL to be at least at the release level of this example
 * frontend code.
 */
#   define UZDLL_MINVERS_MAJOR          UZ_MAJORVER
#   define UZDLL_MINVERS_MINOR          UZ_MINORVER
#   define UZDLL_MINVERS_PATCHLEVEL     UZ_PATCHLEVEL
/* This UnZip DLL stub requires a DLL version of at least: */
if ( (lpUzVersInfo->unzip.major < UZDLL_MINVERS_MAJOR) ||
     ((lpUzVersInfo->unzip.major == UZDLL_MINVERS_MAJOR) &&
      ((lpUzVersInfo->unzip.minor < UZDLL_MINVERS_MINOR) ||
       ((lpUzVersInfo->unzip.minor == UZDLL_MINVERS_MINOR) &&
        (lpUzVersInfo->unzip.patchlevel < UZDLL_MINVERS_PATCHLEVEL)
       )
      )
     ) )
{
  char str[256];
  wsprintf(str, "The version %u.%u%u of the loaded UnZip DLL is too old!",
           lpUzVersInfo->unzip.major, lpUzVersInfo->unzip.minor,
           lpUzVersInfo->unzip.patchlevel);
  printf("%s\n", str);
  FreeLibrary(hUnzipDll);
  FreeUpMemory();
  return -1;
}

if (lpUzVersInfo->structlen >=
    (offsetof(UzpVer, dllapimin) + sizeof(_version_type)))
{
  if ( (lpUzVersInfo->dllapimin.major > UZ_WINAPI_COMP_MAJOR) ||
       ((lpUzVersInfo->dllapimin.major == UZ_WINAPI_COMP_MAJOR) &&
        ((lpUzVersInfo->dllapimin.minor > UZ_WINAPI_COMP_MINOR) ||
         ((lpUzVersInfo->dllapimin.minor == UZ_WINAPI_COMP_MINOR) &&
          (lpUzVersInfo->dllapimin.patchlevel > UZ_WINAPI_COMP_REVIS)
         )
        )
       ) )
  {
    char str[256];
    wsprintf(str, "Found incompatible WinDLL API version %u.%u%u, aborting!",
             lpUzVersInfo->dllapimin.major, lpUzVersInfo->dllapimin.minor,
             lpUzVersInfo->dllapimin.patchlevel);
    printf("%s\n", str);
    FreeLibrary(hUnzipDll);
    FreeUpMemory();
    return -1;
  }
}

/*
   Here is where the actual extraction process begins. First we set up the
   flags to be passed into the dll.
 */
lpDCL->StructVersID = UZ_DCL_STRUCTVER; /* version of this structure */
lpDCL->ncflag = 0;              /* write to stdout if true */
lpDCL->fQuiet = 0;              /* we want all messages
                                   1 = fewer messages,
                                   2 = no messages */
lpDCL->ntflag = 0;              /* test zip file if true */
lpDCL->nvflag = 0;              /* give a verbose listing if true */
lpDCL->nzflag = 0;              /* display zip file comment if true */
lpDCL->ndflag = 1;              /* recreate directories != 0,
                                   skip "../" if < 2 */
lpDCL->naflag = 0;              /* do not convert CR to CRLF */
lpDCL->nfflag = 0;              /* do not freshen existing files only */
lpDCL->noflag = 1;              /* over-write all files if true */
lpDCL->nZIflag = 0;             /* no ZipInfo output mode */
lpDCL->B_flag = 0;              /* do not backup existing files */
lpDCL->C_flag = 0;              /* do not match case-insensitive */
lpDCL->D_flag = 0;              /* restore all timestamps */
lpDCL->U_flag = 0;              /* do not disable UTF-8 support */
lpDCL->ExtractOnlyNewer = 0;    /* do not extract only newer */
lpDCL->SpaceToUnderscore = 0;   /* do not convert space to '_' in filenames */
lpDCL->PromptToOverwrite = 0;   /* "overwrite all" selected -> no query mode */
lpDCL->lpszZipFN = argv[1];     /* the archive name */
lpDCL->lpszExtractDir = NULL;   /* the directory to extract to.
                                   This is set to NULL if you are extracting
                                   to the current directory.
                                 */
/*
   As this is a quite short example, intended primarily to show how to
   load and call in to the dll, the command-line parameters are only
   parsed in a very simplistic way:
   We assume that the command-line parameters after the zip archive
   make up a list of file patterns:
   " [file_i1] [file_i2] ... [file_iN] [-x file_x1 [file_x2] ...]".
   We scan for an argument "-x"; all arguments in front are
   "include file patterns", all arguments after are "exclude file patterns".
   If no more arguments are given, we extract ALL files.

   In summary, the example program should be run like:
   example <archive.name> [files to include] [-x files to exclude]
   ("<...> denotes mandatory arguments, "[...]" optional arguments)
 */
x_opt = NULL;
if (argc > 2) {
  infv = &argv[2];
  for (infc = 0; infc < argc-2; infc++)
    if (!strcmp("-x", infv[infc])) {
        x_opt = infv[infc];
        infv[infc] = NULL;
        break;
    }
  exfc = argc - infc - 3;
  if (exfc > 0)
    exfv = &argv[infc+3];
  else {
    exfc = 0;
    exfv = NULL;
  }
} else {
  infc = exfc = 0;
  infv = exfv = NULL;
}
retcode = (*pWiz_SingleEntryUnzip)(infc, infv, exfc, exfv, lpDCL,
                                   lpUserFunctions);
if (x_opt) {
  infv[infc] = x_opt;
  x_opt = NULL;
}

if (retcode != 0)
   printf("Error unzipping (error/warning code %d)...\n", retcode);

FreeLibrary(hUnzipDll);
FreeUpMemory();
return retcode;
}

int WINAPI GetReplaceDlgRetVal(LPSTR filename, unsigned fnbufsiz)
{
/* This is where you will decide if you want to replace, rename etc existing
   files.
 */
return 1;
}

static void FreeUpMemory(void)
{
if (hUF)
   {
   GlobalUnlock(hUF);
   GlobalFree(hUF);
   }
if (hDCL)
   {
   GlobalUnlock(hDCL);
   GlobalFree(hDCL);
   }
}

/* This is a very stripped down version of what is done in Wiz. Essentially
   what this function is for is to do a listing of an archive contents. It
   is actually never called in this example, but a dummy procedure had to
   be put in, so this was used.
 */
#ifdef Z_UINT8_DEFINED
void WINAPI ReceiveDllMessage(z_uint8 ucsize, z_uint8 csiz,
    unsigned cfactor, unsigned mo, unsigned dy, unsigned yr,
    unsigned hh, unsigned mm, char c, LPCSTR filename,
    LPCSTR methbuf, unsigned long crc, char fCrypt)
#else
void WINAPI ReceiveDllMessage(unsigned long ucsize, unsigned long csiz,
    unsigned cfactor,
    unsigned mo, unsigned dy, unsigned yr, unsigned hh, unsigned mm,
    char c, LPCSTR filename, LPCSTR methbuf, unsigned long crc, char fCrypt)
#endif
{
char psLBEntry[_MAX_PATH];
char LongHdrStats[] =
          "%7lu  %7lu %4s  %02u-%02u-%02u  %02u:%02u  %c%s";
char CompFactorStr[] = "%c%d%%";
char CompFactor100[] = "100%%";
char szCompFactor[10];
char sgn;

if (csiz > ucsize)
   sgn = '-';
else
   sgn = ' ';
if (cfactor == 100)
   lstrcpy(szCompFactor, CompFactor100);
else
   sprintf(szCompFactor, CompFactorStr, sgn, cfactor);
   wsprintf(psLBEntry, LongHdrStats,
      ucsize, csiz, szCompFactor, mo, dy, yr, hh, mm, c, filename);

printf("%s\n", psLBEntry);
}

/* Password entry routine - see password.c in the wiz directory for how
   this is actually implemented in WiZ. If you have an encrypted file,
   this will probably give you great pain.
 */
int WINAPI password(LPSTR p, int n, LPCSTR m, LPCSTR name)
{
return 1;
}

/* Dummy "print" routine that simply outputs what is sent from the dll */
int WINAPI DisplayBuf(LPSTR buf, unsigned long size)
{
printf("%s", (char *)buf);
return (int)(unsigned int) size;
}
