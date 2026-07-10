/*
  Copyright (c) 1990-2000 Info-ZIP.  All rights reserved.

  See the accompanying file LICENSE, version 2000-Apr-09 or later
  (the contents of which are also included in unzip.h) for terms of use.
  If, for some reason, all these files are missing, the Info-ZIP license
  also may be found at:  ftp://ftp.info-zip.org/pub/infozip/license.html
*/
#ifndef _IZQDOS_H
#define _IZQDOS_H

#include <qdos.h>
typedef struct
{
    unsigned short shortid;
    struct
    {
        unsigned char lo;
        unsigned char hi;
    } len;
    char        longid[8];
    struct      qdirect     header;
} qdosextra;

typedef struct
{
    unsigned short shortid;
    struct
    {
        unsigned char lo;
        unsigned char hi;
    } len;
    char        longid[4];
    struct      qdirect     header;
} jbextra;

#define SHORTID     0x4afb
#define JBSHORTID   0x4afb
#define LONGID      "QDOS02"
#define JBLONGID    "QZHD"
#define EXTRALEN    (sizeof(qdosextra) - 2 * sizeof(char) - sizeof(short))
#define JBEXTRALEN  (sizeof(jbextra)   - 2 * sizeof(char) - sizeof(short))

extern short qlflag;
extern short qlwait;
#endif
