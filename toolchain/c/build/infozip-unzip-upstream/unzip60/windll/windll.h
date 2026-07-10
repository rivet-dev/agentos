/*
  Copyright (c) 1990-2003 Info-ZIP.  All rights reserved.

  See the accompanying file LICENSE, version 2000-Apr-09 or later
  (the contents of which are also included in unzip.h) for terms of use.
  If, for some reason, all these files are missing, the Info-ZIP license
  also may be found at:  ftp://ftp.info-zip.org/pub/infozip/license.html
*/
#ifndef __windll_h   /* prevent multiple inclusions */
#define __windll_h

#include <windows.h>
#include <assert.h>    /* required for all Windows applications */
#include <setjmp.h>

#include "../unzip.h"
#include "../windll/structs.h"
#include "../windll/decs.h"

/* Allow compilation under Borland C++ also */
#ifndef __based
#  define __based(A)
#endif

#ifdef UNZIP_INTERNAL

extern jmp_buf dll_error_return;

extern HANDLE hInst;        /* current instance */

int win_fprintf(zvoid *pG, FILE *file, unsigned int, char far *);
#endif

#endif /* __windll_h */
