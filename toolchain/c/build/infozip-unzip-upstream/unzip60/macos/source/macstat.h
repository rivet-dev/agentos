/*
  Copyright (c) 1990-2000 Info-ZIP.  All rights reserved.

  See the accompanying file LICENSE, version 2000-Apr-09 or later
  (the contents of which are also included in unzip.h) for terms of use.
  If, for some reason, all these files are missing, the Info-ZIP license
  also may be found at:  ftp://ftp.info-zip.org/pub/infozip/license.html
*/
/*****************************************************************
 *
 *                stat.h
 *
 *****************************************************************/

#ifndef __macstat_h
#define __macstat_h

#include <time.h>
typedef long dev_t;
typedef long ino_t;
typedef long off_t;

#define _STAT

struct stat {
    dev_t    st_dev;
    ino_t    st_ino;
    unsigned short    st_mode;
    short    st_nlink;
    short    st_uid;
    short    st_gid;
    dev_t    st_rdev;
    off_t    st_size;
    time_t   st_atime, st_mtime, st_ctime;
    long     st_blksize;
    long     st_blocks;
};

#define S_IFMT     0xF000
#define S_IFIFO    0x1000
#define S_IFCHR    0x2000
#define S_IFDIR    0x4000
#define S_IFBLK    0x6000
#define S_IFREG    0x8000
#define S_IFLNK    0xA000
#define S_IFSOCK   0xC000
#define S_ISUID    0x800
#define S_ISGID    0x400
#define S_ISVTX    0x200
#define S_IREAD    0x100
#define S_IWRITE   0x80
#define S_IEXEC    0x40

#define S_IRUSR     00400
#define S_IWUSR     00200
#define S_IXUSR     00100
#define S_IRWXU     (S_IRUSR | S_IWUSR | S_IXUSR)  /* = 00700 */

#define S_IRGRP     00040
#define S_IWGRP     00020
#define S_IXGRP     00010
#define S_IRWXG     (S_IRGRP | S_IWGRP | S_IXGRP)  /* = 00070 */

#define S_IROTH     00004
#define S_IWOTH     00002
#define S_IXOTH     00001
#define S_IRWXO     (S_IROTH | S_IWOTH | S_IXOTH)  /* = 00007 */


extern int UZmacstat(const char *path, struct stat *buf);

#endif /* !__macstat_h */
