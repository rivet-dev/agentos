/*
  Copyright (c) 1990-2000 Info-ZIP.  All rights reserved.

  See the accompanying file LICENSE, version 2000-Apr-09 or later
  (the contents of which are also included in unzip.h) for terms of use.
  If, for some reason, all these files are missing, the Info-ZIP license
  also may be found at:  ftp://ftp.info-zip.org/pub/infozip/license.html
*/
/*****************************************************************
 *
 *                dirent.h
 *
 *****************************************************************/

#ifndef __DIRENT_H
#define __DIRENT_H

#include <errno.h>

#ifndef EINVAL
#define EINVAL      9
#endif

#ifndef EIO
#define EIO         10
#endif

#ifndef ENOTDIR
#define ENOTDIR     20
#endif

#ifndef ENOENT
#define ENOENT      39
#endif

#ifndef NAME_MAX
#define NAME_MAX    1024
#endif

struct dirent {
    unsigned long   d_fileno;
    short           d_reclen;
    short           d_namlen;
    char            d_name[NAME_MAX + 1];
};

typedef struct {
    short           ioFDirIndex;
    short           ioVRefNum;
    long            ioDrDirID;
    short           flags;
    struct dirent   currEntry;
} DIR;

#define direct dirent

DIR *opendir(char *);
struct dirent *readdir(DIR *);
void rewinddir(DIR *);
int closedir(DIR *);

#endif /* !__DIRENT_H */
