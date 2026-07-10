/*
  Copyright (c) 1990-2009 Info-ZIP.  All rights reserved.

  See the accompanying file LICENSE, version 2009-Jan-02 or later
  (the contents of which are also included in unzip.h) for terms of use.
  If, for some reason, all these files are missing, the Info-ZIP license
  also may be found at:  ftp://ftp.info-zip.org/pub/infozip/license.html
*/
/*
 Example header file

 Do not use this header file in the WiZ application, use WIZ.H
 instead.

*/
#ifndef _UZEXAMPL_H
#define _UZEXAMPL_H

#include <windows.h>
#ifdef __RSXNT__
#  include "../win32/rsxntwin.h"
#endif
#include <assert.h>    /* required for all Windows applications */
#include <stdlib.h>
#include <stdio.h>
#include <commdlg.h>
#ifndef __RSXNT__
#  include <dlgs.h>
#endif
#include <windowsx.h>

#include "../unzip.h"
#include "../windll/structs.h"
#include "../windll/decs.h"

/* Defines */

typedef const UzpVer * (WINAPI * _DLL_UZVER)(void);
typedef int (WINAPI * _DLL_UNZIP)(int, char **, int, char **,
                                  LPDCL, LPUSERFUNCTIONS);

/* Global variables */

extern LPUSERFUNCTIONS lpUserFunctions;
extern LPDCL lpDCL;

extern HINSTANCE hUnzipDll;

extern int hFile;                 /* file handle             */

/* Global functions */

int WINAPI DisplayBuf(LPSTR, unsigned long);

/* Procedure Calls */
#ifdef Z_UINT8_DEFINED
void WINAPI ReceiveDllMessage(z_uint8 ucsize, z_uint8 csize,
    unsigned cfactor,
    unsigned mo, unsigned dy, unsigned yr, unsigned hh, unsigned mm,
    char c, LPCSTR filename, LPCSTR methbuf, unsigned long crc, char fCrypt);
#else
void WINAPI ReceiveDllMessage(unsigned long ucsize, unsigned long csize,
    unsigned cfactor,
    unsigned mo, unsigned dy, unsigned yr, unsigned hh, unsigned mm,
    char c, LPCSTR filename, LPCSTR methbuf, unsigned long crc, char fCrypt);
#endif
void WINAPI ReceiveDllMessage_NO_INT64(unsigned long ucsiz_l,
    unsigned long ucsiz_h, unsigned long csiz_l, unsigned long csiz_h,
    unsigned cfactor,
    unsigned mo, unsigned dy, unsigned yr, unsigned hh, unsigned mm,
    char c, LPCSTR filename, LPCSTR methbuf, unsigned long crc, char fCrypt);
#endif /* _UZEXAMPL_H */
