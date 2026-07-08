/*
  Copyright (c) 1990-2000 Info-ZIP.  All rights reserved.

  See the accompanying file LICENSE, version 2000-Apr-09 or later
  (the contents of which are also included in unzip.h) for terms of use.
  If, for some reason, all these files are missing, the Info-ZIP license
  also may be found at:  ftp://ftp.info-zip.org/pub/infozip/license.html
*/
#define _THS_IFMT       0xff00  /* type of file */
#define _THS_IFLIB      0x8000  /* library */
#define _THS_IFDIR      0x4000  /* directory */
#define _THS_IFCHR      0x2000  /* character device */
#define _THS_IFREG      0x1000  /* regular file */
#define _THS_IODRC      0x0800  /* direct */
#define _THS_IOKEY      0x0400  /* keyed */
#define _THS_IOIND      0x0200  /* indexed */
#define _THS_IOPRG      0x0100  /* program */
#define _THS_IO286      0x2100  /* program */
#define _THS_IO386      0x4100  /* program */
#define _THS_IREAD      0x0001  /* read permission */
#define _THS_IWRITE     0x0002  /* write permission */
#define _THS_IEXEC      0x0004  /* execute permission */
#define _THS_IERASE     0x0008  /* erase permission */
#define _THS_IRWXU      0x000f  /* read, write, execute, erase: owner */
#define _THS_IRUSR      0x0001  /* read permission: owner */
#define _THS_IWUSR      0x0002  /* write permission: owner */
#define _THS_IXUSR      0x0004  /* execute permission: owner */
#define _THS_IEUSR      0x0008  /* erase permission: owner */
#define _THS_IROTH      0x0010  /* read permission: other */
#define _THS_IWOTH      0x0020  /* write permission: other */
#define _THS_IXOTH      0x0040  /* execute permission: other */
#define _THS_HIDDN      0x0080  /* hidden, 0 = true */
