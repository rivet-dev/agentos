/*
  Copyright (c) 1990-2007 Info-ZIP.  All rights reserved.

  See the accompanying file LICENSE, version 2007-Mar-04 or later
  (the contents of which are also included in unzip.h) for terms of use.
  If, for some reason, all these files are missing, the Info-ZIP license
  also may be found at:  ftp://ftp.info-zip.org/pub/infozip/license.html
*/
#ifndef __decs_h   /* prevent multiple inclusions */
#define __decs_h

/* for UnZip, the "basic" part of the win32 api is sufficient */
#ifndef WIN32_LEAN_AND_MEAN
#  define WIN32_LEAN_AND_MEAN
#  define IZ_HASDEFINED_WIN32LEAN
#endif
#include <windows.h>
#ifndef __unzip_h
#  include "../unzip.h"
#endif
#ifndef __structs_h
#  include "../windll/structs.h"
#endif
#ifdef IZ_HASDEFINED_WIN32LEAN
#  undef WIN32_LEAN_AND_MEAN
#  undef IZ_HASDEFINED_WIN32LEAN
#endif

#define Wiz_Match match

#ifdef __cplusplus
extern "C" {
#endif

void    WINAPI Wiz_NoPrinting(int f);
int     WINAPI Wiz_Validate(LPSTR archive, int AllCodes);
BOOL    WINAPI Wiz_Init(zvoid *, LPUSERFUNCTIONS);
BOOL    WINAPI Wiz_SetOpts(zvoid *, LPDCL);
int     WINAPI Wiz_Unzip(zvoid *, int, char **, int, char **);
int     WINAPI Wiz_SingleEntryUnzip(int, char **, int, char **,
                                    LPDCL, LPUSERFUNCTIONS);
int     WINAPI Wiz_SingleEntryUnzpList(unsigned, LPCSTR, unsigned, LPCSTR,
                                       LPDCL, LPUSERFUNCTIONS);

int     WINAPI Wiz_UnzipToMemory(LPSTR zip, LPSTR file,
                                 LPUSERFUNCTIONS lpUserFunctions,
                                 UzpBuffer *retstr);
int     WINAPI Wiz_Grep(LPSTR archive, LPSTR file, LPSTR pattern,
                        int cmd, int SkipBin,
                        LPUSERFUNCTIONS lpUserFunctions);

#ifdef __cplusplus
}
#endif

#endif /* __decs_h */
