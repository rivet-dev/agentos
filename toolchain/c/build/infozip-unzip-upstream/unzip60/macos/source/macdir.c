/*
  Copyright (c) 1990-2000 Info-ZIP.  All rights reserved.

  See the accompanying file LICENSE, version 2000-Apr-09 or later
  (the contents of which are also included in unzip.h) for terms of use.
  If, for some reason, all these files are missing, the Info-ZIP license
  also may be found at:  ftp://ftp.info-zip.org/pub/infozip/license.html
*/
/*---------------------------------------------------------------------------

  macdir.c

 *  This file provides dirent-style directory-reading procedures
 *  for V7 Unix systems that don't have such procedures.
 *
 *
  ---------------------------------------------------------------------------*/


/*****************************************************************************/
/*  Includes                                                                 */
/*****************************************************************************/


#include    <Errors.h>
#include    <Files.h>
#include    <Strings.h>
#include    <sound.h>

#include <errno.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>

#include "macdir.h"
#include "helpers.h"
#include "pathname.h"


/*****************************************************************************/
/*  Functions                                                                */
/*****************************************************************************/

int closedir(DIR *dPtr)
{
    free(dPtr);

    return 0;
}


DIR *opendir(char *dirName)
{
    int fullPath;
    unsigned pathLen;
    char *s;
    HParamBlockRec hPB;
    CInfoPBRec cPB;
    DIR *dPtr;
    OSErr err;
    FSSpec spec;
    char CompletePath[NAME_MAX];

    GetCompletePath(CompletePath, dirName, &spec, &err);
    printerr("GetCompletePath", err, err, __LINE__, __FILE__, dirName);

    if (dirName == NULL || *dirName == '\0' ||
        (pathLen = strlen(dirName)) >= 256) {
        errno = EINVAL;
        return NULL;
    }


    /* Get information about volume. */
    memset(&hPB, '\0', sizeof(hPB));

    if (((s = strchr(dirName, ':')) == NULL) || (*dirName == ':')) {
        fullPath = false;
    } else {
        *(s + 1) = '\0';
        hPB.volumeParam.ioVolIndex = -1;
        fullPath = true;
    }

    hPB.volumeParam.ioNamePtr = spec.name;

    err = PBHGetVInfoSync(&hPB);

    if ((err != noErr) || (hPB.volumeParam.ioVFSID != 0)) {
        errno = ENOENT;
        return NULL;
    }

    /* Get information about file. */

    memset(&cPB, '\0', sizeof(cPB));

    if (fullPath)
        cPB.hFileInfo.ioVRefNum = hPB.volumeParam.ioVRefNum;

    cPB.hFileInfo.ioNamePtr = spec.name;

    err = PBGetCatInfoSync(&cPB);

    if (err != noErr) {
        errno = (err == fnfErr) ? ENOENT : EIO;
        return NULL;
    }

    if (!(cPB.hFileInfo.ioFlAttrib & ioDirMask)) {
        errno = ENOTDIR;
        return NULL;
    }

    /* Get space for, and fill in, DIR structure. */

    if ((dPtr = (DIR *)malloc(sizeof(DIR))) == NULL) {
        return NULL;
    }

    dPtr->ioVRefNum = cPB.dirInfo.ioVRefNum;
    dPtr->ioDrDirID = cPB.dirInfo.ioDrDirID;
    dPtr->ioFDirIndex = 1;
    dPtr->flags = 0;

    return dPtr;
}


struct dirent *readdir(DIR *dPtr)
{
    struct dirent *dirPtr;
    CInfoPBRec cPB;
    char name[256];
    OSErr err;

    if (dPtr->flags) {
        return NULL;
    }

    /* Get information about file. */

    memset(&cPB, '\0', sizeof(cPB));

    cPB.hFileInfo.ioNamePtr = (StringPtr)name;
    cPB.hFileInfo.ioFDirIndex = dPtr->ioFDirIndex;
    cPB.hFileInfo.ioVRefNum = dPtr->ioVRefNum;
    cPB.hFileInfo.ioDirID = dPtr->ioDrDirID;

    err = PBGetCatInfoSync(&cPB);

    if (err != noErr) {
        dPtr->flags = 0xff;
        errno = (err == fnfErr) ? ENOENT : EIO;
        return NULL;
    }

    p2cstr((StringPtr)name);

    dirPtr = &dPtr->currEntry;

    dirPtr->d_fileno = dPtr->ioFDirIndex++;
    dirPtr->d_namlen = strlen(name);
    strcpy(dirPtr->d_name, name);
    dirPtr->d_reclen = sizeof(struct dirent) - sizeof(dirPtr->d_name) +
                       dirPtr->d_namlen;

    return dirPtr;
}
