/*
  Copyright (c) 1990-2000 Info-ZIP.  All rights reserved.

  See the accompanying file LICENSE, version 2000-Apr-09 or later
  (the contents of which are also included in unzip.h) for terms of use.
  If, for some reason, all these files are missing, the Info-ZIP license
  also may be found at:  ftp://ftp.info-zip.org/pub/infozip/license.html
*/
/*---------------------------------------------------------------------------
    FlexOS specific configuration section:
  ---------------------------------------------------------------------------*/

#ifndef __flxcfg_h
#define __flxcfg_h

#define __16BIT__
#define MED_MEM
#define EXE_EXTENSION ".286"

#ifndef nearmalloc
#  define nearmalloc malloc
#  define nearfree free
#endif

#define CRTL_CP_IS_OEM

#define near
#define far

#endif /* !__flxcfg_h */
