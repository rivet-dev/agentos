/*
  Copyright (c) 1990-2000 Info-ZIP.  All rights reserved.

  See the accompanying file LICENSE, version 2000-Apr-09 or later
  (the contents of which are also included in unzip.h) for terms of use.
  If, for some reason, all these files are missing, the Info-ZIP license
  also may be found at:  ftp://ftp.info-zip.org/pub/infozip/license.html
*/
/*---------------------------------------------------------------------------

  aosvs.h

  AOS/VS-specific header file for use with Info-ZIP's UnZip 5.2 and later.

  ---------------------------------------------------------------------------*/


/* stuff to set up for system calls (?FSTAT & ?SACL) and the extra field */

#include <paru.h>                   /* parameter definitions */
#include <packets/filestatus.h>     /* AOS/VS ?FSTAT packet defs */
#include <packets/create.h>         /* AOS/VS ?CREATE packet defs */
#include <sys_calls.h>              /* AOS/VS system call interface */

#define ZEXTRA_HEADID   "VS"
#define ZEXTRA_SENTINEL "FCI"
#define ZEXTRA_REV      ((uch)10)   /* change/use this in later revs */


/* functions defined in zvs_create.c */

extern int   zvs_create(ZCONST char *fname, long cretim, long modtim,
                        long acctim, char *pacl, int ftyp, int eltsize,
                        int maxindlev);
extern int   zvs_credir(ZCONST char *dname, long cretim, long modtim,
                        long acctim, char *pacl, int ftyp, long maxblocks,
                        int hashfsize, int maxindlev);
extern long  dgdate(short mm, short dd, short yy);
extern char  *ux_to_vs_name(char *outname, ZCONST char *inname);


/* could probably avoid the unions - all elements in each one are the same
 * size, and we're going to assume this */

typedef union zvsfstat_stru {
    P_FSTAT       norm_fstat_packet;      /* normal fstat packet */
    P_FSTAT_DIR   dir_fstat_packet;       /* DIR/CPD fstat packet */
    P_FSTAT_UNIT  unit_fstat_packet;      /* unit (device) fstat packet */
    P_FSTAT_IPC   ipc_fstat_packet;       /* IPC file fstat packet */
} ZVSFSTAT_STRU;

typedef union zvscreate_stru {
    P_CREATE      norm_create_packet;     /* normal create packet */
    P_CREATE_DIR  dir_create_packet;      /* DIR/CPD create packet */
    P_CREATE_IPC  ipc_create_packet;      /* IPC file create packet */
} ZVSCREATE_STRU;


typedef struct zextrafld {
    char            extra_header_id[2];   /* set to VS - in theory, an int */
    char            extra_data_size[2];   /* size of rest (little-endian) */
    char            extra_sentinel[4];    /* set to FCI w/ trailing null */
    uch             extra_rev;            /* set to 10 for rev 1.0 */
    ZVSFSTAT_STRU   fstat_packet;         /* the fstat packet */
    char            aclbuf[$MXACL];       /* the raw ACL */
} ZEXTRAFLD;
