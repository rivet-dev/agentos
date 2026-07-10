/*
  Copyright (c) 1990-2000 Info-ZIP.  All rights reserved.

  See the accompanying file LICENSE, version 2000-Apr-09 or later
  (the contents of which are also included in unzip.h) for terms of use.
  If, for some reason, all these files are missing, the Info-ZIP license
  also may be found at:  ftp://ftp.info-zip.org/pub/infozip/license.html
*/
#ifndef __vmstat_h
#define __vmstat_h

/* stat.h      definitions   */

#ifndef _INO_T_DEFINED
typedef unsigned short ino_t;       /* i-node number (not used on DOS) */
#define _INO_T_DEFINED
#endif

#ifndef _DEV_T_DEFINED
typedef short dev_t;                /* device code */
#define _DEV_T_DEFINED
#endif

#ifndef _OFF_T_DEFINED
typedef long off_t;                 /* file offset value */
#define _OFF_T_DEFINED
#endif

#ifndef _STAT_DEFINED
struct stat {
  dev_t  st_dev;
  ino_t  st_ino;
  short  st_mode;
  short  st_nlink;
  int    st_uid;
  int    st_gid;
  off_t  st_size;
  time_t st_atime;
  time_t st_mtime;
  time_t st_ctime;
};
#define _STAT_DEFINED
#endif

int stat(const char *path, struct stat *buf);
int fstat(int fd, struct stat *buf);

#define S_IFMT       0xFFFF
#define _FLDATA(m)   (*(fldata_t *) &m)
#define S_ISDIR(m)   (_FLDATA(m).__dsorgPDSdir)
#define S_ISREG(m)   (_FLDATA(m).__dsorgPO | \
                      _FLDATA(m).__dsorgPDSmem | \
                      _FLDATA(m).__dsorgPS)
#define S_ISBLK(m)   (_FLDATA(m).__recfmBlk)
#define S_ISMEM(m)   (_FLDATA(m).__dsorgMem)

#endif /* __vmstat_h */
