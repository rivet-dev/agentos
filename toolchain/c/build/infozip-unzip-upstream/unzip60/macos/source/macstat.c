/*
  Copyright (c) 1990-2003 Info-ZIP.  All rights reserved.

  See the accompanying file LICENSE, version 2000-Apr-09 or later
  (the contents of which are also included in unzip.h) for terms of use.
  If, for some reason, all these files are missing, the Info-ZIP license
  also may be found at:  ftp://ftp.info-zip.org/pub/infozip/license.html
*/
/*---------------------------------------------------------------------------

  macstat.c

 *  This file provides a unix like file-stat routine
 *  for V7 Unix systems that don't have such procedures.
 *
 *
  ---------------------------------------------------------------------------*/


/*****************************************************************************/
/*  Includes                                                                 */
/*****************************************************************************/

#include <string.h>
#include <stdio.h>
#include <sound.h>

#define UNZIP_INTERNAL
#include "unzip.h"

#include "macstat.h"
#include "helpers.h"
#include "pathname.h"
#include "macstuff.h"
#include "mactime.h"


/*****************************************************************************/
/*  Global Vars                                                              */
/*****************************************************************************/

extern int errno;
extern MACINFO newExtraField;  /* contains all extra-field data */
extern short MacZipMode;


/*****************************************************************************/
/*  Prototypes                                                               */
/*****************************************************************************/



/*****************************************************************************/
/*  Functions                                                                */
/*****************************************************************************/


int UZmacstat(const char *path, struct stat *buf)
{
    Boolean isDirectory;
    long dirID;
    char fullpath[NAME_MAX], UnmangledPath[NAME_MAX];
    CInfoPBRec fpb;
    HVolumeParam vpb;
    FSSpec fileSpec;
    OSErr err, err2;
    short CurrentFork;

    AssertStr(path,path)
    Assert_it(buf,"","")

    memset(buf, 0, sizeof(struct stat));        /* zero out all fields */

    RfDfFilen2Real(UnmangledPath, path, MacZipMode,
                   (newExtraField.flags & EB_M3_FL_NOCHANGE), &CurrentFork);
    GetCompletePath(fullpath, path, &fileSpec, &err);
    err2 = PrintUserHFSerr((err != -43) && (err != 0) && (err != -120),
                           err, path);
    printerr("GetCompletePath:", err2, err2, __LINE__, __FILE__, path);

    if (err != noErr) {
        errno = err;
        return -1;
    }

    /*
     * Fill the fpb & vpb struct up with info about file or directory.
     */

    FSpGetDirectoryID(&fileSpec, &dirID, &isDirectory);
    vpb.ioVRefNum = fpb.hFileInfo.ioVRefNum = fileSpec.vRefNum;
    vpb.ioNamePtr = fpb.hFileInfo.ioNamePtr = fileSpec.name;
    if (isDirectory) {
        fpb.hFileInfo.ioDirID = fileSpec.parID;
    } else {
        fpb.hFileInfo.ioDirID = dirID;
    }

    fpb.hFileInfo.ioFDirIndex = 0;
    err = PBGetCatInfo(&fpb, false);
    if (err == noErr) {
        vpb.ioVolIndex = 0;
        err = PBHGetVInfoSync((HParmBlkPtr)&vpb);
        if (err == noErr && buf != NULL) {
            /*
             * Files are always readable by everyone.
             */
            buf->st_mode |= S_IRUSR | S_IRGRP | S_IROTH;

            /*
             * Use the Volume Info & File Info to fill out stat buf.
             */
            if (fpb.hFileInfo.ioFlAttrib & 0x10) {
                buf->st_mode |= S_IFDIR;
                buf->st_nlink = 2;
            } else {
                buf->st_nlink = 1;
                if (fpb.hFileInfo.ioFlFndrInfo.fdFlags & 0x8000) {
                    buf->st_mode |= S_IFLNK;
                } else {
                    buf->st_mode |= S_IFREG;
                }
            }
            if ((fpb.hFileInfo.ioFlAttrib & 0x10) ||
                (fpb.hFileInfo.ioFlFndrInfo.fdType == 'APPL')) {
                /*
                 * Directories and applications are executable by everyone.
                 */

                buf->st_mode |= S_IXUSR | S_IXGRP | S_IXOTH;
            }
            if ((fpb.hFileInfo.ioFlAttrib & 0x01) == 0) {
                /*
                 * If not locked, then everyone has write acces.
                 */

                buf->st_mode |= S_IWUSR | S_IWGRP | S_IWOTH;
            }
            buf->st_ino = fpb.hFileInfo.ioDirID;
            buf->st_dev = fpb.hFileInfo.ioVRefNum;
            buf->st_uid = -1;
            buf->st_gid = -1;
            buf->st_rdev = 0;

            if (CurrentFork == ResourceFork)
                buf->st_size = fpb.hFileInfo.ioFlRLgLen;
            else
                buf->st_size = fpb.hFileInfo.ioFlLgLen;

            buf->st_blksize = vpb.ioVAlBlkSiz;
            buf->st_blocks = (buf->st_size + buf->st_blksize - 1)
                            / buf->st_blksize;

            /*
             * The times returned by the Mac file system are in the
             * local time zone.  We convert them to GMT so that the
             * epoch starts from GMT.  This is also consistent with
             * what is returned from "clock seconds".
             */

            buf->st_mtime = MacFtime2UnixFtime(fpb.hFileInfo.ioFlMdDat);
            buf->st_ctime = MacFtime2UnixFtime(fpb.hFileInfo.ioFlCrDat);
            buf->st_atime = buf->st_ctime;         /* best guess */

#ifdef DEBUG_TIME
            {
            struct tm *tp = localtime(&buf->st_mtime);
            printf(
              "\nUZmacstat: local buf->st_mtime is %ld = %d/%2d/%2d  %2d:%2d:%2d",
              buf->st_mtime, tp->tm_year, tp->tm_mon+1, tp->tm_mday,
              tp->tm_hour, tp->tm_min, tp->tm_sec);
            tp = gmtime(&buf->st_mtime);
            printf(
              "\nUZmacstat: UTC   buf->st_mtime is %ld = %d/%2d/%2d  %2d:%2d:%2d\n",
              buf->st_mtime, tp->tm_year, tp->tm_mon+1, tp->tm_mday,
              tp->tm_hour, tp->tm_min, tp->tm_sec);
            }
#endif /* DEBUG_TIME */
        }
    }

    if (err != noErr) {
        errno = err;
    }

    return (err == noErr ? 0 : -1);
}
